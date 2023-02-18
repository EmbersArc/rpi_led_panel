use std::{
    error::Error,
    fmt::{Display, Formatter},
    fs::{write, OpenOptions},
    mem::replace,
    str::FromStr,
    sync::mpsc::{
        channel, sync_channel, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError,
    },
    thread::{spawn, JoinHandle},
    time::Duration,
};

use thread_priority::{set_current_thread_priority, ThreadPriority};

use crate::{
    canvas::{Canvas, LedSequence, PixelDesignator, PixelDesignatorMap},
    chip::PiChip,
    gpio::{Gpio, GpioInitializationError},
    hardware_mapping::HardwareMapping,
    init_sequence::InitializationSequence,
    multiplex_mapper::{get_multiplex_mapper, MultiplexMapper},
    row_address_setter::get_row_address_setter,
    utils::{set_thread_affinity, FrameRateMonitor},
    RGBMatrixConfig,
};

fn initialize_update_thread(chip: &PiChip) {
    // Pin the thread to the last core to avoid the flicker resulting from context switching.
    let last_core_id = chip.num_cores() - 1;
    set_thread_affinity(last_core_id);

    // Disable realtime throttling.
    if chip.num_cores() > 1 && write("/proc/sys/kernel/sched_rt_runtime_us", "999000").is_err() {
        eprintln!("Could not disable realtime throttling");
    }

    // Set the core to performance mode.
    if chip.num_cores() > 1
        && write(
            "/sys/devices/system/cpu/cpu3/cpufreq/scaling_governor",
            "performance",
        )
        .is_err()
    {
        eprintln!("Could not set core 4 to performance mode.");
    }

    // Set the highest thread priority.
    if set_current_thread_priority(ThreadPriority::Max).is_err() {
        eprintln!("Could not set thread priority. This might lead to reduced performance.",);
    }
}

#[derive(Debug)]
pub enum MatrixCreationError {
    InvalidChipName(String),
    ChipDeterminationError,
    InvalidHardwareMapping(String),
    InvalidLedSequence(String),
    TooManyParallelChains(usize),
    InvalidInitializationSequence(String),
    InvalidDitherBits(usize),
    ThreadTimedOut,
    GpioError(GpioInitializationError),
    MemoryAccessError,
}

impl Error for MatrixCreationError {}

impl Display for MatrixCreationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            MatrixCreationError::InvalidChipName(name) => {
                write!(f, "'{name}' is not a valid chip name")
            }
            MatrixCreationError::ChipDeterminationError => {
                f.write_str("Failed to automatically determine Raspberry Pi model.")
            }
            MatrixCreationError::InvalidHardwareMapping(name) => {
                write!(f, "'{name}' is not a valid hardware mapping.")
            }
            MatrixCreationError::InvalidLedSequence(name) => {
                write!(f, "'{name}' is not a valid LED sequence.")
            }
            MatrixCreationError::TooManyParallelChains(max) => {
                write!(f, "GPIO mapping only supports up to {max} parallel panels.")
            }
            MatrixCreationError::InvalidInitializationSequence(name) => {
                write!(f, "Initialization sequence '{name}' is not supported.")
            }
            MatrixCreationError::InvalidDitherBits(value) => {
                write!(f, "Unsupported dither bits '{value}'.")
            }
            MatrixCreationError::ThreadTimedOut => {
                f.write_str("The update thread did not return in time.")
            }
            MatrixCreationError::GpioError(error) => {
                write!(f, "GPIO initialization error: {error}")
            }
            MatrixCreationError::MemoryAccessError => f.write_str(
                "Failed to access the physical memory. Not running with root privileges?",
            ),
        }
    }
}

pub struct RGBMatrix {
    /// The join handle of the update thread.
    thread_handle: Option<JoinHandle<()>>,
    /// Sender for the shutdown signal.
    shutdown_sender: Sender<()>,
    /// Receiver for GPIO inputs.
    input_receiver: Receiver<u32>,
    /// Channel to send canvas to update thread.
    canvas_to_thread_sender: SyncSender<Box<Canvas>>,
    /// Channel to receive canvas from update thread.
    canvas_from_thread_receiver: Receiver<Box<Canvas>>,
    /// Additional requested inputs that can be received.
    enabled_input_bits: u32,
    /// Frame rate measurement.
    frame_rate_monitor: FrameRateMonitor,
}

impl RGBMatrix {
    /// Create a new RGB matrix controller. This starts a new thread to update the matrix. Returns the
    /// controller and a canvas for drawing.
    ///
    /// You can additionally request user readable GPIO bits which can later be received with
    /// [`RGBMatrix::receive_new_inputs`]. Only bits that are not already in use for reading or writing by the
    /// matrix are allowed. Use [`RGBMatrix::enabled_input_bits`] after calling this function to check which
    /// bits were actually available.
    pub fn new(
        mut config: RGBMatrixConfig,
        requested_inputs: u32,
    ) -> Result<(Self, Box<Canvas>), MatrixCreationError> {
        // Check if we can access the memory before doing anything else.
        OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/mem")
            .map_err(|_| MatrixCreationError::MemoryAccessError)?;

        let chip = if let Some(name) = config.pi_chip.as_ref() {
            PiChip::from_name(name)
                .ok_or_else(|| MatrixCreationError::InvalidChipName(name.to_string()))
        } else {
            PiChip::determine().ok_or(MatrixCreationError::ChipDeterminationError)
        }?;

        let hardware_mapping =
            HardwareMapping::from_name(&config.gpio_mapping).ok_or_else(|| {
                MatrixCreationError::InvalidHardwareMapping(config.gpio_mapping.to_string())
            })?;
        let led_sequence = LedSequence::from_str(&config.led_sequence)
            .ok()
            .ok_or_else(|| {
                MatrixCreationError::InvalidLedSequence(config.led_sequence.to_string())
            })?;

        let max_parallel = hardware_mapping.max_parallel_chains();
        if config.parallel > max_parallel {
            return Err(MatrixCreationError::TooManyParallelChains(max_parallel));
        }

        let pixel_designator = PixelDesignator::new(&hardware_mapping, led_sequence);
        let mut shared_mapper =
            PixelDesignatorMap::new(pixel_designator, &hardware_mapping, led_sequence, &config);

        if let Some(mapper_name) = config.multiplexing.as_ref() {
            let mapper = get_multiplex_mapper(mapper_name.as_str());
            shared_mapper = Self::apply_pixel_mapper(
                shared_mapper,
                mapper,
                &mut config,
                &hardware_mapping,
                led_sequence,
                pixel_designator,
            );
        }

        let initialization_sequence = config
            .panel_type
            .as_deref()
            .map(|name| {
                InitializationSequence::from_name(name).ok_or_else(|| {
                    MatrixCreationError::InvalidInitializationSequence(name.to_string())
                })
            })
            .transpose()?;

        let dither_start_bits = match config.dither_bits {
            0 => [0, 0, 0, 0],
            1 => [0, 1, 0, 1],
            2 => [0, 1, 2, 2],
            _ => return Err(MatrixCreationError::InvalidDitherBits(config.dither_bits)),
        };

        // Create two canvases, one for the display update thread and one for the user to modify. They will be
        // swapped out after each frame.
        let canvas = Box::new(Canvas::new(&config, shared_mapper));
        let mut thread_canvas = canvas.clone();

        let (canvas_to_thread_sender, canvas_to_thread_receiver) = sync_channel::<Box<Canvas>>(0);
        let (canvas_from_thread_sender, canvas_from_thread_receiver) =
            sync_channel::<Box<Canvas>>(1);
        let (shutdown_sender, shutdown_receiver) = channel::<()>();
        let (input_sender, input_receiver) = channel::<u32>();
        let (thread_start_result_sender, thread_start_result_receiver) =
            channel::<Result<u32, MatrixCreationError>>();

        let thread_handle = spawn(move || {
            initialize_update_thread(&chip);

            let mut address_setter =
                get_row_address_setter(config.row_setter.as_str(), &hardware_mapping, &config);

            let mut gpio =
                match Gpio::new(chip, &hardware_mapping, &config, address_setter.as_ref()) {
                    Ok(gpio) => gpio,
                    Err(error) => {
                        thread_start_result_sender
                            .send(Err(MatrixCreationError::GpioError(error)))
                            .expect("Could not send to main thread.");
                        return;
                    }
                };

            // Run the initialization sequence if necessary.
            if let Some(sequence) = initialization_sequence {
                sequence.run(&mut gpio, &hardware_mapping, config.cols);
            }

            let mut last_gpio_inputs: u32 = 0;

            // Dither sequence
            let mut dither_low_bit_sequence = 0;

            let frame_time_target_us = (1_000_000.0 / config.refresh_rate as f64) as u64;

            let color_clk_mask = hardware_mapping.get_color_clock_mask(config.parallel);

            let enabled_input_bits = gpio.request_enabled_inputs(requested_inputs);
            thread_start_result_sender
                .send(Ok(enabled_input_bits))
                .expect("Could not send to main thread.");

            'thread: loop {
                let start_time = gpio.get_time();
                loop {
                    // Try to receive a shutdown request.
                    if shutdown_receiver.try_recv() != Err(TryRecvError::Empty) {
                        break 'thread;
                    }
                    // Read input bits and send them if they have changed.
                    let new_inputs = gpio.read();
                    if new_inputs != last_gpio_inputs {
                        match input_sender.send(new_inputs) {
                            Ok(()) => {}
                            Err(_) => {
                                break 'thread;
                            }
                        }
                        last_gpio_inputs = new_inputs;
                    }
                    // Wait for a swap canvas.
                    match canvas_to_thread_receiver.recv_timeout(Duration::from_millis(1)) {
                        Ok(new_canvas) => {
                            let old_canvas = replace(&mut thread_canvas, new_canvas);
                            match canvas_from_thread_sender.send(old_canvas) {
                                Ok(_) => break,
                                Err(_) => {
                                    break 'thread;
                                }
                            };
                        }
                        Err(RecvTimeoutError::Disconnected) => {
                            break 'thread;
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                }

                thread_canvas.dump_to_matrix(
                    &mut gpio,
                    &hardware_mapping,
                    address_setter.as_mut(),
                    dither_start_bits[dither_low_bit_sequence % dither_start_bits.len()],
                    color_clk_mask,
                );
                dither_low_bit_sequence += 1;

                // Sleep for the rest of the frame.
                let now_time = gpio.get_time();
                let end_time = start_time + frame_time_target_us;
                if let Some(remaining_time) = end_time.checked_sub(now_time) {
                    gpio.sleep(remaining_time);
                }
            }

            // Turn it off.
            thread_canvas.fill(0, 0, 0);
            thread_canvas.dump_to_matrix(
                &mut gpio,
                &hardware_mapping,
                address_setter.as_mut(),
                0,
                color_clk_mask,
            );
        });

        let enabled_input_bits = thread_start_result_receiver
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| MatrixCreationError::ThreadTimedOut)??;

        let rgbmatrix = Self {
            thread_handle: Some(thread_handle),
            input_receiver,
            shutdown_sender,
            canvas_to_thread_sender,
            canvas_from_thread_receiver,
            enabled_input_bits,
            frame_rate_monitor: FrameRateMonitor::new(),
        };

        Ok((rgbmatrix, canvas))
    }

    fn apply_pixel_mapper(
        shared_mapper: PixelDesignatorMap,
        mut mapper: Box<dyn MultiplexMapper>,
        config: &mut RGBMatrixConfig,
        hardware_mapping: &HardwareMapping,
        led_sequence: LedSequence,
        pixel_designator: PixelDesignator,
    ) -> PixelDesignatorMap {
        let old_width = shared_mapper.width();
        let old_height = shared_mapper.height();
        mapper.edit_rows_cols(&mut config.rows, &mut config.cols);
        let [new_width, new_height] = mapper.get_size_mapping(old_width, old_height);
        let mut new_mapper =
            PixelDesignatorMap::new(pixel_designator, hardware_mapping, led_sequence, config);
        for y in 0..new_height {
            for x in 0..new_width {
                let [orig_x, orig_y] = mapper.map_visible_to_matrix(old_width, old_height, x, y);
                if !(0..orig_x).contains(&old_width) || !(0..orig_y).contains(&old_height) {
                    eprintln!("Error in pixel mapper"); // TODO
                    continue;
                }
                let orig_designator = shared_mapper.get(orig_x, orig_y).unwrap();
                *new_mapper.get_mut(x, y).unwrap() = *orig_designator;
            }
        }
        new_mapper
    }

    /// Updates the matrix with the new canvas. Blocks until the end of the current frame.
    pub fn update_on_vsync(&mut self, canvas: Box<Canvas>) -> Box<Canvas> {
        let Self {
            canvas_to_thread_sender,
            canvas_from_thread_receiver,
            frame_rate_monitor,
            ..
        } = self;

        canvas_to_thread_sender.send(canvas).unwrap();

        frame_rate_monitor.update();

        canvas_from_thread_receiver.recv().unwrap()
    }

    /// Get the bits that were available for input.
    pub fn enabled_input_bits(&self) -> u32 {
        self.enabled_input_bits
    }

    /// Tries to receive a new GPIO input as specified with [`RGBMatrix::request_enabled_inputs`].
    pub fn receive_new_inputs(&mut self, timeout: Duration) -> Option<u32> {
        self.input_receiver.recv_timeout(timeout).ok()
    }

    /// Get the average frame rate over the last 60 frames.
    pub fn get_framerate(&self) -> usize {
        self.frame_rate_monitor.get_fps().round() as usize
    }
}

impl Drop for RGBMatrix {
    fn drop(&mut self) {
        let Self {
            thread_handle,
            shutdown_sender,
            ..
        } = self;
        if let Some(handle) = thread_handle.take() {
            shutdown_sender.send(()).ok();
            let _result = handle.join();
        }
    }
}

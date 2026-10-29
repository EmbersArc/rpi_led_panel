use std::{
    any::Any,
    error::Error,
    fmt::{Display, Formatter},
    fs::{OpenOptions, write},
    mem::replace,
    sync::Arc,
    sync::atomic::{AtomicU32, Ordering},
    sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError, channel, sync_channel},
    thread::{JoinHandle, spawn},
    time::Duration,
};
use thread_priority::{
    RealtimeThreadSchedulePolicy, ThreadPriority, ThreadSchedulePolicy,
    set_current_thread_priority, set_thread_priority_and_policy, thread_native_id,
};

use crate::{
    RGBMatrixConfig,
    canvas::{Canvas, PixelDesignator, PixelDesignatorMap},
    chip::PiChip,
    gpio::{Gpio, GpioInitializationError},
    multiplex_mapper::MultiplexMapperType,
    pixel_mapper::PixelMapper,
    utils::{FrameRateMonitor, linux_has_isol_cpu, set_thread_affinity},
};

/// How often GPIO inputs are polled while display updates are paused.
const SUSPENDED_INPUT_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Messages sent from the main thread to the display update thread.
enum ThreadMessage {
    Canvas(Box<Canvas>),
    Suspend,
    Shutdown,
}

fn initialize_update_thread(chip: PiChip) {
    // Pin the thread to the last core to avoid the flicker resulting from context switching.
    let last_core_id = chip.num_cores() - 1;
    if !set_thread_affinity(last_core_id) {
        eprintln!("Could not pin the display update thread to core {last_core_id}.");
    }

    // If the user has not setup isolcpus, let them know about the performance improvement.
    if chip.num_cores() > 1 && !linux_has_isol_cpu(last_core_id) {
        eprintln!(
            "Suggestion: to slightly improve display update, add\n\tisolcpus={last_core_id}\nat \
            the end of /boot/cmdline.txt and reboot"
        );
    }

    // Disable realtime throttling.
    if chip.num_cores() > 1 && write("/proc/sys/kernel/sched_rt_runtime_us", "999000").is_err() {
        eprintln!("Could not disable realtime throttling");
    }

    // Set the last core to performance mode.
    if chip.num_cores() > 1
        && write(
            format!("/sys/devices/system/cpu/cpu{last_core_id}/cpufreq/scaling_governor"),
            "performance",
        )
        .is_err()
    {
        eprintln!(
            "Could not set core {} to performance mode.",
            last_core_id + 1
        );
    }

    // Run as a realtime thread with the highest priority.
    let realtime = set_thread_priority_and_policy(
        thread_native_id(),
        ThreadPriority::Max,
        ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::Fifo),
    );
    if realtime.is_err() {
        eprintln!("Could not set realtime scheduling. This might lead to flickering.");
        // Fall back to the highest non-realtime priority.
        if set_current_thread_priority(ThreadPriority::Max).is_err() {
            eprintln!("Could not set thread priority. This might lead to reduced performance.");
        }
    }
}

/// The message of a panic payload, which is a `&str` or `String` unless `panic_any` was used.
fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        String::from(*message)
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        String::from("Unknown panic payload.")
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum MatrixCreationError {
    ChipDeterminationError,
    InvalidConfig(String),
    TooManyParallelChains(usize),
    InvalidDitherBits(usize),
    ThreadTimedOut,
    ThreadPanicked(String),
    GpioError(GpioInitializationError),
    MemoryAccessError,
    PixelMapperError(String),
}

impl Error for MatrixCreationError {}

impl Display for MatrixCreationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            MatrixCreationError::ChipDeterminationError => {
                f.write_str("Failed to automatically determine Raspberry Pi model.")
            }
            MatrixCreationError::InvalidConfig(message) => {
                write!(f, "Invalid configuration: {message}")
            }
            MatrixCreationError::TooManyParallelChains(max) => {
                write!(f, "GPIO mapping only supports up to {max} parallel panels.")
            }
            MatrixCreationError::InvalidDitherBits(value) => {
                write!(f, "Unsupported dither bits '{value}'.")
            }
            MatrixCreationError::ThreadTimedOut => {
                f.write_str("The update thread did not return in time.")
            }
            MatrixCreationError::ThreadPanicked(message) => {
                write!(
                    f,
                    "The update thread panicked during initialization: {message}"
                )
            }
            MatrixCreationError::GpioError(error) => {
                write!(f, "GPIO initialization error: {error}")
            }
            MatrixCreationError::MemoryAccessError => f.write_str(
                "Failed to access the physical memory. Not running with root privileges?",
            ),
            MatrixCreationError::PixelMapperError(message) => {
                write!(f, "Error in pixel mapper: {message}")
            }
        }
    }
}

/// The display update thread stopped while the matrix was in use, which only happens if it panicked.
#[derive(Debug, Clone)]
pub struct UpdateThreadError(String);

impl Error for UpdateThreadError {}

impl Display for UpdateThreadError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "The display update thread stopped unexpectedly: {}",
            self.0
        )
    }
}

pub struct RGBMatrix {
    /// The join handle of the update thread.
    thread_handle: Option<JoinHandle<()>>,
    /// Why the update thread stopped, once it did.
    thread_error: Option<UpdateThreadError>,
    /// Receiver for GPIO inputs.
    input_receiver: Receiver<u32>,
    /// Channel to send canvases and control messages to the update thread.
    message_sender: SyncSender<ThreadMessage>,
    /// Channel to receive canvas from update thread.
    canvas_from_thread_receiver: Receiver<Box<Canvas>>,
    /// Additional requested inputs that can be received.
    enabled_input_bits: u32,
    /// Frame rate measurement.
    framerate: Arc<AtomicU32>,
    /// Whether the display is currently suspended.
    suspended: bool,
    /// Brightness in percent for the canvases returned by `update_on_vsync`.
    brightness: u8,
    /// PWM bits for the canvases returned by `update_on_vsync`.
    pwm_bits: usize,
}

impl RGBMatrix {
    /// Create a new RGB matrix controller. This starts a new thread to update the matrix. Returns the
    /// controller and a canvas for drawing.
    ///
    /// You can additionally request user readable GPIO bits which can later be received with
    /// [`RGBMatrix::receive_new_inputs`]. Only bits that are not already in use for reading or writing by the
    /// matrix are allowed. Use [`RGBMatrix::enabled_input_bits`] after calling this function to check which
    /// bits were actually available.
    ///
    /// Note: If privilege dropping is enabled in the config, then calling this function will have the side
    /// effect of dropping the application's privileges down to the configured user/group.
    pub fn new(
        mut config: RGBMatrixConfig,
        requested_inputs: u32,
    ) -> Result<(Self, Box<Canvas>), MatrixCreationError> {
        let shared_mapper = Self::create_pixel_map(&mut config)?;

        // Check if we can access the memory before setting up the hardware.
        OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/mem")
            .map_err(|_| MatrixCreationError::MemoryAccessError)?;

        let chip = config
            .pi_chip
            .or_else(PiChip::determine)
            .ok_or(MatrixCreationError::ChipDeterminationError)?;

        let dither_start_bits = match config.dither_bits {
            0 => [0, 0, 0, 0],
            1 => [0, 1, 0, 1],
            2 => [0, 1, 2, 2],
            _ => unreachable!("dither_bits is checked in RGBMatrixConfig::validate"),
        };

        // Create two canvases, one for the display update thread and one for the user to modify. They will be
        // swapped out after each frame.
        let canvas = Box::new(Canvas::new(&config, shared_mapper));
        let mut thread_canvas = canvas.clone();

        let (message_sender, message_receiver) = sync_channel::<ThreadMessage>(0);
        let (canvas_from_thread_sender, canvas_from_thread_receiver) =
            sync_channel::<Box<Canvas>>(1);
        let (input_sender, input_receiver) = channel::<u32>();
        let (thread_start_result_sender, thread_start_result_receiver) =
            channel::<Result<u32, MatrixCreationError>>();
        let framerate = Arc::new(AtomicU32::new(0));
        let mut frame_rate_monitor = FrameRateMonitor::new(framerate.clone());
        let (brightness, pwm_bits) = (config.led_brightness, config.pwm_bits);

        let thread_handle = spawn(move || {
            initialize_update_thread(chip);

            let mut address_setter = config.row_setter.create(&config);

            let mut gpio = match Gpio::new(chip, &config, address_setter.as_ref()) {
                Ok(gpio) => gpio,
                Err(error) => {
                    // Fails only if the main thread stopped waiting, in which case there is no one to tell.
                    thread_start_result_sender
                        .send(Err(MatrixCreationError::GpioError(error)))
                        .ok();
                    return;
                }
            };

            // Run the initialization sequence if necessary.
            if let Some(panel_type) = config.panel_type {
                panel_type.run_init_sequence(&mut gpio, &config);
            }

            let mut last_gpio_inputs: u32 = 0;

            // Dither sequence
            let mut dither_low_bit_sequence = 0;

            let frame_time_target_us = (1_000_000.0 / config.refresh_rate as f64) as u64;

            let color_clk_mask = config
                .hardware_mapping
                .get_color_clock_mask(config.parallel);

            let enabled_input_bits = gpio.request_enabled_inputs(requested_inputs);
            // The main thread stopped waiting and returned an error, so nobody will use the matrix.
            if thread_start_result_sender
                .send(Ok(enabled_input_bits))
                .is_err()
            {
                return;
            }

            let mut suspended = false;

            'thread: loop {
                let start_time = gpio.get_time();
                // Read input bits and send them if they have changed.
                let new_inputs = gpio.read();
                if new_inputs != last_gpio_inputs {
                    if input_sender.send(new_inputs).is_err() {
                        break 'thread;
                    }
                    last_gpio_inputs = new_inputs;
                }
                // Check for a new message. While suspended, wait for one but keep polling the inputs.
                let message = if suspended {
                    match message_receiver.recv_timeout(SUSPENDED_INPUT_POLL_INTERVAL) {
                        Ok(message) => Some(message),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break 'thread,
                    }
                } else {
                    match message_receiver.try_recv() {
                        Ok(message) => Some(message),
                        Err(TryRecvError::Empty) => None,
                        Err(TryRecvError::Disconnected) => break 'thread,
                    }
                };
                match message {
                    Some(ThreadMessage::Canvas(new_canvas)) => {
                        let old_canvas = replace(&mut thread_canvas, new_canvas);
                        let Ok(()) = canvas_from_thread_sender.send(old_canvas) else {
                            break 'thread;
                        };
                        suspended = false;
                    }
                    Some(ThreadMessage::Suspend) => {
                        // Draw a blank frame so that no row stays lit, but keep the current canvas for
                        // when the display is resumed.
                        let mut blank_canvas = thread_canvas.clone();
                        blank_canvas.fill(0, 0, 0);
                        blank_canvas.dump_to_matrix(
                            &mut gpio,
                            &config.hardware_mapping,
                            address_setter.as_mut(),
                            0,
                            color_clk_mask,
                        );
                        frame_rate_monitor.pause();
                        suspended = true;
                    }
                    Some(ThreadMessage::Shutdown) => break 'thread,
                    None => {}
                }
                if suspended {
                    continue;
                }

                thread_canvas.dump_to_matrix(
                    &mut gpio,
                    &config.hardware_mapping,
                    address_setter.as_mut(),
                    dither_start_bits[dither_low_bit_sequence % dither_start_bits.len()],
                    color_clk_mask,
                );
                dither_low_bit_sequence += 1;

                frame_rate_monitor.update();

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
                &config.hardware_mapping,
                address_setter.as_mut(),
                0,
                color_clk_mask,
            );
        });

        let enabled_input_bits =
            match thread_start_result_receiver.recv_timeout(Duration::from_secs(10)) {
                Ok(result) => result?,
                Err(RecvTimeoutError::Timeout) => return Err(MatrixCreationError::ThreadTimedOut),
                // The thread only hangs up without sending a result if it panicked.
                Err(RecvTimeoutError::Disconnected) => {
                    let message = match thread_handle.join() {
                        Ok(()) => String::from("The thread stopped without sending a result."),
                        Err(payload) => panic_message(payload.as_ref()),
                    };
                    return Err(MatrixCreationError::ThreadPanicked(message));
                }
            };

        let rgbmatrix = Self {
            thread_handle: Some(thread_handle),
            thread_error: None,
            input_receiver,
            message_sender,
            canvas_from_thread_receiver,
            enabled_input_bits,
            framerate,
            suspended: false,
            brightness,
            pwm_bits,
        };

        Ok((rgbmatrix, canvas))
    }

    /// Validate the config and map the canvas pixels to the bitplane buffer. Multiplexing can change the
    /// physical layout of the panels, in which case the rows and columns of the config are updated to match.
    fn create_pixel_map(
        config: &mut RGBMatrixConfig,
    ) -> Result<PixelDesignatorMap, MatrixCreationError> {
        let mut multiplex_mapper = config.multiplexing.map(MultiplexMapperType::create);
        let stretch_factor = multiplex_mapper
            .as_ref()
            .map_or(1, |mapper| mapper.panel_stretch_factor());
        config.validate(stretch_factor)?;

        // The multiplexers might choose to have a different physical layout.
        // We need to configure that first before setting up the hardware.
        if let Some(mapper) = &mut multiplex_mapper {
            mapper.edit_rows_cols(&mut config.rows, &mut config.cols);
        }

        let pixel_designator = PixelDesignator::new(&config.hardware_mapping, config.led_sequence);
        let width = config.cols * config.chain_length;
        let height = config.rows * config.parallel;
        let mut shared_mapper = PixelDesignatorMap::new(pixel_designator, width, height, config);

        // Apply the mapping for the panels first.
        if let Some(mapper) = multiplex_mapper {
            let mapper = PixelMapper::Multiplex(mapper);
            shared_mapper = Self::apply_pixel_mapper(&shared_mapper, &mapper, pixel_designator)?;
        }

        // Apply higher level mappers that might arrange panels.
        for mapper_type in config.pixelmapper.iter() {
            let mapper = mapper_type.create(config.chain_length, config.parallel)?;
            let mapper = PixelMapper::Named(mapper);
            shared_mapper = Self::apply_pixel_mapper(&shared_mapper, &mapper, pixel_designator)?;
        }

        Ok(shared_mapper)
    }

    fn apply_pixel_mapper(
        shared_mapper: &PixelDesignatorMap,
        mapper: &PixelMapper,
        pixel_designator: PixelDesignator,
    ) -> Result<PixelDesignatorMap, MatrixCreationError> {
        let old_width = shared_mapper.width();
        let old_height = shared_mapper.height();
        let [new_width, new_height] = mapper.get_size_mapping(old_width, old_height)?;
        // Every pixel is copied from the old map below, so there is no need to compute the physical layout.
        let mut new_mapper =
            PixelDesignatorMap::new_unmapped(pixel_designator, new_width, new_height);
        for y in 0..new_height {
            for x in 0..new_width {
                let [orig_x, orig_y] = mapper.map_visible_to_matrix(old_width, old_height, x, y);
                if orig_x >= old_width || orig_y >= old_height {
                    return Err(MatrixCreationError::PixelMapperError(
                        "Invalid dimensions detected. This is likely a bug.".to_string(),
                    ));
                }
                let orig_designator = shared_mapper.get(orig_x, orig_y).unwrap();
                *new_mapper.get_mut(x, y).unwrap() = *orig_designator;
            }
        }
        Ok(new_mapper)
    }

    /// Updates the matrix with the new canvas. Blocks until the end of the current frame.
    /// This returns a cleared canvas that has to be drawn to before calling this function again.
    /// If this function is not called before the next frame update, the previous
    /// frame is drawn again.
    ///
    /// If the display is suspended, this resumes it and returns immediately.
    pub fn update_on_vsync(
        &mut self,
        canvas: Box<Canvas>,
    ) -> Result<Box<Canvas>, UpdateThreadError> {
        self.message_sender
            .send(ThreadMessage::Canvas(canvas))
            .map_err(|_| self.update_thread_error())?;
        self.suspended = false;

        let mut canvas = self
            .canvas_from_thread_receiver
            .recv()
            .map_err(|_| self.update_thread_error())?;

        canvas.set_brightness(self.brightness);
        canvas.set_pwm_bits(self.pwm_bits);
        // Clear the canvas before handing it back to the user.
        // There is no practical use for not clearing it here as we switch
        // back and forth between two canvases.
        canvas.fill(0, 0, 0);
        Ok(canvas)
    }

    /// Set the brightness in percent, clamped to 1-100.
    pub fn set_brightness(&mut self, brightness: u8) {
        self.brightness = brightness;
    }

    /// Set the PWM bits used for output, clamped to 1-11. Default is 11, but if you only deal with simple
    /// comic-colors, 1 might be sufficient. Lower values require less CPU.
    pub fn set_pwm_bits(&mut self, pwm_bits: usize) {
        self.pwm_bits = pwm_bits;
    }

    /// Blank the display and stop refreshing it until the next call to [`RGBMatrix::update_on_vsync`]. This
    /// frees up the CPU core used by the display update thread. Blocks until the end of the current frame.
    pub fn suspend(&mut self) -> Result<(), UpdateThreadError> {
        if !self.suspended {
            self.message_sender
                .send(ThreadMessage::Suspend)
                .map_err(|_| self.update_thread_error())?;
            self.suspended = true;
        }
        Ok(())
    }

    /// Why the update thread stopped. Joins it the first time and keeps the reason for later calls.
    fn update_thread_error(&mut self) -> UpdateThreadError {
        self.thread_error
            .get_or_insert_with(|| {
                let message = match self.thread_handle.take().map(JoinHandle::join) {
                    Some(Err(payload)) => panic_message(payload.as_ref()),
                    _ => String::from("It stopped without panicking."),
                };
                UpdateThreadError(message)
            })
            .clone()
    }

    /// Whether the display is currently suspended.
    #[must_use]
    pub fn is_suspended(&self) -> bool {
        self.suspended
    }

    /// Get the bits that were available for input.
    #[must_use]
    pub fn enabled_input_bits(&self) -> u32 {
        self.enabled_input_bits
    }

    /// Tries to receive a new GPIO input as requested with the `requested_inputs` argument of [`RGBMatrix::new`].
    /// Returns `None` if the inputs didn't change before the timeout.
    pub fn receive_new_inputs(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<u32>, UpdateThreadError> {
        match self.input_receiver.recv_timeout(timeout) {
            Ok(inputs) => Ok(Some(inputs)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(self.update_thread_error()),
        }
    }

    /// Get the average frame rate over the last 60 frames.
    #[must_use]
    pub fn get_framerate(&self) -> usize {
        self.framerate.load(Ordering::Relaxed) as usize
    }
}

impl Drop for RGBMatrix {
    fn drop(&mut self) {
        if let Some(handle) = self.thread_handle.take() {
            self.message_sender.send(ThreadMessage::Shutdown).ok();
            handle.join().ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        sync::Arc,
        sync::mpsc::{channel, sync_channel},
        thread::spawn,
        time::Duration,
    };

    use strum::VariantNames;

    use super::{
        MatrixCreationError, PixelDesignator, PixelDesignatorMap, PixelMapper, RGBMatrix,
        panic_message,
    };
    use crate::{MultiplexMapperType, NamedPixelMapperType, RGBMatrixConfig};

    #[test]
    fn test_panic_message() {
        let literal = std::thread::spawn(|| panic!("literal")).join().unwrap_err();
        assert_eq!(panic_message(literal.as_ref()), "literal");
        let formatted = std::thread::spawn(|| panic!("formatted {}", 1))
            .join()
            .unwrap_err();
        assert_eq!(panic_message(formatted.as_ref()), "formatted 1");
    }

    #[test]
    fn test_receive_inputs_after_panic() {
        // A stand-in for the update thread that sends one input and panics when told to.
        let (input_sender, input_receiver) = channel();
        let (panic_sender, panic_receiver) = channel();
        let thread_handle = spawn(move || {
            input_sender.send(1).unwrap();
            panic_receiver.recv().unwrap();
            panic!("update failed");
        });
        let (message_sender, _message_receiver) = sync_channel(0);
        let (_canvas_sender, canvas_from_thread_receiver) = sync_channel(1);
        let mut matrix = RGBMatrix {
            thread_handle: Some(thread_handle),
            thread_error: None,
            input_receiver,
            message_sender,
            canvas_from_thread_receiver,
            enabled_input_bits: 1,
            framerate: Arc::default(),
            suspended: false,
            brightness: 100,
            pwm_bits: 11,
        };
        let timeout = Duration::from_millis(10);

        assert_eq!(matrix.receive_new_inputs(timeout).unwrap(), Some(1));
        assert_eq!(matrix.receive_new_inputs(timeout).unwrap(), None);
        panic_sender.send(()).unwrap();
        let error = matrix.receive_new_inputs(timeout).unwrap_err();
        assert!(error.to_string().ends_with("update failed"), "{error}");
    }

    #[test]
    fn test_rotate_long_chain() {
        // Four 64x32 panels in one chain, rotated to be taller than the number of supported parallel chains.
        let config = RGBMatrixConfig {
            rows: 32,
            cols: 64,
            chain_length: 4,
            ..Default::default()
        };
        let pixel_designator = PixelDesignator::new(&config.hardware_mapping, config.led_sequence);
        let map = PixelDesignatorMap::new(pixel_designator, 256, 32, &config);
        let mapper = PixelMapper::Named(NamedPixelMapperType::Rotate(90).create(4, 1).unwrap());

        let rotated = RGBMatrix::apply_pixel_mapper(&map, &mapper, pixel_designator).unwrap();

        assert_eq!((rotated.width(), rotated.height()), (32, 256));
        assert_eq!(rotated.get(0, 0), map.get(255, 0));
        assert_eq!(rotated.get(31, 255), map.get(0, 31));
    }

    #[test]
    fn test_multiplex_mappers() {
        // Every panel size that a mapper accepts needs to light a different LED for each pixel.
        for name in MultiplexMapperType::VARIANTS {
            let multiplexing: MultiplexMapperType = name.parse().unwrap();
            let mut accepted_sizes = 0;
            for rows in (2..=48).step_by(2) {
                for cols in 1..=48 {
                    let mut config = RGBMatrixConfig {
                        rows,
                        cols,
                        chain_length: 2,
                        parallel: 2,
                        multiplexing: Some(multiplexing),
                        ..Default::default()
                    };
                    let panel = format!("{name} with {rows}x{cols} panels");
                    let map = match RGBMatrix::create_pixel_map(&mut config) {
                        Err(MatrixCreationError::InvalidConfig(_)) => continue,
                        result => result.unwrap_or_else(|error| panic!("{panel}: {error}")),
                    };
                    accepted_sizes += 1;

                    assert_eq!((map.width(), map.height()), (2 * cols, 2 * rows), "{panel}");
                    assert_eq!(distinct_leds(&map), 4 * rows * cols, "{panel}");
                }
            }
            assert!(accepted_sizes > 0, "{name} accepts no panel size");
        }
    }

    #[test]
    fn test_named_pixel_mappers() {
        // Rearranging the panels needs to keep a different LED for each pixel.
        for name in [
            "Mirror:H",
            "Mirror:V",
            "Rotate:90",
            "Rotate:180",
            "Rotate:270",
            "U-mapper",
            "ChainLink",
        ] {
            let mapper: NamedPixelMapperType = name.parse().unwrap();
            for (rows, cols) in [(16, 32), (32, 32), (32, 48), (32, 64), (40, 80), (64, 64)] {
                for (chain_length, parallel) in [(2, 1), (4, 1), (2, 3), (6, 2)] {
                    let mut config = RGBMatrixConfig {
                        rows,
                        cols,
                        chain_length,
                        parallel,
                        pixelmapper: vec![mapper],
                        ..Default::default()
                    };
                    let panels =
                        format!("{name} with {chain_length}x{parallel} {rows}x{cols} panels");
                    let map = RGBMatrix::create_pixel_map(&mut config)
                        .unwrap_or_else(|error| panic!("{panels}: {error}"));
                    let leds = rows * cols * chain_length * parallel;
                    assert_eq!(map.width() * map.height(), leds, "{panels}");
                    assert_eq!(distinct_leds(&map), leds, "{panels}");
                }
            }
        }
    }

    /// How many different LEDs the pixels of the map light up.
    fn distinct_leds(map: &PixelDesignatorMap) -> usize {
        let designators: HashSet<_> = (0..map.height())
            .flat_map(|y| (0..map.width()).map(move |x| (x, y)))
            .map(|(x, y)| *map.get(x, y).unwrap())
            .collect();
        designators.len()
    }
}

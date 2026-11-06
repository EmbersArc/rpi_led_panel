use log::warn;
use std::{
    error::Error,
    fmt::{Debug, Display, Formatter},
    fs::{File, write},
    io::{self, Write},
    mem::replace,
    sync::atomic::{AtomicU32, Ordering},
    sync::mpsc::{
        Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, channel, sync_channel,
    },
    sync::{Arc, Mutex},
    thread::{Builder, JoinHandle, sleep},
    time::{Duration, Instant},
};
use thread_priority::{
    RealtimeThreadSchedulePolicy, ThreadPriority, ThreadSchedulePolicy,
    set_current_thread_priority, set_thread_priority_and_policy, thread_native_id,
};

use crate::{
    RGBMatrixConfig,
    backend::Backend,
    canvas::{Canvas, PixelDesignator, PixelDesignatorMap},
    chip::PiChip,
    config::{BIT_PLANES, MAX_REFRESH_RATE},
    gpio::GpioInitializationError,
    inputs::{InputReader, input_channel},
    pin_pulser::uses_hardware_pulse,
    pixel_mapper::{MappingDirection, PixelMapper},
    registers::{STORE_BARRIER_SUPPORTED, UserGpioOutputs, open_dev_mem},
    row_address_setter::RowAddressSetterType,
    rp1::is_pi5_model,
    utils::{FrameRateMonitor, linux_has_isol_cpu, set_thread_affinity, sleep_at_most},
};

/// How often GPIO inputs and requests are polled while the display is suspended.
const SUSPENDED_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// The name of the thread that refreshes the panels, as shown by e.g. `top -H`.
const REFRESH_THREAD_NAME: &str = "led-refresh";

fn initialize_update_thread(chip: PiChip, realtime_priority: Option<u8>) {
    // Pin the thread to the last core to avoid the flicker resulting from context switching.
    let last_core_id = chip.num_cores() - 1;
    if !set_thread_affinity(last_core_id) {
        warn!("Could not pin the display update thread to core {last_core_id}.");
    }

    // If the user has not set up isolcpus, let them know about the performance improvement.
    if chip.num_cores() > 1 && !linux_has_isol_cpu(last_core_id) {
        warn!(
            "Suggestion: to slightly improve display update, add\n\tisolcpus={last_core_id}\nat \
            the end of /boot/firmware/cmdline.txt (/boot/cmdline.txt on older systems) and reboot"
        );
    }

    // Set the last core to performance mode.
    if chip.num_cores() > 1
        && write(
            format!("/sys/devices/system/cpu/cpu{last_core_id}/cpufreq/scaling_governor"),
            "performance",
        )
        .is_err()
    {
        warn!("Could not set core {last_core_id} to performance mode.");
    }

    if let Some(priority) = realtime_priority
        && set_realtime_priority(chip, priority)
    {
        return;
    }

    // The highest priority of the normal scheduling policy, which is nice -20 on Linux.
    if set_current_thread_priority(ThreadPriority::Max).is_err() {
        warn!(
            "Could not raise the priority of the display update thread (nice -20). This might lead to reduced \
            performance."
        );
    }
}

/// Switches the current thread to the real-time SCHED_FIFO policy. Returns `true` on success.
fn set_realtime_priority(chip: PiChip, priority: u8) -> bool {
    // Real-time threads are throttled to 95% of the time by default, which shows as flicker. Leave 1 ms per
    // second for the rest of the system, like upstream. This only matters for real-time threads.
    if chip.num_cores() > 1 && write("/proc/sys/kernel/sched_rt_runtime_us", "999000").is_err() {
        warn!("Could not reduce the real-time throttling (/proc/sys/kernel/sched_rt_runtime_us).");
    }
    let result = ThreadPriority::try_from(priority)
        .map_err(|e| e.to_string())
        .and_then(|priority| {
            set_thread_priority_and_policy(
                thread_native_id(),
                priority,
                ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::Fifo),
            )
            .map_err(|e| format!("{e:?}"))
        });
    if let Err(error) = result {
        warn!(
            "Could not switch the display update thread to SCHED_FIFO priority {priority} ({error}), using the \
            normal policy."
        );
        return false;
    }
    true
}

/// An error returned by [`RGBMatrix::new`].
#[derive(Debug)]
#[non_exhaustive]
pub enum MatrixCreationError {
    /// The Raspberry Pi model could not be determined. It can be set with [`RGBMatrixConfig::pi_chip`] instead.
    ChipDeterminationError,
    /// The display update thread did not finish its setup in time.
    ThreadTimedOut,
    /// The display update thread panicked or could not be started during its setup, with the given message.
    ThreadPanicked(String),
    /// An option of the config is invalid, as described by the message.
    InvalidConfig(String),
    /// The GPIO pins could not be set up.
    GpioError(GpioInitializationError),
    /// The memory of the peripherals could not be accessed, which needs root privileges.
    MemoryAccessError(io::Error),
    /// A pixel mapper can't be used with the panels, as described by the message.
    PixelMapperError(String),
}

impl Error for MatrixCreationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            // The GPIO error is displayed as is, so its source comes next in the chain.
            MatrixCreationError::GpioError(error) => error.source(),
            MatrixCreationError::MemoryAccessError(error) => Some(error),
            _ => None,
        }
    }
}

impl Display for MatrixCreationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            MatrixCreationError::ChipDeterminationError => {
                f.write_str("Failed to automatically determine Raspberry Pi model.")
            }
            MatrixCreationError::ThreadTimedOut => {
                f.write_str("The update thread did not return in time.")
            }
            MatrixCreationError::ThreadPanicked(message) => {
                write!(
                    f,
                    "The update thread failed during initialization: {message}"
                )
            }
            MatrixCreationError::InvalidConfig(message) => {
                write!(f, "Invalid configuration: {message}")
            }
            MatrixCreationError::GpioError(error) => Display::fmt(error, f),
            MatrixCreationError::MemoryAccessError(error) => {
                f.write_str("Failed to access the physical memory.")?;
                if error.kind() == io::ErrorKind::PermissionDenied {
                    f.write_str(" Not running with root privileges?")?;
                }
                Ok(())
            }
            MatrixCreationError::PixelMapperError(message) => {
                write!(f, "Error in pixel mapper: {message}")
            }
        }
    }
}

/// The display update thread stopped while the matrix was in use, e.g. because it panicked or the Raspberry Pi 5
/// PIO driver failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateThreadError(pub(crate) String);

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

/// Extracts the message of a panic.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(ToString::to_string)
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string())
}

/// Requests from the user to the display update thread.
enum Control {
    Shutdown,
    /// Blanks the display and stops refreshing it until the next canvas.
    Suspend,
    /// Turns the pins into outputs and replies with the ones that were available.
    RequestOutputs(u32, Sender<u32>),
}

/// The controller of the display. A separate thread keeps refreshing the display with the latest canvas, and
/// dropping the controller stops that thread and turns the display off.
pub struct RGBMatrix {
    /// The join handle of the update thread.
    thread_handle: Option<JoinHandle<()>>,
    /// Requests to the update thread.
    control_sender: Sender<Control>,
    /// Reader for GPIO inputs, until it is taken with [`RGBMatrix::take_input_reader`].
    input_reader: Option<InputReader>,
    /// Channel to send canvas to update thread, with the number of frames to show the previous canvas for.
    canvas_to_thread_sender: SyncSender<(Canvas, u32)>,
    /// Channel to receive canvas from update thread.
    canvas_from_thread_receiver: Receiver<Canvas>,
    /// Additional requested inputs that can be received.
    enabled_input_bits: u32,
    /// The chip, to map the registers for the user's outputs.
    chip: PiChip,
    /// `/dev/mem`, opened before the privileges are dropped, to map the user's outputs when they are requested.
    /// Not available on the Raspberry Pi 5.
    dev_mem: Option<File>,
    /// The registers to write the outputs of the user, once requested.
    user_outputs: Option<UserGpioOutputs>,
    /// The pins that the user can write to.
    enabled_output_bits: u32,
    /// Frame rate measurement.
    framerate: Arc<AtomicU32>,
    /// Why the update thread stopped, if it did.
    thread_error: Arc<Mutex<Option<String>>>,
    /// Whether the canvases that are handed back are cleared.
    clear_on_vsync: bool,
    /// Brightness in percent for the canvases returned by `update_on_vsync`.
    brightness: u8,
    /// PWM bits for the canvases returned by `update_on_vsync`.
    pwm_bits: usize,
    /// Whether the display is currently suspended.
    suspended: bool,
}

impl Debug for RGBMatrix {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RGBMatrix")
            .field("enabled_input_bits", &self.enabled_input_bits)
            .field("enabled_output_bits", &self.enabled_output_bits)
            .field("framerate", &self.framerate())
            .field("brightness", &self.brightness)
            .field("pwm_bits", &self.pwm_bits)
            .field("suspended", &self.suspended)
            .finish_non_exhaustive()
    }
}

impl RGBMatrix {
    /// Create a new RGB matrix controller. This starts a new thread to update the matrix. Returns the
    /// controller and a canvas for drawing.
    ///
    /// You can additionally request user readable GPIO bits with [`RGBMatrixConfig::input_bits`], which can
    /// later be read with [`RGBMatrix::take_input_reader`]. Only bits that are not already in use for
    /// reading or writing by the matrix are allowed. Use [`RGBMatrix::enabled_input_bits`] after calling this
    /// function to check which bits were actually available. GPIO inputs are not supported on the Raspberry
    /// Pi 5.
    ///
    /// Note: If privilege dropping is enabled in the config, then calling this function will have the side
    /// effect of dropping the application's privileges down to the configured user/group.
    pub fn new(mut config: RGBMatrixConfig) -> Result<(Self, Canvas), MatrixCreationError> {
        let chip = if let Some(chip) = config.pi_chip {
            chip
        } else {
            PiChip::determine().ok_or(MatrixCreationError::ChipDeterminationError)?
        };
        let uses_rp1 = chip.uses_rp1() || is_pi5_model();

        // Check if we can access the memory before doing anything else. The Raspberry Pi 5 doesn't need it. It
        // stays open for the user's outputs, as the update thread drops the privileges.
        let dev_mem = if uses_rp1 {
            None
        } else {
            Some(open_dev_mem().map_err(MatrixCreationError::MemoryAccessError)?)
        };

        Self::validate_config(&config)?;
        Self::validate_for_backend(&config, uses_rp1)?;

        let shared_mapper = Self::create_pixel_map(&mut config)?;

        let dither_start_bits = match config.dither_bits {
            0 => [0, 0, 0, 0],
            1 => [0, 1, 0, 1],
            2 => [0, 1, 2, 2],
            _ => unreachable!("dither_bits is checked in validate_config"),
        };

        // Create two canvases, one for the display update thread and one for the user to modify. They will be
        // swapped out after each frame.
        let canvas = Canvas::new(&config, Arc::new(shared_mapper));
        let mut thread_canvas = canvas.clone();

        let (canvas_to_thread_sender, canvas_to_thread_receiver) = sync_channel::<(Canvas, u32)>(0);
        let (canvas_from_thread_sender, canvas_from_thread_receiver) = sync_channel::<Canvas>(1);
        let (control_sender, control_receiver) = channel::<Control>();
        let thread_error = Arc::new(Mutex::new(None));
        let (mut input_writer, input_reader) = input_channel(Arc::clone(&thread_error));
        let (thread_start_result_sender, thread_start_result_receiver) =
            channel::<Result<u32, MatrixCreationError>>();
        let framerate = Arc::new(AtomicU32::new(0));
        let mut frame_rate_monitor = FrameRateMonitor::new(framerate.clone());
        let thread_error_sender = Arc::clone(&thread_error);
        let clear_on_vsync = config.clear_on_vsync;
        let (brightness, pwm_bits) = (config.led_brightness, config.pwm_bits);

        let spawn_result = Builder::new()
            .name(REFRESH_THREAD_NAME.to_string())
            .spawn(move || {
                initialize_update_thread(chip, config.realtime_priority);

                let mut backend = match Backend::new(chip, &config) {
                    Ok(backend) => backend,
                    Err(error) => {
                        // If sending fails, the main thread has already given up waiting.
                        thread_start_result_sender.send(Err(error)).ok();
                        return;
                    }
                };

                // Run the initialization sequence if necessary.
                if let Some(panel_type) = config.panel_type
                    && let Err(error) = backend.init_panels(&panel_type.init(&config))
                {
                    thread_start_result_sender.send(Err(error)).ok();
                    return;
                }

                // Dither sequence
                let mut dither_low_bit_sequence = 0;

                // `None` refreshes as fast as possible.
                let frame_time_target = config.frame_time_target();
                let min_sys_sleep_time = Duration::from_micros(config.min_sys_sleep_microseconds);

                // The canvas to show next, and for how many frames to show the current one before.
                let mut next_canvas: Option<Canvas> = None;
                let mut frame_multiple = 1;
                let mut frame_count: u32 = 0;
                let mut suspended = false;
                let mut refresh_display = config.show_refresh.then(RefreshRateDisplay::new);

                let enabled_input_bits = backend.request_inputs(config.input_bits);
                input_writer.set_initial(backend.read_inputs());
                if thread_start_result_sender
                    .send(Ok(enabled_input_bits))
                    .is_err()
                {
                    // The main thread has already given up waiting.
                    return;
                }

                'thread: loop {
                    let start_time = Instant::now();
                    loop {
                        match control_receiver.try_recv() {
                            Ok(Control::RequestOutputs(bits, reply)) => {
                                reply.send(backend.request_outputs(bits)).ok();
                            }
                            Ok(Control::Suspend) => {
                                // Draw a blank frame so that no row stays lit, but keep the current canvas
                                // for when the display is resumed.
                                let mut blank_canvas = thread_canvas.clone();
                                blank_canvas.fill(0, 0, 0);
                                if let Err(error) = backend.dump(&blank_canvas, 0) {
                                    *thread_error_sender
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner()) =
                                        Some(error.to_string());
                                    return;
                                }
                                frame_rate_monitor.pause();
                                suspended = true;
                            }
                            Ok(Control::Shutdown) | Err(TryRecvError::Disconnected) => {
                                break 'thread;
                            }
                            Err(TryRecvError::Empty) => break,
                        }
                    }
                    // Read input bits and publish them if they have changed.
                    input_writer.update(backend.read_inputs());
                    // Check for a swap canvas. While suspended, wait for one but keep polling the inputs.
                    if next_canvas.is_none() {
                        let received = if suspended {
                            canvas_to_thread_receiver
                                .recv_timeout(SUSPENDED_POLL_INTERVAL)
                                .map_err(|error| match error {
                                    RecvTimeoutError::Timeout => TryRecvError::Empty,
                                    RecvTimeoutError::Disconnected => TryRecvError::Disconnected,
                                })
                        } else {
                            canvas_to_thread_receiver.try_recv()
                        };
                        match received {
                            Ok((new_canvas, multiple)) => {
                                next_canvas = Some(new_canvas);
                                frame_multiple = multiple.max(1);
                                if suspended {
                                    // Show the first canvas after resuming right away.
                                    suspended = false;
                                    frame_count = 0;
                                }
                            }
                            Err(TryRecvError::Disconnected) => {
                                break 'thread;
                            }
                            Err(TryRecvError::Empty) => {}
                        }
                    }
                    if suspended {
                        continue;
                    }
                    // Only swap after the requested number of frames.
                    if frame_count.is_multiple_of(frame_multiple) {
                        frame_count = 0;
                        if let Some(new_canvas) = next_canvas.take() {
                            let old_canvas = replace(&mut thread_canvas, new_canvas);
                            let Ok(()) = canvas_from_thread_sender.send(old_canvas) else {
                                break 'thread;
                            };
                        }
                    }

                    if let Err(error) = backend.dump(
                        &thread_canvas,
                        dither_start_bits[dither_low_bit_sequence % dither_start_bits.len()],
                    ) {
                        // Keep the reason for the user, who gets an error on the next update.
                        *thread_error_sender
                            .lock()
                            .unwrap_or_else(|e| e.into_inner()) = Some(error.to_string());
                        return;
                    }
                    dither_low_bit_sequence += 1;
                    frame_count += 1;

                    frame_rate_monitor.update();

                    // Wait for the rest of the frame.
                    if let Some(frame_time_target) = frame_time_target {
                        let end_time = start_time + frame_time_target;
                        if config.busy_waiting {
                            sleep_at_most(
                                end_time.saturating_duration_since(Instant::now()),
                                min_sys_sleep_time,
                            );
                            while Instant::now() < end_time {
                                std::hint::spin_loop();
                            }
                        } else {
                            sleep(end_time.saturating_duration_since(Instant::now()));
                        }
                    }

                    if let Some(display) = refresh_display.as_mut() {
                        display.update(start_time.elapsed());
                    }
                }

                // Turn it off. Dropping the backend afterwards releases the pins.
                thread_canvas.fill(0, 0, 0);
                let _ = backend.dump(&thread_canvas, 0);
            });
        let thread_handle = spawn_result.map_err(|error| {
            MatrixCreationError::ThreadPanicked(format!("could not start the thread: {error}"))
        })?;

        let enabled_input_bits =
            match thread_start_result_receiver.recv_timeout(Duration::from_secs(10)) {
                Ok(result) => result?,
                Err(RecvTimeoutError::Timeout) => return Err(MatrixCreationError::ThreadTimedOut),
                Err(RecvTimeoutError::Disconnected) => {
                    // The thread exited without reporting back, so it must have panicked.
                    let message = match thread_handle.join() {
                        Err(payload) => panic_message(payload.as_ref()),
                        Ok(()) => "thread exited unexpectedly".to_string(),
                    };
                    return Err(MatrixCreationError::ThreadPanicked(message));
                }
            };

        let rgbmatrix = Self {
            thread_handle: Some(thread_handle),
            control_sender,
            input_reader: Some(input_reader),
            canvas_to_thread_sender,
            canvas_from_thread_receiver,
            enabled_input_bits,
            chip,
            dev_mem,
            user_outputs: None,
            enabled_output_bits: 0,
            framerate,
            thread_error,
            clear_on_vsync,
            brightness,
            pwm_bits,
            suspended: false,
        };

        Ok((rgbmatrix, canvas))
    }

    /// Creates the map from visible pixels to the bit plane buffer. Multiplexers might change the physical
    /// layout, so this updates the rows and columns in the config before the hardware is set up.
    pub(crate) fn create_pixel_map(
        config: &mut RGBMatrixConfig,
    ) -> Result<PixelDesignatorMap, MatrixCreationError> {
        let multiplex_mapper = config
            .multiplexing
            .as_ref()
            .map(|mapper_type| {
                let mut mapper = mapper_type.create();
                mapper.edit_rows_cols(&mut config.rows, &mut config.cols)?;
                Ok::<_, MatrixCreationError>(mapper)
            })
            .transpose()?;
        // The multiplexer changes the electrical layout, which has to be supported as well.
        Self::validate_rows(config.rows)?;

        let pixel_designator = PixelDesignator::new(&config.hardware_mapping, config.led_sequence);
        let matrix = PixelDesignatorMap::new(pixel_designator, config);
        let mut shared_mapper = matrix.clone();

        // Apply the mapping for the panels first.
        let multiplex_mapper_applied = multiplex_mapper.is_some();
        if let Some(mapper) = multiplex_mapper {
            shared_mapper = Self::apply_pixel_mapper(&shared_mapper, &mapper, pixel_designator)?;
        }

        // Apply higher level mappers that might arrange panels.
        for mapper_type in config.pixelmapper.iter() {
            let mapper = mapper_type.create(config.chain_length, config.parallel)?;
            shared_mapper =
                Self::apply_pixel_mapper(&shared_mapper, mapper.as_ref(), pixel_designator)?;
        }
        if multiplex_mapper_applied || !config.pixelmapper.is_empty() {
            shared_mapper.find_hidden_bits(&matrix);
        }
        Ok(shared_mapper)
    }

    /// Rejects configurations that would otherwise lead to panics or a blank display.
    fn validate_config(config: &RGBMatrixConfig) -> Result<(), MatrixCreationError> {
        let invalid = |message: String| Err(MatrixCreationError::InvalidConfig(message));
        if config.cols == 0 || config.chain_length == 0 || config.parallel == 0 {
            return invalid(format!(
                "cols ({}), chain_length ({}) and parallel ({}) must be at least 1.",
                config.cols, config.chain_length, config.parallel
            ));
        }
        let max_parallel = config.hardware_mapping.max_parallel_chains();
        if config.parallel > max_parallel {
            return invalid(format!(
                "parallel must be at most {max_parallel} for this hardware mapping, got {}.",
                config.parallel
            ));
        }
        Self::validate_rows(config.rows)?;
        if !(1..=100).contains(&config.led_brightness) {
            return invalid(format!(
                "led_brightness must be between 1 and 100, got {}.",
                config.led_brightness
            ));
        }
        if config.dither_bits > 2 {
            return invalid(format!(
                "dither_bits must be at most 2, got {}.",
                config.dither_bits
            ));
        }
        if !(1..=BIT_PLANES).contains(&config.pwm_bits) {
            return invalid(format!(
                "pwm_bits must be between 1 and {BIT_PLANES}, got {}.",
                config.pwm_bits
            ));
        }
        // The on-time doubles for each of the bit planes after the lowest, which must not overflow.
        let max_lsb_nanoseconds = u32::MAX >> (BIT_PLANES - 1);
        if !(1..=max_lsb_nanoseconds).contains(&config.pwm_lsb_nanoseconds) {
            return invalid(format!(
                "pwm_lsb_nanoseconds must be between 1 and {max_lsb_nanoseconds}, got {}.",
                config.pwm_lsb_nanoseconds
            ));
        }
        // The rate is ignored without a cap, but is still checked so that turning it on later can't fail.
        if !(1..=MAX_REFRESH_RATE).contains(&config.refresh_rate) {
            return invalid(format!(
                "refresh_rate must be between 1 and {MAX_REFRESH_RATE}, got {}.",
                config.refresh_rate
            ));
        }
        if let Some(priority) = config.realtime_priority
            && !(1..=99).contains(&priority)
        {
            return invalid(format!(
                "realtime_priority must be between 1 and 99, got {priority}."
            ));
        }
        if let Some(slowdown) = config.slowdown {
            if !(-1..=60).contains(&slowdown) {
                return invalid(format!(
                    "slowdown must be between -1 and 60, got {slowdown}."
                ));
            }
            if slowdown == -1 && !STORE_BARRIER_SUPPORTED {
                return invalid("slowdown -1 needs an ARMv7 or newer processor.".to_string());
            }
        }
        // The B707 shift register advances by one row with every address, so the rows have to be in order.
        if config.interlaced && config.row_setter == RowAddressSetterType::B707ShiftRegister {
            return invalid(
                "The B707ShiftRegister row setter does not support interlacing.".to_string(),
            );
        }
        Ok(())
    }

    /// Checks the limits of the hardware that drives the panels.
    fn validate_for_backend(
        config: &RGBMatrixConfig,
        uses_rp1: bool,
    ) -> Result<(), MatrixCreationError> {
        // The PWM clock divider of the BCM hardware pulses is `pwm_lsb_nanoseconds / 4` and needs to fit in 12
        // bits. The software pulses and the Raspberry Pi 5 don't have this limit.
        if !uses_rp1
            && uses_hardware_pulse(config)
            && !(4..16384).contains(&config.pwm_lsb_nanoseconds)
        {
            return Err(MatrixCreationError::InvalidConfig(format!(
                "pwm_lsb_nanoseconds must be between 4 and 16383 with hardware pulses, got {}. Use a wiring \
                 without hardware pulses or set hardware_pulse to false for other values.",
                config.pwm_lsb_nanoseconds
            )));
        }
        Ok(())
    }

    /// The row address setters support up to 32 double rows.
    fn validate_rows(rows: usize) -> Result<(), MatrixCreationError> {
        if !(2..=64).contains(&rows) || !rows.is_multiple_of(2) {
            return Err(MatrixCreationError::InvalidConfig(format!(
                "rows must be an even number between 2 and 64, got {rows}."
            )));
        }
        Ok(())
    }

    fn apply_pixel_mapper(
        shared_mapper: &PixelDesignatorMap,
        mapper: &dyn PixelMapper,
        pixel_designator: PixelDesignator,
    ) -> Result<PixelDesignatorMap, MatrixCreationError> {
        let old_width = shared_mapper.width();
        let old_height = shared_mapper.height();
        let [new_width, new_height] = mapper.size_mapping(old_width, old_height)?;
        if new_width == 0 || new_height == 0 {
            return Err(MatrixCreationError::PixelMapperError(format!(
                "The pixel mapper turns the {old_width}x{old_height} matrix into an empty canvas."
            )));
        }
        let mut new_mapper = PixelDesignatorMap::blank(pixel_designator, new_width, new_height);
        let invalid_dimensions = || {
            MatrixCreationError::PixelMapperError(
                "Invalid dimensions detected. This is likely a bug.".to_string(),
            )
        };
        match mapper.mapping_direction() {
            MappingDirection::VisibleToMatrix => {
                for y in 0..new_height {
                    for x in 0..new_width {
                        let [orig_x, orig_y] = mapper
                            .map_visible_to_matrix(old_width, old_height, x, y)
                            .ok_or_else(|| {
                                MatrixCreationError::PixelMapperError(format!(
                                    "The pixel mapper can't map pixel ({x}, {y}) of the \
                                     {new_width}x{new_height} canvas. The panel size might not be \
                                     supported."
                                ))
                            })?;
                        let orig_designator = shared_mapper
                            .get(orig_x, orig_y)
                            .ok_or_else(invalid_dimensions)?;
                        *new_mapper.get_mut(x, y).unwrap() = *orig_designator;
                    }
                }
            }
            MappingDirection::MatrixToVisible => {
                let mut collision_reported = false;
                for y in 0..old_height {
                    for x in 0..old_width {
                        let Some([new_x, new_y]) =
                            mapper.map_matrix_to_visible(old_width, old_height, x, y)
                        else {
                            continue;
                        };
                        let new_designator = new_mapper
                            .get_mut(new_x, new_y)
                            .ok_or_else(invalid_dimensions)?;
                        if new_designator.is_used() && !collision_reported {
                            warn!(
                                "Warning: pixel mapper maps more than one pixel to ({new_x}, {new_y})."
                            );
                            collision_reported = true;
                        }
                        *new_designator = *shared_mapper.get(x, y).unwrap();
                    }
                }
            }
        }
        Ok(new_mapper)
    }

    /// Updates the matrix with the new canvas. Blocks until the end of the current frame.
    /// This returns a cleared canvas that has to be drawn to before calling this function again
    /// (unless `clear_on_vsync` is turned off in the config).
    /// If this function is not called before the next frame update, the previous
    /// frame is drawn again.
    ///
    /// The returned canvas has the brightness and PWM bits of the matrix, and the luminance correction of the one
    /// passed in.
    ///
    /// If the display is suspended, this resumes it and returns immediately.
    ///
    /// Fails if the display update thread has stopped, e.g. because the Raspberry Pi 5 PIO driver failed. The
    /// matrix can't show anything after that.
    pub fn update_on_vsync(&mut self, canvas: Canvas) -> Result<Canvas, UpdateThreadError> {
        self.update_on_vsync_every(canvas, 1)
    }

    /// Like [`RGBMatrix::update_on_vsync`], but the current canvas is shown for a multiple of `frames` frames
    /// before the new one. This gives a stable frame rate that is a fraction of the refresh rate, e.g. for
    /// animations.
    pub fn update_on_vsync_every(
        &mut self,
        canvas: Canvas,
        frames: u32,
    ) -> Result<Canvas, UpdateThreadError> {
        let luminance_correct = canvas.luminance_correct();

        if self.canvas_to_thread_sender.send((canvas, frames)).is_err() {
            return Err(self.update_thread_error());
        }
        self.suspended = false;
        let Ok(mut canvas) = self.canvas_from_thread_receiver.recv() else {
            return Err(self.update_thread_error());
        };

        // Both canvases need the same settings, otherwise they would alternate between frames.
        canvas.set_brightness(self.brightness);
        canvas.set_pwm_bits(self.pwm_bits);
        canvas.set_luminance_correct(luminance_correct);

        // Clear the canvas before handing it back to the user, unless they redraw every pixel anyway. Without
        // clearing, it holds the frame before the one that was just passed in, as the two canvases alternate.
        if self.clear_on_vsync {
            canvas.fill(0, 0, 0);
        }
        Ok(canvas)
    }

    /// Set the brightness in percent, clamped to 1-100. This applies to the canvases returned by
    /// [`RGBMatrix::update_on_vsync`] from now on, not to the one that is currently being drawn to. Only affects
    /// pixels drawn afterwards.
    pub fn set_brightness(&mut self, brightness: u8) {
        self.brightness = brightness.clamp(1, 100);
    }

    /// The brightness in percent that the canvases returned by [`RGBMatrix::update_on_vsync`] get.
    #[must_use]
    pub fn brightness(&self) -> u8 {
        self.brightness
    }

    /// Set the PWM bits used for output, clamped to 1-11. Default is 11, but if you only deal with simple
    /// comic-colors, 1 might be sufficient. Lower values require less CPU. This applies to the canvases returned
    /// by [`RGBMatrix::update_on_vsync`] from now on, not to the one that is currently being drawn to.
    pub fn set_pwm_bits(&mut self, pwm_bits: usize) {
        self.pwm_bits = pwm_bits.clamp(1, BIT_PLANES);
    }

    /// The PWM bits that the canvases returned by [`RGBMatrix::update_on_vsync`] get.
    #[must_use]
    pub fn pwm_bits(&self) -> usize {
        self.pwm_bits
    }

    /// Blank the display and stop refreshing it until the next call to [`RGBMatrix::update_on_vsync`]. This
    /// frees up the CPU core used by the display update thread. GPIO inputs are still polled meanwhile.
    pub fn suspend(&mut self) -> Result<(), UpdateThreadError> {
        if !self.suspended {
            self.control_sender
                .send(Control::Suspend)
                .map_err(|_| self.update_thread_error())?;
            self.suspended = true;
        }
        Ok(())
    }

    /// Whether the display is currently suspended.
    #[must_use]
    pub fn is_suspended(&self) -> bool {
        self.suspended
    }

    /// Why the update thread stopped. Waits for it to finish if it is still shutting down.
    fn update_thread_error(&mut self) -> UpdateThreadError {
        let mut thread_error = self.thread_error.lock().unwrap_or_else(|e| e.into_inner());
        if thread_error.is_none()
            && let Some(handle) = self.thread_handle.take()
        {
            *thread_error = Some(match handle.join() {
                Err(payload) => format!("it panicked: {}", panic_message(payload.as_ref())),
                Ok(()) => "it exited unexpectedly".to_string(),
            });
        }
        UpdateThreadError(
            thread_error
                .clone()
                .unwrap_or_else(|| "unknown reason".to_string()),
        )
    }

    /// Get the bits that were available for input.
    #[must_use]
    pub fn enabled_input_bits(&self) -> u32 {
        self.enabled_input_bits
    }

    /// Takes the reader for the GPIO inputs requested with [`RGBMatrixConfig::input_bits`]. The reader can be
    /// moved to another thread. Returns `None` if it was already taken.
    pub fn take_input_reader(&mut self) -> Option<InputReader> {
        self.input_reader.take()
    }

    /// Turns GPIO pins that are not used by the matrix into outputs, which can be set with
    /// [`RGBMatrix::output_gpio`]. Returns the pins that were available. GPIO outputs are not supported on the
    /// Raspberry Pi 5. Returns 0 if the registers can't be mapped, see [`RGBMatrix::try_request_outputs`].
    pub fn request_outputs(&mut self, bits: u32) -> u32 {
        self.try_request_outputs(bits).unwrap_or(0)
    }

    /// Like [`RGBMatrix::request_outputs`], but fails with [`MatrixCreationError::MemoryAccessError`] if the GPIO
    /// registers for the outputs can't be mapped. They are only mapped on the first request.
    pub fn try_request_outputs(&mut self, bits: u32) -> Result<u32, MatrixCreationError> {
        let Some(dev_mem) = &self.dev_mem else {
            return Ok(0);
        };
        if self.user_outputs.is_none() {
            self.user_outputs = Some(
                UserGpioOutputs::new(dev_mem, self.chip)
                    .map_err(MatrixCreationError::MemoryAccessError)?,
            );
        }
        let (reply_sender, reply_receiver) = channel();
        if self
            .control_sender
            .send(Control::RequestOutputs(bits, reply_sender))
            .is_err()
        {
            return Ok(0);
        }
        let enabled = reply_receiver.recv().unwrap_or(0);
        self.enabled_output_bits |= enabled;
        Ok(enabled)
    }

    /// Sets the outputs from [`RGBMatrix::request_outputs`] to the given values. Other bits are ignored.
    pub fn output_gpio(&self, values: u32) {
        if let Some(outputs) = &self.user_outputs {
            outputs.write_masked_bits(values, self.enabled_output_bits);
        }
    }

    /// Get the average frame rate over the last 60 frames.
    #[must_use]
    pub fn framerate(&self) -> usize {
        self.framerate.load(Ordering::Relaxed) as usize
    }
}

impl Drop for RGBMatrix {
    fn drop(&mut self) {
        let Self {
            thread_handle,
            control_sender,
            ..
        } = self;
        if let Some(handle) = thread_handle.take() {
            control_sender.send(Control::Shutdown).ok();
            let _result = handle.join();
        }
    }
}

/// Prints the refresh rate of each frame, and the lowest one after the start up.
struct RefreshRateDisplay {
    start: Instant,
    longest_frame: Duration,
}

impl RefreshRateDisplay {
    /// Times are janky at the start, so the lowest refresh rate is only measured after this.
    const HOLDOFF: Duration = Duration::from_secs(2);

    fn new() -> Self {
        Self {
            start: Instant::now(),
            longest_frame: Duration::ZERO,
        }
    }

    fn update(&mut self, frame_time: Duration) {
        let mut stdout = std::io::stdout().lock();
        let hz = 1.0 / frame_time.as_secs_f64();
        let _ = write!(stdout, "\r{hz:6.1}Hz");
        if self.start.elapsed() > Self::HOLDOFF && frame_time > self.longest_frame {
            self.longest_frame = frame_time;
        }
        if !self.longest_frame.is_zero() {
            let lowest = 1.0 / self.longest_frame.as_secs_f64();
            let _ = write!(stdout, " (lowest: {lowest:.1}Hz)");
        }
        let _ = stdout.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::RGBMatrix;
    use crate::{MultiplexMapperType, NamedPixelMapperType, RGBMatrixConfig, Rotation};

    #[test]
    fn rotates_long_chain() {
        let mut config = RGBMatrixConfig {
            rows: 32,
            cols: 64,
            chain_length: 4,
            pixelmapper: vec![NamedPixelMapperType::Rotate(Rotation::Deg90)],
            ..Default::default()
        };
        let map = RGBMatrix::create_pixel_map(&mut config).unwrap();
        assert_eq!((map.width(), map.height()), (32, 256));
    }

    #[test]
    fn multiplexes_parallel_chains() {
        let mut config = RGBMatrixConfig {
            rows: 16,
            cols: 32,
            parallel: 2,
            multiplexing: Some(MultiplexMapperType::Kaler2Scan),
            ..Default::default()
        };
        let map = RGBMatrix::create_pixel_map(&mut config).unwrap();
        assert_eq!((map.width(), map.height()), (32, 32));
    }

    #[test]
    fn remaps_every_pixel_once() {
        let mut config = RGBMatrixConfig {
            rows: 16,
            cols: 32,
            chain_length: 2,
            pixelmapper: vec!["Remap:32,32|0,0n|0,16s".parse().unwrap()],
            ..Default::default()
        };
        let map = RGBMatrix::create_pixel_map(&mut config).unwrap();
        assert_eq!((map.width(), map.height()), (32, 32));
        for y in 0..32 {
            for x in 0..32 {
                assert!(map.get(x, y).unwrap().is_used(), "({x}, {y}) is not mapped");
            }
        }
    }

    #[test]
    fn creates_maps_for_new_multiplexers() {
        for (multiplexing, rows, cols) in [
            (MultiplexMapperType::P10Outdoor32x16QuarterScan, 16, 32),
            (MultiplexMapperType::P3Outdoor64x64, 64, 64),
            (MultiplexMapperType::DoubleZ, 32, 64),
            (MultiplexMapperType::P4Outdoor80x40, 40, 80),
            (MultiplexMapperType::ZStripe44For40x20, 20, 40),
            (MultiplexMapperType::P10Outdoor1R1G1B2For40x20, 20, 40),
        ] {
            let mut config = RGBMatrixConfig {
                rows,
                cols,
                multiplexing: Some(multiplexing),
                ..Default::default()
            };
            assert!(
                RGBMatrix::create_pixel_map(&mut config).is_ok(),
                "{multiplexing:?}"
            );
        }
    }

    #[test]
    fn rejects_invalid_config() {
        let valid = RGBMatrixConfig::default;
        assert!(RGBMatrix::validate_config(&valid()).is_ok());
        for config in [
            RGBMatrixConfig {
                pwm_bits: 0,
                ..valid()
            },
            RGBMatrixConfig {
                pwm_bits: 12,
                ..valid()
            },
            RGBMatrixConfig {
                rows: 128,
                ..valid()
            },
            RGBMatrixConfig {
                rows: 33,
                ..valid()
            },
            RGBMatrixConfig {
                parallel: 0,
                ..valid()
            },
            RGBMatrixConfig {
                chain_length: 0,
                ..valid()
            },
            RGBMatrixConfig {
                refresh_rate: 0,
                ..valid()
            },
            RGBMatrixConfig {
                pwm_lsb_nanoseconds: 0,
                ..valid()
            },
            RGBMatrixConfig {
                interlaced: true,
                row_setter: crate::RowAddressSetterType::B707ShiftRegister,
                ..valid()
            },
            RGBMatrixConfig {
                refresh_rate: usize::MAX,
                ..valid()
            },
            RGBMatrixConfig {
                refresh_rate: 0,
                uncapped_refresh_rate: true,
                ..valid()
            },
            RGBMatrixConfig {
                pwm_lsb_nanoseconds: u32::MAX,
                ..valid()
            },
            RGBMatrixConfig {
                realtime_priority: Some(0),
                ..valid()
            },
            RGBMatrixConfig {
                realtime_priority: Some(100),
                ..valid()
            },
        ] {
            assert!(RGBMatrix::validate_config(&config).is_err(), "{config:?}");
        }
        for config in [
            RGBMatrixConfig {
                refresh_rate: 100_000,
                ..valid()
            },
            RGBMatrixConfig {
                realtime_priority: Some(50),
                ..valid()
            },
            RGBMatrixConfig {
                pwm_lsb_nanoseconds: 1,
                ..valid()
            },
        ] {
            assert!(RGBMatrix::validate_config(&config).is_ok(), "{config:?}");
        }
    }

    #[test]
    fn limits_pwm_lsb_only_for_hardware_pulses() {
        use crate::HardwareMapping;
        let config = |hardware_mapping, hardware_pulse, pwm_lsb_nanoseconds| RGBMatrixConfig {
            hardware_mapping,
            hardware_pulse,
            pwm_lsb_nanoseconds,
            ..Default::default()
        };
        let pwm = HardwareMapping::adafruit_hat_pwm();
        let check = |config: &RGBMatrixConfig, uses_rp1| {
            RGBMatrix::validate_config(config).is_ok()
                && RGBMatrix::validate_for_backend(config, uses_rp1).is_ok()
        };
        // The PWM divider has 12 bits.
        assert!(check(&config(pwm, true, 130), false));
        assert!(!check(&config(pwm, true, 20_000), false));
        assert!(!check(&config(pwm, true, 2), false));
        // Software pulses, the Raspberry Pi 5 and wirings without the PWM pin have no such limit.
        assert!(check(&config(pwm, false, 20_000), false));
        assert!(check(&config(pwm, true, 20_000), true));
        assert!(check(
            &config(HardwareMapping::adafruit_hat(), true, 2),
            false
        ));
    }

    #[test]
    fn computes_frame_times() {
        use std::time::Duration;
        let config = RGBMatrixConfig::default();
        assert_eq!(
            config.frame_time_target(),
            Some(Duration::from_nanos(8_333_333))
        );
        let uncapped = RGBMatrixConfig {
            uncapped_refresh_rate: true,
            ..Default::default()
        };
        assert_eq!(uncapped.frame_time_target(), None);
        let fastest = RGBMatrixConfig {
            refresh_rate: 100_000,
            ..Default::default()
        };
        assert_eq!(fastest.frame_time_target(), Some(Duration::from_micros(10)));
    }

    #[test]
    fn describes_update_errors() {
        let error = super::UpdateThreadError("PIO failed".to_string());
        assert_eq!(
            error.to_string(),
            "The display update thread stopped unexpectedly: PIO failed"
        );
    }
}

use log::{info, warn};
use std::{
    any::Any,
    error::Error,
    fmt::{Display, Formatter},
    fs::{OpenOptions, write},
    io,
    mem::replace,
    sync::Arc,
    sync::atomic::{AtomicU32, Ordering},
    sync::mpsc::{
        Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, channel, sync_channel,
    },
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
    hardware_mapping::HardwareMapping,
    inputs::{InputReader, InputWriter, input_channel},
    multiplex_mapper::{MultiplexMapper, MultiplexMapperType},
    pixel_mapper::PixelMapper,
    row_address_setter::RowAddressSetter,
    utils::{FrameRateMonitor, linux_has_isol_cpu, set_thread_affinity},
};

/// How often GPIO inputs are polled while display updates are paused.
const SUSPENDED_INPUT_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Messages sent from the main thread to the display update thread.
enum ThreadMessage {
    Canvas(Canvas),
    Suspend,
    Shutdown,
}

fn initialize_update_thread(chip: PiChip) {
    // Pin the thread to the last core to avoid the flicker resulting from context switching.
    let last_core_id = chip.num_cores() - 1;
    if !set_thread_affinity(last_core_id) {
        warn!("Could not pin the display update thread to core {last_core_id}.");
    }

    // If the user has not set up isolcpus, let them know about the performance improvement.
    if chip.num_cores() > 1 && !linux_has_isol_cpu(last_core_id) {
        info!(
            "Suggestion: to slightly improve display update, add\n\tisolcpus={last_core_id}\nat \
            the end of /boot/cmdline.txt and reboot"
        );
    }

    // Disable realtime throttling.
    if chip.num_cores() > 1 && write("/proc/sys/kernel/sched_rt_runtime_us", "999000").is_err() {
        warn!("Could not disable realtime throttling");
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

    // Run as a realtime thread with the highest priority.
    let realtime = set_thread_priority_and_policy(
        thread_native_id(),
        ThreadPriority::Max,
        ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::Fifo),
    );
    if realtime.is_err() {
        warn!("Could not set realtime scheduling. This might lead to flickering.");
        // Fall back to the highest non-realtime priority.
        if set_current_thread_priority(ThreadPriority::Max).is_err() {
            warn!("Could not set thread priority. This might lead to reduced performance.");
        }
    }
}

/// The hardware that the display update thread draws to. The GPIO registers can't leave that thread.
struct MatrixOutput {
    gpio: Gpio,
    address_setter: RowAddressSetter,
    hardware_mapping: HardwareMapping,
    color_clk_mask: u32,
}

impl MatrixOutput {
    fn draw(&mut self, canvas: &Canvas, pwm_low_bit: usize) {
        canvas.dump_to_matrix(
            &mut self.gpio,
            &self.hardware_mapping,
            &mut self.address_setter,
            pwm_low_bit,
            self.color_clk_mask,
        );
    }

    /// Clear the canvas and draw it so that no row stays lit.
    fn blank(&mut self, canvas: &mut Canvas) {
        canvas.fill(0, 0, 0);
        self.draw(canvas, 0);
    }
}

/// What the display update thread takes over from [`RGBMatrix::new`].
struct UpdateThread {
    chip: PiChip,
    config: RGBMatrixConfig,
    canvas: Canvas,
    frame_rate_monitor: FrameRateMonitor,
    message_receiver: Receiver<ThreadMessage>,
    canvas_from_thread_sender: SyncSender<Canvas>,
    inputs: InputWriter,
    thread_start_result_sender: Sender<Result<u32, MatrixCreationError>>,
}

impl UpdateThread {
    fn run(mut self) {
        initialize_update_thread(self.chip);

        let address_setter = self.config.row_setter.create(&self.config);

        let mut gpio = match Gpio::new(self.chip, &self.config, &address_setter) {
            Ok(gpio) => gpio,
            Err(error) => {
                // Fails only if the main thread stopped waiting, in which case there is no one to tell.
                self.thread_start_result_sender
                    .send(Err(MatrixCreationError::GpioError(error)))
                    .ok();
                return;
            }
        };

        // Run the initialization sequence if necessary.
        if let Some(panel_type) = self.config.panel_type {
            panel_type.run_init_sequence(&mut gpio, &self.config);
        }
        self.inputs.set_initial(gpio.read());

        // The main thread stopped waiting and returned an error, so nobody will use the matrix.
        if self
            .thread_start_result_sender
            .send(Ok(gpio.input_bits()))
            .is_err()
        {
            return;
        }

        let color_clk_mask = self
            .config
            .hardware_mapping
            .color_clock_mask(self.config.parallel);
        let mut output = MatrixOutput {
            gpio,
            address_setter,
            hardware_mapping: self.config.hardware_mapping,
            color_clk_mask,
        };

        self.refresh_display(&mut output);

        // Turn it off.
        output.blank(&mut self.canvas);
    }

    /// Keep showing the latest canvas until the matrix is dropped.
    fn refresh_display(&mut self, output: &mut MatrixOutput) {
        // Dither sequence
        let mut dither_low_bit_sequence = 0;
        let dither_start_bits = match self.config.dither_bits {
            0 => [0, 0, 0, 0],
            1 => [0, 1, 0, 1],
            2 => [0, 1, 2, 2],
            _ => unreachable!("dither_bits is checked in RGBMatrixConfig::validate"),
        };

        let frame_time_target_us = (1_000_000.0 / self.config.refresh_rate as f64) as u64;

        let mut suspended = false;

        loop {
            let start_time = output.gpio.time();
            self.inputs.update(output.gpio.read());
            // Check for a new message. While suspended, wait for one but keep polling the inputs.
            let message = if suspended {
                match self
                    .message_receiver
                    .recv_timeout(SUSPENDED_INPUT_POLL_INTERVAL)
                {
                    Ok(message) => Some(message),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            } else {
                match self.message_receiver.try_recv() {
                    Ok(message) => Some(message),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return,
                }
            };
            match message {
                Some(ThreadMessage::Canvas(new_canvas)) => {
                    let old_canvas = replace(&mut self.canvas, new_canvas);
                    let Ok(()) = self.canvas_from_thread_sender.send(old_canvas) else {
                        return;
                    };
                    suspended = false;
                }
                Some(ThreadMessage::Suspend) => {
                    output.blank(&mut self.canvas);
                    self.frame_rate_monitor.pause();
                    suspended = true;
                }
                Some(ThreadMessage::Shutdown) => return,
                None => {}
            }
            if suspended {
                continue;
            }

            output.draw(
                &self.canvas,
                dither_start_bits[dither_low_bit_sequence % dither_start_bits.len()],
            );
            dither_low_bit_sequence += 1;

            self.frame_rate_monitor.update();

            // Sleep for the rest of the frame.
            let now_time = output.gpio.time();
            let end_time = start_time + frame_time_target_us;
            if let Some(remaining_time) = end_time.checked_sub(now_time) {
                output.gpio.sleep(remaining_time);
            }
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

/// An error returned by [`RGBMatrix::new`].
#[derive(Debug)]
#[non_exhaustive]
pub enum MatrixCreationError {
    /// The Raspberry Pi model could not be determined. It can be set with [`RGBMatrixConfig::pi_chip`] instead.
    ChipDeterminationError,
    /// An option of the config is invalid, as described by the message.
    InvalidConfig(String),
    /// The display update thread did not finish its setup in time.
    ThreadTimedOut,
    /// The display update thread panicked during its setup, with the given message.
    ThreadPanicked(String),
    /// The GPIO pins could not be set up.
    GpioError(GpioInitializationError),
    /// `/dev/mem` could not be opened, which needs root privileges.
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
            MatrixCreationError::InvalidConfig(message) => {
                write!(f, "Invalid configuration: {message}")
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
            MatrixCreationError::GpioError(error) => error.fmt(f),
            MatrixCreationError::MemoryAccessError(error) => {
                f.write_str("Failed to open '/dev/mem'.")?;
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

/// The display update thread stopped while the matrix was in use, which only happens if it panicked.
#[derive(Debug, Clone)]
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

/// The controller of the display. A separate thread keeps refreshing the display with the latest canvas, and
/// dropping the controller stops that thread and turns the display off.
pub struct RGBMatrix {
    /// The join handle of the update thread.
    thread_handle: Option<JoinHandle<()>>,
    /// Why the update thread stopped, once it did.
    thread_error: Option<UpdateThreadError>,
    /// Reader for GPIO inputs, until it is taken.
    input_reader: Option<InputReader>,
    /// Channel to send canvases and control messages to the update thread.
    message_sender: SyncSender<ThreadMessage>,
    /// Channel to receive canvas from update thread.
    canvas_from_thread_receiver: Receiver<Canvas>,
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

impl std::fmt::Debug for RGBMatrix {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RGBMatrix")
            .field("enabled_input_bits", &self.enabled_input_bits)
            .field("framerate", &self.framerate())
            .field("suspended", &self.suspended)
            .field("brightness", &self.brightness)
            .field("pwm_bits", &self.pwm_bits)
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
    /// function to check which bits were actually available.
    ///
    /// Note: If privilege dropping is enabled in the config, then calling this function will have the side
    /// effect of dropping the application's privileges down to the configured user/group.
    pub fn new(mut config: RGBMatrixConfig) -> Result<(Self, Canvas), MatrixCreationError> {
        let shared_mapper = Self::create_pixel_map(&mut config)?;

        // Check if we can access the memory before setting up the hardware.
        OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/mem")
            .map_err(MatrixCreationError::MemoryAccessError)?;

        let chip = config
            .pi_chip
            .or_else(PiChip::determine)
            .ok_or(MatrixCreationError::ChipDeterminationError)?;

        // Create two canvases, one for the display update thread and one for the user to modify. They will be
        // swapped out after each frame.
        let canvas = Canvas::new(&config, shared_mapper);
        let thread_canvas = canvas.clone();

        let (message_sender, message_receiver) = sync_channel::<ThreadMessage>(0);
        let (canvas_from_thread_sender, canvas_from_thread_receiver) = sync_channel::<Canvas>(1);
        let (input_writer, input_reader) = input_channel();
        let (thread_start_result_sender, thread_start_result_receiver) =
            channel::<Result<u32, MatrixCreationError>>();
        let framerate = Arc::new(AtomicU32::new(0));
        let (brightness, pwm_bits) = (config.led_brightness, config.pwm_bits);

        let update_thread = UpdateThread {
            chip,
            config,
            canvas: thread_canvas,
            frame_rate_monitor: FrameRateMonitor::new(framerate.clone()),
            message_receiver,
            canvas_from_thread_sender,
            inputs: input_writer,
            thread_start_result_sender,
        };
        let thread_handle = spawn(move || update_thread.run());

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
            input_reader: Some(input_reader),
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
        let stretch_factor = config
            .multiplexing
            .map_or(1, MultiplexMapperType::stretch_factor);
        config.validate(stretch_factor)?;

        let multiplex_mapper = config
            .multiplexing
            .map(|multiplexing| MultiplexMapper::new(multiplexing, config.rows, config.cols));

        // The multiplexers might choose to have a different physical layout.
        // We need to configure that first before setting up the hardware.
        config.rows /= stretch_factor;
        config.cols *= stretch_factor;

        let pixel_designator = PixelDesignator::new(&config.hardware_mapping, config.led_sequence);
        let width = config.cols * config.chain_length;
        let height = config.rows * config.parallel;
        let mut shared_mapper = PixelDesignatorMap::new(pixel_designator, width, height, config);

        // Apply the mapping for the panels first.
        if let Some(mapper) = &multiplex_mapper {
            shared_mapper = Self::apply_pixel_mapper(&shared_mapper, mapper)?;
        }

        // Apply higher level mappers that might arrange panels.
        for mapper_type in config.pixelmapper.iter() {
            let mapper = mapper_type.create(config.chain_length, config.parallel)?;
            shared_mapper = Self::apply_pixel_mapper(&shared_mapper, mapper.as_ref())?;
        }

        Ok(shared_mapper)
    }

    fn apply_pixel_mapper(
        shared_mapper: &PixelDesignatorMap,
        mapper: &dyn PixelMapper,
    ) -> Result<PixelDesignatorMap, MatrixCreationError> {
        let old_width = shared_mapper.width();
        let old_height = shared_mapper.height();
        let [new_width, new_height] = mapper.size_mapping(old_width, old_height)?;
        // Every pixel is copied from the old map below, so there is no need to compute the physical layout.
        let mut new_mapper = PixelDesignatorMap::new_unmapped(
            shared_mapper.pixel_designator(),
            new_width,
            new_height,
        );
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
    pub fn update_on_vsync(&mut self, canvas: Canvas) -> Result<Canvas, UpdateThreadError> {
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

    /// Set the brightness in percent, clamped to 1-100. This applies to the canvases returned by
    /// [`RGBMatrix::update_on_vsync`] from now on, not to the one that is currently being drawn to.
    pub fn set_brightness(&mut self, brightness: u8) {
        self.brightness = brightness;
    }

    /// Set the PWM bits used for output, clamped to 1-11. Default is 11, but if you only deal with simple
    /// comic-colors, 1 might be sufficient. Lower values require less CPU. This applies to the canvases returned
    /// by [`RGBMatrix::update_on_vsync`] from now on, not to the one that is currently being drawn to.
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

    /// Take the reader for the GPIO inputs requested with [`RGBMatrixConfig::input_bits`]. There is only one,
    /// so this returns `None` after the first call.
    pub fn take_input_reader(&mut self) -> Option<InputReader> {
        self.input_reader.take()
    }

    /// Get the average frame rate over the last 60 frames.
    #[must_use]
    pub fn framerate(&self) -> usize {
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
        error::Error,
        io,
        sync::Arc,
        sync::mpsc::{channel, sync_channel},
        thread::spawn,
    };

    use strum::VariantNames;

    use super::{
        MatrixCreationError, PixelDesignator, PixelDesignatorMap, RGBMatrix, panic_message,
    };
    use crate::{
        GpioInitializationError, MultiplexMapperType, NamedPixelMapperType, RGBMatrixConfig,
        Rotation,
    };

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
    fn test_error_sources() {
        let denied = MatrixCreationError::MemoryAccessError(io::ErrorKind::PermissionDenied.into());
        assert!(
            denied
                .to_string()
                .ends_with("Not running with root privileges?")
        );
        assert!(denied.source().is_some());
        let missing = MatrixCreationError::MemoryAccessError(io::ErrorKind::NotFound.into());
        assert_eq!(missing.to_string(), "Failed to open '/dev/mem'.");

        // The wrapped GPIO error is shown directly, followed by its own source.
        let gpio = GpioInitializationError::FailedRegisterMapping(io::ErrorKind::NotFound.into());
        let gpio_message = gpio.to_string();
        let wrapped = MatrixCreationError::GpioError(gpio);
        assert_eq!(wrapped.to_string(), gpio_message);
        let source = wrapped.source().unwrap();
        assert_eq!(
            source.to_string(),
            io::Error::from(io::ErrorKind::NotFound).to_string()
        );
    }

    #[test]
    fn test_update_thread_error_after_panic() {
        // A stand-in for the update thread that panics when told to.
        let (panic_sender, panic_receiver) = channel();
        let thread_handle = spawn(move || {
            panic_receiver.recv().unwrap();
            panic!("update failed");
        });
        let (message_sender, _message_receiver) = sync_channel(0);
        let (_canvas_sender, canvas_from_thread_receiver) = sync_channel(1);
        let mut matrix = RGBMatrix {
            thread_handle: Some(thread_handle),
            thread_error: None,
            input_reader: None,
            message_sender,
            canvas_from_thread_receiver,
            enabled_input_bits: 0,
            framerate: Arc::default(),
            suspended: false,
            brightness: 100,
            pwm_bits: 11,
        };

        panic_sender.send(()).unwrap();
        let error = matrix.update_thread_error();
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
        let mapper = NamedPixelMapperType::Rotate(Rotation::Deg90)
            .create(4, 1)
            .unwrap();

        let rotated = RGBMatrix::apply_pixel_mapper(&map, mapper.as_ref()).unwrap();

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

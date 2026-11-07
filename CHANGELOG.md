# Changelog

## Unreleased

### Added

Ported from the upstream C++ library:

- Raspberry Pi 5 support through the RP1 I/O controller, with the RIO backend that sets the pins from the CPU, and
  the PIO backend that clocks out the data with a PIO state machine (`--rp1-pio true`).
- Multiplexers `P10Outdoor32x16QuarterScan`, `P3Outdoor64x64`, `DoubleZ`, `P4Outdoor80x40`, `ZStripe44For40x20` and `P10Outdoor1R1G1B2For40x20`.
- Pixel mappers `V-mapper`, `StackToRow` and `Remap`.
- The `ComputeModule` hardware mapping, with all 6 parallel chains when the new `wide-gpio` feature is enabled.
- The `B707ShiftRegister` row address setter.
- The options `hardware_pulse` (turn off the PWM hardware pulses), `inverse_colors`, `show_refresh` and
  `busy_waiting`, and `slowdown -1` to wait for a memory barrier.
- `RGBMatrix::update_on_vsync_every` to show each canvas for a multiple of frames, and `RGBMatrix::request_outputs`
  and `RGBMatrix::output_gpio` for GPIO outputs of the user.
- `Canvas::set_pixels`, `Canvas::sub_fill`, `Canvas::serialize`, `Canvas::deserialize`, `Canvas::copy_from` and
  `Canvas::set_luminance_correct`. Solid fills with embedded-graphics use `sub_fill`.
- `FM6126A` as a name for the `FM6126` panel type.
- The options `realtime_priority` (run the update thread with `SCHED_FIFO`, off by default), `uncapped_refresh_rate`, `clear_on_vsync` and `input_bits`, and `RGBMatrix::try_request_outputs`.

Merged from upstream:

- `InputReader` and `InputChange` to read the GPIO inputs from any thread, through `RGBMatrix::take_input_reader`.
- `RGBMatrix::suspend`/`RGBMatrix::is_suspended` to clear the display and pause the update thread until the next `update_on_vsync`.
- The `min_sys_sleep_microseconds` option to tune how early sleeps wake up.
- Export the `GpioInitializationError`, `InvalidVariantError`, `InvalidPixelMapperError` and `UpdateThreadError` types, and `Rotation` and `MirrorDirection`. `MatrixCreationError` and `GpioInitializationError` are `#[non_exhaustive]`.
- `Clone` for `RGBMatrixConfig` and `Debug` for `RGBMatrix` and `Canvas`.
- Document the public API.

### Fixed

- Fix a panic when a pixel mapper makes the canvas taller than six panels, e.g. `Rotate:90` on a long chain or a 1:4 multiplexer with parallel chains.
- Support the `AdafruitHat`, `Classic` and `ClassicPi1` hardware mappings, which panicked because their output enable pin can't be driven by the PWM hardware. They now use timer based pulses.
- Only claim the GPIO pins of the parallel chains in use. Previously, e.g. the I2C and UART pins were always turned into outputs.
- Fix the 64 bit system timer read, which could jump by about 71 minutes when the low half overflowed.
- Fix the `GBR` and `BRG` LED sequences, which were swapped compared to the upstream library.
- Fix the vertical blocks of the `P8Outdoor1R1G1B` multiplexer.
- Fix the `ABCShiftRegister` row address setter to match the upstream library.
- Fix the `RegularPi1` hardware mapping, which used the wrong pins for red and green.
- Send the FM6126 and FM6127 init registers most significant bit first and to all panels of the chain.
- Fix interlaced scanning with an odd number of rows.
- Allow negative angles for `Rotate` and reject angles that aren't multiples of 90 when creating the matrix.
- Report invalid configurations (`pwm_bits`, `rows`, `cols`, `chain_length`, `parallel`, `refresh_rate`, `pwm_lsb_nanoseconds`) as errors instead of panicking, and report panics of the update thread during initialization with their message.
- Fix the canvas size, centering and redundant clearing in the examples.
- Fix the `P10Outdoor32x16QuarterScan` multiplexer, which made the canvas twice as tall as the panel and drew a
  second parallel chain onto the lower data lines of the first one. Upstream has the same bug.
- Fix the canvas width of the `U-mapper` for panels whose width isn't a multiple of 32, e.g. 40x20 or 80x40 panels.
- Reject interlacing with the `B707ShiftRegister` row setter, which needs the rows in order.
- `Canvas::fill` no longer lights up LEDs that aren't part of the canvas, e.g. panels left out with `Remap`.
- Determine the Raspberry Pi model from the device tree if /proc/cpuinfo has no revision, instead of misreading the
  CPU core revision.
- Report pixel mappers that produce an empty canvas as an error.
- Return an error instead of panicking when the user or group name to drop privileges to contains a NUL byte.
- Fix a SIGBUS on aarch64 by reading the system timer as two 32-bit registers. [#28](https://github.com/EmbersArc/rpi_led_panel/pull/28)
- Fix automatic chip detection on mainline kernels (through the device tree `compatible` property) and for old-style revision codes with the warranty bit set.
- Fix the checks for loaded kernel modules and isolated CPUs, which could match the wrong module or CPU.
- Validate `led_brightness` and `dither_bits` up front.

### Changed

- The colors are rounded like upstream, instead of rounded down.
- The frame rate is timed with the monotonic clock instead of the system timer of the BCM chips.
- Privileges are only dropped when running as root.
- The waits for pulses and frames wake up `min_sys_sleep_microseconds` (default 100) early and busy wait the rest, instead of the previous fixed 200 µs threshold and 50 µs margin. Use `--min-sys-sleep-microseconds 200` for the old threshold.
- Report warnings and suggestions through the `log` crate instead of printing them to stderr. A logger such as `env_logger` is needed to see them.
- Add more detail to error messages and implement `Error::source` for errors caused by I/O failures.
- Declare 1.88 as the minimum supported Rust version.
- Updated dependencies.
- Share the pixel map and color lookup table between canvases and shrink the pixel map entries.

### Breaking

- `RGBMatrix::new` returns a `Canvas` instead of a `Box<Canvas>`, and `RGBMatrix::update_on_vsync` and `RGBMatrix::update_on_vsync_every` take and return a `Canvas`, so `canvas.as_mut()` becomes `&mut canvas` when drawing.
- `update_on_vsync` and `update_on_vsync_every` return a `Result` with an `UpdateThreadError` instead of panicking when the update thread shuts down unexpectedly. This replaces `try_update_on_vsync`, `try_update_on_vsync_every` and `MatrixUpdateError`.
- Move `set_brightness` and `set_pwm_bits` from `Canvas` to `RGBMatrix`, where `RGBMatrix::brightness` and `RGBMatrix::pwm_bits` read them back. `Canvas::brightness` and `Canvas::pwm_bits` are gone. On the canvas the setters only applied to every other frame.
- Replace `RGBMatrix::receive_new_inputs` with an `InputReader` from `RGBMatrix::take_input_reader`, which can be moved to another thread.
- Rename `RGBMatrix::get_framerate` to `RGBMatrix::framerate`.
- `RGBMatrix::new` no longer takes the `requested_inputs` argument. Use the new `input_bits` option of `RGBMatrixConfig` instead, also available as `--input-bits`.
- `MatrixCreationError::MemoryAccessError` holds the `io::Error`. The variants `TooManyParallelChains` and `InvalidDitherBits` are gone, as `InvalidConfig` reports them.
- Parsing a `HardwareMapping` returns an `InvalidVariantError` and parsing a `NamedPixelMapperType` returns the new `InvalidPixelMapperError`.
- `PanelType`, `MultiplexMapperType`, `RowAddressSetterType`, `NamedPixelMapperType` and `PiChip` are `#[non_exhaustive]`, so `match` expressions on them need a wildcard arm.
- `NamedPixelMapperType::Mirror` takes a `MirrorDirection` instead of a `bool`, and `NamedPixelMapperType::Rotate` takes a `Rotation` instead of a `usize`, so an invalid angle can no longer cause a panic.
- Turn the CLI `interlaced` argument into a switch.
- Replace the `drop_privs` option with the `no_drop_privs` switch.
- `led_brightness` outside of 1 to 100 is now an error instead of being clamped.
- New `MatrixCreationError` variants `ThreadPanicked` and `InvalidConfig`.
- `NamedPixelMapperType` is no longer `Copy`, because `Remap` holds a list of panels.
- `RGBMatrixConfig::slowdown` is an `i32`, to allow -1, and `RGBMatrixConfig` has new fields.
- New `PiChip::BCM2712` and `GpioInitializationError::Rp1Pio` variants.

## Version 0.9.1

### Fixed 
- Fix the framerate monitor. [#27](https://github.com/EmbersArc/rpi_led_panel/pull/27)
- Reworked the rotating square example to avoid black pixels on the square.


## Version 0.9.0

### Added

- Add the option to drop privileges after initialization, enabled by default. [#21](https://github.com/EmbersArc/rpi_led_panel/pull/21) and [#22](https://github.com/EmbersArc/rpi_led_panel/pull/22)

### Changed

- Keep drawing the last frame, even when no new frame has arrived. [#25](https://github.com/EmbersArc/rpi_led_panel/pull/25)

### Breaking

- Change the default hardware mapping in `RGBMatrixConfig::default()` to `Regular` to be consistent with the commandline default.

## Version 0.8.1

### Changed

- Improve busy-wait loop and avoid sleeping for too long by picking a more conservative minimum sleep duration. This may increase CPU usage but avoids flickering.

## Version 0.8.0

### Added

- Added the `ChainLink` pixel mapper. [#19](https://github.com/EmbersArc/rpi_led_panel/pull/19)

### Changed

- Improved the argument parsing and print available options when entering an invalid option.
- Clear the canvas before returning it to the user to avoid confusion behavior.
- Improve the documentation of `update_on_vsync`.
- Re-export `embedded_graphics` if the `drawing` feature is enabled.
- Updated dependencies.
- Updated to the 2024 edition.

### Breaking

- The parser is a bit stricter about capitalization as a consequence of the improved argument parsing.

## Version 0.7.0

### Changed

- Default to `Regular` hardware mapping. The previous default was `AdafruitHat` which now has to be specified with `--hardware-mapping AdafruitHat`.
- Overall improved error handling. More errors are now returned instead of panicking or printing.

### Fixed

- Fixed a potential panic when multiplexing is enabled.
- Improved the automatic chip detection on some distributions.
- Made `NamedPixelMapperType` public. [#16](https://github.com/EmbersArc/rpi_led_panel/pull/16)

## Version 0.6.0

- Added the `--led-brightness` CLI argument. [#15](https://github.com/EmbersArc/rpi_led_panel/pull/15)

## Version 0.5.1

### Fixed

- Fixed an issue with the `UArrangeMapper` when `parallel` is greater than 1. [#14](https://github.com/EmbersArc/rpi_led_panel/pull/14)

## Version 0.5.0

### Added

- Added rotate and mirror mappers. [#9](https://github.com/EmbersArc/rpi_led_panel/pull/9)
- Added a U mapper [#12](https://github.com/EmbersArc/rpi_led_panel/pull/12)

### Fixed

- Actually make use of the `slowdown` config option [#13](https://github.com/EmbersArc/rpi_led_panel/pull/13)

### Breaking

- Potentially breaking: Changed the default Model 4 GPIO slowdown to 3.

### Changed

- Added the `PixelMapper` trait. [#8](https://github.com/EmbersArc/rpi_led_panel/pull/8)
- Updated `Canvas` to use `height()` and `width()` for `Mapper` dimensions.
  [#10](https://github.com/EmbersArc/rpi_led_panel/pull/11)
- Refactored `PixelDesignatorMap::new` [#11](https://github.com/EmbersArc/rpi_led_panel/pull/11)
- Updated `memmap2` to `0.9.0`.

## Version 0.4.0

### Added

- Show a message when isolating a CPU core could improve performance [#4](https://github.com/EmbersArc/rpi_led_panel/pull/4).

### Fixed

- Corrected the bounds check in pixel mappers [#5](https://github.com/EmbersArc/rpi_led_panel/issues/5).

## Version 0.3.0

### Breaking

- Made the `RGBMatrixConfig` strongly typed and added a default implementation.

### Added

- Support daisy-chained panels and parallel chains ([#3](https://github.com/EmbersArc/rpi_led_panel/pull/3)).
- An option to configure the LED sequence.

## Version 0.2.0

### Breaking

- Improved error handling.

## Version 0.1.0

- Initial release.

# Changelog

## Unreleased

### Added

- Add `RGBMatrix::suspend`/`RGBMatrix::is_suspended` to clear the display and pause the update thread until the next `update_on_vsync`.
- Add the `min_sys_sleep_microseconds` option to tune how early sleeps wake up.
- Export the `MatrixCreationError`, `GpioInitializationError`, `UpdateThreadError` and `InvalidVariantError` error types. `MatrixCreationError` and `GpioInitializationError` are `#[non_exhaustive]`.
- Implement `Clone` for `RGBMatrixConfig` and `Debug` for `RGBMatrix` and `Canvas`.
- Document the public API.

### Changed

- Run the display update thread with realtime (`SCHED_FIFO`) scheduling if possible.
- Wake up early from sleeps and busy wait the rest to avoid overshooting.
- Reduce memory use by sharing the color lookup table and pixel mapping between canvases and halving the size of the pixel mapping.
- Implement `fill_solid` for embedded-graphics to speed up filled rectangles.
- Report warnings and suggestions through the `log` crate instead of printing them to stderr. A logger such as `env_logger` is needed to see them.
- Add more detail to error messages and implement `Error::source` for errors caused by I/O failures.
- Accept negative angles for the `Rotate` pixel mapper, e.g. `Rotate:-90`.
- Declare 1.87 as the minimum supported Rust version.
- Updated dependencies.

### Fixed

- Fix the examples for canvases of a different size than one panel and for panels that are not square.
- Fix a SIGBUS on aarch64 by reading the system timer as two 32-bit registers. [#28](https://github.com/EmbersArc/rpi_led_panel/pull/28)
- Fix automatic chip detection on mainline kernels and for old-style revision codes with the warranty bit set.
- Fix the `AdafruitHat`, `Classic` and `ClassicPi1` hardware mappings, which failed on startup. They now time their pulses in software, so the sound module can stay loaded.
- Fix the `RegularPi1` hardware mapping, which put the top red channel on GPIO 15 (the `E` address line) and 27 instead of 11, and the top green channel only on GPIO 21.
- Fix the `FM6126` and `FM6127` panel initialization, which shifted the register values in reversed bit order and only reached the first panel of a chain.
- Fix the `ABCShiftRegister` row address setter, which didn't invert the data and clock.
- Fix the `P8Outdoor1R1G1B` multiplex mapper.
- Fix the `U-mapper` pixel mapper for chains that aren't a multiple of 64 pixels wide.
- Fix a panic when rotating a long chain.
- Fix a panic in interlaced mode when the number of rows is not a multiple of 4.
- Fix the checks for loaded kernel modules and isolated CPUs, which could match the wrong module or CPU.
- Report panics of the update thread during initialization instead of a timeout.
- Return an error instead of panicking when the registers can't be mapped.
- Validate the configuration up front instead of panicking on values like `rows > 64`, `parallel = 0` or `refresh_rate = 0`, and reject panel sizes that a multiplex mapper doesn't support.

### Breaking

- `update_on_vsync` returns a `Result` with an `UpdateThreadError` instead of panicking when the update thread shuts down unexpectedly.
- `receive_new_inputs` returns a `Result<Option<u32>, UpdateThreadError>`. An unexpected shutdown of the update thread is now an error instead of `None`.
- Move `set_brightness` and `set_pwm_bits` from `Canvas` to `RGBMatrix`. On the canvas they only applied to every other frame.
- Parsing a `HardwareMapping` returns an `InvalidVariantError` and parsing a `NamedPixelMapperType` returns the new `InvalidPixelMapperError`.
- Rename `RGBMatrix::get_framerate` to `RGBMatrix::framerate`.
- `RGBMatrix::new` no longer takes the `requested_inputs` argument. Use the new `input_bits` option of `RGBMatrixConfig` instead, also available as `--input-bits`.
- `RGBMatrix::new` and `RGBMatrix::update_on_vsync` use `Canvas` instead of `Box<Canvas>`, so `canvas.as_mut()` becomes `&mut canvas` when drawing.
- `PanelType`, `MultiplexMapperType`, `RowAddressSetterType`, `NamedPixelMapperType` and `PiChip` are `#[non_exhaustive]`, so `match` expressions on them need a wildcard arm.
- `NamedPixelMapperType::Mirror` takes a `MirrorDirection` instead of a `bool`, and `NamedPixelMapperType::Rotate` takes a `Rotation` instead of a `usize`, so an invalid angle can no longer cause a panic.
- Turn the CLI `interlaced` argument into a switch.
- Replace the `drop_privs` option with the `no_drop_privs` switch.
- `led_brightness` outside of 1 to 100 is now an error instead of being clamped, and `pwm_lsb_nanoseconds` has to be from 4 to 10000.

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

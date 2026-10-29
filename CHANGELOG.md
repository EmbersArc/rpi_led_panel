# Changelog

## Unreleased

- Add `RGBMatrix::suspend` to blank the display and pause the update thread until the next `update_on_vsync`.
- Add the `min_sys_sleep_microseconds` option to tune how early sleeps wake up.
- Fix SIGBUS on aarch64: Read the system timer as two 32-bit registers. [#28](https://github.com/EmbersArc/rpi_led_panel/pull/28)
- Fix automatic chip detection on various kernels.
- Run the display update thread with realtime (`SCHED_FIFO`) scheduling.
- Share the color lookup table and pixel mapping between canvases instead of copying them.
- Halve the memory used by the pixel mapping.
- Wake up early from sleeps and busy wait the rest to avoid overshooting.
- Faster `fill_solid` for embedded-graphics, which is used for filled rectangles.
- Fix the `AdafruitHat`, `Classic` and `ClassicPi1` hardware mappings by timing their pulses in sofware.
- **Breaking:** Move `set_brightness` and `set_pwm_bits` from `Canvas` to `RGBMatrix`. On the canvas they only applied to every other frame.
- Export the `MatrixCreationError`, `GpioInitializationError` and `InvalidVariantError` error types.
- Fix the checks for loaded kernel modules and isolated CPUs, which could match the wrong module or CPU.
- Rotating a long chain no longer panics.
- Report failures of the update thread during initialization instead of a timeout, and return an error instead of panicking when the registers can't be mapped.
- Validate the configuration up front instead of panicking on values like `rows > 64`, `parallel = 0` or `refresh_rate = 0`.
- Fix the `FM6126` and `FM6127` panel initialization, which shifted the register values in reversed bit order and only reached the first panel of a chain.
- Fix the `P8Outdoor1R1G1B` multiplex mapper.
- Fix a panic in interlaced mode when the number of rows is not a multiple of 4.
- Fix the `ABCShiftRegister` row address setter, which didn't invert the data and clock.
- **Breaking:** `update_on_vsync` and `suspend` return an `UpdateThreadError` with the reason instead of panicking when the update thread shuts down unexpectedly.
- Reject panel sizes that a multiplex mapper doesn't support.

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

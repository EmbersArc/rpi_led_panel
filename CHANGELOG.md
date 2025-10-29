# Changelog

### Version 0.8.0

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

//! Control HUB75 RGB LED matrix panels from a Raspberry Pi, as a Rust rewrite of the
//! [rpi-rgb-led-matrix](https://github.com/hzeller/rpi-rgb-led-matrix) library.
//!
//! [`RGBMatrix::new`] starts a thread that keeps refreshing the display and returns a [`Canvas`] to draw on.
//! [`RGBMatrix::update_on_vsync`] shows the canvas and hands back another one for the next frame. With the
//! `drawing` feature, which is enabled by default, the canvas is a `DrawTarget` of the re-exported
//! `embedded_graphics` crate.
//!
//! Accessing the GPIO pins needs root privileges, which are dropped after the setup unless
//! [`RGBMatrixConfig::no_drop_privs`] is set.
//!
//! This moves a red pixel along the top row of a 64x32 panel:
//!
//! ```no_run
//! use rpi_led_panel::{RGBMatrix, RGBMatrixConfig};
//!
//! let config = RGBMatrixConfig {
//!     rows: 32,
//!     cols: 64,
//!     ..Default::default()
//! };
//! let (mut matrix, mut canvas) = RGBMatrix::new(config)?;
//! for x in (0..canvas.width()).cycle() {
//!     canvas.set_pixel(x, 0, 255, 0, 0);
//!     canvas = matrix.update_on_vsync(canvas)?;
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod canvas;
mod chip;
mod color;
mod config;
mod error;
mod gpio;
mod hardware_mapping;
mod init_sequence;
mod multiplex_mapper;
mod named_pixel_mapper;
mod pin_pulser;
mod pixel_mapper;
mod registers;
mod rgb_matrix;
mod row_address_setter;
mod utils;

pub use canvas::{Canvas, LedSequence};
pub use chip::PiChip;
pub use config::RGBMatrixConfig;
pub use error::InvalidVariantError;
pub use gpio::GpioInitializationError;
pub use hardware_mapping::HardwareMapping;
pub use init_sequence::PanelType;
pub use multiplex_mapper::MultiplexMapperType;
pub use named_pixel_mapper::{MirrorDirection, NamedPixelMapperType, Rotation};
pub use rgb_matrix::{MatrixCreationError, RGBMatrix, UpdateThreadError};
pub use row_address_setter::RowAddressSetterType;

#[cfg(feature = "drawing")]
pub use embedded_graphics;

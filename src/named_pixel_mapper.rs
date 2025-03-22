use std::{error::Error, str::FromStr};

use crate::rgb_matrix::MatrixCreationError;

/// Enum representing different pixel mapping options for mapping the logical layout of your boards
/// to your physical arrangement. These options allow you to customize the mapping to match your unique setup.
///
/// These options can be used with the `--pixelmapper` flag to choose between different mappings.
///
/// You can apply multiple mappers in your configuration, and they will be applied in the order you specify.
/// For example, to first mirror the panels horizontally and then rotate the resulting screen,
/// You can use `--pixelmapper Mirror:H --pixelmapper Rotate:90`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamedPixelMapperType {
    /// The "Mirror" mapper allows you to mirror the output either horizontally or vertically.
    /// Specify 'H' for horizontal mirroring or 'V' for vertical mirroring as a parameter after a colon.
    /// Example: `--pixelmapper Mirror:H`
    Mirror(bool),
    /// The "Rotate" mapper allows you to rotate your screen by a specified angle in degrees.
    /// Specify the desired angle as a parameter after a colon.
    /// Example: `--pixelmapper Rotate:90` for a 90-degree rotation.
    Rotate(usize),
    /// The `UMapper` represents a pixel mapping strategy where a long chain of display panels
    /// is arranged in a U-shape configuration. This arrangement allows for a single chain display
    /// with panels of double height but still utilizing only one data chain.
    ///
    /// In this U-shape configuration, the panels are bent around after half of them to continue below,
    /// creating a visually unified display. This arrangement is ideal for scenarios where you want to maximize
    /// the display area without increasing the number of data chains.
    ///
    /// `UMapper` takes no parameters.
    /// `--pixelmapping U-mapper`
    ///
    /// For example, a single chain display with four 32x32 panels like this:
    ///    [<][<][<][<] }- Raspberry Pi connector
    ///
    /// can be arranged in this 64x64 U-shaped display:
    ///    [<][<] }----- Raspberry Pi connector
    ///    [>][>]
    ///
    /// This U-shape configuration can also be applied to displays with multiple chains.
    /// For instance, an arrangement with two chains, each consisting of 8 panels, can be represented as follows:
    ///   [<][<][<][<]  }--- Pi connector #1
    ///   [>][>][>][>]
    ///   [<][<][<][<]  }--- Pi connector #2
    ///   [>][>][>][>]
    UMapper,
}

impl FromStr for NamedPixelMapperType {
    type Err = Box<dyn Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some((command, param)) = s.split_once(':') {
            match command {
                "Mirror" => match param {
                    "H" | "h" => Ok(Self::Mirror(true)),
                    "V" | "v" => Ok(Self::Mirror(false)),
                    other => Err(format!(
                        "'{other}' is not valid. Mirror parameter should be either 'V' or 'H'"
                    )
                    .into()),
                },
                "Rotate" => {
                    if let Ok(angle) = param.parse::<usize>() {
                        if angle % 90 != 0 {
                            return Err(format!(
                                "'{angle}' is not valid. Rotation needs to be a multiple of 90 degrees"
                            )
                            .into());
                        }
                        return Ok(Self::Rotate((angle + 360) % 360));
                    }
                    Err("Rotation angle is missing or invalid".into())
                }
                other => Err(format!("'{other}' is not a valid Pixel mapping.").into()),
            }
        } else if s == "U-mapper" {
            Ok(Self::UMapper)
        } else {
            Err(format!("'{s}' is not a valid Pixel mapping.").into())
        }
    }
}

impl NamedPixelMapperType {
    pub(crate) fn create(
        self,
        chain: usize,
        parallel: usize,
    ) -> Result<Box<dyn NamedPixelMapper>, MatrixCreationError> {
        match self {
            NamedPixelMapperType::Mirror(horizontal) => {
                Ok(Box::new(MirrorPixelMapper { horizontal }))
            }
            NamedPixelMapperType::Rotate(angle) => Ok(Box::new(RotatePixelMapper { angle })),
            NamedPixelMapperType::UMapper => Ok(Box::new(UArrangeMapper::new_with_parameters(
                chain, parallel,
            )?)),
        }
    }
}

/// A pixel mapper is a way for you to map pixels of LED matrixes to a different
/// layout. If you have an implementation of a [`PixelMapper`], you can give it
/// to the [`RGBMatrix::apply_pixel_mapper`], which then presents you a canvas
/// that has the new [`visible_width`], [`visible_height`].
pub(crate) trait NamedPixelMapper {
    fn get_size_mapping(
        &self,
        matrix_width: usize,
        matrix_height: usize,
    ) -> Result<[usize; 2], MatrixCreationError>;

    fn map_visible_to_matrix(
        &self,
        matrix_width: usize,
        matrix_height: usize,
        visible_x: usize,
        visible_y: usize,
    ) -> [usize; 2];
}

struct MirrorPixelMapper {
    horizontal: bool,
}

impl NamedPixelMapper for MirrorPixelMapper {
    fn get_size_mapping(
        &self,
        matrix_width: usize,
        matrix_height: usize,
    ) -> Result<[usize; 2], MatrixCreationError> {
        Ok([matrix_width, matrix_height])
    }

    fn map_visible_to_matrix(
        &self,
        matrix_width: usize,
        matrix_height: usize,
        x: usize,
        y: usize,
    ) -> [usize; 2] {
        if self.horizontal {
            [matrix_width - 1 - x, y]
        } else {
            [x, matrix_height - 1 - y]
        }
    }
}

struct RotatePixelMapper {
    angle: usize,
}

impl NamedPixelMapper for RotatePixelMapper {
    fn get_size_mapping(
        &self,
        matrix_width: usize,
        matrix_height: usize,
    ) -> Result<[usize; 2], MatrixCreationError> {
        if self.angle % 180 == 0 {
            Ok([matrix_width, matrix_height])
        } else {
            Ok([matrix_height, matrix_width])
        }
    }

    fn map_visible_to_matrix(
        &self,
        matrix_width: usize,
        matrix_height: usize,
        x: usize,
        y: usize,
    ) -> [usize; 2] {
        match self.angle {
            0 => [x, y],
            90 => [matrix_width - y - 1, x],
            180 => [matrix_width - x - 1, matrix_height - y - 1],
            270 => [y, matrix_height - x - 1],
            _ => unreachable!(),
        }
    }
}

struct UArrangeMapper {
    parallel: usize,
}

impl UArrangeMapper {
    fn new_with_parameters(chain: usize, parallel: usize) -> Result<Self, MatrixCreationError> {
        if chain < 2 {
            let message = format!(
                "UArrangeMapper: Chain length needs to be larger than 2 for useful folding"
            );
            return Err(MatrixCreationError::PixelMapperError(message));
        }
        if chain % 2 != 0 {
            let message = format!("UArrangeMapper: Chain length needs to be divisible by 2.");
            return Err(MatrixCreationError::PixelMapperError(message));
        }
        Ok(Self { parallel })
    }
}

impl NamedPixelMapper for UArrangeMapper {
    fn get_size_mapping(
        &self,
        matrix_width: usize,
        matrix_height: usize,
    ) -> Result<[usize; 2], MatrixCreationError> {
        let visible_width = (matrix_width / 64) * 32; // Div at 32px boundary
        let visible_height = 2 * matrix_height;
        if matrix_height % self.parallel != 0 {
            let message = format!(
                "UArrangeMapper: For parallel={} we would expect the \
                height={matrix_height} to be divisible by {}.",
                self.parallel, self.parallel
            );
            return Err(MatrixCreationError::PixelMapperError(message));
        }
        Ok([visible_width, visible_height])
    }

    fn map_visible_to_matrix(
        &self,
        matrix_width: usize,
        matrix_height: usize,
        x: usize,
        y: usize,
    ) -> [usize; 2] {
        let panel_height = matrix_height / self.parallel;
        let visible_width = (matrix_width / 64) * 32;
        let slab_height = 2 * panel_height; // one folded u-shape
        let base_y = (y / slab_height) * panel_height;
        let y_in_slab = y % slab_height;

        let [matrix_x, matrix_y] = if y_in_slab < panel_height {
            // Upper panel of the slab
            [(x + matrix_width / 2), y_in_slab]
        } else {
            // Lower panel of the slab
            [(visible_width - x - 1), (slab_height - y_in_slab - 1)]
        };

        [matrix_x, base_y + matrix_y]
    }
}

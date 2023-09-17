use std::{error::Error, str::FromStr};

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
                        "'{}' is not valid. Mirror parameter should be either 'V' or 'H'",
                        other
                    )
                    .into()),
                },
                "Rotate" => {
                    if let Ok(angle) = param.parse::<usize>() {
                        if angle % 90 != 0 {
                            return Err(format!(
                                "'{}' is not valid. Rotation needs to be a multiple of 90 degrees",
                                angle
                            )
                            .into());
                        }
                        return Ok(Self::Rotate((angle + 360) % 360));
                    }
                    Err("Rotation angle is missing or invalid".into())
                }
                other => Err(format!("'{}' is not a valid Pixel mapping.", other).into()),
            }
        } else {
            Err("Invalid format: no ':' separator found.".into())
        }
    }
}

impl NamedPixelMapperType {
    pub(crate) fn create(self) -> Box<dyn NamedPixelMapper> {
        match self {
            NamedPixelMapperType::Mirror(horizontal) => Box::new(MirrorPixelMapper { horizontal }),
            NamedPixelMapperType::Rotate(angle) => Box::new(RotatePixelMapper { angle }),
        }
    }
}

/// A pixel mapper is a way for you to map pixels of LED matrixes to a different
/// layout. If you have an implementation of a PixelMapper, you can give it
/// to the RGBMatrix::apply_pixel_mapper(), which then presents you a canvas
/// that has the new "visible_width", "visible_height".
pub(crate) trait NamedPixelMapper {
    fn get_size_mapping(&self, matrix_width: usize, matrix_height: usize) -> [usize; 2];

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
    fn get_size_mapping(&self, matrix_width: usize, matrix_height: usize) -> [usize; 2] {
        [matrix_width, matrix_height]
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
    fn get_size_mapping(&self, matrix_width: usize, matrix_height: usize) -> [usize; 2] {
        if self.angle % 180 == 0 {
            [matrix_width, matrix_height]
        } else {
            [matrix_height, matrix_width]
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

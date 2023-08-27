use std::{error::Error, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamedPixelMapperType {
    Mirror(bool),
    Rotate(usize),
}

impl FromStr for NamedPixelMapperType {
    type Err = Box<dyn Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(':').collect();

        match parts[0] {
            "Mirror" => match parts.get(1).map(|&param| param) {
                Some("H") | Some("h") => Ok(Self::Mirror(true)),
                Some("V") | Some("v") => Ok(Self::Mirror(false)),
                Some(other) => Err(format!(
                    "'{}' is not valid. Mirror parameter should be either 'V' or 'H'",
                    other
                )
                .into()),
                None => Err("Mirror parameter is missing".into()),
            },
            "Rotate" => {
                match parts
                    .get(1)
                    .and_then(|angle_str| angle_str.parse::<usize>().ok())
                {
                    Some(angle) if angle % 90 != 0 => Err(format!(
                        "'{}' is not valid. Rotation needs to be a multiple of 90 degrees",
                        angle
                    )
                    .into()),
                    Some(angle) => Ok(Self::Rotate((angle + 360) % 360)),
                    None => Err("Rotation angle is missing or invalid".into()),
                }
            }
            other => Err(format!("'{}' is not a valid Pixel mapping.", other).into()),
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

use std::str::FromStr;

use crate::{error::InvalidPixelMapperError, rgb_matrix::MatrixCreationError};

/// The direction in which [`NamedPixelMapperType::Mirror`] flips the display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MirrorDirection {
    /// Swap left and right.
    Horizontal,
    /// Swap top and bottom.
    Vertical,
}

/// The clockwise angle by which [`NamedPixelMapperType::Rotate`] turns the display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rotation {
    /// Keep the display as it is.
    Deg0,
    /// A quarter turn clockwise.
    Deg90,
    /// A half turn.
    Deg180,
    /// A quarter turn counterclockwise.
    Deg270,
}

/// Enum representing different pixel mapping options for mapping the logical layout of your boards
/// to your physical arrangement. These options allow you to customize the mapping to match your unique setup.
///
/// These options can be used with the `--pixelmapper` flag to choose between different mappings.
///
/// You can apply multiple mappers in your configuration, and they will be applied in the order you specify.
/// For example, to first mirror the panels horizontally and then rotate the resulting screen,
/// You can use `--pixelmapper Mirror:H --pixelmapper Rotate:90`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NamedPixelMapperType {
    /// The "Mirror" mapper allows you to mirror the output either horizontally or vertically.
    /// Specify 'H' for horizontal mirroring or 'V' for vertical mirroring as a parameter after a colon.
    /// Example: `--pixelmapper Mirror:H`
    Mirror(MirrorDirection),
    /// The "Rotate" mapper allows you to rotate your screen clockwise by a multiple of 90 degrees.
    /// Specify the desired angle as a parameter after a colon. Negative angles rotate counterclockwise.
    /// Example: `--pixelmapper Rotate:90` for a 90-degree rotation.
    Rotate(Rotation),
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
    /// ```text
    ///    [<][<][<][<] }- Raspberry Pi connector
    /// ```
    ///
    /// can be arranged in this 64x64 U-shaped display:
    /// ```text
    ///    [<][<] }----- Raspberry Pi connector
    ///    [>][>]
    /// ```
    ///
    /// This U-shape configuration can also be applied to displays with multiple chains.
    /// For instance, an arrangement with two chains, each consisting of 8 panels, can be represented as follows:
    /// ```text
    ///   [<][<][<][<]  }--- Pi connector #1
    ///   [>][>][>][>]
    ///   [<][<][<][<]  }--- Pi connector #2
    ///   [>][>][>][>]
    /// ```
    UMapper,

    /// The `ChainLink` represents a pixel mapping strategy where a long chain
    /// of display panels is arranged in a linear, single-direction chain.
    ///
    /// For instance, a chain consisting of 12 panels, with 4 panels
    /// (`chain_length` = 4, `parallel` = 3) per connector, can be represented as follows:
    /// ```text
    ///   [>][>][>][>] [>][>][>][>] [>][>][>][>]
    ///    │            │            └─ Pi connector #3
    ///    │            └─ Pi connector #2
    ///    └─> Pi connector #1
    /// ```
    ChainLink,

    /// The "V-mapper" stacks the panels of each chain vertically instead of horizontally, with the parallel
    /// chains next to each other.
    ///
    /// The optional parameter 'Z' flips every other panel upside down so that cabling can be shorter:
    /// ```text
    /// [ O < I ]   without Z       [ O < I  ]
    ///   ,---^      <----                ^
    /// [ O < I ]                   [ I > O  ]
    ///   ,---^            with Z     ^
    /// [ O < I ]            --->   [ O < I  ]
    /// ```
    /// Example: `--pixelmapper V-mapper` or `--pixelmapper V-mapper:Z`
    VMapper(bool),

    /// The "StackToRow" mapper takes the vertically stacked parallel chains and arranges them in a horizontal row.
    ///
    /// Optional parameters: 'Z' flips every other chain (serpentine) and 'F' flips the right half by 180 degrees
    /// (for windmill symmetry).
    /// Example: `--pixelmapper StackToRow` or `--pixelmapper StackToRow:ZF`
    StackToRow { serpentine: bool, flip_right: bool },

    /// The "Remap" mapper places every panel at an arbitrary position and orientation on a canvas of the given
    /// size. See [`RemapLayout`] for the format.
    /// Example: `--pixelmapper "Remap:64,64|0,0n|32,0e|0,32x"`
    Remap(RemapLayout),
}

/// The layout for [`NamedPixelMapperType::Remap`], parsed from `<width>,<height>|<panel>|<panel>...`.
///
/// There is one entry per panel, starting with the first panel of the first chain. Panels are counted from the
/// Raspberry Pi, so the first panel is the top right one. Each entry is `<x>,<y><orientation>`: the position of
/// the panel's top left corner on the canvas and the side the panel's top points to. The orientation is one of
/// `n` (north, not rotated), `e` (east), `s` (south) and `w` (west), or `x` to leave the panel out.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RemapLayout {
    width: usize,
    height: usize,
    tiles: Vec<RemapTile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct RemapTile {
    x: isize,
    y: isize,
    orientation: Option<Orientation>,
}

/// Rotation of a remapped panel. The direction is opposite to the Rotate mapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Orientation {
    Rot0,
    Rot90,
    Rot180,
    Rot270,
}

impl FromStr for RemapLayout {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('|');
        let size = parts.next().unwrap_or_default();
        let (width, height) = size
            .split_once(',')
            .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)))
            .ok_or_else(|| format!("Expected '<width>,<height>' at the start, got '{size}'."))?;
        let tiles = parts
            .enumerate()
            .map(|(index, tile)| {
                let invalid =
                    || format!("Panel {index} '{tile}' is not in the format '<x>,<y><n|e|s|w|x>'.");
                let tile = tile.trim();
                let (position, orientation) = tile
                    .split_at_checked(tile.len().saturating_sub(1))
                    .ok_or_else(invalid)?;
                let orientation = match orientation.to_ascii_lowercase().as_str() {
                    "n" => Some(Orientation::Rot0),
                    "s" => Some(Orientation::Rot180),
                    "e" => Some(Orientation::Rot270),
                    "w" => Some(Orientation::Rot90),
                    "x" => None,
                    _ => return Err(invalid()),
                };
                let (x, y) = position.split_once(',').ok_or_else(invalid)?;
                Ok(RemapTile {
                    x: x.trim().parse().map_err(|_| invalid())?,
                    y: y.trim().parse().map_err(|_| invalid())?,
                    orientation,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            width,
            height,
            tiles,
        })
    }
}

impl FromStr for NamedPixelMapperType {
    type Err = InvalidPixelMapperError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (command, param) = s.split_once(':').unwrap_or((s, ""));
        match command {
            "U-mapper" if param.is_empty() => return Ok(Self::UMapper),
            "ChainLink" if param.is_empty() => return Ok(Self::ChainLink),
            "V-mapper" => {
                return match param {
                    "" => Ok(Self::VMapper(false)),
                    "Z" | "z" => Ok(Self::VMapper(true)),
                    other => Err(InvalidPixelMapperError::new(format!(
                        "'{other}' is not valid. V-mapper parameter should be either empty or 'Z'"
                    ))),
                };
            }
            "StackToRow" => {
                let (mut serpentine, mut flip_right) = (false, false);
                for c in param.chars().filter(|c| !matches!(c, ',' | ' ')) {
                    match c {
                        'Z' | 'z' => serpentine = true,
                        'F' | 'f' => flip_right = true,
                        other => {
                            return Err(InvalidPixelMapperError::new(format!(
                                "'{other}' is not valid. StackToRow parameters are 'Z' and 'F'"
                            )));
                        }
                    }
                }
                return Ok(Self::StackToRow {
                    serpentine,
                    flip_right,
                });
            }
            "Remap" => {
                return Ok(Self::Remap(
                    param.parse().map_err(InvalidPixelMapperError::new)?,
                ));
            }
            _ => {}
        }
        if let Some((command, param)) = s.split_once(':') {
            match command {
                "Mirror" => match param {
                    "H" | "h" => Ok(Self::Mirror(MirrorDirection::Horizontal)),
                    "V" | "v" => Ok(Self::Mirror(MirrorDirection::Vertical)),
                    other => Err(InvalidPixelMapperError::new(format!(
                        "'{other}' is not valid. Mirror parameter should be either 'V' or 'H'"
                    ))),
                },
                "Rotate" => {
                    let Ok(angle) = param.parse::<i64>() else {
                        return Err(InvalidPixelMapperError::new(
                            "Rotation angle is missing or invalid",
                        ));
                    };
                    match angle.rem_euclid(360) {
                        0 => Ok(Self::Rotate(Rotation::Deg0)),
                        90 => Ok(Self::Rotate(Rotation::Deg90)),
                        180 => Ok(Self::Rotate(Rotation::Deg180)),
                        270 => Ok(Self::Rotate(Rotation::Deg270)),
                        _ => Err(InvalidPixelMapperError::new(format!(
                            "'{angle}' is not valid. Rotation needs to be a multiple of 90 degrees"
                        ))),
                    }
                }
                other => Err(InvalidPixelMapperError::new(format!(
                    "'{other}' is not a valid Pixel mapping."
                ))),
            }
        } else {
            Err(InvalidPixelMapperError::new(format!(
                "'{s}' is not a valid Pixel mapping."
            )))
        }
    }
}

impl NamedPixelMapperType {
    pub(crate) fn create(
        &self,
        chain: usize,
        parallel: usize,
    ) -> Result<Box<dyn NamedPixelMapper>, MatrixCreationError> {
        match *self {
            NamedPixelMapperType::Mirror(direction) => {
                Ok(Box::new(MirrorPixelMapper { direction }))
            }
            NamedPixelMapperType::Rotate(rotation) => Ok(Box::new(RotatePixelMapper { rotation })),
            NamedPixelMapperType::UMapper => Ok(Box::new(UArrangeMapper::new_with_parameters(
                chain, parallel,
            )?)),
            NamedPixelMapperType::ChainLink => {
                Ok(Box::new(ChainLinkMapper::new_with_parameters(parallel)?))
            }
            NamedPixelMapperType::VMapper(z) => Ok(Box::new(VerticalMapper { z, chain, parallel })),
            NamedPixelMapperType::StackToRow {
                serpentine,
                flip_right,
            } => Ok(Box::new(StackToRowMapper {
                serpentine,
                flip_right,
                bands: parallel,
            })),
            NamedPixelMapperType::Remap(ref layout) => {
                if layout.tiles.len() != chain * parallel {
                    return Err(MatrixCreationError::PixelMapperError(format!(
                        "Remap needs one entry for each of the {} panels, got {}.",
                        chain * parallel,
                        layout.tiles.len()
                    )));
                }
                Ok(Box::new(RemapMapper {
                    layout: layout.clone(),
                    chain,
                    parallel,
                }))
            }
        }
    }
}

/// In which direction a pixel mapper maps coordinates.
pub(crate) enum MappingDirection {
    /// Every visible pixel is mapped to a matrix pixel.
    VisibleToMatrix,
    /// Every matrix pixel is mapped to at most one visible pixel. Visible pixels without a matrix pixel stay off.
    MatrixToVisible,
}

/// A pixel mapper is a way for you to map pixels of LED matrixes to a different
/// layout. If you have an implementation of a [`PixelMapper`], you can give it
/// to the [`RGBMatrix::apply_pixel_mapper`], which then presents you a canvas
/// that has the new [`visible_width`], [`visible_height`].
pub(crate) trait NamedPixelMapper {
    fn size_mapping(
        &self,
        matrix_width: usize,
        matrix_height: usize,
    ) -> Result<[usize; 2], MatrixCreationError>;

    fn mapping_direction(&self) -> MappingDirection {
        MappingDirection::VisibleToMatrix
    }

    /// Used for [`MappingDirection::VisibleToMatrix`].
    fn map_visible_to_matrix(
        &self,
        matrix_width: usize,
        matrix_height: usize,
        visible_x: usize,
        visible_y: usize,
    ) -> [usize; 2];

    /// Used for [`MappingDirection::MatrixToVisible`]. Returns `None` if the matrix pixel is not visible.
    fn map_matrix_to_visible(
        &self,
        _matrix_width: usize,
        _matrix_height: usize,
        _matrix_x: usize,
        _matrix_y: usize,
    ) -> Option<[usize; 2]> {
        None
    }
}

struct MirrorPixelMapper {
    direction: MirrorDirection,
}

impl NamedPixelMapper for MirrorPixelMapper {
    fn size_mapping(
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
        match self.direction {
            MirrorDirection::Horizontal => [matrix_width - 1 - x, y],
            MirrorDirection::Vertical => [x, matrix_height - 1 - y],
        }
    }
}

struct RotatePixelMapper {
    rotation: Rotation,
}

impl NamedPixelMapper for RotatePixelMapper {
    fn size_mapping(
        &self,
        matrix_width: usize,
        matrix_height: usize,
    ) -> Result<[usize; 2], MatrixCreationError> {
        match self.rotation {
            Rotation::Deg0 | Rotation::Deg180 => Ok([matrix_width, matrix_height]),
            Rotation::Deg90 | Rotation::Deg270 => Ok([matrix_height, matrix_width]),
        }
    }

    fn map_visible_to_matrix(
        &self,
        matrix_width: usize,
        matrix_height: usize,
        x: usize,
        y: usize,
    ) -> [usize; 2] {
        match self.rotation {
            Rotation::Deg0 => [x, y],
            Rotation::Deg90 => [matrix_width - y - 1, x],
            Rotation::Deg180 => [matrix_width - x - 1, matrix_height - y - 1],
            Rotation::Deg270 => [y, matrix_height - x - 1],
        }
    }
}

struct UArrangeMapper {
    parallel: usize,
}

impl UArrangeMapper {
    fn new_with_parameters(chain: usize, parallel: usize) -> Result<Self, MatrixCreationError> {
        if chain < 2 {
            let message = "UArrangeMapper: Chain length needs to be at least 2 for useful folding"
                .to_string();
            return Err(MatrixCreationError::PixelMapperError(message));
        }
        if !chain.is_multiple_of(2) {
            let message = "UArrangeMapper: Chain length needs to be divisible by 2.".to_string();
            return Err(MatrixCreationError::PixelMapperError(message));
        }
        Ok(Self { parallel })
    }
}

impl NamedPixelMapper for UArrangeMapper {
    fn size_mapping(
        &self,
        matrix_width: usize,
        matrix_height: usize,
    ) -> Result<[usize; 2], MatrixCreationError> {
        // The chain length is even, so this is a panel boundary.
        let visible_width = matrix_width / 2;
        let visible_height = 2 * matrix_height;
        if !matrix_height.is_multiple_of(self.parallel) {
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
        let visible_width = matrix_width / 2;
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

struct ChainLinkMapper {
    parallel: usize,
}

impl ChainLinkMapper {
    fn new_with_parameters(parallel: usize) -> Result<Self, MatrixCreationError> {
        Ok(Self { parallel })
    }
}

impl NamedPixelMapper for ChainLinkMapper {
    fn size_mapping(
        &self,
        matrix_width: usize,
        matrix_height: usize,
    ) -> Result<[usize; 2], MatrixCreationError> {
        Ok([matrix_width * self.parallel, matrix_height / self.parallel])
    }

    fn map_visible_to_matrix(
        &self,
        matrix_width: usize,
        matrix_height: usize,
        visible_x: usize,
        visible_y: usize,
    ) -> [usize; 2] {
        [
            visible_x % matrix_width,
            (matrix_height / self.parallel) * (visible_x / matrix_width) + visible_y,
        ]
    }
}

struct VerticalMapper {
    z: bool,
    chain: usize,
    parallel: usize,
}

impl NamedPixelMapper for VerticalMapper {
    fn size_mapping(
        &self,
        matrix_width: usize,
        matrix_height: usize,
    ) -> Result<[usize; 2], MatrixCreationError> {
        Ok([
            matrix_width * self.parallel / self.chain,
            matrix_height * self.chain / self.parallel,
        ])
    }

    fn map_visible_to_matrix(
        &self,
        matrix_width: usize,
        matrix_height: usize,
        x: usize,
        y: usize,
    ) -> [usize; 2] {
        let panel_width = matrix_width / self.chain;
        let panel_height = matrix_height / self.parallel;
        // The panel you plug into ends up being the bottom panel while coordinates start from the top panel. You
        // typically don't wire the bottom panel (first in the chain) upside down, so whether each panel gets
        // flipped depends on the number of panels.
        let odd_panel_count = (matrix_width / panel_width) % 2;
        let x_panel_start = y / panel_height * panel_width;
        let y_panel_start = x / panel_width * panel_height;
        let x_within_panel = x % panel_width;
        let y_within_panel = y % panel_height;
        let needs_flipping = self.z && odd_panel_count == (y / panel_height) % 2;
        if needs_flipping {
            [
                x_panel_start + panel_width - 1 - x_within_panel,
                y_panel_start + panel_height - 1 - y_within_panel,
            ]
        } else {
            [
                x_panel_start + x_within_panel,
                y_panel_start + y_within_panel,
            ]
        }
    }
}

struct StackToRowMapper {
    serpentine: bool,
    flip_right: bool,
    bands: usize,
}

impl NamedPixelMapper for StackToRowMapper {
    fn size_mapping(
        &self,
        matrix_width: usize,
        matrix_height: usize,
    ) -> Result<[usize; 2], MatrixCreationError> {
        Ok([matrix_width * self.bands, matrix_height / self.bands])
    }

    fn map_visible_to_matrix(
        &self,
        matrix_width: usize,
        matrix_height: usize,
        x: usize,
        y: usize,
    ) -> [usize; 2] {
        let band_width = matrix_width;
        let band_height = matrix_height / self.bands;
        let band = x / band_width;
        let mut src_x = x % band_width;
        let mut src_y = y;

        // Each flip rotates the band by 180 degrees, so two of them cancel out.
        let flip_serpentine = self.serpentine && band % 2 == 1;
        let flip_right = self.flip_right && band >= self.bands / 2;
        if flip_serpentine != flip_right {
            src_x = band_width - 1 - src_x;
            src_y = band_height - 1 - src_y;
        }

        [src_x, band * band_height + src_y]
    }
}

struct RemapMapper {
    layout: RemapLayout,
    chain: usize,
    parallel: usize,
}

impl RemapTile {
    /// Maps a pixel of the panel to the canvas. The result can be outside of the canvas.
    fn map_to_visible(
        &self,
        panel_width: usize,
        panel_height: usize,
        x: usize,
        y: usize,
    ) -> Option<[isize; 2]> {
        let [w, h, x, y] = [panel_width, panel_height, x, y].map(|v| v as isize);
        let [dx, dy] = match self.orientation? {
            Orientation::Rot0 => [x, y],
            Orientation::Rot90 => [y, w - x - 1],
            Orientation::Rot180 => [w - x - 1, h - y - 1],
            Orientation::Rot270 => [h - y - 1, x],
        };
        Some([self.x + dx, self.y + dy])
    }
}

impl RemapMapper {
    fn visible(&self, [x, y]: [isize; 2]) -> Option<[usize; 2]> {
        let x = usize::try_from(x).ok().filter(|x| *x < self.layout.width)?;
        let y = usize::try_from(y)
            .ok()
            .filter(|y| *y < self.layout.height)?;
        Some([x, y])
    }
}

impl NamedPixelMapper for RemapMapper {
    fn size_mapping(
        &self,
        matrix_width: usize,
        matrix_height: usize,
    ) -> Result<[usize; 2], MatrixCreationError> {
        let panel_width = matrix_width / self.chain;
        let panel_height = matrix_height / self.parallel;
        let [width, height] = [self.layout.width, self.layout.height].map(|v| v as isize);
        // Check that all panels are at least partially visible. Opposite corners give the panel's placement.
        for (index, tile) in self.layout.tiles.iter().enumerate() {
            let Some([x0, y0]) = tile.map_to_visible(panel_width, panel_height, 0, 0) else {
                continue;
            };
            let [x1, y1] = tile
                .map_to_visible(panel_width, panel_height, panel_width - 1, panel_height - 1)
                .expect("Tile is used.");
            let [x_min, x_max] = [x0.min(x1), x0.max(x1)];
            let [y_min, y_max] = [y0.min(y1), y0.max(y1)];
            if x_max < 0 || x_min >= width || y_max < 0 || y_min >= height {
                return Err(MatrixCreationError::PixelMapperError(format!(
                    "Remap: panel {index} is fully outside of the visible area."
                )));
            }
        }
        Ok([self.layout.width, self.layout.height])
    }

    fn mapping_direction(&self) -> MappingDirection {
        MappingDirection::MatrixToVisible
    }

    fn map_visible_to_matrix(&self, _: usize, _: usize, _: usize, _: usize) -> [usize; 2] {
        unreachable!("Remap maps from the matrix to the visible area.")
    }

    fn map_matrix_to_visible(
        &self,
        matrix_width: usize,
        matrix_height: usize,
        x: usize,
        y: usize,
    ) -> Option<[usize; 2]> {
        let panel_width = matrix_width / self.chain;
        let panel_height = matrix_height / self.parallel;
        // Count panels from the Raspberry Pi, so the first panel is the top right one.
        let panel_col = self.chain - x / panel_width - 1;
        let panel_row = y / panel_height;
        let tile = &self.layout.tiles[panel_row * self.chain + panel_col];
        tile.map_to_visible(panel_width, panel_height, x % panel_width, y % panel_height)
            .and_then(|position| self.visible(position))
    }
}

#[cfg(test)]
mod tests {
    use super::{MirrorDirection, NamedPixelMapperType, Rotation};
    use std::str::FromStr;

    #[test]
    fn parses_rotation() {
        let parse = |s| NamedPixelMapperType::from_str(s).ok();
        let rotate = |rotation| Some(NamedPixelMapperType::Rotate(rotation));
        assert_eq!(parse("Rotate:90"), rotate(Rotation::Deg90));
        assert_eq!(parse("Rotate:-90"), rotate(Rotation::Deg270));
        assert_eq!(parse("Rotate:450"), rotate(Rotation::Deg90));
        assert_eq!(parse("Rotate:45"), None);
        assert_eq!(
            parse("Mirror:v"),
            Some(NamedPixelMapperType::Mirror(MirrorDirection::Vertical))
        );
    }

    #[test]
    fn parses_new_mappers() {
        let parse = |s| NamedPixelMapperType::from_str(s).ok();
        assert_eq!(
            parse("V-mapper"),
            Some(NamedPixelMapperType::VMapper(false))
        );
        assert_eq!(
            parse("V-mapper:Z"),
            Some(NamedPixelMapperType::VMapper(true))
        );
        assert_eq!(parse("V-mapper:Q"), None);
        assert_eq!(
            parse("StackToRow:Z,F"),
            Some(NamedPixelMapperType::StackToRow {
                serpentine: true,
                flip_right: true
            })
        );
        assert_eq!(parse("StackToRow:Q"), None);
        assert!(parse("Remap:64,32|0,0n|-8,16w|0,0x").is_some());
        assert_eq!(parse("Remap:64,32|0,0q"), None);
        assert_eq!(parse("Remap:64|0,0n"), None);
        assert_eq!(parse("U-mapper:Z"), None);
    }

    /// Maps every visible pixel of a `chain` x `parallel` arrangement of 32x16 panels.
    fn map_all(
        mapper: &NamedPixelMapperType,
        chain: usize,
        parallel: usize,
    ) -> Vec<Vec<[usize; 2]>> {
        let mapper = mapper.create(chain, parallel).unwrap();
        let [w, h] = [32 * chain, 16 * parallel];
        let [vw, vh] = mapper.size_mapping(w, h).unwrap();
        (0..vh)
            .map(|y| {
                (0..vw)
                    .map(|x| mapper.map_visible_to_matrix(w, h, x, y))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn v_mapper_stacks_chain_vertically() {
        let map = map_all(&NamedPixelMapperType::VMapper(false), 2, 1);
        assert_eq!((map[0].len(), map.len()), (32, 32));
        assert_eq!(map[0][0], [0, 0]);
        assert_eq!(map[16][0], [32, 0]);
        // With Z, the panel in the odd position of an even chain is upside down.
        let map = map_all(&NamedPixelMapperType::VMapper(true), 2, 1);
        assert_eq!(map[0][0], [31, 15]);
        assert_eq!(map[16][0], [32, 0]);
    }

    #[test]
    fn stack_to_row_puts_chains_next_to_each_other() {
        let plain = NamedPixelMapperType::StackToRow {
            serpentine: false,
            flip_right: false,
        };
        let map = map_all(&plain, 1, 2);
        assert_eq!((map[0].len(), map.len()), (64, 16));
        assert_eq!(map[0][32], [0, 16]);
        let serpentine = NamedPixelMapperType::StackToRow {
            serpentine: true,
            flip_right: false,
        };
        assert_eq!(map_all(&serpentine, 1, 2)[0][32], [31, 31]);
        let both = NamedPixelMapperType::StackToRow {
            serpentine: true,
            flip_right: true,
        };
        // Both flips cancel out on the second band.
        assert_eq!(map_all(&both, 1, 2)[0][32], [0, 16]);
    }

    #[test]
    fn remap_places_panels() {
        // Two 32x16 panels in one chain, the second one below the first and upside down.
        let remap = NamedPixelMapperType::from_str("Remap:32,32|0,0n|0,16s").unwrap();
        let mapper = remap.create(2, 1).unwrap();
        assert_eq!(mapper.size_mapping(64, 16).unwrap(), [32, 32]);
        // The first panel is the one furthest right in matrix coordinates.
        assert_eq!(mapper.map_matrix_to_visible(64, 16, 32, 0), Some([0, 0]));
        assert_eq!(mapper.map_matrix_to_visible(64, 16, 0, 0), Some([31, 31]));

        let outside = NamedPixelMapperType::from_str("Remap:32,16|0,0n|100,0n").unwrap();
        assert!(
            outside
                .create(2, 1)
                .unwrap()
                .size_mapping(64, 16)
                .is_err()
        );
        let discarded = NamedPixelMapperType::from_str("Remap:32,16|0,0n|0,0x").unwrap();
        assert_eq!(
            discarded
                .create(2, 1)
                .unwrap()
                .map_matrix_to_visible(64, 16, 0, 0),
            None
        );
        assert!(discarded.create(1, 1).is_err());
    }

    #[test]
    fn u_mapper_folds_panels_of_any_width() {
        // Two 40x20 panels in one chain fold into a 40x40 canvas.
        let mapper = NamedPixelMapperType::UMapper.create(2, 1).unwrap();
        assert_eq!(mapper.size_mapping(80, 20).unwrap(), [40, 40]);
        assert_eq!(mapper.map_visible_to_matrix(80, 20, 0, 0), [40, 0]);
        assert_eq!(mapper.map_visible_to_matrix(80, 20, 0, 20), [39, 19]);
    }
}

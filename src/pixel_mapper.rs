use crate::{
    multiplex_mapper::MultiplexMapper,
    named_pixel_mapper::{MappingDirection, NamedPixelMapper},
    rgb_matrix::MatrixCreationError,
};

/// Maps the visible pixels of the canvas to a different layout of the underlying matrix, either to undo the
/// multiplexing of a panel or to arrange panels as chosen with `--pixelmapper`.
pub(crate) enum PixelMapper {
    Multiplex(MultiplexMapper),
    Named(Box<dyn NamedPixelMapper>),
}

impl PixelMapper {
    /// Given an underlying matrix (width, height), returns the
    /// visible (width, height) after the mapping.
    /// E.g. a 90 degree rotation might map matrix=(64, 32) -> visible=(32, 64)
    /// Some multiplexing matrices will double the height and half the width.
    pub(crate) fn size_mapping(
        &self,
        matrix_width: usize,
        matrix_height: usize,
    ) -> Result<[usize; 2], MatrixCreationError> {
        match self {
            PixelMapper::Multiplex(mapper) => mapper.size_mapping(matrix_width, matrix_height),
            PixelMapper::Named(mapper) => mapper.size_mapping(matrix_width, matrix_height),
        }
    }

    /// Map where a visible pixel (x,y) is mapped to the underlying matrix (x,y), or `None` if the mapper can't
    /// map it, e.g. because the panel is larger than the layout of a multiplexer.
    pub(crate) fn map_visible_to_matrix(
        &self,
        matrix_width: usize,
        matrix_height: usize,
        visible_x: usize,
        visible_y: usize,
    ) -> Option<[usize; 2]> {
        match self {
            PixelMapper::Multiplex(mapper) => {
                mapper.map_visible_to_matrix(matrix_width, matrix_height, visible_x, visible_y)
            }
            PixelMapper::Named(mapper) => Some(mapper.map_visible_to_matrix(
                matrix_width,
                matrix_height,
                visible_x,
                visible_y,
            )),
        }
    }

    pub(crate) fn mapping_direction(&self) -> MappingDirection {
        match self {
            PixelMapper::Multiplex(_) => MappingDirection::VisibleToMatrix,
            PixelMapper::Named(mapper) => mapper.mapping_direction(),
        }
    }

    /// Map where an underlying matrix pixel (x,y) is visible, if at all.
    pub(crate) fn map_matrix_to_visible(
        &self,
        matrix_width: usize,
        matrix_height: usize,
        matrix_x: usize,
        matrix_y: usize,
    ) -> Option<[usize; 2]> {
        match self {
            PixelMapper::Multiplex(_) => None,
            PixelMapper::Named(mapper) => {
                mapper.map_matrix_to_visible(matrix_width, matrix_height, matrix_x, matrix_y)
            }
        }
    }
}

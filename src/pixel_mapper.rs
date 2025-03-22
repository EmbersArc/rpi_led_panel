use crate::{
    multiplex_mapper::MultiplexMapper, named_pixel_mapper::NamedPixelMapper,
    rgb_matrix::MatrixCreationError,
};

/// A pixel mapper is a way for you to map pixels of LED matrixes to a different
/// layout. If you have an implementation of a [`PixelMapper`], you can give it
/// to the `RGBMatrix::apply_pixel_mapper()`, which then presents you with a canvas
/// that has the new "visible width" and "visible height".
pub(crate) enum PixelMapper {
    Multiplex(Box<dyn MultiplexMapper>),
    Named(Box<dyn NamedPixelMapper>),
}

impl PixelMapper {
    /// Given a underlying matrix (width, height), returns the
    /// visible (width, height) after the mapping.
    /// E.g. a 90 degree rotation might map matrix=(64, 32) -> visible=(32, 64)
    /// Some multiplexing matrices will double the height and half the width.
    pub(crate) fn get_size_mapping(
        &self,
        matrix_width: usize,
        matrix_height: usize,
    ) -> Result<[usize; 2], MatrixCreationError> {
        match self {
            PixelMapper::Multiplex(mapper) => mapper.get_size_mapping(matrix_width, matrix_height),
            PixelMapper::Named(mapper) => mapper.get_size_mapping(matrix_width, matrix_height),
        }
    }

    /// Map where a visible pixel (x,y) is mapped to the underlying matrix (x,y).
    pub(crate) fn map_visible_to_matrix(
        &self,
        matrix_width: usize,
        matrix_height: usize,
        visible_x: usize,
        visible_y: usize,
    ) -> [usize; 2] {
        match self {
            PixelMapper::Multiplex(mapper) => {
                mapper.map_visible_to_matrix(matrix_width, matrix_height, visible_x, visible_y)
            }
            PixelMapper::Named(mapper) => {
                mapper.map_visible_to_matrix(matrix_width, matrix_height, visible_x, visible_y)
            }
        }
    }
}

use crate::{multiplex_mapper::MultiplexMapper, named_pixel_mapper::NamedPixelMapper};

/// A pixel mapper is a way for you to map pixels of LED matrixes to a different
/// layout. If you have an implementation of a PixelMapper, you can give it
/// to the RGBMatrix::apply_pixel_mapper(), which then presents you with a canvas
/// that has the new "visible width" and "visible height".
pub(crate) trait PixelMapper {
    /// Given a underlying matrix (width, height), returns the
    /// visible (width, height) after the mapping.
    /// E.g. a 90 degree rotation might map matrix=(64, 32) -> visible=(32, 64)
    /// Some multiplexing matrices will double the height and half the width.
    fn get_size_mapping(&self, matrix_width: usize, matrix_height: usize) -> [usize; 2];

    /// Map where a visible pixel (x,y) is mapped to the underlying matrix (x,y).
    fn map_visible_to_matrix(
        &self,
        matrix_width: usize,
        matrix_height: usize,
        visible_x: usize,
        visible_y: usize,
    ) -> [usize; 2];
}

pub(crate) struct MultiplexMapperWrapper(pub(crate) Box<dyn MultiplexMapper>);

impl PixelMapper for MultiplexMapperWrapper {
    fn get_size_mapping(&self, matrix_width: usize, matrix_height: usize) -> [usize; 2] {
        // Delegate the call to the underlying MultiplexMapper
        self.0.get_size_mapping(matrix_width, matrix_height)
    }

    fn map_visible_to_matrix(
        &self,
        matrix_width: usize,
        matrix_height: usize,
        visible_x: usize,
        visible_y: usize,
    ) -> [usize; 2] {
        // Delegate the call to the underlying MultiplexMapper
        self.0
            .map_visible_to_matrix(matrix_width, matrix_height, visible_x, visible_y)
    }
}

pub(crate) struct NamedPixelMapperWrapper(pub(crate) Box<dyn NamedPixelMapper>);

impl PixelMapper for NamedPixelMapperWrapper {
    fn get_size_mapping(&self, width: usize, height: usize) -> [usize; 2] {
        // Delegate the call to the underlying NamedPixelMapper
        self.0.get_size_mapping(width, height)
    }

    fn map_visible_to_matrix(
        &self,
        old_width: usize,
        old_height: usize,
        x: usize,
        y: usize,
    ) -> [usize; 2] {
        // Delegate the call to the underlying NamedPixelMapper
        self.0.map_visible_to_matrix(old_width, old_height, x, y)
    }
}

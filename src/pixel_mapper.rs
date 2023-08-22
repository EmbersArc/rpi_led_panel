use crate::multiplex_mapper::MultiplexMapper;

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

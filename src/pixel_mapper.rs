use crate::rgb_matrix::MatrixCreationError;

/// In which direction a pixel mapper maps coordinates.
pub(crate) enum MappingDirection {
    /// Every visible pixel is mapped to a matrix pixel.
    VisibleToMatrix,
    /// Every matrix pixel is mapped to at most one visible pixel. Visible pixels without a matrix pixel stay off.
    MatrixToVisible,
}

/// Maps the visible pixels of the canvas to a different layout of the underlying matrix, either to undo the
/// multiplexing of a panel or to arrange panels as chosen with `--pixelmapper`.
pub(crate) trait PixelMapper {
    /// Given an underlying matrix (width, height), returns the
    /// visible (width, height) after the mapping.
    /// E.g. a 90 degree rotation might map matrix=(64, 32) -> visible=(32, 64)
    /// Some multiplexing matrices will double the height and half the width.
    fn size_mapping(
        &self,
        matrix_width: usize,
        matrix_height: usize,
    ) -> Result<[usize; 2], MatrixCreationError>;

    fn mapping_direction(&self) -> MappingDirection {
        MappingDirection::VisibleToMatrix
    }

    /// Map where a visible pixel (x,y) is mapped to the underlying matrix (x,y), or `None` if the mapper can't
    /// map it, e.g. because the panel is larger than the layout of a multiplexer. Used for
    /// [`MappingDirection::VisibleToMatrix`].
    fn map_visible_to_matrix(
        &self,
        matrix_width: usize,
        matrix_height: usize,
        visible_x: usize,
        visible_y: usize,
    ) -> Option<[usize; 2]>;

    /// Map where an underlying matrix pixel (x,y) is visible, if at all. Used for
    /// [`MappingDirection::MatrixToVisible`].
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

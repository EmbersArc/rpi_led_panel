use crate::rgb_matrix::MatrixCreationError;

/// Maps the visible pixels of the canvas to a different layout of the underlying matrix, either to undo the
/// multiplexing of a panel or to arrange panels as chosen with `--pixelmapper`.
pub(crate) trait PixelMapper {
    /// Given an underlying matrix (width, height), returns the
    /// visible (width, height) after the mapping.
    /// E.g. a 90 degree rotation might map matrix=(64, 32) -> visible=(32, 64)
    /// Some multiplexing matrices will double the height and halve the width.
    fn size_mapping(
        &self,
        matrix_width: usize,
        matrix_height: usize,
    ) -> Result<[usize; 2], MatrixCreationError>;

    /// Map where a visible pixel (x,y) is mapped to the underlying matrix (x,y).
    fn map_visible_to_matrix(
        &self,
        matrix_width: usize,
        matrix_height: usize,
        visible_x: usize,
        visible_y: usize,
    ) -> [usize; 2];
}

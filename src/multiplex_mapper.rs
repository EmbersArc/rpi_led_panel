use crate::{error::InvalidVariantError, rgb_matrix::MatrixCreationError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumString, strum::VariantNames)]
#[strum(
    parse_err_fn = InvalidVariantError::new::<Self>,
    parse_err_ty = InvalidVariantError
)]
pub enum MultiplexMapperType {
    Stripe,
    Checkered,
    Spiral,
    ZStripe08,
    ZStripe44,
    ZStripe80,
    Coreman,
    Kaler2Scan,
    P10Z,
    QiangLiQ8,
    InversedZStripe,
    P10Outdoor1R1G1B1,
    P10Outdoor1R1G1B2,
    P10Outdoor1R1G1B3,
    P10Coreman,
    P8Outdoor1R1G1B,
    FlippedStripe,
    P10Outdoor32x16HalfScan,
}

impl MultiplexMapperType {
    pub(crate) fn create(self) -> Box<dyn MultiplexMapper> {
        match self {
            MultiplexMapperType::Stripe => Box::new(StripeMultiplexMapper::new()),
            MultiplexMapperType::Checkered => Box::new(CheckeredMultiplexMapper::new()),
            MultiplexMapperType::Spiral => Box::new(SpiralMultiplexMapper::new()),
            MultiplexMapperType::ZStripe08 => Box::new(ZStripeMultiplexMapper::new(0, 8)),
            MultiplexMapperType::ZStripe44 => Box::new(ZStripeMultiplexMapper::new(4, 4)),
            MultiplexMapperType::ZStripe80 => Box::new(ZStripeMultiplexMapper::new(8, 0)),
            MultiplexMapperType::Coreman => Box::new(CoremanMapper::new()),
            MultiplexMapperType::Kaler2Scan => Box::new(Kaler2ScanMapper::new()),
            MultiplexMapperType::P10Z => Box::new(P10MapperZ::new()),
            MultiplexMapperType::QiangLiQ8 => Box::new(QiangLiQ8::new()),
            MultiplexMapperType::InversedZStripe => Box::new(InversedZStripe::new()),
            MultiplexMapperType::P10Outdoor1R1G1B1 => {
                Box::new(P10Outdoor1R1G1BMultiplexMapper1::new())
            }
            MultiplexMapperType::P10Outdoor1R1G1B2 => {
                Box::new(P10Outdoor1R1G1BMultiplexMapper2::new())
            }
            MultiplexMapperType::P10Outdoor1R1G1B3 => {
                Box::new(P10Outdoor1R1G1BMultiplexMapper3::new())
            }
            MultiplexMapperType::P10Coreman => Box::new(P10CoremanMapper::new()),
            MultiplexMapperType::P8Outdoor1R1G1B => Box::new(P8Outdoor1R1G1BMultiplexMapper::new()),
            MultiplexMapperType::FlippedStripe => Box::new(FlippedStripeMultiplexMapper::new()),
            MultiplexMapperType::P10Outdoor32x16HalfScan => {
                Box::new(P10Outdoor32x16HalfScanMapper::new())
            }
        }
    }
}

pub(crate) trait MultiplexMapper {
    fn panel_rows(&self) -> usize;
    fn panel_cols(&self) -> usize;
    fn panel_rows_mut(&mut self) -> &mut usize;
    fn panel_cols_mut(&mut self) -> &mut usize;
    fn panel_stretch_factor(&self) -> usize;

    fn edit_rows_cols(&mut self, rows: &mut usize, cols: &mut usize) {
        *self.panel_rows_mut() = *rows;
        *self.panel_cols_mut() = *cols;

        *rows /= self.panel_stretch_factor();
        *cols *= self.panel_stretch_factor();
    }

    fn get_size_mapping(
        &self,
        matrix_width: usize,
        matrix_height: usize,
    ) -> Result<[usize; 2], MatrixCreationError> {
        // Matrix width has been altered. Alter it back.
        let visible_width = matrix_width / self.panel_stretch_factor();
        let visible_height = matrix_height * self.panel_stretch_factor();
        Ok([visible_width, visible_height])
    }

    fn map_visible_to_matrix(
        &self,
        _matrix_width: usize,
        _matrix_height: usize,
        visible_x: usize,
        visible_y: usize,
    ) -> [usize; 2] {
        let chained_panel = visible_x / self.panel_cols();
        let parallel_panel = visible_y / self.panel_rows();

        let within_panel_x = visible_x % self.panel_cols();
        let within_panel_y = visible_y % self.panel_rows();

        let [new_x, new_y] = self.map_single_panel(within_panel_x, within_panel_y);
        let matrix_x = chained_panel * self.panel_stretch_factor() * self.panel_cols() + new_x;
        let matrix_y = parallel_panel * self.panel_rows() / self.panel_stretch_factor() + new_y;
        [matrix_x, matrix_y]
    }

    fn map_single_panel(&self, x: usize, y: usize) -> [usize; 2];
}

pub(crate) struct StripeMultiplexMapper {
    panel_rows: usize,
    panel_cols: usize,
    stretch_factor: usize,
}

impl StripeMultiplexMapper {
    pub(crate) fn new() -> Self {
        Self {
            panel_rows: 0,
            panel_cols: 0,
            stretch_factor: 2,
        }
    }
}

impl MultiplexMapper for StripeMultiplexMapper {
    fn panel_rows(&self) -> usize {
        self.panel_rows
    }

    fn panel_cols(&self) -> usize {
        self.panel_cols
    }

    fn panel_rows_mut(&mut self) -> &mut usize {
        &mut self.panel_rows
    }

    fn panel_cols_mut(&mut self) -> &mut usize {
        &mut self.panel_cols
    }

    fn panel_stretch_factor(&self) -> usize {
        self.stretch_factor
    }

    fn map_single_panel(&self, x: usize, y: usize) -> [usize; 2] {
        let is_top_stripe = (y % (self.panel_rows() / 2)) < self.panel_rows() / 4;
        let matrix_x = if is_top_stripe {
            x + self.panel_cols()
        } else {
            x
        };
        let matrix_y =
            (y / (self.panel_rows() / 2)) * (self.panel_rows() / 4) + y % (self.panel_rows() / 4);
        [matrix_x, matrix_y]
    }
}

pub(crate) struct FlippedStripeMultiplexMapper {
    panel_rows: usize,
    panel_cols: usize,
    stretch_factor: usize,
}

impl FlippedStripeMultiplexMapper {
    pub(crate) fn new() -> Self {
        Self {
            panel_rows: 0,
            panel_cols: 0,
            stretch_factor: 2,
        }
    }
}

impl MultiplexMapper for FlippedStripeMultiplexMapper {
    fn panel_rows(&self) -> usize {
        self.panel_rows
    }

    fn panel_cols(&self) -> usize {
        self.panel_cols
    }

    fn panel_rows_mut(&mut self) -> &mut usize {
        &mut self.panel_rows
    }

    fn panel_cols_mut(&mut self) -> &mut usize {
        &mut self.panel_cols
    }

    fn panel_stretch_factor(&self) -> usize {
        self.stretch_factor
    }

    fn map_single_panel(&self, x: usize, y: usize) -> [usize; 2] {
        let is_top_stripe = (y % (self.panel_rows() / 2)) >= self.panel_rows() / 4;
        let matrix_x = if is_top_stripe {
            x + self.panel_cols()
        } else {
            x
        };
        let matrix_y =
            (y / (self.panel_rows() / 2)) * (self.panel_rows() / 4) + y % (self.panel_rows() / 4);
        [matrix_x, matrix_y]
    }
}

pub(crate) struct CheckeredMultiplexMapper {
    panel_rows: usize,
    panel_cols: usize,
    stretch_factor: usize,
}

impl CheckeredMultiplexMapper {
    pub(crate) fn new() -> Self {
        Self {
            panel_rows: 0,
            panel_cols: 0,
            stretch_factor: 2,
        }
    }
}

impl MultiplexMapper for CheckeredMultiplexMapper {
    fn panel_rows(&self) -> usize {
        self.panel_rows
    }

    fn panel_cols(&self) -> usize {
        self.panel_cols
    }

    fn panel_rows_mut(&mut self) -> &mut usize {
        &mut self.panel_rows
    }

    fn panel_cols_mut(&mut self) -> &mut usize {
        &mut self.panel_cols
    }

    fn panel_stretch_factor(&self) -> usize {
        self.stretch_factor
    }

    fn map_single_panel(&self, x: usize, y: usize) -> [usize; 2] {
        let is_top_check = (y % (self.panel_rows() / 2)) < self.panel_rows() / 4;
        let is_left_check = x < self.panel_cols() / 2;
        let matrix_x = if is_top_check {
            if is_left_check {
                x + self.panel_cols() / 2
            } else {
                x + self.panel_cols()
            }
        } else if is_left_check {
            x
        } else {
            x + self.panel_cols() / 2
        };
        let matrix_y =
            (y / (self.panel_rows() / 2)) * (self.panel_rows() / 4) + y % (self.panel_rows() / 4);
        [matrix_x, matrix_y]
    }
}

pub(crate) struct SpiralMultiplexMapper {
    panel_rows: usize,
    panel_cols: usize,
    stretch_factor: usize,
}

impl SpiralMultiplexMapper {
    pub(crate) fn new() -> Self {
        Self {
            panel_rows: 0,
            panel_cols: 0,
            stretch_factor: 2,
        }
    }
}

impl MultiplexMapper for SpiralMultiplexMapper {
    fn panel_rows(&self) -> usize {
        self.panel_rows
    }

    fn panel_cols(&self) -> usize {
        self.panel_cols
    }

    fn panel_rows_mut(&mut self) -> &mut usize {
        &mut self.panel_rows
    }

    fn panel_cols_mut(&mut self) -> &mut usize {
        &mut self.panel_cols
    }

    fn panel_stretch_factor(&self) -> usize {
        self.stretch_factor
    }

    fn map_single_panel(&self, x: usize, y: usize) -> [usize; 2] {
        let is_top_stripe = (y % (self.panel_rows() / 2)) < self.panel_rows() / 4;
        let panel_quarter = self.panel_cols() / 4;
        let quarter = x / panel_quarter;
        let offset = x % panel_quarter;
        let matrix_x = (2 * quarter * panel_quarter)
            + if is_top_stripe {
                panel_quarter - 1 - offset
            } else {
                panel_quarter + offset
            };
        let matrix_y =
            (y / (self.panel_rows() / 2)) * (self.panel_rows() / 4) + y % (self.panel_rows() / 4);
        [matrix_x, matrix_y]
    }
}

pub(crate) struct ZStripeMultiplexMapper {
    panel_rows: usize,
    panel_cols: usize,
    stretch_factor: usize,
    even_vblock_offset: usize,
    odd_vblock_offset: usize,
}

impl ZStripeMultiplexMapper {
    pub(crate) fn new(even_vblock_offset: usize, odd_vblock_offset: usize) -> Self {
        Self {
            panel_rows: 0,
            panel_cols: 0,
            stretch_factor: 2,
            even_vblock_offset,
            odd_vblock_offset,
        }
    }
}

impl MultiplexMapper for ZStripeMultiplexMapper {
    fn panel_rows(&self) -> usize {
        self.panel_rows
    }

    fn panel_cols(&self) -> usize {
        self.panel_cols
    }

    fn panel_rows_mut(&mut self) -> &mut usize {
        &mut self.panel_rows
    }

    fn panel_cols_mut(&mut self) -> &mut usize {
        &mut self.panel_cols
    }

    fn panel_stretch_factor(&self) -> usize {
        self.stretch_factor
    }

    fn map_single_panel(&self, x: usize, y: usize) -> [usize; 2] {
        let tile_width = 8;
        let tile_height = 4;

        let vert_block_is_odd = (y / tile_height) % 2;

        let even_vblock_shift = (1 - vert_block_is_odd) * self.even_vblock_offset;
        let odd_vblock_shitf = vert_block_is_odd * self.odd_vblock_offset;

        let matrix_x = x + ((x + even_vblock_shift) / tile_width) * tile_width + odd_vblock_shitf;
        let matrix_y = (y % tile_height) + tile_height * (y / (tile_height * 2));
        [matrix_x, matrix_y]
    }
}

pub(crate) struct CoremanMapper {
    panel_rows: usize,
    panel_cols: usize,
    stretch_factor: usize,
}

impl CoremanMapper {
    pub(crate) fn new() -> Self {
        Self {
            panel_rows: 0,
            panel_cols: 0,
            stretch_factor: 2,
        }
    }
}

impl MultiplexMapper for CoremanMapper {
    fn panel_rows(&self) -> usize {
        self.panel_rows
    }

    fn panel_cols(&self) -> usize {
        self.panel_cols
    }

    fn panel_rows_mut(&mut self) -> &mut usize {
        &mut self.panel_rows
    }

    fn panel_cols_mut(&mut self) -> &mut usize {
        &mut self.panel_cols
    }

    fn panel_stretch_factor(&self) -> usize {
        self.stretch_factor
    }

    fn map_single_panel(&self, x: usize, y: usize) -> [usize; 2] {
        let is_left_check = x < self.panel_cols() / 2;

        if (y <= 7) || (16..=23).contains(&y) {
            let matrix_x =
                ((x / (self.panel_cols() / 2)) * self.panel_cols()) + (x % (self.panel_cols() / 2));
            let matrix_y = if (y & (self.panel_rows() / 4)) == 0 {
                (y / (self.panel_rows() / 2)) * (self.panel_rows() / 4)
                    + (y % (self.panel_rows() / 4))
            } else {
                0
            };
            [matrix_x, matrix_y]
        } else {
            let matrix_x = if is_left_check {
                x + self.panel_cols() / 2
            } else {
                x + self.panel_cols()
            };
            let matrix_y = (y / (self.panel_rows() / 2)) * (self.panel_rows() / 4)
                + y % (self.panel_rows() / 4);
            [matrix_x, matrix_y]
        }
    }
}

pub(crate) struct Kaler2ScanMapper {
    panel_rows: usize,
    panel_cols: usize,
    stretch_factor: usize,
}

impl Kaler2ScanMapper {
    pub(crate) fn new() -> Self {
        Self {
            panel_rows: 0,
            panel_cols: 0,
            stretch_factor: 4,
        }
    }
}

impl MultiplexMapper for Kaler2ScanMapper {
    fn panel_rows(&self) -> usize {
        self.panel_rows
    }

    fn panel_cols(&self) -> usize {
        self.panel_cols
    }

    fn panel_rows_mut(&mut self) -> &mut usize {
        &mut self.panel_rows
    }

    fn panel_cols_mut(&mut self) -> &mut usize {
        &mut self.panel_cols
    }

    fn panel_stretch_factor(&self) -> usize {
        self.stretch_factor
    }

    fn map_single_panel(&self, x: usize, y: usize) -> [usize; 2] {
        // Now we have a 128x4 matrix
        let offset: isize = if ((y % 4) / 2) == 0 { -1 } else { 1 }; // Add or subtract
        let delta_offset: isize = if offset < 0 { 7 } else { 8 };
        let delta_column: isize = if ((y % 8) / 4) == 0 { 64 } else { 0 };

        let matrix_y = y % 2 + (y / 8) * 2;
        let matrix_x =
            (delta_column + (16 * (x as isize / 8)) + delta_offset + ((x as isize % 8) * offset))
                as usize;
        [matrix_x, matrix_y]
    }
}

pub(crate) struct P10MapperZ {
    panel_rows: usize,
    panel_cols: usize,
    stretch_factor: usize,
}

impl P10MapperZ {
    pub(crate) fn new() -> Self {
        Self {
            panel_rows: 0,
            panel_cols: 0,
            stretch_factor: 4,
        }
    }
}

impl MultiplexMapper for P10MapperZ {
    fn panel_rows(&self) -> usize {
        self.panel_rows
    }

    fn panel_cols(&self) -> usize {
        self.panel_cols
    }

    fn panel_rows_mut(&mut self) -> &mut usize {
        &mut self.panel_rows
    }

    fn panel_cols_mut(&mut self) -> &mut usize {
        &mut self.panel_cols
    }

    fn panel_stretch_factor(&self) -> usize {
        self.stretch_factor
    }

    fn map_single_panel(&self, x: usize, y: usize) -> [usize; 2] {
        let y_comp = match y {
            0 | 1 | 8 | 9 => 127,
            2 | 3 | 10 | 11 => 112,
            4 | 5 | 12 | 13 => 111,
            6 | 7 | 14 | 15 => 96,
            _ => 0,
        };

        let matrix_x = match y {
            0 | 1 | 4 | 5 | 8 | 9 | 12 | 13 => y_comp - x - 24 * (x / 8),
            _ => y_comp + x - 40 * (x / 8),
        };

        let matrix_y = match y {
            0 | 2 | 4 | 6 => 3,
            1 | 3 | 5 | 7 => 2,
            8 | 10 | 12 | 14 => 1,
            9 | 11 | 13 | 15 => 0,
            _ => y,
        };

        [matrix_x, matrix_y]
    }
}

pub(crate) struct QiangLiQ8 {
    panel_rows: usize,
    panel_cols: usize,
    stretch_factor: usize,
}

impl QiangLiQ8 {
    pub(crate) fn new() -> Self {
        Self {
            panel_rows: 0,
            panel_cols: 0,
            stretch_factor: 2,
        }
    }
}

impl MultiplexMapper for QiangLiQ8 {
    fn panel_rows(&self) -> usize {
        self.panel_rows
    }

    fn panel_cols(&self) -> usize {
        self.panel_cols
    }

    fn panel_rows_mut(&mut self) -> &mut usize {
        &mut self.panel_rows
    }

    fn panel_cols_mut(&mut self) -> &mut usize {
        &mut self.panel_cols
    }

    fn panel_stretch_factor(&self) -> usize {
        self.stretch_factor
    }

    fn map_single_panel(&self, x: usize, y: usize) -> [usize; 2] {
        let matrix_x = if (15..=19).contains(&y) || (5..=9).contains(&y) {
            x + (4 * (x / 4))
        } else {
            x + (4 + 4 * (x / 4))
        };
        let matrix_y = y % 5 + (y / 10) * 5;
        [matrix_x, matrix_y]
    }
}

pub(crate) struct InversedZStripe {
    panel_rows: usize,
    panel_cols: usize,
    stretch_factor: usize,
}

impl InversedZStripe {
    pub(crate) fn new() -> Self {
        Self {
            panel_rows: 0,
            panel_cols: 0,
            stretch_factor: 2,
        }
    }
}

impl MultiplexMapper for InversedZStripe {
    fn panel_rows(&self) -> usize {
        self.panel_rows
    }

    fn panel_cols(&self) -> usize {
        self.panel_cols
    }

    fn panel_rows_mut(&mut self) -> &mut usize {
        &mut self.panel_rows
    }

    fn panel_cols_mut(&mut self) -> &mut usize {
        &mut self.panel_cols
    }

    fn panel_stretch_factor(&self) -> usize {
        self.stretch_factor
    }

    fn map_single_panel(&self, x: usize, y: usize) -> [usize; 2] {
        let tile_width = 8;
        let tile_height = 4;

        let vert_block_is_even = (y / tile_height) % 2 == 0;
        let even_offset: [usize; 8] = [15, 13, 11, 9, 7, 5, 3, 1];

        let matrix_x = x
            + (x / tile_width) * tile_width
            + if vert_block_is_even {
                even_offset[x % 8]
            } else {
                0
            };
        let matrix_y = (y % tile_height) + tile_height * (y / (tile_height * 2));
        [matrix_x, matrix_y]
    }
}

/*
 * Vairous P10 1R1G1B Outdoor implementations for 16x16 modules with separate
 * RGB LEDs, e.g.:
 * https://www.ledcontrollercard.com/english/p10-outdoor-rgb-led-module-160x160mm-dip.html
 *
 */

const P10_TILE_WIDTH: usize = 8;
const P10_TILE_HEIGHT: usize = 4;
const P10_EVEN_VBLOCK_OFFSET: usize = 0;
const P10_ODD_VBLOCK_OFFSET: usize = 8;

pub(crate) struct P10Outdoor1R1G1BMultiplexMapper1 {
    panel_rows: usize,
    panel_cols: usize,
    stretch_factor: usize,
}

impl P10Outdoor1R1G1BMultiplexMapper1 {
    pub(crate) fn new() -> Self {
        Self {
            panel_rows: 0,
            panel_cols: 0,
            stretch_factor: 2,
        }
    }
}

impl MultiplexMapper for P10Outdoor1R1G1BMultiplexMapper1 {
    fn panel_rows(&self) -> usize {
        self.panel_rows
    }

    fn panel_cols(&self) -> usize {
        self.panel_cols
    }

    fn panel_rows_mut(&mut self) -> &mut usize {
        &mut self.panel_rows
    }

    fn panel_cols_mut(&mut self) -> &mut usize {
        &mut self.panel_cols
    }

    fn panel_stretch_factor(&self) -> usize {
        self.stretch_factor
    }

    fn map_single_panel(&self, x: usize, y: usize) -> [usize; 2] {
        let vblock_is_even = (y / P10_TILE_HEIGHT) % 2 == 0;

        let matrix_x = P10_TILE_WIDTH
            * (1 + usize::from(vblock_is_even) + 2 * (x / P10_TILE_WIDTH))
            - (x % P10_TILE_WIDTH)
            - 1;
        let matrix_y = (y % P10_TILE_HEIGHT) + P10_TILE_HEIGHT * (y / (P10_TILE_HEIGHT * 2));

        [matrix_x, matrix_y]
    }
}

pub(crate) struct P10Outdoor1R1G1BMultiplexMapper2 {
    panel_rows: usize,
    panel_cols: usize,
    stretch_factor: usize,
}

impl P10Outdoor1R1G1BMultiplexMapper2 {
    pub(crate) fn new() -> Self {
        Self {
            panel_rows: 0,
            panel_cols: 0,
            stretch_factor: 2,
        }
    }
}

impl MultiplexMapper for P10Outdoor1R1G1BMultiplexMapper2 {
    fn panel_rows(&self) -> usize {
        self.panel_rows
    }

    fn panel_cols(&self) -> usize {
        self.panel_cols
    }

    fn panel_rows_mut(&mut self) -> &mut usize {
        &mut self.panel_rows
    }

    fn panel_cols_mut(&mut self) -> &mut usize {
        &mut self.panel_cols
    }

    fn panel_stretch_factor(&self) -> usize {
        self.stretch_factor
    }

    fn map_single_panel(&self, x: usize, y: usize) -> [usize; 2] {
        let vblock_is_even = (y / P10_TILE_HEIGHT) % 2 == 0;
        let even_vblock_shift = usize::from(vblock_is_even) * P10_EVEN_VBLOCK_OFFSET;
        let odd_vblock_shift = usize::from(!vblock_is_even) * P10_ODD_VBLOCK_OFFSET;

        let matrix_x = if vblock_is_even {
            P10_TILE_WIDTH * (1 + 2 * (x / P10_TILE_WIDTH)) - (x % P10_TILE_WIDTH) - 1
        } else {
            x + ((x + even_vblock_shift) / P10_TILE_WIDTH) * P10_TILE_WIDTH + odd_vblock_shift
        };
        let matrix_y = (y % P10_TILE_HEIGHT) + P10_TILE_HEIGHT * (y / (P10_TILE_HEIGHT * 2));

        [matrix_x, matrix_y]
    }
}

pub(crate) struct P10Outdoor1R1G1BMultiplexMapper3 {
    panel_rows: usize,
    panel_cols: usize,
    stretch_factor: usize,
}

impl P10Outdoor1R1G1BMultiplexMapper3 {
    pub(crate) fn new() -> Self {
        Self {
            panel_rows: 0,
            panel_cols: 0,
            stretch_factor: 2,
        }
    }
}

impl MultiplexMapper for P10Outdoor1R1G1BMultiplexMapper3 {
    fn panel_rows(&self) -> usize {
        self.panel_rows
    }

    fn panel_cols(&self) -> usize {
        self.panel_cols
    }

    fn panel_rows_mut(&mut self) -> &mut usize {
        &mut self.panel_rows
    }

    fn panel_cols_mut(&mut self) -> &mut usize {
        &mut self.panel_cols
    }

    fn panel_stretch_factor(&self) -> usize {
        self.stretch_factor
    }

    fn map_single_panel(&self, x: usize, y: usize) -> [usize; 2] {
        let vblock_is_even = (y / P10_TILE_HEIGHT) % 2 == 0;
        let even_vblock_shift = usize::from(vblock_is_even) * P10_EVEN_VBLOCK_OFFSET;
        let odd_vblock_shift = usize::from(!vblock_is_even) * P10_ODD_VBLOCK_OFFSET;

        let matrix_x = if vblock_is_even {
            x + ((x + even_vblock_shift) / P10_TILE_WIDTH) * P10_TILE_WIDTH + odd_vblock_shift
        } else {
            P10_TILE_WIDTH * (2 + 2 * (x / P10_TILE_WIDTH)) - (x % P10_TILE_WIDTH) - 1
        };
        let matrix_y = (y % P10_TILE_HEIGHT) + P10_TILE_HEIGHT * (y / (P10_TILE_HEIGHT * 2));

        [matrix_x, matrix_y]
    }
}

pub(crate) struct P10CoremanMapper {
    panel_rows: usize,
    panel_cols: usize,
    stretch_factor: usize,
}

impl P10CoremanMapper {
    pub(crate) fn new() -> Self {
        Self {
            panel_rows: 0,
            panel_cols: 0,
            stretch_factor: 4,
        }
    }
}

impl MultiplexMapper for P10CoremanMapper {
    fn panel_rows(&self) -> usize {
        self.panel_rows
    }

    fn panel_cols(&self) -> usize {
        self.panel_cols
    }

    fn panel_rows_mut(&mut self) -> &mut usize {
        &mut self.panel_rows
    }

    fn panel_cols_mut(&mut self) -> &mut usize {
        &mut self.panel_cols
    }

    fn panel_stretch_factor(&self) -> usize {
        self.stretch_factor
    }

    fn map_single_panel(&self, x: usize, y: usize) -> [usize; 2] {
        // Row offset 8,8,8,8,0,0,0,0,8,8,8,8,0,0,0,0
        let mut mul_y = if (y & 4) > 0 { 0 } else { 8 };

        // Row offset 9,9,8,8,1,1,0,0,9,9,8,8,1,1,0,0
        mul_y += usize::from((y & 2) == 0);
        mul_y += (x >> 2) & !1; // Drop lsb

        let matrix_x = (mul_y << 3) + x % 8;
        let matrix_y = (y & 1) + ((y >> 2) & !1);
        [matrix_x, matrix_y]
    }
}

pub(crate) struct P10Outdoor32x16HalfScanMapper {
    panel_rows: usize,
    panel_cols: usize,
    stretch_factor: usize,
}

impl P10Outdoor32x16HalfScanMapper {
    pub(crate) fn new() -> Self {
        Self {
            panel_rows: 0,
            panel_cols: 0,
            stretch_factor: 4,
        }
    }
}

impl MultiplexMapper for P10Outdoor32x16HalfScanMapper {
    fn panel_rows(&self) -> usize {
        self.panel_rows
    }

    fn panel_cols(&self) -> usize {
        self.panel_cols
    }

    fn panel_rows_mut(&mut self) -> &mut usize {
        &mut self.panel_rows
    }

    fn panel_cols_mut(&mut self) -> &mut usize {
        &mut self.panel_cols
    }

    fn panel_stretch_factor(&self) -> usize {
        self.stretch_factor
    }

    fn map_single_panel(&self, x: usize, y: usize) -> [usize; 2] {
        let base = (x / 8) * 32;
        let reverse = (y % 4) / 2 == 0;
        let offset = (3 - ((y % 8) / 2)) * 8;
        let dx = x % 8;

        let matrix_y = if y / 8 == 0 {
            if y % 2 == 0 {
                0
            } else {
                1
            }
        } else if y % 2 == 0 {
            2
        } else {
            3
        };
        let matrix_x = base
            + if reverse {
                offset + (7 - dx)
            } else {
                offset + dx
            };
        [matrix_x, matrix_y]
    }
}

/*
 * P8 1R1G1B Outdoor P8-5S-V3.2-HX 20x40
 */

const P8_TILE_WIDTH: usize = 8;
const P8_TILE_HEIGHT: usize = 5;

pub(crate) struct P8Outdoor1R1G1BMultiplexMapper {
    panel_rows: usize,
    panel_cols: usize,
    stretch_factor: usize,
}

impl P8Outdoor1R1G1BMultiplexMapper {
    pub(crate) fn new() -> Self {
        Self {
            panel_rows: 0,
            panel_cols: 0,
            stretch_factor: 2,
        }
    }
}

impl MultiplexMapper for P8Outdoor1R1G1BMultiplexMapper {
    fn panel_rows(&self) -> usize {
        self.panel_rows
    }

    fn panel_cols(&self) -> usize {
        self.panel_cols
    }

    fn panel_rows_mut(&mut self) -> &mut usize {
        &mut self.panel_rows
    }

    fn panel_cols_mut(&mut self) -> &mut usize {
        &mut self.panel_cols
    }

    fn panel_stretch_factor(&self) -> usize {
        self.stretch_factor
    }

    fn map_single_panel(&self, x: usize, y: usize) -> [usize; 2] {
        let vblock_is_even = (y / P10_TILE_HEIGHT) % 2 == 0;
        let matrix_x = if vblock_is_even {
            P8_TILE_WIDTH * (1 + P8_TILE_WIDTH - 2 * (x / P8_TILE_WIDTH)) + P8_TILE_WIDTH
                - (x % P8_TILE_WIDTH)
                - 1
        } else {
            P8_TILE_WIDTH * (1 + P8_TILE_WIDTH - 2 * (x / P8_TILE_WIDTH)) - P8_TILE_WIDTH
                + (x % P8_TILE_WIDTH)
        };

        let matrix_y = (P8_TILE_HEIGHT - y % P8_TILE_HEIGHT)
            + P8_TILE_HEIGHT * (1 - y / (P8_TILE_HEIGHT * 2))
            - 1;
        [matrix_x, matrix_y]
    }
}

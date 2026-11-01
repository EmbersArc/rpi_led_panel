use std::fmt::{Display, Formatter};

use crate::{
    error::InvalidVariantError, pixel_mapper::PixelMapper, rgb_matrix::MatrixCreationError,
};

/// The panel rows or columns that a multiplex mapper works with.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PanelDimension {
    Any,
    Exactly(usize),
    MultipleOf(usize),
}

impl PanelDimension {
    pub(crate) fn contains(self, value: usize) -> bool {
        match self {
            PanelDimension::Any => true,
            PanelDimension::Exactly(size) => value == size,
            PanelDimension::MultipleOf(step) => value.is_multiple_of(step),
        }
    }
}

impl Display for PanelDimension {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PanelDimension::Any => f.write_str("any number"),
            PanelDimension::Exactly(size) => write!(f, "{size}"),
            PanelDimension::MultipleOf(step) => write!(f, "a multiple of {step}"),
        }
    }
}

/// The wiring of panels that use fewer row addresses than a regular panel of their size, which is common for
/// outdoor panels. They are driven like a wider and lower panel, and the mapper arranges the pixels so that the
/// canvas still has the size of the panel. Regular panels don't need a mapper.
///
/// `rows` and `cols` of the config are the physical size of a panel. Each mapper only works for some sizes, and
/// [`RGBMatrix::new`](crate::RGBMatrix::new) returns an error for the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumString, strum::VariantNames)]
#[strum(
    parse_err_fn = InvalidVariantError::new::<Self>,
    parse_err_ty = InvalidVariantError
)]
#[non_exhaustive]
pub enum MultiplexMapperType {
    /// Uses half of the row addresses. Stripes of `rows / 4` rows are grouped in pairs, and the data of the upper
    /// stripe comes after the data of the lower stripe.
    Stripe,
    /// Uses half of the row addresses. Like [`Stripe`](Self::Stripe), with the stripes split at half of the panel
    /// width into a checkerboard.
    Checkered,
    /// Uses half of the row addresses. In each quarter of the panel width, the data runs backwards through the
    /// upper stripe and then forwards through the lower one. `cols` needs to be a multiple of 4.
    Spiral,
    /// Uses half of the row addresses, with tiles of 8x4 pixels in a Z pattern. The odd rows of tiles have an offset
    /// of 8. `rows` and `cols` need to be multiples of 8.
    ZStripe08,
    /// Like [`ZStripe08`](Self::ZStripe08), with an offset of 4 for all rows of tiles. `cols` only needs to be a
    /// multiple of 4.
    ZStripe44,
    /// Like [`ZStripe08`](Self::ZStripe08), with an offset of 8 for the even rows of tiles.
    ZStripe80,
    /// Uses half of the row addresses. `rows` needs to be 32 and `cols` a multiple of 2.
    Coreman,
    /// Uses a quarter of the row addresses, with blocks of 8 columns. `cols` needs to be 32.
    Kaler2Scan,
    /// P10 panels of 32x16 pixels that are driven like a 128x4 panel in a Z pattern. `rows` needs to be 16 and `cols`
    /// 32.
    P10Z,
    /// Uses half of the row addresses, with blocks of 5 rows and 4 columns. `rows` needs to be 20 and `cols` a
    /// multiple of 4.
    QiangLiQ8,
    /// Like [`ZStripe08`](Self::ZStripe08), but the even rows of tiles run backwards through the second half of the
    /// data.
    InversedZStripe,
    /// P10 outdoor panels that use half of the row addresses, with tiles of 8x4 pixels that all run backwards. `rows`
    /// and `cols` need to be multiples of 8.
    P10Outdoor1R1G1B1,
    /// Like [`P10Outdoor1R1G1B1`](Self::P10Outdoor1R1G1B1), but only the even rows of tiles run backwards.
    P10Outdoor1R1G1B2,
    /// Like [`P10Outdoor1R1G1B1`](Self::P10Outdoor1R1G1B1), but only the odd rows of tiles run backwards.
    P10Outdoor1R1G1B3,
    /// P10 panels that use a quarter of the row addresses. `cols` needs to be 32.
    P10Coreman,
    /// P8 outdoor panels that use half of the row addresses, with tiles of 8x5 pixels. `rows` needs to be 20 and
    /// `cols` 40.
    P8Outdoor1R1G1B,
    /// Like [`Stripe`](Self::Stripe), with the data of the lower stripe after the data of the upper stripe.
    FlippedStripe,
    /// P10 outdoor panels that use a quarter of the row addresses. `rows` needs to be 16 and `cols` a multiple of 8.
    P10Outdoor32x16HalfScan,
}

impl MultiplexMapperType {
    /// How many times wider and lower than the physical panel it is driven.
    pub(crate) const fn stretch_factor(self) -> usize {
        match self {
            MultiplexMapperType::Stripe
            | MultiplexMapperType::Checkered
            | MultiplexMapperType::Spiral
            | MultiplexMapperType::ZStripe08
            | MultiplexMapperType::ZStripe44
            | MultiplexMapperType::ZStripe80
            | MultiplexMapperType::Coreman
            | MultiplexMapperType::QiangLiQ8
            | MultiplexMapperType::InversedZStripe
            | MultiplexMapperType::P10Outdoor1R1G1B1
            | MultiplexMapperType::P10Outdoor1R1G1B2
            | MultiplexMapperType::P10Outdoor1R1G1B3
            | MultiplexMapperType::P8Outdoor1R1G1B
            | MultiplexMapperType::FlippedStripe => 2,
            MultiplexMapperType::Kaler2Scan
            | MultiplexMapperType::P10Z
            | MultiplexMapperType::P10Coreman
            | MultiplexMapperType::P10Outdoor32x16HalfScan => 4,
        }
    }

    /// Where a pixel of a panel with the given physical size ends up in the stretched layout.
    fn map_single_panel(self, rows: usize, cols: usize, x: usize, y: usize) -> [usize; 2] {
        match self {
            MultiplexMapperType::Stripe => map_stripe(rows, cols, x, y),
            MultiplexMapperType::Checkered => map_checkered(rows, cols, x, y),
            MultiplexMapperType::Spiral => map_spiral(rows, cols, x, y),
            MultiplexMapperType::ZStripe08 => map_z_stripe(0, 8, x, y),
            MultiplexMapperType::ZStripe44 => map_z_stripe(4, 4, x, y),
            MultiplexMapperType::ZStripe80 => map_z_stripe(8, 0, x, y),
            MultiplexMapperType::Coreman => map_coreman(rows, cols, x, y),
            MultiplexMapperType::Kaler2Scan => map_kaler_2_scan(x, y),
            MultiplexMapperType::P10Z => map_p10_z(x, y),
            MultiplexMapperType::QiangLiQ8 => map_qiang_li_q8(x, y),
            MultiplexMapperType::InversedZStripe => map_inversed_z_stripe(x, y),
            MultiplexMapperType::P10Outdoor1R1G1B1 => map_p10_outdoor_1r1g1b_1(x, y),
            MultiplexMapperType::P10Outdoor1R1G1B2 => map_p10_outdoor_1r1g1b_2(x, y),
            MultiplexMapperType::P10Outdoor1R1G1B3 => map_p10_outdoor_1r1g1b_3(x, y),
            MultiplexMapperType::P10Coreman => map_p10_coreman(x, y),
            MultiplexMapperType::P8Outdoor1R1G1B => map_p8_outdoor_1r1g1b(x, y),
            MultiplexMapperType::FlippedStripe => map_flipped_stripe(rows, cols, x, y),
            MultiplexMapperType::P10Outdoor32x16HalfScan => map_p10_outdoor_32x16_half_scan(x, y),
        }
    }

    /// The panel dimensions that the mapping works for.
    pub(crate) const fn supported_panel_size(self) -> [PanelDimension; 2] {
        use PanelDimension::{Any, Exactly, MultipleOf};
        match self {
            MultiplexMapperType::Stripe
            | MultiplexMapperType::Checkered
            | MultiplexMapperType::FlippedStripe => [Any, Any],
            MultiplexMapperType::Spiral => [Any, MultipleOf(4)],
            MultiplexMapperType::ZStripe08
            | MultiplexMapperType::ZStripe80
            | MultiplexMapperType::InversedZStripe
            | MultiplexMapperType::P10Outdoor1R1G1B1
            | MultiplexMapperType::P10Outdoor1R1G1B2
            | MultiplexMapperType::P10Outdoor1R1G1B3 => [MultipleOf(8), MultipleOf(8)],
            MultiplexMapperType::ZStripe44 => [MultipleOf(8), MultipleOf(4)],
            MultiplexMapperType::Coreman => [Exactly(32), MultipleOf(2)],
            MultiplexMapperType::Kaler2Scan | MultiplexMapperType::P10Coreman => [Any, Exactly(32)],
            MultiplexMapperType::P10Z => [Exactly(16), Exactly(32)],
            MultiplexMapperType::QiangLiQ8 => [Exactly(20), MultipleOf(4)],
            MultiplexMapperType::P8Outdoor1R1G1B => [Exactly(20), Exactly(40)],
            MultiplexMapperType::P10Outdoor32x16HalfScan => [Exactly(16), MultipleOf(8)],
        }
    }
}

/// Undoes the multiplexing of panels with the given physical size.
pub(crate) struct MultiplexMapper {
    mapper_type: MultiplexMapperType,
    panel_rows: usize,
    panel_cols: usize,
}

impl MultiplexMapper {
    pub(crate) fn new(
        mapper_type: MultiplexMapperType,
        panel_rows: usize,
        panel_cols: usize,
    ) -> Self {
        Self {
            mapper_type,
            panel_rows,
            panel_cols,
        }
    }
}

impl PixelMapper for MultiplexMapper {
    fn size_mapping(
        &self,
        matrix_width: usize,
        matrix_height: usize,
    ) -> Result<[usize; 2], MatrixCreationError> {
        // Matrix width has been altered. Alter it back.
        let stretch_factor = self.mapper_type.stretch_factor();
        let visible_width = matrix_width / stretch_factor;
        let visible_height = matrix_height * stretch_factor;
        Ok([visible_width, visible_height])
    }

    fn map_visible_to_matrix(
        &self,
        _matrix_width: usize,
        _matrix_height: usize,
        visible_x: usize,
        visible_y: usize,
    ) -> [usize; 2] {
        let stretch_factor = self.mapper_type.stretch_factor();
        let chained_panel = visible_x / self.panel_cols;
        let parallel_panel = visible_y / self.panel_rows;

        let within_panel_x = visible_x % self.panel_cols;
        let within_panel_y = visible_y % self.panel_rows;

        let [new_x, new_y] = self.mapper_type.map_single_panel(
            self.panel_rows,
            self.panel_cols,
            within_panel_x,
            within_panel_y,
        );
        let matrix_x = chained_panel * stretch_factor * self.panel_cols + new_x;
        let matrix_y = parallel_panel * self.panel_rows / stretch_factor + new_y;
        [matrix_x, matrix_y]
    }
}

fn map_stripe(rows: usize, cols: usize, x: usize, y: usize) -> [usize; 2] {
    let is_top_stripe = (y % (rows / 2)) < rows / 4;
    let matrix_x = if is_top_stripe { x + cols } else { x };
    let matrix_y = (y / (rows / 2)) * (rows / 4) + y % (rows / 4);
    [matrix_x, matrix_y]
}

fn map_flipped_stripe(rows: usize, cols: usize, x: usize, y: usize) -> [usize; 2] {
    let is_top_stripe = (y % (rows / 2)) >= rows / 4;
    let matrix_x = if is_top_stripe { x + cols } else { x };
    let matrix_y = (y / (rows / 2)) * (rows / 4) + y % (rows / 4);
    [matrix_x, matrix_y]
}

fn map_checkered(rows: usize, cols: usize, x: usize, y: usize) -> [usize; 2] {
    let is_top_check = (y % (rows / 2)) < rows / 4;
    let is_left_check = x < cols / 2;
    let matrix_x = if is_top_check {
        if is_left_check {
            x + cols / 2
        } else {
            x + cols
        }
    } else if is_left_check {
        x
    } else {
        x + cols / 2
    };
    let matrix_y = (y / (rows / 2)) * (rows / 4) + y % (rows / 4);
    [matrix_x, matrix_y]
}

fn map_spiral(rows: usize, cols: usize, x: usize, y: usize) -> [usize; 2] {
    let is_top_stripe = (y % (rows / 2)) < rows / 4;
    let panel_quarter = cols / 4;
    let quarter = x / panel_quarter;
    let offset = x % panel_quarter;
    let matrix_x = (2 * quarter * panel_quarter)
        + if is_top_stripe {
            panel_quarter - 1 - offset
        } else {
            panel_quarter + offset
        };
    let matrix_y = (y / (rows / 2)) * (rows / 4) + y % (rows / 4);
    [matrix_x, matrix_y]
}

fn map_z_stripe(
    even_vblock_offset: usize,
    odd_vblock_offset: usize,
    x: usize,
    y: usize,
) -> [usize; 2] {
    let tile_width = 8;
    let tile_height = 4;

    let vert_block_is_odd = (y / tile_height) % 2;

    let even_vblock_shift = (1 - vert_block_is_odd) * even_vblock_offset;
    let odd_vblock_shift = vert_block_is_odd * odd_vblock_offset;

    let matrix_x = x + ((x + even_vblock_shift) / tile_width) * tile_width + odd_vblock_shift;
    let matrix_y = (y % tile_height) + tile_height * (y / (tile_height * 2));
    [matrix_x, matrix_y]
}

fn map_coreman(rows: usize, cols: usize, x: usize, y: usize) -> [usize; 2] {
    let is_left_check = x < cols / 2;

    if (y <= 7) || (16..=23).contains(&y) {
        let matrix_x = ((x / (cols / 2)) * cols) + (x % (cols / 2));
        let matrix_y = if (y & (rows / 4)) == 0 {
            (y / (rows / 2)) * (rows / 4) + (y % (rows / 4))
        } else {
            0
        };
        [matrix_x, matrix_y]
    } else {
        let matrix_x = if is_left_check {
            x + cols / 2
        } else {
            x + cols
        };
        let matrix_y = (y / (rows / 2)) * (rows / 4) + y % (rows / 4);
        [matrix_x, matrix_y]
    }
}

fn map_kaler_2_scan(x: usize, y: usize) -> [usize; 2] {
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

fn map_p10_z(x: usize, y: usize) -> [usize; 2] {
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

fn map_qiang_li_q8(x: usize, y: usize) -> [usize; 2] {
    let matrix_x = if (15..=19).contains(&y) || (5..=9).contains(&y) {
        x + (4 * (x / 4))
    } else {
        x + (4 + 4 * (x / 4))
    };
    let matrix_y = y % 5 + (y / 10) * 5;
    [matrix_x, matrix_y]
}

fn map_inversed_z_stripe(x: usize, y: usize) -> [usize; 2] {
    let tile_width = 8;
    let tile_height = 4;

    let vert_block_is_even = (y / tile_height).is_multiple_of(2);
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

/*
 * Various P10 1R1G1B Outdoor implementations for 16x16 modules with separate
 * RGB LEDs, e.g.:
 * https://www.ledcontrollercard.com/english/p10-outdoor-rgb-led-module-160x160mm-dip.html
 *
 */

const P10_TILE_WIDTH: usize = 8;
const P10_TILE_HEIGHT: usize = 4;
const P10_EVEN_VBLOCK_OFFSET: usize = 0;
const P10_ODD_VBLOCK_OFFSET: usize = 8;

fn map_p10_outdoor_1r1g1b_1(x: usize, y: usize) -> [usize; 2] {
    let vblock_is_even = (y / P10_TILE_HEIGHT).is_multiple_of(2);

    let matrix_x = P10_TILE_WIDTH * (1 + usize::from(vblock_is_even) + 2 * (x / P10_TILE_WIDTH))
        - (x % P10_TILE_WIDTH)
        - 1;
    let matrix_y = (y % P10_TILE_HEIGHT) + P10_TILE_HEIGHT * (y / (P10_TILE_HEIGHT * 2));

    [matrix_x, matrix_y]
}

fn map_p10_outdoor_1r1g1b_2(x: usize, y: usize) -> [usize; 2] {
    let vblock_is_even = (y / P10_TILE_HEIGHT).is_multiple_of(2);
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

fn map_p10_outdoor_1r1g1b_3(x: usize, y: usize) -> [usize; 2] {
    let vblock_is_even = (y / P10_TILE_HEIGHT).is_multiple_of(2);
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

fn map_p10_coreman(x: usize, y: usize) -> [usize; 2] {
    // Row offset 8,8,8,8,0,0,0,0,8,8,8,8,0,0,0,0
    let mut mul_y = if (y & 4) > 0 { 0 } else { 8 };

    // Row offset 9,9,8,8,1,1,0,0,9,9,8,8,1,1,0,0
    mul_y += usize::from((y & 2) == 0);
    mul_y += (x >> 2) & !1; // Drop lsb

    let matrix_x = (mul_y << 3) + x % 8;
    let matrix_y = (y & 1) + ((y >> 2) & !1);
    [matrix_x, matrix_y]
}

fn map_p10_outdoor_32x16_half_scan(x: usize, y: usize) -> [usize; 2] {
    let base = (x / 8) * 32;
    let reverse = (y % 4) / 2 == 0;
    let offset = (3 - ((y % 8) / 2)) * 8;
    let dx = x % 8;

    let matrix_y = if y / 8 == 0 {
        if y.is_multiple_of(2) { 0 } else { 1 }
    } else if y.is_multiple_of(2) {
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

/*
 * P8 1R1G1B Outdoor P8-5S-V3.2-HX 20x40
 */

const P8_TILE_WIDTH: usize = 8;
const P8_TILE_HEIGHT: usize = 5;

fn map_p8_outdoor_1r1g1b(x: usize, y: usize) -> [usize; 2] {
    let vblock_is_even = (y / P8_TILE_HEIGHT).is_multiple_of(2);
    let matrix_x = if vblock_is_even {
        P8_TILE_WIDTH * (1 + P8_TILE_WIDTH - 2 * (x / P8_TILE_WIDTH)) + P8_TILE_WIDTH
            - (x % P8_TILE_WIDTH)
            - 1
    } else {
        P8_TILE_WIDTH * (1 + P8_TILE_WIDTH - 2 * (x / P8_TILE_WIDTH)) - P8_TILE_WIDTH
            + (x % P8_TILE_WIDTH)
    };

    let matrix_y =
        (P8_TILE_HEIGHT - y % P8_TILE_HEIGHT) + P8_TILE_HEIGHT * (1 - y / (P8_TILE_HEIGHT * 2)) - 1;
    [matrix_x, matrix_y]
}

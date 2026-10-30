use std::{
    fmt::{Debug, Formatter},
    sync::Arc,
};

use crate::{
    RGBMatrixConfig, color::ColorLookup, config::K_BIT_PLANES, error::InvalidVariantError,
    gpio::Gpio, hardware_mapping::HardwareMapping, row_address_setter::RowAddressSetter,
};

#[derive(Clone, Copy)]
pub(crate) enum Channel {
    First,
    Second,
    Third,
}

/// The data lines that the red, green and blue channels of a pixel are sent to, in this order. Panels that swap
/// their colors need something other than the default of [`LedSequence::Rgb`].
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, strum::EnumString, strum::VariantNames,
)]
#[strum(serialize_all = "UPPERCASE")]
#[strum(
    parse_err_fn = InvalidVariantError::new::<Self>,
    parse_err_ty = InvalidVariantError
)]
pub enum LedSequence {
    /// Red on R, green on G and blue on B.
    #[default]
    Rgb,
    /// Red on R, green on B and blue on G.
    Rbg,
    /// Red on G, green on R and blue on B.
    Grb,
    /// Red on G, green on B and blue on R.
    Gbr,
    /// Red on B, green on R and blue on G.
    Brg,
    /// Red on B, green on G and blue on R.
    Bgr,
}

impl LedSequence {
    fn get_gpio(self, channel: Channel, red_bits: u32, green_bits: u32, blue_bits: u32) -> u32 {
        match channel {
            Channel::First => match self {
                LedSequence::Rgb | LedSequence::Rbg => red_bits,
                LedSequence::Grb | LedSequence::Gbr => green_bits,
                LedSequence::Brg | LedSequence::Bgr => blue_bits,
            },
            Channel::Second => match self {
                LedSequence::Rgb | LedSequence::Bgr => green_bits,
                LedSequence::Grb | LedSequence::Brg => red_bits,
                LedSequence::Gbr | LedSequence::Rbg => blue_bits,
            },
            Channel::Third => match self {
                LedSequence::Rgb | LedSequence::Grb => blue_bits,
                LedSequence::Gbr | LedSequence::Bgr => red_bits,
                LedSequence::Rbg | LedSequence::Brg => green_bits,
            },
        }
    }
}

const UNUSED_GPIO_WORD: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PixelDesignator {
    /// Offset into the bitplane buffer, or [`UNUSED_GPIO_WORD`].
    gpio_word: u32,
    r_bit: u32,
    g_bit: u32,
    b_bit: u32,
}

impl PixelDesignator {
    pub(crate) fn new(hardware_mapping: &HardwareMapping, sequence: LedSequence) -> Self {
        let h = hardware_mapping;
        let r = h.panels.red_bits();
        let g = h.panels.green_bits();
        let b = h.panels.blue_bits();
        Self {
            gpio_word: UNUSED_GPIO_WORD,
            r_bit: sequence.get_gpio(Channel::First, r, g, b),
            g_bit: sequence.get_gpio(Channel::Second, r, g, b),
            b_bit: sequence.get_gpio(Channel::Third, r, g, b),
        }
    }

    /// The GPIO bits of the colors that are on in the bit plane.
    fn plane_bits(&self, [red, green, blue]: [u16; 3], plane: usize) -> u32 {
        let mask = 1 << plane;
        let mut bits = 0;
        if red & mask != 0 {
            bits |= self.r_bit;
        }
        if green & mask != 0 {
            bits |= self.g_bit;
        }
        if blue & mask != 0 {
            bits |= self.b_bit;
        }
        bits
    }
}

#[derive(Clone)]
pub(crate) struct PixelDesignatorMap {
    width: usize,
    height: usize,
    pixel_designator: PixelDesignator,
    buffer: Vec<PixelDesignator>,
}

impl PixelDesignatorMap {
    /// A map in which every pixel has the default designator, for a pixel mapper to fill in.
    pub(crate) fn new_unmapped(
        pixel_designator: PixelDesignator,
        width: usize,
        height: usize,
    ) -> Self {
        Self {
            width,
            height,
            pixel_designator,
            buffer: vec![pixel_designator; width * height],
        }
    }

    pub(crate) fn new(
        pixel_designator: PixelDesignator,
        width: usize,
        height: usize,
        config: &RGBMatrixConfig,
    ) -> Self {
        let mut buffer = vec![pixel_designator; width * height];
        let hm = config.hardware_mapping;
        let double_rows = config.double_rows();
        for y in 0..height {
            for x in 0..width {
                let position = y * width + x;
                let d = &mut buffer[position];
                let offset = (y % double_rows) * (width * K_BIT_PLANES) + x;
                d.gpio_word = u32::try_from(offset).expect("Bitplane buffer too large.");

                let panel = y / config.rows;
                let color_bits = hm.panels.color_bits[panel];
                let (r, g, b) = if y - panel * config.rows < double_rows {
                    (color_bits.r1, color_bits.g1, color_bits.b1)
                } else {
                    (color_bits.r2, color_bits.g2, color_bits.b2)
                };

                d.r_bit = config.led_sequence.get_gpio(Channel::First, r, g, b);
                d.g_bit = config.led_sequence.get_gpio(Channel::Second, r, g, b);
                d.b_bit = config.led_sequence.get_gpio(Channel::Third, r, g, b);
            }
        }
        Self {
            width,
            height,
            pixel_designator,
            buffer,
        }
    }

    pub(crate) fn get(&self, x: usize, y: usize) -> Option<&PixelDesignator> {
        let position = (y * self.width) + x;
        self.buffer.get(position)
    }

    pub(crate) fn get_mut(&mut self, x: usize, y: usize) -> Option<&mut PixelDesignator> {
        let position = (y * self.width) + x;
        self.buffer.get_mut(position)
    }

    fn get_pixel_designator(&self) -> PixelDesignator {
        self.pixel_designator
    }

    pub(crate) fn width(&self) -> usize {
        self.width
    }

    pub(crate) fn height(&self) -> usize {
        self.height
    }
}

/// A frame to draw on, which is shown by passing it to [`RGBMatrix::update_on_vsync`].
///
/// [`RGBMatrix::update_on_vsync`]: crate::RGBMatrix::update_on_vsync
#[derive(Clone)]
pub struct Canvas {
    cols: usize,
    double_rows: usize,
    bitplane_buffer: Vec<u32>,
    shared_mapper: Arc<PixelDesignatorMap>,
    pwm_bits: usize,
    brightness: u8,
    color_lookup: &'static ColorLookup,
    interlaced: bool,
}

impl Debug for Canvas {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Canvas")
            .field("width", &self.width())
            .field("height", &self.height())
            .field("pwm_bits", &self.pwm_bits)
            .field("brightness", &self.brightness)
            .finish_non_exhaustive()
    }
}

impl Canvas {
    pub(crate) fn new(config: &RGBMatrixConfig, shared_mapper: PixelDesignatorMap) -> Self {
        let color_lookup = ColorLookup::cie1931();
        let cols = config.cols * config.chain_length;
        let double_rows = config.double_rows();
        Self {
            cols,
            double_rows,
            bitplane_buffer: vec![0u32; double_rows * cols * K_BIT_PLANES],
            shared_mapper: Arc::new(shared_mapper),
            pwm_bits: config.pwm_bits.clamp(1, K_BIT_PLANES),
            brightness: config.led_brightness.clamp(1, 100),
            color_lookup,
            interlaced: config.interlaced,
        }
    }

    /// The height in pixels. Multiple panels and pixel mappers can make this differ from the configured rows.
    #[must_use]
    pub fn height(&self) -> usize {
        self.shared_mapper.height
    }

    /// The width in pixels. Multiple panels and pixel mappers can make this differ from the configured columns.
    #[must_use]
    pub fn width(&self) -> usize {
        self.shared_mapper.width
    }

    fn row_start(&self, double_row: usize, bit: usize) -> usize {
        double_row * (self.cols * K_BIT_PLANES) + bit * self.cols
    }

    fn row_at(&self, double_row: usize, bit: usize) -> &[u32] {
        let start = self.row_start(double_row, bit);
        &self.bitplane_buffer[start..start + self.cols]
    }

    fn row_at_mut(&mut self, double_row: usize, bit: usize) -> &mut [u32] {
        let start = self.row_start(double_row, bit);
        &mut self.bitplane_buffer[start..start + self.cols]
    }

    /// Set the color of a pixel. Pixels outside of the canvas are ignored.
    pub fn set_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8) {
        if x >= self.width() || y >= self.height() {
            return;
        }
        let designator = *self
            .shared_mapper
            .get(x, y)
            .expect("Pixel not in designator map. This is a bug.");

        if designator.gpio_word == UNUSED_GPIO_WORD {
            return;
        }
        let pos_start = designator.gpio_word as usize;
        let designator_mask = !(designator.r_bit | designator.g_bit | designator.b_bit);

        let color = self.color_lookup.lookup_rgb(self.brightness, r, g, b);

        let min_bit_plane = K_BIT_PLANES - self.pwm_bits;

        (min_bit_plane..K_BIT_PLANES).for_each(|plane| {
            let pos = pos_start + self.cols * plane;
            self.bitplane_buffer[pos] &= designator_mask;
            self.bitplane_buffer[pos] |= designator.plane_bits(color, plane);
        });
    }

    /// Set every pixel to the same color.
    pub fn fill(&mut self, r: u8, g: u8, b: u8) {
        let designator = self.shared_mapper.get_pixel_designator();

        let color = self.color_lookup.lookup_rgb(self.brightness, r, g, b);

        (K_BIT_PLANES - self.pwm_bits..K_BIT_PLANES).for_each(|plane| {
            let plane_bits = designator.plane_bits(color, plane);
            (0..self.double_rows).for_each(|row| {
                self.row_at_mut(row, plane).fill(plane_bits);
            });
        });
    }

    pub(crate) fn dump_to_matrix(
        &self,
        gpio: &mut Gpio,
        hardware_mapping: &HardwareMapping,
        row_setter: &mut dyn RowAddressSetter,
        pwm_low_bit: usize,
        color_clk_mask: u32,
    ) {
        // Depending on if we do dithering, we might not always show the lowest bits.
        let start_bit = (K_BIT_PLANES - self.pwm_bits).max(pwm_low_bit);

        let half_double = self.double_rows.div_ceil(2);
        for row_loop in 0..self.double_rows {
            let d_row = if self.interlaced {
                if row_loop < half_double {
                    2 * row_loop
                } else {
                    2 * (row_loop - half_double) + 1
                }
            } else {
                row_loop
            };

            // Rows can't be switched very quickly without ghosting, so we do the
            // full PWM of one row before switching rows.
            for b in start_bit..K_BIT_PLANES {
                // While the output enable is still on, we can already clock in the next data.
                let row = self.row_at(d_row, b);
                row.iter().for_each(|col| {
                    gpio.write_masked_bits(*col, color_clk_mask); // col + reset clock
                    gpio.set_bits(hardware_mapping.clock); // Rising edge: clock color in.
                });

                gpio.clear_bits(color_clk_mask); // clock back to normal.

                // OE of the previous row-data must be finished before strobe.
                gpio.wait_pulse_finished();

                // Setting address and strobing needs to happen in dark time.
                row_setter.set_row_address(gpio, d_row);

                // Strobe in the previously clocked in row.
                gpio.set_bits(hardware_mapping.strobe);
                gpio.clear_bits(hardware_mapping.strobe);

                // Now switch on for the sleep time necessary for that bit-plane.
                gpio.send_pulse(b);
            }
        }
    }

    pub(crate) fn set_pwm_bits(&mut self, pwm_bits: usize) {
        self.pwm_bits = pwm_bits.clamp(1, K_BIT_PLANES);
    }

    pub(crate) fn set_brightness(&mut self, brightness: u8) {
        self.brightness = brightness.clamp(1, 100);
    }
}

#[cfg(feature = "drawing")]
pub mod embedded_graphics_support {
    use super::Canvas;
    use embedded_graphics::{
        Pixel,
        draw_target::DrawTarget,
        pixelcolor::Rgb888,
        prelude::{Dimensions, OriginDimensions, Point, RgbColor, Size},
        primitives::{PointsIter, Rectangle},
    };

    impl OriginDimensions for Canvas {
        fn size(&self) -> Size {
            Size::new(self.width() as u32, self.height() as u32)
        }
    }

    impl DrawTarget for Canvas {
        type Color = Rgb888;

        type Error = core::convert::Infallible;

        fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Self::Color>>,
        {
            for Pixel(coord, color) in pixels {
                // `DrawTarget` implementation are required to discard any out of bounds pixels without returning
                // an error or causing a panic.
                if (0..self.width() as i32).contains(&coord.x)
                    && (0..self.height() as i32).contains(&coord.y)
                {
                    self.set_pixel(
                        coord.x as usize,
                        coord.y as usize,
                        color.r(),
                        color.g(),
                        color.b(),
                    );
                }
            }
            Ok(())
        }

        fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
            // Clip once instead of checking every pixel.
            let bounding_box = self.bounding_box();
            let area = area.intersection(&bounding_box);
            if area == bounding_box {
                self.fill(color.r(), color.g(), color.b());
            } else {
                for Point { x, y } in area.points() {
                    self.set_pixel(x as usize, y as usize, color.r(), color.g(), color.b());
                }
            }
            Ok(())
        }

        fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
            self.fill(color.r(), color.g(), color.b());
            Ok(())
        }
    }
}

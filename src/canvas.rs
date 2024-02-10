use std::{error::Error, str::FromStr};

use crate::{
    color::ColorLookup, config::K_BIT_PLANES, gpio::Gpio, hardware_mapping::HardwareMapping,
    row_address_setter::RowAddressSetter, RGBMatrixConfig,
};

pub(crate) enum Channel {
    First,
    Second,
    Third,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LedSequence {
    #[default]
    Rgb,
    Rbg,
    Grb,
    Gbr,
    Brg,
    Bgr,
}

impl FromStr for LedSequence {
    type Err = Box<dyn Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let ok = match s.to_uppercase().as_str() {
            "RGB" => Self::Rgb,
            "RBG" => Self::Rbg,
            "GRB" => Self::Grb,
            "GBR" => Self::Gbr,
            "BRG" => Self::Brg,
            "BGR" => Self::Bgr,
            other => return Err(format!("Invalid LED sequence: {other}").into()),
        };
        Ok(ok)
    }
}

impl LedSequence {
    fn get_gpio(&self, channel: Channel, red_bits: u32, green_bits: u32, blue_bits: u32) -> u32 {
        match channel {
            Channel::First => match self {
                LedSequence::Rgb => red_bits,
                LedSequence::Rbg => red_bits,
                LedSequence::Grb => green_bits,
                LedSequence::Gbr => green_bits,
                LedSequence::Brg => blue_bits,
                LedSequence::Bgr => blue_bits,
            },
            Channel::Second => match self {
                LedSequence::Rgb => green_bits,
                LedSequence::Rbg => blue_bits,
                LedSequence::Grb => red_bits,
                LedSequence::Gbr => blue_bits,
                LedSequence::Brg => red_bits,
                LedSequence::Bgr => green_bits,
            },
            Channel::Third => match self {
                LedSequence::Rgb => blue_bits,
                LedSequence::Rbg => green_bits,
                LedSequence::Grb => blue_bits,
                LedSequence::Gbr => red_bits,
                LedSequence::Brg => green_bits,
                LedSequence::Bgr => red_bits,
            },
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PixelDesignator {
    gpio_word: Option<usize>,
    r_bit: u32,
    g_bit: u32,
    b_bit: u32,
    mask: u32,
}

impl PixelDesignator {
    pub(crate) fn new(hardware_mapping: &HardwareMapping, sequence: LedSequence) -> Self {
        let h = hardware_mapping;
        let r = h.panels.red_bits();
        let g = h.panels.green_bits();
        let b = h.panels.blue_bits();
        Self {
            gpio_word: None,
            r_bit: sequence.get_gpio(Channel::First, r, g, b),
            g_bit: sequence.get_gpio(Channel::Second, r, g, b),
            b_bit: sequence.get_gpio(Channel::Third, r, g, b),
            mask: !0u32,
        }
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
    pub(crate) fn new(
        pixel_designator: PixelDesignator,
        width: usize,
        height: usize,
        config: &RGBMatrixConfig,
    ) -> Self {
        let mut buffer = vec![pixel_designator; width * height];
        let h = config.hardware_mapping;
        let double_rows = config.double_rows();
        for y in 0..height {
            for x in 0..width {
                let position = y * width + x;
                let d = &mut buffer[position];
                let offset = (y % double_rows) * (width * K_BIT_PLANES) + x;
                d.gpio_word = Some(offset);

                let panel = y / config.rows;
                let color_bits = h.panels.color_bits[panel];
                let (r, g, b) = if y - panel * config.rows < double_rows {
                    (color_bits.r1, color_bits.g1, color_bits.b1)
                } else {
                    (color_bits.r2, color_bits.g2, color_bits.b2)
                };

                d.r_bit = config.led_sequence.get_gpio(Channel::First, r, g, b);
                d.g_bit = config.led_sequence.get_gpio(Channel::Second, r, g, b);
                d.b_bit = config.led_sequence.get_gpio(Channel::Third, r, g, b);
                d.mask = !(d.r_bit | d.g_bit | d.b_bit);
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

#[derive(Clone)]
pub struct Canvas {
    #[allow(unused)]
    rows: usize,
    cols: usize,
    double_rows: usize,
    bitplane_buffer: Vec<u32>,
    shared_mapper: PixelDesignatorMap,
    pwm_bits: usize,
    brightness: u8,
    color_lookup: ColorLookup,
    interlaced: bool,
}

impl Canvas {
    pub(crate) fn new(config: &RGBMatrixConfig, shared_mapper: PixelDesignatorMap) -> Self {
        let color_lookup = ColorLookup::new_cie1931();
        let rows = config.rows * config.parallel;
        let cols = config.cols * config.chain_length;
        let double_rows = config.double_rows();
        Self {
            rows,
            cols,
            double_rows,
            bitplane_buffer: vec![0u32; double_rows * cols * K_BIT_PLANES],
            shared_mapper,
            pwm_bits: config.pwm_bits,
            brightness: config.led_brightness.clamp(1, 100),
            color_lookup,
            interlaced: config.interlaced,
        }
    }

    pub fn height(&self) -> usize {
        self.shared_mapper.height
    }

    pub fn width(&self) -> usize {
        self.shared_mapper.width
    }

    fn position_at(&self, double_row: usize, column: usize, bit: usize) -> usize {
        double_row * (self.cols * K_BIT_PLANES) + bit * self.cols + column
    }

    fn row_at(&self, double_row: usize, column: usize, bit: usize) -> &[u32] {
        let start = self.position_at(double_row, column, bit);
        &self.bitplane_buffer[start..start + self.cols]
    }

    fn row_at_mut(&mut self, double_row: usize, column: usize, bit: usize) -> &mut [u32] {
        let start = self.position_at(double_row, column, bit);
        &mut self.bitplane_buffer[start..start + self.cols]
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8) {
        if x >= self.width() || y >= self.height() {
            return;
        }
        let designator = match self.shared_mapper.get(x, y) {
            Some(d) => d,
            None => panic!("Pixel not in designator map. This is a bug."),
        };
        let PixelDesignator {
            gpio_word,
            r_bit,
            g_bit,
            b_bit,
            mask: designator_mask,
        } = *designator;

        let pos_start = match gpio_word {
            Some(w) => w,
            None => {
                // non-used pixel marker.
                return;
            }
        };

        let [red, green, blue] = self.color_lookup.lookup_rgb(self.brightness, r, g, b);

        let min_bit_plane = K_BIT_PLANES - self.pwm_bits;

        (min_bit_plane..K_BIT_PLANES).for_each(|plane| {
            let pos = pos_start + self.cols * plane;
            let mask = 1 << plane;
            let mut color_bits = 0;
            if (red & mask) != 0 {
                color_bits |= r_bit
            };
            if (green & mask) != 0 {
                color_bits |= g_bit
            };
            if (blue & mask) != 0 {
                color_bits |= b_bit
            };
            self.bitplane_buffer[pos] &= designator_mask;
            self.bitplane_buffer[pos] |= color_bits;
        });
    }

    pub fn fill(&mut self, r: u8, g: u8, b: u8) {
        let designator = self.shared_mapper.get_pixel_designator();
        let PixelDesignator {
            r_bit,
            g_bit,
            b_bit,
            ..
        } = designator;

        let [red, green, blue] = self.color_lookup.lookup_rgb(self.brightness, r, g, b);

        (K_BIT_PLANES - self.pwm_bits..K_BIT_PLANES).for_each(|b| {
            let mask = 1 << b;
            let mut plane_bits = 0;
            if (red & mask) == mask {
                plane_bits |= r_bit
            };
            if (green & mask) == mask {
                plane_bits |= g_bit
            };
            if (blue & mask) == mask {
                plane_bits |= b_bit
            };
            (0..self.double_rows).for_each(|row| {
                self.row_at_mut(row, 0, b).fill(plane_bits);
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

        let half_double = self.double_rows / 2;
        for row_loop in 0..self.double_rows {
            let d_row = match self.interlaced {
                false => row_loop,
                true => {
                    if row_loop < half_double {
                        2 * row_loop
                    } else {
                        2 * (row_loop - half_double) + 1
                    }
                }
            };

            // Rows can't be switched very quickly without ghosting, so we do the
            // full PWM of one row before switching rows.
            for b in start_bit..K_BIT_PLANES {
                // While the output enable is still on, we can already clock in the next data.
                let row = self.row_at(d_row, 0, b);
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

    // Set PWM bits used for output. Default is 11, but if you only deal with
    // simple comic-colors, 1 might be sufficient. Lower values require less CPU.
    pub fn set_pwm_bits(&mut self, pwm_bits: usize) {
        self.pwm_bits = pwm_bits;
    }

    /// Set the canvas' brightness in percent.
    pub fn set_brightness(&mut self, brightness: u8) {
        self.brightness = brightness.clamp(1, 100);
    }
}

#[cfg(feature = "drawing")]
pub mod embedded_graphics_support {
    use super::Canvas;
    use embedded_graphics::{
        draw_target::DrawTarget,
        pixelcolor::Rgb888,
        prelude::{OriginDimensions, RgbColor, Size},
        Pixel,
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
            for Pixel(coord, color) in pixels.into_iter() {
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

        fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
            self.fill(color.r(), color.g(), color.b());
            Ok(())
        }
    }
}

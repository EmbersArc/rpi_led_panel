use crate::{RGBMatrixConfig, error::InvalidVariantError, gpio::Gpio, gpio_bits};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumString, strum::VariantNames)]
#[strum(
    parse_err_fn = InvalidVariantError::new::<Self>,
    parse_err_ty = InvalidVariantError
)]
pub enum PanelType {
    FM6126,
    FM6127,
}

impl PanelType {
    pub(crate) fn run_init_sequence(self, gpio: &mut Gpio, config: &RGBMatrixConfig) {
        match self {
            Self::FM6126 => Self::init_fm6126(gpio, config),
            Self::FM6127 => Self::init_fm6127(gpio, config),
        }
    }

    fn init_fm6126(gpio: &mut Gpio, config: &RGBMatrixConfig) {
        let hm = &config.hardware_mapping;
        let columns = config.cols;
        let bits_on = hm.panels.used_bits() | hm.a;
        let bits_off = hm.a;
        let mask = bits_on | hm.strobe;

        let init_b12 = 0b0111_1111_1111_1111; // full bright
        let init_b13 = 0b0000_0000_0100_0000; // panel on

        gpio.clear_bits(hm.clock | hm.strobe);

        (0..columns).for_each(|c| {
            let mut value = if init_b12 & (gpio_bits!(c % 16)) == 0 {
                bits_off
            } else {
                bits_on
            };
            if c > columns - 12 {
                value |= hm.strobe;
            };
            gpio.write_masked_bits(value, mask);
            gpio.set_bits(hm.clock);
            gpio.clear_bits(hm.clock);
        });
        gpio.clear_bits(hm.strobe);

        (0..columns).for_each(|c| {
            let mut value = if init_b13 & (gpio_bits!(c % 16)) == 0 {
                bits_off
            } else {
                bits_on
            };
            if c > columns - 13 {
                value |= hm.strobe;
            };
            gpio.write_masked_bits(value, mask);
            gpio.set_bits(hm.clock);
            gpio.clear_bits(hm.clock);
        });
        gpio.clear_bits(hm.strobe);
    }

    /// The FM6217 is very similar to the FM6216. FM6217 adds Register 3 to allow for automatic bad pixel
    /// suppression.
    fn init_fm6127(gpio: &mut Gpio, config: &RGBMatrixConfig) {
        let hm = &config.hardware_mapping;
        let columns = config.cols;
        let bits_on = hm.panels.color_bits[0].used_bits() | hm.a;
        let bits_off = 0;
        let mask = bits_on | hm.strobe;

        let init_b12 = 0b1111_1111_1100_1110; // register 1
        let init_b13 = 0b1110_0000_0110_0010; // register 2.
        let init_b11 = 0b0101_1111_0000_0000; // register 3.

        gpio.clear_bits(hm.clock | hm.strobe);

        (0..columns).for_each(|c| {
            let mut value = if init_b12 & (gpio_bits!(c % 16)) == 0 {
                bits_off
            } else {
                bits_on
            };
            if c > columns - 12 {
                value |= hm.strobe;
            };
            gpio.write_masked_bits(value, mask);
            gpio.set_bits(hm.clock);
            gpio.clear_bits(hm.clock);
        });
        gpio.clear_bits(hm.strobe);

        (0..columns).for_each(|c| {
            let mut value = if init_b13 & (gpio_bits!(c % 16)) == 0 {
                bits_off
            } else {
                bits_on
            };
            if c > columns - 13 {
                value |= hm.strobe;
            };
            gpio.write_masked_bits(value, mask);
            gpio.set_bits(hm.clock);
            gpio.clear_bits(hm.clock);
        });
        gpio.clear_bits(hm.strobe);

        (0..columns).for_each(|c| {
            let mut value = if init_b11 & (gpio_bits!(c % 16)) == 0 {
                bits_off
            } else {
                bits_on
            };
            if c > columns - 11 {
                value |= hm.strobe;
            };
            gpio.write_masked_bits(value, mask);
            gpio.set_bits(hm.clock);
            gpio.clear_bits(hm.clock);
        });
        gpio.clear_bits(hm.strobe);
    }
}

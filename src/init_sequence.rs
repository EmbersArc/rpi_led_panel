use crate::{gpio::Gpio, gpio_bits, HardwareMapping};

pub(crate) enum InitializationSequence {
    FM6126,
    FM6127,
}

impl InitializationSequence {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "fm6126" => Some(Self::FM6126),
            "fm6127" => Some(Self::FM6127),
            _ => None,
        }
    }

    pub(crate) fn run(&self, gpio: &mut Gpio, hm: &HardwareMapping, columns: usize) {
        match self {
            Self::FM6126 => Self::init_fm6126(gpio, hm, columns),
            Self::FM6127 => Self::init_fm6127(gpio, hm, columns),
        }
    }

    fn init_fm6126(gpio: &mut Gpio, hm: &HardwareMapping, columns: usize) {
        let bits_on = hm.panels.used_bits() | hm.a;
        let bits_off = hm.a;
        let mask = bits_on | hm.strobe;

        let init_b12 = 0b0111111111111111; // full bright
        let init_b13 = 0b0000000001000000; // panel on

        gpio.clear_bits(hm.clock | hm.strobe);

        (0..columns).for_each(|c| {
            let mut value = if init_b12 & (gpio_bits!(c % 16)) == 0 {
                bits_off
            } else {
                bits_on
            };
            if c > columns - 12 {
                value |= hm.strobe
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
                value |= hm.strobe
            };
            gpio.write_masked_bits(value, mask);
            gpio.set_bits(hm.clock);
            gpio.clear_bits(hm.clock);
        });
        gpio.clear_bits(hm.strobe);
    }

    /// The FM6217 is very similar to the FM6216. FM6217 adds Register 3 to allow for automatic bad pixel
    /// suppression.
    fn init_fm6127(gpio: &mut Gpio, hm: &HardwareMapping, columns: usize) {
        let bits_on = hm.panels.color_bits[0].used_bits() | hm.a;
        let bits_off = 0;
        let mask = bits_on | hm.strobe;

        let init_b12 = 0b1111111111001110; // register 1
        let init_b13 = 0b1110000001100010; // register 2.
        let init_b11 = 0b0101111100000000; // register 3.

        gpio.clear_bits(hm.clock | hm.strobe);

        (0..columns).for_each(|c| {
            let mut value = if init_b12 & (gpio_bits!(c % 16)) == 0 {
                bits_off
            } else {
                bits_on
            };
            if c > columns - 12 {
                value |= hm.strobe
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
                value |= hm.strobe
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
                value |= hm.strobe
            };
            gpio.write_masked_bits(value, mask);
            gpio.set_bits(hm.clock);
            gpio.clear_bits(hm.clock);
        });
        gpio.clear_bits(hm.strobe);
    }
}

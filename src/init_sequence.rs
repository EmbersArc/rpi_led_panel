use crate::{RGBMatrixConfig, error::InvalidVariantError, gpio::Gpio};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumString, strum::VariantNames)]
#[strum(
    parse_err_fn = InvalidVariantError::new::<Self>,
    parse_err_ty = InvalidVariantError
)]
pub enum PanelType {
    FM6126,
    /// Very similar to the FM6126, but adds a third register that allows for automatic bad pixel suppression.
    FM6127,
}

impl PanelType {
    pub(crate) fn run_init_sequence(self, gpio: &mut Gpio, config: &RGBMatrixConfig) {
        let hm = &config.hardware_mapping;
        gpio.clear_bits(hm.clock | hm.strobe);
        match self {
            Self::FM6126 => {
                let bits_on = hm.panels.used_bits() | hm.a;
                let bits_off = hm.a;
                // full bright
                write_register(gpio, config, 12, 0b0111_1111_1111_1111, bits_on, bits_off);
                // panel on
                write_register(gpio, config, 13, 0b0000_0000_0100_0000, bits_on, bits_off);
            }
            Self::FM6127 => {
                let bits_on = hm.panels.color_bits[0].used_bits() | hm.a;
                let bits_off = 0;
                // register 1
                write_register(gpio, config, 12, 0b1111_1111_1100_1110, bits_on, bits_off);
                // register 2
                write_register(gpio, config, 13, 0b1110_0000_0110_0010, bits_on, bits_off);
                // register 3
                write_register(gpio, config, 11, 0b0101_1111_0000_0000, bits_on, bits_off);
            }
        }
    }
}

/// Shift `value` into a configuration register of all driver chips in the chain. The register is selected by
/// holding the strobe for the last `register - 1` clocks.
fn write_register(
    gpio: &mut Gpio,
    config: &RGBMatrixConfig,
    register: usize,
    value: u16,
    bits_on: u32,
    bits_off: u32,
) {
    let hm = &config.hardware_mapping;
    let columns = config.cols;
    let mask = bits_on | hm.strobe;

    for c in 0..columns {
        let mut bits = if value & (1 << (c % 16)) == 0 {
            bits_off
        } else {
            bits_on
        };
        if c > columns.saturating_sub(register) {
            bits |= hm.strobe;
        }
        gpio.write_masked_bits(bits, mask);
        gpio.set_bits(hm.clock);
        gpio.clear_bits(hm.clock);
    }
    gpio.clear_bits(hm.strobe);
}

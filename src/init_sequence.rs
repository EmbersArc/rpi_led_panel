use crate::{RGBMatrixConfig, error::InvalidVariantError, hardware_mapping::GpioBits};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumString, strum::VariantNames)]
#[strum(
    parse_err_fn = InvalidVariantError::new::<Self>,
    parse_err_ty = InvalidVariantError
)]
#[non_exhaustive]
pub enum PanelType {
    #[strum(serialize = "FM6126", serialize = "FM6126A")]
    FM6126,
    FM6127,
}

/// The data that initializes the panels: registers that are clocked into every panel of the chain.
pub(crate) struct PanelInit {
    /// The GPIO bits that the words may change, apart from the clock.
    pub(crate) mask: GpioBits,
    /// One word per column for each register. The strobe bit is set during the last columns, which selects the
    /// register.
    pub(crate) registers: Vec<Vec<GpioBits>>,
}

impl PanelType {
    pub(crate) fn init(self, config: &RGBMatrixConfig) -> PanelInit {
        let hm = &config.hardware_mapping;
        match self {
            Self::FM6126 => {
                // Address bit 'A' is always on.
                let bits_on = hm.panels.used_bits() | hm.a;
                let bits_off = hm.a;
                let init_b12 = 0b0111_1111_1111_1111; // full bright
                let init_b13 = 0b0000_0000_0100_0000; // panel on
                PanelInit {
                    mask: bits_on | hm.strobe,
                    registers: vec![
                        register(config, bits_on, bits_off, init_b12, 12),
                        register(config, bits_on, bits_off, init_b13, 13),
                    ],
                }
            }
            // The FM6217 is very similar to the FM6216. FM6217 adds Register 3 to allow for automatic bad pixel
            // suppression.
            Self::FM6127 => {
                let bits_on = hm.panels.color_bits[0].used_bits();
                let bits_off = 0;
                let init_b12 = 0b1111_1111_1100_1110; // register 1
                let init_b13 = 0b1110_0000_0110_0010; // register 2.
                let init_b11 = 0b0101_1111_0000_0000; // register 3.
                PanelInit {
                    mask: bits_on | hm.strobe,
                    registers: vec![
                        register(config, bits_on, bits_off, init_b12, 12),
                        register(config, bits_on, bits_off, init_b13, 13),
                        register(config, bits_on, bits_off, init_b11, 11),
                    ],
                }
            }
        }
    }
}

/// The words that clock the 16 bit `value` (most significant bit first) into every panel of the chain, latching
/// it into the register selected by the number of clocks during which the strobe is held high.
fn register(
    config: &RGBMatrixConfig,
    bits_on: GpioBits,
    bits_off: GpioBits,
    value: u16,
    strobe_clocks: usize,
) -> Vec<GpioBits> {
    let columns = config.cols * config.chain_length;
    (0..columns)
        .map(|c| {
            let mut word = if register_bit(value, c) {
                bits_on
            } else {
                bits_off
            };
            // Same as `c > columns - strobe_clocks`, without the underflow for short chains.
            if c + strobe_clocks > columns {
                word |= config.hardware_mapping.strobe;
            }
            word
        })
        .collect()
}

/// The register bit that is sent at the given column, most significant bit first.
fn register_bit(value: u16, column: usize) -> bool {
    value & (1 << (15 - column % 16)) != 0
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{PanelType, register_bit};
    use crate::RGBMatrixConfig;

    #[test]
    fn register_bits_are_sent_most_significant_first() {
        // The upstream library describes the FM6126 "panel on" register as the string "0000000001000000".
        let sent: String = (0..32)
            .map(|c| {
                if register_bit(0b0000_0000_0100_0000, c) {
                    '1'
                } else {
                    '0'
                }
            })
            .collect();
        assert_eq!(sent, "00000000010000000000000001000000");
    }

    #[test]
    fn strobe_selects_the_register() {
        let config = RGBMatrixConfig {
            cols: 32,
            ..Default::default()
        };
        let init = PanelType::FM6127.init(&config);
        let strobe = config.hardware_mapping.strobe;
        let strobed: Vec<usize> = init
            .registers
            .iter()
            .map(|words| words.iter().filter(|w| *w & strobe != 0).count())
            .collect();
        // The strobe is high for one clock less than the register number.
        assert_eq!(strobed, [11, 12, 10]);
    }

    #[test]
    fn parses_upstream_panel_names() {
        assert_eq!(PanelType::from_str("FM6126A").unwrap(), PanelType::FM6126);
    }
}

//! Shared parts of the Raspberry Pi 5 backends, which drive the panels through the RP1 I/O controller.
//!
//! Both backends present one GPIO word per pixel clock: the color bits from the canvas, the row address and the
//! output enable. While the color data of the next bit plane is clocked in, the LEDs of the previous bit plane
//! stay on for as many words as its on-time needs, so the dark time between bit planes stays short.

use std::fs::read_to_string;

use crate::{
    HardwareMapping, RGBMatrixConfig, RowAddressSetterType,
    config::BIT_PLANES,
    hardware_mapping::{GpioBits, low_pins},
    rgb_matrix::MatrixCreationError,
};

/// The RP1 GPIO pins on the 40 pin header.
pub(crate) const RP1_GPIO_COUNT: u32 = 28;

/// Whether this is a Raspberry Pi 5 family board, according to the device tree.
pub(crate) fn is_pi5_model() -> bool {
    read_to_string("/proc/device-tree/model")
        .is_ok_and(|model| model.contains("Raspberry Pi 5") || model.contains("Compute Module 5"))
}

/// The pins and timing of the matrix on the RP1.
/// The address lines on the RP1 pins.
#[derive(Clone, Copy)]
pub(crate) struct AddressPins {
    a: u32,
    b: u32,
    c: u32,
    d: u32,
    e: u32,
}

pub(crate) struct Rp1Layout {
    address: AddressPins,
    pub(crate) row_setter: RowAddressSetterType,
    pub(crate) double_rows: usize,
    pub(crate) output_enable: u32,
    pub(crate) clock: u32,
    pub(crate) strobe: u32,
    /// All pins that are driven.
    pub(crate) used_bits: u32,
}

impl Rp1Layout {
    /// Checks that the configuration can be driven by the RP1 backend. `single_pin_signals` requires every signal
    /// to be on exactly one pin, instead of only the clock.
    pub(crate) fn new(
        config: &RGBMatrixConfig,
        single_pin_signals: bool,
    ) -> Result<Self, MatrixCreationError> {
        let invalid = |message: String| Err(MatrixCreationError::InvalidConfig(message));
        let h = config.hardware_mapping;
        if config.parallel > 3 {
            return invalid("The Raspberry Pi 5 supports up to 3 parallel chains.".to_string());
        }
        // The mappings that upstream validated for the RP1.
        let supported = [
            HardwareMapping::regular(),
            HardwareMapping::classic(),
            HardwareMapping::adafruit_hat(),
            HardwareMapping::adafruit_hat_pwm(),
        ];
        let regular_pi1 = !single_pin_signals && h == HardwareMapping::regular_pi1();
        if !supported.contains(&h) && !regular_pi1 {
            return invalid(
                "The Raspberry Pi 5 supports the Regular, Classic, AdafruitHat and AdafruitHatPwm hardware \
                 mappings, and RegularPi1 with rp1_pio."
                    .to_string(),
            );
        }

        let double_rows = config.double_rows();
        let address_bits = address_signals(&h, double_rows, config.row_setter)
            .iter()
            .fold(0, |bits, (_, signal)| bits | signal);
        let color_bits = color_bits(&h, config.parallel);
        let used_bits = h.output_enable | h.clock | h.strobe | address_bits | color_bits;
        if used_bits >> RP1_GPIO_COUNT != 0 || h.e >> RP1_GPIO_COUNT != 0 {
            return invalid(format!(
                "The Raspberry Pi 5 only supports GPIO pins 0 to {}.",
                RP1_GPIO_COUNT - 1
            ));
        }

        let mut signals = vec![("clock", h.clock)];
        if single_pin_signals {
            signals.extend([("strobe", h.strobe), ("output enable", h.output_enable)]);
            signals.extend(address_signals(&h, double_rows, config.row_setter));
            for (chain, bits) in h.panels.color_bits.iter().take(config.parallel).enumerate() {
                for (name, bit) in [
                    ("r1", bits.r1),
                    ("g1", bits.g1),
                    ("b1", bits.b1),
                    ("r2", bits.r2),
                    ("g2", bits.g2),
                    ("b2", bits.b2),
                ] {
                    if bit.count_ones() != 1 {
                        return invalid(format!(
                            "The {name} signal of chain {chain} needs to be on exactly one pin."
                        ));
                    }
                }
            }
        }
        if let Some((name, _)) = signals.iter().find(|(_, bits)| bits.count_ones() != 1) {
            return invalid(format!("The {name} signal needs to be on exactly one pin."));
        }

        // All pins fit in 32 bits now.
        Ok(Self {
            address: AddressPins {
                a: low_pins(h.a),
                b: low_pins(h.b),
                c: low_pins(h.c),
                d: low_pins(h.d),
                e: low_pins(h.e),
            },
            row_setter: config.row_setter,
            double_rows,
            output_enable: low_pins(h.output_enable),
            clock: low_pins(h.clock),
            strobe: low_pins(h.strobe),
            used_bits: low_pins(used_bits),
        })
    }

    /// Whether the row address is shifted into a register instead of being set directly.
    pub(crate) fn uses_shift_register(&self) -> bool {
        matches!(
            self.row_setter,
            RowAddressSetterType::ShiftRegister
                | RowAddressSetterType::ABCShiftRegister
                | RowAddressSetterType::SM5266
                | RowAddressSetterType::B707ShiftRegister
        )
    }

    /// The ABC shift register setter doesn't remember the last row, so the address is shifted in for every bit
    /// plane. The others only shift it in once per row.
    pub(crate) fn shifts_every_bitplane(&self) -> bool {
        self.row_setter == RowAddressSetterType::ABCShiftRegister
    }

    /// The address lines while a row is shown.
    pub(crate) fn row_address_bits(&self, row: usize) -> u32 {
        let h = &self.address;
        match self.row_setter {
            RowAddressSetterType::Direct => [h.a, h.b, h.c, h.d, h.e]
                .iter()
                .enumerate()
                .filter(|(bit, _)| row & (1 << bit) != 0)
                .fold(0, |bits, (_, line)| bits | line),
            RowAddressSetterType::ShiftRegister => h.a | if row == 0 { 0 } else { h.b },
            RowAddressSetterType::DirectABCDLine => match row % 4 {
                0 => h.b | h.c | h.d,
                1 => h.a | h.c | h.d,
                2 => h.a | h.b | h.d,
                _ => h.a | h.b | h.c,
            },
            RowAddressSetterType::ABCShiftRegister | RowAddressSetterType::B707ShiftRegister => {
                if row == 0 {
                    h.c
                } else {
                    0
                }
            }
            RowAddressSetterType::SM5266 => sm5266_direct_bits(h, row),
        }
    }

    /// The pin states that shift the row address into the shift register, one per pixel clock, while the LEDs
    /// are off. Empty if the address is set directly.
    pub(crate) fn shift_row_address_states(&self, row: usize) -> Vec<u32> {
        let h = &self.address;
        let blank = self.output_enable;
        let mut states = Vec::new();
        match self.row_setter {
            RowAddressSetterType::ShiftRegister | RowAddressSetterType::ABCShiftRegister => {
                let abc = self.row_setter == RowAddressSetterType::ABCShiftRegister;
                let (clock, data) = (h.a, if abc { h.c } else { h.b });
                let mut row_bits = 0;
                for activate in 0..self.double_rows {
                    let active = activate == self.double_rows - 1 - row;
                    // The ABC shift register has inverted data.
                    row_bits = if active == abc { data } else { 0 };
                    states.extend([blank, blank | row_bits, blank | row_bits | clock]);
                }
                states.push(blank | row_bits);
                if !abc {
                    states.push(blank | row_bits | clock);
                }
            }
            RowAddressSetterType::SM5266 => {
                let (clock, data, enable) = (h.a, h.b, h.c);
                states.push(blank | enable);
                for r in (0..8).rev() {
                    let row_bits = if row % 8 == r { data } else { 0 };
                    let base = blank | enable | row_bits;
                    // Longer clock time; tested with Pi3
                    states.extend([base, base | clock, base | clock, base]);
                }
                states.push(blank | sm5266_direct_bits(h, row));
            }
            RowAddressSetterType::B707ShiftRegister => {
                let (clock, enable) = (h.a, h.b);
                let row_bits = self.row_address_bits(row);
                let base = blank | enable | row_bits;
                states.extend([
                    blank | enable,
                    base,
                    base | clock,
                    base | clock,
                    blank | row_bits,
                ]);
            }
            RowAddressSetterType::Direct | RowAddressSetterType::DirectABCDLine => {}
        }
        states
    }
}

/// The address lines that the row setter uses.
fn address_signals(
    h: &HardwareMapping,
    double_rows: usize,
    row_setter: RowAddressSetterType,
) -> Vec<(&'static str, GpioBits)> {
    let (a, b, c, d, e) = (
        ("address A", h.a),
        ("address B", h.b),
        ("address C", h.c),
        ("address D", h.d),
        ("address E", h.e),
    );
    let mut signals = match row_setter {
        RowAddressSetterType::Direct => vec![a],
        RowAddressSetterType::ShiftRegister => vec![a, b],
        RowAddressSetterType::DirectABCDLine => vec![a, b, c, d],
        RowAddressSetterType::ABCShiftRegister => vec![a, c],
        RowAddressSetterType::SM5266 | RowAddressSetterType::B707ShiftRegister => vec![a, b, c],
    };
    // These setters only use the lines that are needed for the number of rows.
    if row_setter == RowAddressSetterType::Direct {
        signals.extend(
            [(b, 2), (c, 4)]
                .into_iter()
                .filter(|(_, rows)| double_rows > *rows)
                .map(|(s, _)| s),
        );
    }
    if matches!(
        row_setter,
        RowAddressSetterType::Direct | RowAddressSetterType::SM5266
    ) {
        signals.extend(
            [(d, 8), (e, 16)]
                .into_iter()
                .filter(|(_, rows)| double_rows > *rows)
                .map(|(s, _)| s),
        );
    }
    signals
}

fn color_bits(h: &HardwareMapping, parallel: usize) -> GpioBits {
    h.panels
        .color_bits
        .iter()
        .take(parallel)
        .fold(0, |bits, chain| bits | chain.used_bits())
}

fn sm5266_direct_bits(h: &AddressPins, row: usize) -> u32 {
    let mut bits = 0;
    if row & 0x08 != 0 {
        bits |= h.d;
    }
    if row & 0x10 != 0 {
        bits |= h.e;
    }
    bits
}

/// For how many words of `word_ns` each bit plane stays on.
pub(crate) fn bitplane_active_words(config: &RGBMatrixConfig, word_ns: f64) -> [usize; BIT_PLANES] {
    config
        .bitplane_timings_ns()
        .map(|timing| ((f64::from(timing) / word_ns).ceil() as usize).max(1))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{Rp1Layout, bitplane_active_words};
    use crate::{
        HardwareMapping, RGBMatrixConfig, RowAddressSetterType, hardware_mapping::low_pins,
    };

    /// The pins of the regular mapping, as the RP1 backends use them.
    pub(crate) struct Pins {
        pub(crate) oe: u32,
        pub(crate) strobe: u32,
        pub(crate) a: u32,
        pub(crate) b: u32,
        pub(crate) c: u32,
        pub(crate) d: u32,
        pub(crate) e: u32,
        pub(crate) r1: u32,
        pub(crate) b2: u32,
    }

    pub(crate) fn regular_pins() -> Pins {
        let h = HardwareMapping::regular();
        let chain = h.panels.color_bits[0];
        Pins {
            oe: low_pins(h.output_enable),
            strobe: low_pins(h.strobe),
            a: low_pins(h.a),
            b: low_pins(h.b),
            c: low_pins(h.c),
            d: low_pins(h.d),
            e: low_pins(h.e),
            r1: low_pins(chain.r1),
            b2: low_pins(chain.b2),
        }
    }

    fn layout(config: RGBMatrixConfig) -> Rp1Layout {
        Rp1Layout::new(&config, true).unwrap()
    }

    #[test]
    fn accepts_validated_mappings_only() {
        let config = |hardware_mapping| RGBMatrixConfig {
            hardware_mapping,
            ..Default::default()
        };
        assert!(Rp1Layout::new(&config(HardwareMapping::regular()), true).is_ok());
        assert!(Rp1Layout::new(&config(HardwareMapping::adafruit_hat()), true).is_ok());
        // Green of chain 0 is on two pins.
        assert!(Rp1Layout::new(&config(HardwareMapping::regular_pi1()), true).is_err());
        assert!(Rp1Layout::new(&config(HardwareMapping::regular_pi1()), false).is_ok());
        assert!(Rp1Layout::new(&config(HardwareMapping::classic_pi1()), false).is_err());
        assert!(Rp1Layout::new(&config(HardwareMapping::compute_module()), false).is_err());
        let four_chains = RGBMatrixConfig {
            parallel: 4,
            ..Default::default()
        };
        assert!(Rp1Layout::new(&four_chains, true).is_err());
    }

    #[test]
    fn uses_only_needed_address_lines() {
        let h = regular_pins();
        let rows_32 = layout(RGBMatrixConfig {
            rows: 32,
            ..Default::default()
        });
        assert_eq!(rows_32.used_bits & (h.d | h.e), h.d);
        let rows_64 = layout(RGBMatrixConfig::default());
        assert_eq!(rows_64.used_bits & h.e, h.e);
    }

    #[test]
    fn computes_direct_row_addresses() {
        let h = regular_pins();
        let layout = layout(RGBMatrixConfig::default());
        assert_eq!(layout.row_address_bits(0), 0);
        assert_eq!(layout.row_address_bits(5), h.a | h.c);
        assert_eq!(layout.row_address_bits(31), h.a | h.b | h.c | h.d | h.e);
        assert!(layout.shift_row_address_states(5).is_empty());
    }

    #[test]
    fn shifts_row_addresses() {
        let h = regular_pins();
        let blank = h.oe;
        let shift_register = layout(RGBMatrixConfig {
            rows: 4,
            row_setter: RowAddressSetterType::ShiftRegister,
            ..Default::default()
        });
        // Two double rows: the data line is low for the active row, which is shifted in first for row 1.
        assert_eq!(
            shift_register.shift_row_address_states(1),
            [
                blank,
                blank,
                blank | h.a,
                blank,
                blank | h.b,
                blank | h.b | h.a,
                blank | h.b,
                blank | h.b | h.a,
            ]
        );
        let b707 = layout(RGBMatrixConfig {
            row_setter: RowAddressSetterType::B707ShiftRegister,
            ..Default::default()
        });
        assert_eq!(
            b707.shift_row_address_states(0),
            [
                blank | h.b,
                blank | h.b | h.c,
                blank | h.b | h.c | h.a,
                blank | h.b | h.c | h.a,
                blank | h.c,
            ]
        );
    }

    #[test]
    fn computes_on_time_in_words() {
        let config = RGBMatrixConfig::default();
        // 130 ns at 50 ns per word, doubling for each bit plane.
        let words = bitplane_active_words(&config, 50.0);
        assert_eq!(&words[..3], [3, 6, 11]);
        assert_eq!(words[10], (130.0 * 1024.0 / 50.0f64).ceil() as usize);
    }
}

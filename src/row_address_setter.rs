use crate::{RGBMatrixConfig, error::InvalidVariantError, gpio::Gpio};

/// How a panel selects the row to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumString, strum::VariantNames)]
#[strum(
    parse_err_fn = InvalidVariantError::new::<Self>,
    parse_err_ty = InvalidVariantError
)]
#[non_exhaustive]
pub enum RowAddressSetterType {
    /// The row address is set directly on the lines A to E.
    Direct,
    /// A shift register with line A as the clock and line B as the data.
    ShiftRegister,
    /// Panels with 4 row addresses, where one of the lines A to D goes low to select its row.
    DirectABCDLine,
    /// Like [`ShiftRegister`](Self::ShiftRegister), with line C as the data and inverted data and clock.
    ABCShiftRegister,
    /// SM5266 shifters that are set through the lines A to C, with the lines D and E selecting the active shifter.
    SM5266,
}

impl RowAddressSetterType {
    pub(crate) fn create(self, config: &RGBMatrixConfig) -> RowAddressSetter {
        let h = config.hardware_mapping;
        match self {
            RowAddressSetterType::Direct => RowAddressSetter::direct(config),
            RowAddressSetterType::ShiftRegister => {
                RowAddressSetter::shift_register(config, h.b, false)
            }
            RowAddressSetterType::DirectABCDLine => RowAddressSetter::direct_abcd_line(config),
            RowAddressSetterType::ABCShiftRegister => {
                RowAddressSetter::shift_register(config, h.c, true)
            }
            RowAddressSetterType::SM5266 => RowAddressSetter::sm5266(config),
        }
    }
}

/// Different panel types use different techniques to set the row address.
enum RowWiring {
    Direct {
        row_lookup: [u32; 32],
    },
    /// ABC Shifter + DE direct: sets bits ABC using
    /// a 8 bit shifter and DE directly. The panel this works with has 8 SM5266
    /// shifters (4 for the top 32 rows and 4 for the bottom 32 rows).
    /// DE is used to select the active shifter
    /// (rows 1-8/33-40, 9-16/41-48, 17-24/49-56, 25-32/57-64).
    /// Rows are enabled by shifting in 8 bits (high bit first) with a high bit
    /// enabling that row. This allows up to 8 rows per group to be active at the
    /// same time (if they have the same content), but that isn't implemented here.
    /// BK, DIN and DCK are the designations on the SM5266P datasheet.
    /// BK = Enable Input, DIN = Serial In, DCK = Clock
    SM5266 {
        row_lookup: [u32; 32],
        bk: u32,
        din: u32,
        dck: u32,
    },
    ShiftRegister {
        clock: u32,
        data: u32,
        double_rows: usize,
        /// Data and clock are inverted.
        inverted: bool,
    },
    /// Sets the address by one of
    /// row pin ABCD for 32x16 matrix 1:4 multiplexing. The matrix has
    /// 4 addressable rows. Row is selected by a low level on the
    /// corresponding row address pin. Other row address pins must be in high level.
    ///
    /// Row addr| 0 | 1 | 2 | 3
    /// --------+---+---+---+---
    /// Line A  | 0 | 1 | 1 | 1
    /// Line B  | 1 | 0 | 1 | 1
    /// Line C  | 1 | 1 | 0 | 1
    /// Line D  | 1 | 1 | 1 | 0
    DirectABCDLine {
        row_lines: [u32; 4],
    },
}

/// Selects the row to show. A row stays selected for all of its bit planes, so the pins are only written when the
/// row changes.
pub(crate) struct RowAddressSetter {
    row_mask: u32,
    last_row: Option<usize>,
    wiring: RowWiring,
}

impl RowAddressSetter {
    fn new(row_mask: u32, wiring: RowWiring) -> Self {
        Self {
            row_mask,
            last_row: None,
            wiring,
        }
    }

    fn direct(config: &RGBMatrixConfig) -> Self {
        let double_rows = config.double_rows();

        let h = config.hardware_mapping;

        let mut row_mask = 0;
        row_mask |= h.a;
        row_mask |= if double_rows > 2 { h.b } else { 0 };
        row_mask |= if double_rows > 4 { h.c } else { 0 };
        row_mask |= if double_rows > 8 { h.d } else { 0 };
        row_mask |= if double_rows > 16 { h.e } else { 0 };

        // To avoid the bit-fiddle in the critical path, utilize
        // a lookup-table for all possible rows.
        let mut row_lookup = [0u32; 32];
        for (i, row_address) in row_lookup.iter_mut().enumerate().take(double_rows) {
            *row_address |= if i & 0b00001 != 0 { h.a } else { 0 };
            *row_address |= if i & 0b00010 != 0 { h.b } else { 0 };
            *row_address |= if i & 0b00100 != 0 { h.c } else { 0 };
            *row_address |= if i & 0b01000 != 0 { h.d } else { 0 };
            *row_address |= if i & 0b10000 != 0 { h.e } else { 0 };
        }

        Self::new(row_mask, RowWiring::Direct { row_lookup })
    }

    fn sm5266(config: &RGBMatrixConfig) -> Self {
        let h = config.hardware_mapping;
        let mut row_mask = h.a | h.b | h.c;
        assert!(config.double_rows() <= 32); // designed for up to 1/32 panel
        if config.double_rows() > 8 {
            row_mask |= h.d;
        }
        if config.double_rows() > 16 {
            row_mask |= h.e;
        }
        let mut row_lookup = [0u32; 32];
        for (i, row_address) in row_lookup.iter_mut().enumerate().take(config.double_rows()) {
            *row_address |= if i & 0x08 != 0 { h.d } else { 0 };
            *row_address |= if i & 0x10 != 0 { h.e } else { 0 };
        }
        let wiring = RowWiring::SM5266 {
            row_lookup,
            bk: h.c,
            din: h.b,
            dck: h.a,
        };
        Self::new(row_mask, wiring)
    }

    fn shift_register(config: &RGBMatrixConfig, data: u32, inverted: bool) -> Self {
        let clock = config.hardware_mapping.a;
        let wiring = RowWiring::ShiftRegister {
            clock,
            data,
            double_rows: config.double_rows(),
            inverted,
        };
        Self::new(clock | data, wiring)
    }

    fn direct_abcd_line(config: &RGBMatrixConfig) -> Self {
        let h = config.hardware_mapping;
        let wiring = RowWiring::DirectABCDLine {
            row_lines: [
                h.b | h.c | h.d,
                h.a | h.c | h.d,
                h.a | h.b | h.d,
                h.a | h.b | h.c,
            ],
        };
        Self::new(h.a | h.b | h.c | h.d, wiring)
    }

    pub(crate) fn used_bits(&self) -> u32 {
        self.row_mask
    }

    pub(crate) fn set_row_address(&mut self, gpio: &mut Gpio, row: usize) {
        if self.last_row == Some(row) {
            return;
        }
        match self.wiring {
            RowWiring::Direct { ref row_lookup } => {
                gpio.write_masked_bits(row_lookup[row], self.row_mask);
            }
            RowWiring::SM5266 {
                ref row_lookup,
                bk,
                din,
                dck,
            } => {
                gpio.set_bits(bk); // Enable serial input for the shifter.
                for r in (0..8).rev() {
                    if row % 8 == r {
                        gpio.set_bits(din);
                    } else {
                        gpio.clear_bits(din);
                    }
                    gpio.set_bits(dck);
                    gpio.set_bits(dck); // Longer clock time; tested with Pi3
                    gpio.clear_bits(dck);
                }
                gpio.clear_bits(bk); // Disable serial input to keep unwanted bits out of the shifters.
                // Set bits D and E to enable the proper shifter to display the selected row.
                gpio.write_masked_bits(row_lookup[row], self.row_mask);
            }
            RowWiring::ShiftRegister {
                clock,
                data,
                double_rows,
                inverted,
            } => {
                for activate in 0..double_rows {
                    gpio.clear_bits(clock);
                    if (activate == double_rows - 1 - row) == inverted {
                        gpio.set_bits(data);
                    } else {
                        gpio.clear_bits(data);
                    }
                    gpio.set_bits(clock);
                }
                if inverted {
                    gpio.set_bits(clock);
                    gpio.clear_bits(clock);
                } else {
                    gpio.clear_bits(clock);
                    gpio.set_bits(clock);
                }
            }
            RowWiring::DirectABCDLine { ref row_lines } => {
                gpio.write_masked_bits(row_lines[row % 4], self.row_mask);
            }
        }
        self.last_row = Some(row);
    }
}

use crate::{gpio::Gpio, HardwareMapping, RGBMatrixConfig};

/// Different panel types use different techniques to set the row address.
pub(crate) trait RowAddressSetter {
    fn used_bits(&self) -> u32;
    fn set_row_address(&mut self, gpio: &mut Gpio, row: usize);
}

pub(crate) fn get_row_address_setter(
    name: &str,
    hardware_mapping: &HardwareMapping,
    config: &RGBMatrixConfig,
) -> Box<dyn RowAddressSetter> {
    let c = config;
    let h = hardware_mapping;
    match name {
        "DirectRowAddressSetter" => Box::new(DirectRowAddressSetter::new(h, c)),
        "ShiftRegisterRowAddressSetter" => Box::new(ShiftRegisterRowAddressSetter::new(h, c)),
        "DirectABCDLineRowAddressSetter" => Box::new(DirectABCDLineRowAddressSetter::new(h, c)),
        "ABCShiftRegisterRowAddressSetter" => Box::new(ABCShiftRegisterRowAddressSetter::new(h, c)),
        "SM5266RowAddressSetter" => Box::new(SM5266RowAddressSetter::new(h, c)),
        other => panic!("Unknown row_setter: {}", other),
    }
}

pub(crate) struct DirectRowAddressSetter {
    row_mask: u32,
    row_lookup: [u32; 32],
    last_row: Option<usize>,
}

impl DirectRowAddressSetter {
    pub(crate) fn new(hardware_mapping: &HardwareMapping, config: &RGBMatrixConfig) -> Self {
        let double_rows = config.double_rows();

        let h = hardware_mapping;

        let mut row_mask = 0;
        row_mask |= h.a;
        row_mask |= if double_rows > 2 { h.b } else { 0 };
        row_mask |= if double_rows > 4 { h.c } else { 0 };
        row_mask |= if double_rows > 8 { h.d } else { 0 };
        row_mask |= if double_rows > 16 { h.e } else { 0 };

        let mut row_lookup = [0u32; 32];
        (0..double_rows).for_each(|i| {
            // To avoid the bit-fiddle in the critical path, utilize
            // a lookup-table for all possible rows.
            let mut row_address = 0;
            row_address |= if i & 0x00001 != 0 { h.a } else { 0 };
            row_address |= if i & 0b00010 != 0 { h.b } else { 0 };
            row_address |= if i & 0b00100 != 0 { h.c } else { 0 };
            row_address |= if i & 0b01000 != 0 { h.d } else { 0 };
            row_address |= if i & 0b10000 != 0 { h.e } else { 0 };
            row_lookup[i] = row_address;
        });

        Self {
            row_mask,
            row_lookup,
            last_row: None,
        }
    }
}

impl RowAddressSetter for DirectRowAddressSetter {
    fn used_bits(&self) -> u32 {
        self.row_mask
    }

    fn set_row_address(&mut self, gpio: &mut Gpio, row: usize) {
        if self.last_row == Some(row) {
            return;
        }
        gpio.write_masked_bits(self.row_lookup[row], self.row_mask);
        self.last_row = Some(row);
    }
}

/// The SM5266RowAddressSetter (ABC Shifter + DE direct) sets bits ABC using
/// a 8 bit shifter and DE directly. The panel this works with has 8 SM5266
/// shifters (4 for the top 32 rows and 4 for the bottom 32 rows).
/// DE is used to select the active shifter
/// (rows 1-8/33-40, 9-16/41-48, 17-24/49-56, 25-32/57-64).
/// Rows are enabled by shifting in 8 bits (high bit first) with a high bit
/// enabling that row. This allows up to 8 rows per group to be active at the
/// same time (if they have the same content), but that isn't implemented here.
/// BK, DIN and DCK are the designations on the SM5266P datasheet.
/// BK = Enable Input, DIN = Serial In, DCK = Clock
pub(crate) struct SM5266RowAddressSetter {
    row_mask: u32,
    row_lookup: [u32; 32],
    last_row: Option<usize>,
    bk: u32,
    din: u32,
    dck: u32,
}

impl SM5266RowAddressSetter {
    pub(crate) fn new(hardware_mapping: &HardwareMapping, config: &RGBMatrixConfig) -> Self {
        let h = hardware_mapping;
        let mut row_mask = h.a | h.b | h.c;
        assert!(config.double_rows() <= 32); // designed for up to 1/32 panel
        if config.double_rows() > 8 {
            row_mask |= h.d;
        }
        if config.double_rows() > 16 {
            row_mask |= h.e;
        }
        let mut row_lookup = [0u32; 32];
        (0..config.double_rows()).for_each(|i| {
            let mut row_address = 0;
            row_address |= if i & 0x08 != 0 { h.d } else { 0 };
            row_address |= if i & 0x10 != 0 { h.e } else { 0 };
            row_lookup[i] = row_address;
        });
        Self {
            row_mask,
            row_lookup,
            last_row: None,
            bk: h.c,
            din: h.b,
            dck: h.a,
        }
    }
}

impl RowAddressSetter for SM5266RowAddressSetter {
    fn used_bits(&self) -> u32 {
        self.row_mask
    }

    fn set_row_address(&mut self, gpio: &mut Gpio, row: usize) {
        if self.last_row == Some(row) {
            return;
        }
        gpio.set_bits(self.bk); // Enable serial input for the shifter.
        (0..8).rev().for_each(|r| {
            if row % 8 == r {
                gpio.set_bits(self.din);
            } else {
                gpio.clear_bits(self.din);
            }
            gpio.set_bits(self.dck);
            gpio.set_bits(self.dck); // Longer clock time; tested with Pi3
            gpio.clear_bits(self.dck);
        });
        gpio.clear_bits(self.bk); // Disable serial input to keep unwanted bits out of the shifters.
        self.last_row = Some(row);
        // Set bits D and E to enable the proper shifter to display the selected row.
        gpio.write_masked_bits(self.row_lookup[row], self.row_mask);
    }
}

pub(crate) struct ShiftRegisterRowAddressSetter {
    row_mask: u32,
    last_row: Option<usize>,
    clock: u32,
    data: u32,
    double_rows: usize,
}

impl ShiftRegisterRowAddressSetter {
    pub(crate) fn new(hardware_mapping: &HardwareMapping, config: &RGBMatrixConfig) -> Self {
        let h = hardware_mapping;
        let row_mask = h.a | h.b;
        let clock = h.a;
        let data = h.b;
        Self {
            row_mask,
            last_row: None,
            clock,
            data,
            double_rows: config.double_rows(),
        }
    }
}

impl RowAddressSetter for ShiftRegisterRowAddressSetter {
    fn used_bits(&self) -> u32 {
        self.row_mask
    }

    fn set_row_address(&mut self, gpio: &mut Gpio, row: usize) {
        if self.last_row == Some(row) {
            return;
        }
        (0..self.double_rows).for_each(|activate| {
            gpio.clear_bits(self.clock);
            if activate == self.double_rows - 1 - row {
                gpio.clear_bits(self.data);
            } else {
                gpio.set_bits(self.data);
            }
            gpio.set_bits(self.clock);
        });
        gpio.clear_bits(self.clock);
        gpio.set_bits(self.clock);
        self.last_row = Some(row);
    }
}

/// A shift register row address setter that does not use B but C for the data. Clock is inverted.
pub(crate) struct ABCShiftRegisterRowAddressSetter {
    row_mask: u32,
    last_row: Option<usize>,
    clock: u32,
    data: u32,
    double_rows: usize,
}

impl ABCShiftRegisterRowAddressSetter {
    pub(crate) fn new(hardware_mapping: &HardwareMapping, config: &RGBMatrixConfig) -> Self {
        let h = hardware_mapping;
        let row_mask = h.a | h.c;
        let clock = h.a;
        let data = h.c;
        Self {
            row_mask,
            last_row: None,
            clock,
            data,
            double_rows: config.double_rows(),
        }
    }
}

impl RowAddressSetter for ABCShiftRegisterRowAddressSetter {
    fn used_bits(&self) -> u32 {
        self.row_mask
    }

    fn set_row_address(&mut self, gpio: &mut Gpio, row: usize) {
        if self.last_row == Some(row) {
            return;
        }
        (0..self.double_rows).for_each(|activate| {
            gpio.clear_bits(self.clock);
            if activate == self.double_rows - 1 - row {
                gpio.clear_bits(self.data);
            } else {
                gpio.set_bits(self.data);
            }
            gpio.set_bits(self.clock);
        });
        gpio.clear_bits(self.clock);
        gpio.set_bits(self.clock);
        self.last_row = Some(row);
    }
}

/// The DirectABCDRowAddressSetter sets the address by one of
/// row pin ABCD for 32х16 matrix 1:4 multiplexing. The matrix has
/// 4 addressable rows. Row is selected by a low level on the
/// corresponding row address pin. Other row address pins must be in high level.
///
/// Row addr| 0 | 1 | 2 | 3
/// --------+---+---+---+---
/// Line A  | 0 | 1 | 1 | 1
/// Line B  | 1 | 0 | 1 | 1
/// Line C  | 1 | 1 | 0 | 1
/// Line D  | 1 | 1 | 1 | 0
pub(crate) struct DirectABCDLineRowAddressSetter {
    row_lines: [u32; 4],
    row_mask: u32,
    last_row: Option<usize>,
}

impl DirectABCDLineRowAddressSetter {
    pub(crate) fn new(hardware_mapping: &HardwareMapping, _config: &RGBMatrixConfig) -> Self {
        let h = hardware_mapping;
        Self {
            row_lines: [
                /*h.a |*/ h.b | h.c | h.d,
                h.a /*| h.b*/ | h.c | h.d,
                h.a | h.b /*| h.c */| h.d,
                h.a | h.b | h.c, /*| h.d*/
            ],
            row_mask: h.a | h.b | h.c | h.d,
            last_row: None,
        }
    }
}

impl RowAddressSetter for DirectABCDLineRowAddressSetter {
    fn used_bits(&self) -> u32 {
        self.row_mask
    }

    fn set_row_address(&mut self, gpio: &mut Gpio, row: usize) {
        if self.last_row == Some(row) {
            return;
        }
        let row_address = self.row_lines[row % 4];
        gpio.write_masked_bits(row_address, self.row_mask);
        self.last_row = Some(row);
    }
}

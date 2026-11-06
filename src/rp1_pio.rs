//! Raspberry Pi 5 backend that clocks out the data with a PIO state machine of the RP1, through the kernel's PIO
//! driver at /dev/pio0.
//!
//! The CPU prepares a command stream with one GPIO word per pixel clock and the delays that give each bit plane
//! its brightness. The PIO program, which is the same as upstream's, sets the pins and generates the clock.

use std::{
    ffi::c_void,
    fs::{File, OpenOptions},
    mem::size_of,
    os::fd::AsRawFd,
};

use crate::{
    RGBMatrixConfig,
    canvas::Canvas,
    config::BIT_PLANES,
    hardware_mapping::low_pins,
    init_sequence::PanelInit,
    rgb_matrix::MatrixCreationError,
    rp1::{RP1_GPIO_COUNT, Rp1Layout, bitplane_active_words},
};

const PIO_DEVICE: &str = "/dev/pio0";
const MAX_TRANSFER_BYTES: usize = 2 * 1024 * 1024;
const TRANSFER_BUFFERS: u32 = 3;
/// Command words with this bit set announce how many GPIO words follow. Without it, they hold the next word for
/// the encoded number of cycles.
const COMMAND_DATA: u32 = 1 << 31;
/// Clock cycles that a delay command takes on top of the encoded delay.
const DELAY_OVERHEAD_CLOCKS: usize = 5;
const CLOCKS_PER_DATA_WORD: usize = 2;
const POST_ADDRESS_DELAY_CLOCKS: usize = 5;
const BASE_PIO_CLOCK_HZ: f64 = 27_000_000.0;
const SYSTEM_CLOCK_HZ: f64 = 200_000_000.0;
const GPIO_FUNCTION_PIO: u16 = 7;
const DIRECTION_TO_STATE_MACHINE: u16 = 0;
const ORIGIN_ANY: u16 = u16::MAX;

/// The HUB75 program from Adafruit's piomatter, as used upstream.
///
/// ```text
/// ; data format (out-shift-right):
/// ; MSB ... LSB
/// ; 0 ddd......ddd: 31-bit delay
/// ; 1 ccc......ccc: 31 bit data count
/// .side_set 1 opt
/// .wrap_target
/// top:
///     out x, 1
///     out y, 31
///     jmp !x do_delay
/// data_loop:
///     out pins, 32
///     jmp y--, data_loop  side 1 ; assert clk bit
/// .wrap
/// do_delay:
///     out pins, 32
/// delay_loop:
///     jmp y--, delay_loop
///     jmp top
/// ```
const PROGRAM: [u16; 8] = [
    0x6021, 0x605f, 0x0025, 0x6000, 0x1883, 0x6000, 0x0086, 0x0000,
];
const PROGRAM_WRAP_TARGET: u32 = 0;
const PROGRAM_WRAP: u32 = 4;
/// The program fills the whole instruction memory, so that nothing else can be loaded.
const INSTRUCTION_COUNT: usize = 32;
const NOP: u16 = 0xa042;

/// The register values of a state machine, as in the RP1 PIO driver.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SmConfig {
    clkdiv: u32,
    execctrl: u32,
    shiftctrl: u32,
    pinctrl: u32,
}

impl SmConfig {
    fn new(offset: u32, clock_pin: u32, clock_divider: f64) -> Self {
        let (div_int, div_frac) = split_clock_divider(clock_divider);
        // Out-shift to the left with autopull at 32 bits, joined FIFOs for sending only.
        let out_shift_right = 0;
        let autopull = 1;
        let pull_threshold = 0; // 32
        let in_shift_right = 1;
        let fifo_join_tx = 1;
        // ".side_set 1 opt" needs the side-set bit and the optional-enable bit.
        let sideset_bits = 2;
        let sideset_optional = 1;
        Self {
            clkdiv: (u32::from(div_frac) << 8) | (u32::from(div_int) << 16),
            execctrl: (sideset_optional << 30)
                | ((offset + PROGRAM_WRAP) << 12)
                | ((offset + PROGRAM_WRAP_TARGET) << 7),
            shiftctrl: (fifo_join_tx << 30)
                | (pull_threshold << 25)
                | (out_shift_right << 19)
                | (in_shift_right << 18)
                | (autopull << 17),
            pinctrl: (sideset_bits << 29) | (RP1_GPIO_COUNT << 20) | (clock_pin << 10),
        }
    }
}

fn split_clock_divider(divider: f64) -> (u16, u8) {
    let div_int = divider.clamp(1.0, 65535.0) as u16;
    let div_frac = ((divider - f64::from(div_int)) * 256.0).clamp(0.0, 255.0) as u8;
    (div_int, div_frac)
}

#[repr(C)]
struct SmConfigXfer32Args {
    sm: u16,
    dir: u16,
    buf_size: u32,
    buf_count: u32,
}

#[repr(C)]
struct SmXferData32Args {
    sm: u16,
    dir: u16,
    data_bytes: u32,
    data: *mut c_void,
}

#[repr(C)]
struct AddProgramArgs {
    num_instrs: u16,
    origin: u16,
    instrs: [u16; INSTRUCTION_COUNT],
}

#[repr(C)]
struct SmClaimArgs {
    mask: u16,
}

#[repr(C)]
struct SmInitArgs {
    sm: u16,
    initial_pc: u16,
    config: SmConfig,
}

#[repr(C)]
struct SmArgs {
    sm: u16,
}

#[repr(C)]
struct SmSetClkdivArgs {
    sm: u16,
    div_int: u16,
    div_frac: u8,
    rsvd: u8,
}

#[repr(C)]
struct SmSetPinsArgs {
    sm: u16,
    rsvd: u16,
    values: u32,
    mask: u32,
}

#[repr(C)]
struct SmSetEnabledArgs {
    mask: u16,
    enable: u8,
    rsvd: u8,
}

#[repr(C)]
struct GpioSetFunctionArgs {
    gpio: u16,
    function: u16,
}

const PIO_IOC_MAGIC: u32 = 102;

/// The Linux `_IOW` request number.
const fn iow<T>(nr: u32) -> u32 {
    (1 << 30) | ((size_of::<T>() as u32) << 16) | (PIO_IOC_MAGIC << 8) | nr
}

const PIO_IOC_SM_XFER_DATA32: u32 = iow::<SmXferData32Args>(2);
const PIO_IOC_SM_CONFIG_XFER32: u32 = iow::<SmConfigXfer32Args>(3);
const PIO_IOC_ADD_PROGRAM: u32 = iow::<AddProgramArgs>(11);
const PIO_IOC_SM_CLAIM: u32 = iow::<SmClaimArgs>(20);
const PIO_IOC_SM_UNCLAIM: u32 = iow::<SmClaimArgs>(21);
const PIO_IOC_SM_INIT: u32 = iow::<SmInitArgs>(30);
const PIO_IOC_SM_CLEAR_FIFOS: u32 = iow::<SmArgs>(33);
const PIO_IOC_SM_SET_CLKDIV: u32 = iow::<SmSetClkdivArgs>(34);
const PIO_IOC_SM_SET_PINS: u32 = iow::<SmSetPinsArgs>(35);
const PIO_IOC_SM_SET_PINDIRS: u32 = iow::<SmSetPinsArgs>(36);
const PIO_IOC_SM_SET_ENABLED: u32 = iow::<SmSetEnabledArgs>(37);
const PIO_IOC_GPIO_SET_FUNCTION: u32 = iow::<GpioSetFunctionArgs>(51);

pub(crate) struct Rp1Pio {
    device: File,
    sm: u16,
    layout: Rp1Layout,
    active_words: [usize; BIT_PLANES],
    buffer: Vec<u32>,
}

impl Rp1Pio {
    pub(crate) fn new(config: &RGBMatrixConfig) -> Result<Self, MatrixCreationError> {
        let layout = Rp1Layout::new(config, false)?;
        let slowdown = config.slowdown.unwrap_or(1).max(1);
        let pio_clock_hz = BASE_PIO_CLOCK_HZ / f64::from(slowdown);
        let word_ns = 1e9 * CLOCKS_PER_DATA_WORD as f64 / pio_clock_hz;
        let device = OpenOptions::new()
            .read(true)
            .write(true)
            .open(PIO_DEVICE)
            .map_err(|error| Self::error(format!("Could not open {PIO_DEVICE}: {error}")))?;
        let mut pio = Self {
            device,
            sm: 0,
            layout,
            active_words: bitplane_active_words(config, word_ns),
            buffer: Vec::new(),
        };
        pio.configure_state_machine(SYSTEM_CLOCK_HZ / pio_clock_hz)?;
        Ok(pio)
    }

    fn error(message: String) -> MatrixCreationError {
        MatrixCreationError::GpioError(crate::gpio::GpioInitializationError::Rp1Pio(message))
    }

    fn ioctl<T>(&self, request: u32, args: &mut T) -> Result<i32, MatrixCreationError> {
        let result = unsafe {
            libc::ioctl(
                self.device.as_raw_fd(),
                request as _,
                (args as *mut T).cast::<c_void>(),
            )
        };
        if result < 0 {
            return Err(Self::error(format!(
                "The RP1 PIO request {:#x} failed: {}",
                request & 0xff,
                std::io::Error::last_os_error()
            )));
        }
        Ok(result)
    }

    fn configure_state_machine(&mut self, clock_divider: f64) -> Result<(), MatrixCreationError> {
        // Claim any unused state machine.
        let sm = self.ioctl(PIO_IOC_SM_CLAIM, &mut SmClaimArgs { mask: 0 })?;
        self.sm = sm as u16;
        let sm = self.sm;
        self.ioctl(
            PIO_IOC_SM_CONFIG_XFER32,
            &mut SmConfigXfer32Args {
                sm,
                dir: DIRECTION_TO_STATE_MACHINE,
                buf_size: MAX_TRANSFER_BYTES as u32,
                buf_count: TRANSFER_BUFFERS,
            },
        )?;

        let mut instrs = [NOP; INSTRUCTION_COUNT];
        instrs[..PROGRAM.len()].copy_from_slice(&PROGRAM);
        let offset = self.ioctl(
            PIO_IOC_ADD_PROGRAM,
            &mut AddProgramArgs {
                num_instrs: INSTRUCTION_COUNT as u16,
                origin: ORIGIN_ANY,
                instrs,
            },
        )? as u32;

        self.ioctl(PIO_IOC_SM_CLEAR_FIFOS, &mut SmArgs { sm })?;
        self.ioctl(
            PIO_IOC_SM_SET_CLKDIV,
            &mut SmSetClkdivArgs {
                sm,
                div_int: 1,
                div_frac: 0,
                rsvd: 0,
            },
        )?;
        let clock_pin = self.layout.clock.trailing_zeros();
        self.ioctl(
            PIO_IOC_SM_INIT,
            &mut SmInitArgs {
                sm,
                initial_pc: offset as u16,
                config: SmConfig::new(offset, clock_pin, clock_divider),
            },
        )?;
        self.set_pin_directions(true)?;
        self.ioctl(
            PIO_IOC_SM_SET_ENABLED,
            &mut SmSetEnabledArgs {
                mask: 1 << sm,
                enable: 1,
                rsvd: 0,
            },
        )?;
        self.ioctl(
            PIO_IOC_SM_SET_PINS,
            &mut SmSetPinsArgs {
                sm,
                rsvd: 0,
                values: self.layout.output_enable,
                mask: self.layout.used_bits,
            },
        )?;
        Ok(())
    }

    fn set_pin_directions(&self, output: bool) -> Result<(), MatrixCreationError> {
        for pin in 0..RP1_GPIO_COUNT {
            let bit = 1 << pin;
            if self.layout.used_bits & bit == 0 {
                continue;
            }
            self.ioctl(
                PIO_IOC_GPIO_SET_FUNCTION,
                &mut GpioSetFunctionArgs {
                    gpio: pin as u16,
                    function: GPIO_FUNCTION_PIO,
                },
            )?;
            self.ioctl(
                PIO_IOC_SM_SET_PINDIRS,
                &mut SmSetPinsArgs {
                    sm: self.sm,
                    rsvd: 0,
                    values: if output { bit } else { 0 },
                    mask: bit,
                },
            )?;
        }
        Ok(())
    }

    /// Sends the command buffer to the state machine.
    fn transfer(&mut self) -> Result<(), MatrixCreationError> {
        let sm = self.sm;
        let words_per_chunk = MAX_TRANSFER_BYTES / size_of::<u32>();
        let mut buffer = std::mem::take(&mut self.buffer);
        let result = buffer.chunks_mut(words_per_chunk).try_for_each(|chunk| {
            self.ioctl(
                PIO_IOC_SM_XFER_DATA32,
                &mut SmXferData32Args {
                    sm,
                    dir: DIRECTION_TO_STATE_MACHINE,
                    data_bytes: size_of_val(chunk) as u32,
                    data: chunk.as_mut_ptr().cast(),
                },
            )
            .map(|_| ())
        });
        buffer.clear();
        self.buffer = buffer;
        result
    }

    pub(crate) fn init_panels(&mut self, init: &PanelInit) -> Result<(), MatrixCreationError> {
        let blank = self.layout.output_enable;
        let words: Vec<u32> = init
            .registers
            .iter()
            .flatten()
            .map(|&w| low_pins(w) | blank)
            .collect();
        if words.is_empty() {
            return Ok(());
        }
        append_data_header(&mut self.buffer, words.len());
        self.buffer.extend(words);
        append_delay(&mut self.buffer, blank, 0);
        self.transfer()
    }

    pub(crate) fn dump(
        &mut self,
        canvas: &Canvas,
        pwm_low_bit: usize,
    ) -> Result<(), MatrixCreationError> {
        build_frame(
            &self.layout,
            &self.active_words,
            canvas,
            pwm_low_bit,
            &mut self.buffer,
        );
        self.transfer()
    }
}

/// Fills the command buffer for one frame.
fn build_frame(
    layout: &Rp1Layout,
    active_words: &[usize; BIT_PLANES],
    canvas: &Canvas,
    pwm_low_bit: usize,
    buffer: &mut Vec<u32>,
) {
    let start_bit = canvas.start_bit(pwm_low_bit);
    let double_rows = canvas.double_rows();
    let Rp1Layout {
        output_enable,
        strobe,
        ..
    } = *layout;
    let uses_shift_register = layout.uses_shift_register();
    let shifts_every_bitplane = layout.shifts_every_bitplane();
    buffer.clear();
    buffer.reserve(double_rows * (BIT_PLANES - start_bit) * (canvas.columns() + 8) + 8);

    let mut previous_addr = layout.row_address_bits(canvas.display_row(double_rows - 1));
    let mut previous_active_words = 0;
    let mut last_shift_register_row = None;

    for row_loop in 0..double_rows {
        let display_row = canvas.display_row(row_loop);
        let current_addr = layout.row_address_bits(display_row);

        for (bit, &bit_active_words) in active_words.iter().enumerate().skip(start_bit) {
            let row = canvas.row_at(display_row, 0, bit);
            append_data_header(buffer, row.len());
            // The previous bit plane stays on while the data of this one is clocked in.
            let mut remaining_overlap_words = previous_active_words;
            for &col in row {
                // The RP1 has 28 pins, so the bits of chains above 31 are dropped.
                let mut pins = low_pins(col) | previous_addr;
                if remaining_overlap_words == 0 {
                    pins |= output_enable;
                } else {
                    remaining_overlap_words -= 1;
                }
                buffer.push(pins);
            }
            if remaining_overlap_words > 0 {
                append_delay(
                    buffer,
                    previous_addr,
                    remaining_overlap_words * CLOCKS_PER_DATA_WORD,
                );
            }

            if uses_shift_register {
                if shifts_every_bitplane || last_shift_register_row != Some(display_row) {
                    let states = layout.shift_row_address_states(display_row);
                    let last = states.len().saturating_sub(1);
                    for (i, state) in states.into_iter().enumerate() {
                        // The direct address lines of the SM5266 need time to settle.
                        let settle =
                            layout.row_setter == crate::RowAddressSetterType::SM5266 && i == last;
                        let clocks = if settle { POST_ADDRESS_DELAY_CLOCKS } else { 0 };
                        append_delay(buffer, state, clocks);
                    }
                    last_shift_register_row = Some(display_row);
                }
            } else if current_addr != previous_addr {
                append_delay(
                    buffer,
                    current_addr | output_enable,
                    POST_ADDRESS_DELAY_CLOCKS,
                );
            }
            // Latch the data while the LEDs are off.
            append_delay(buffer, current_addr | output_enable | strobe, 0);

            previous_addr = current_addr;
            previous_active_words = bit_active_words;
        }
    }

    if previous_active_words > 0 {
        append_delay(
            buffer,
            previous_addr,
            previous_active_words * CLOCKS_PER_DATA_WORD,
        );
    }
    append_delay(buffer, previous_addr | output_enable, 0);
}

impl Drop for Rp1Pio {
    fn drop(&mut self) {
        let sm = self.sm;
        let _ = self.ioctl(
            PIO_IOC_SM_SET_ENABLED,
            &mut SmSetEnabledArgs {
                mask: 1 << sm,
                enable: 0,
                rsvd: 0,
            },
        );
        let _ = self.ioctl(PIO_IOC_SM_CLEAR_FIFOS, &mut SmArgs { sm });
        let _ = self.set_pin_directions(false);
        let _ = self.ioctl(PIO_IOC_SM_UNCLAIM, &mut SmClaimArgs { mask: 1 << sm });
    }
}

/// Holds `pins` for the given number of clock cycles.
fn append_delay(buffer: &mut Vec<u32>, pins: u32, clock_cycles: usize) {
    let encoded_cycles = clock_cycles.saturating_sub(DELAY_OVERHEAD_CLOCKS).max(1);
    buffer.push((encoded_cycles - 1) as u32);
    buffer.push(pins);
}

/// Announces that `words` GPIO words follow, which are clocked out one by one.
fn append_data_header(buffer: &mut Vec<u32>, words: usize) {
    buffer.push(COMMAND_DATA | (words - 1) as u32);
}

#[cfg(test)]
mod tests {
    use super::{
        AddProgramArgs, COMMAND_DATA, PIO_IOC_ADD_PROGRAM, PIO_IOC_SM_CLAIM,
        PIO_IOC_SM_XFER_DATA32, SmConfig, SmXferData32Args, append_delay,
    };
    use std::mem::size_of;

    #[test]
    fn encodes_requests_like_the_kernel_headers() {
        // _IOW(102, 20, struct rp1_pio_sm_claim_args) with a 2 byte argument.
        assert_eq!(PIO_IOC_SM_CLAIM, 0x4002_6614);
        // The program argument holds 32 instructions after two 16 bit fields.
        assert_eq!(size_of::<AddProgramArgs>(), 68);
        assert_eq!(PIO_IOC_ADD_PROGRAM, 0x4044_660b);
        assert_eq!(
            PIO_IOC_SM_XFER_DATA32 >> 16 & 0x3fff,
            size_of::<SmXferData32Args>() as u32
        );
    }

    #[test]
    fn configures_state_machine_like_upstream() {
        // Program loaded at 0, clock on GPIO 17, divided by 200 MHz / 27 MHz = 7.41.
        let config = SmConfig::new(0, 17, 200.0 / 27.0);
        assert_eq!(config.clkdiv, (7 << 16) | (104 << 8));
        assert_eq!(config.execctrl, (1 << 30) | (4 << 12));
        assert_eq!(config.shiftctrl, (1 << 30) | (1 << 18) | (1 << 17));
        assert_eq!(config.pinctrl, (2 << 29) | (28 << 20) | (17 << 10));
    }

    #[test]
    fn encodes_delays_and_data() {
        let mut buffer = Vec::new();
        append_delay(&mut buffer, 0xab, 0);
        append_delay(&mut buffer, 0xcd, 20);
        assert_eq!(buffer, [0, 0xab, 14, 0xcd]);
        assert_ne!(COMMAND_DATA & 1 << 31, 0);
    }
}

#[cfg(test)]
mod frame_tests {
    use super::{COMMAND_DATA, build_frame};
    use crate::{
        config::BIT_PLANES,
        rp1::{self, Rp1Layout},
        rp1_rio,
    };

    #[test]
    fn builds_commands_like_upstream() {
        let (config, canvas) = rp1_rio::tests::small_canvas();
        let h = rp1::tests::regular_pins();
        let (oe, strobe, a, r1, b2) = (h.oe, h.strobe, h.a, h.r1, h.b2);
        let layout = Rp1Layout::new(&config, false).unwrap();
        let mut buffer = Vec::new();
        build_frame(&layout, &[3; BIT_PLANES], &canvas, 0, &mut buffer);
        assert_eq!(
            buffer,
            [
                // Row 0, dark while clocking.
                COMMAND_DATA | 1,
                r1 | a | oe,
                a | oe,
                // Change the address, then latch.
                0,
                oe,
                0,
                oe | strobe,
                // Row 1, row 0 stays on for 3 words: 2 while clocking, then a delay of 2 clocks.
                COMMAND_DATA | 1,
                0,
                b2,
                0,
                0,
                0,
                a | oe,
                0,
                a | oe | strobe,
                // Row 1 is on for 3 words of 2 clocks, minus the overhead of 5 clocks.
                0,
                a,
                0,
                a | oe,
            ]
        );
    }
}

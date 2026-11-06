use std::{
    fs::{self, File},
    io,
    rc::Rc,
    sync::Mutex,
};

use memmap2::{MmapMut, MmapOptions};

use crate::{
    chip::PiChip,
    hardware_mapping::{GpioBits, low_pins},
};

// See https://elinux.org/BCM2835_registers

struct MmapPtr<T> {
    ptr: *mut T,
    /// We need to hold on to the map.
    _map_ref: Rc<MmapMut>,
}

impl<T> MmapPtr<T> {
    fn new(map: Rc<MmapMut>, byte_offset: usize) -> Self {
        let ptr = unsafe { map.as_ptr().add(byte_offset) as *mut T };
        Self { ptr, _map_ref: map }
    }

    #[inline(always)]
    fn read(&self) -> T {
        unsafe { self.ptr.read_volatile() }
    }

    #[inline(always)]
    fn write(&self, value: T) {
        unsafe { self.ptr.write_volatile(value) }
    }
}

// General Purpose IO
const GP_OFFSET: u64 = 0x0020_0000;
const GP_SIZE_BYTES: usize = 41 * std::mem::size_of::<u32>();
const GP_FSEL0: usize = 0x0;
const GP_SET0: usize = 0x1C;
const GP_CLR0: usize = 0x28;
const GP_LEV0: usize = 0x34;

struct GPIOFunctionSelectRegisters {
    /// Each register selects the functions of 10 pins.
    registers: [MmapPtr<u32>; 6],
}

impl GPIOFunctionSelectRegisters {
    fn new(map: Rc<MmapMut>, byte_offset: usize) -> Self {
        let registers =
            std::array::from_fn(|index| MmapPtr::new(map.clone(), byte_offset + index * 4));
        Self { registers }
    }

    fn set_function(&mut self, pin: u8, function: GPIOFunction) {
        let register = &self.registers[(pin / 10) as usize];
        let shift = (pin % 10) * 3;
        let clear = !(0b111 << shift);
        let set = function.bits() << shift;
        let value_before = register.read();
        register.write((value_before & clear) | set);
    }
}

#[derive(Clone, Copy)]
#[allow(unused)]
pub(crate) enum GPIOFunction {
    Input,
    Output,
    Alt0,
    Alt1,
    Alt2,
    Alt3,
    Alt4,
    Alt5,
}

impl GPIOFunction {
    fn bits(self) -> u32 {
        match self {
            GPIOFunction::Input => 0b000,
            GPIOFunction::Output => 0b001,
            GPIOFunction::Alt0 => 0b100,
            GPIOFunction::Alt1 => 0b101,
            GPIOFunction::Alt2 => 0b110,
            GPIOFunction::Alt3 => 0b111,
            GPIOFunction::Alt4 => 0b011,
            GPIOFunction::Alt5 => 0b010,
        }
    }
}

/// Maps a register block of the BCM peripherals through `/dev/mem`, shared by the registers in it.
pub(crate) fn mmap_bcm_register(
    chip: PiChip,
    offset: u64,
    size_bytes: usize,
) -> io::Result<Rc<MmapMut>> {
    mmap_bcm_register_unshared(chip, offset, size_bytes).map(Rc::new)
}

/// Opens `/dev/mem`, which needs root privileges (CAP_SYS_RAWIO).
pub(crate) fn open_dev_mem() -> io::Result<File> {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/mem")
}

fn mmap_bcm_register_unshared(chip: PiChip, offset: u64, size_bytes: usize) -> io::Result<MmapMut> {
    mmap_bcm_register_from(&open_dev_mem()?, chip, offset, size_bytes)
}

/// Maps a register block from an already opened `/dev/mem`. The privileges are only checked when opening it, so
/// this also works after they were dropped.
fn mmap_bcm_register_from(
    dev_mem: &File,
    chip: PiChip,
    offset: u64,
    size_bytes: usize,
) -> io::Result<MmapMut> {
    let base = chip.peripherals_base().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "The chip has no BCM peripherals for the GPIO pins.",
        )
    })?;
    // SAFETY: the mapping is of device memory that nothing else in this process maps as Rust objects; it is only
    // accessed through volatile reads and writes of whole registers within `size_bytes`.
    unsafe {
        MmapOptions::new()
            .offset(base + offset)
            .len(size_bytes)
            .map_mut(dev_mem)
    }
}

pub(crate) struct GPIORegisters {
    clr0: MmapPtr<u32>,
    set0: MmapPtr<u32>,
    lvl0: MmapPtr<u32>,
    /// The registers for the pins above 31.
    #[cfg(feature = "wide-gpio")]
    clr1: MmapPtr<u32>,
    #[cfg(feature = "wide-gpio")]
    set1: MmapPtr<u32>,
    #[cfg(feature = "wide-gpio")]
    lvl1: MmapPtr<u32>,
    /// Only write the registers for the pins above 31 if they are used, as each write takes time.
    #[cfg(feature = "wide-gpio")]
    uses_high_bits: bool,
    function_select: GPIOFunctionSelectRegisters,
}

impl GPIORegisters {
    pub(crate) fn new(chip: PiChip) -> io::Result<Self> {
        let map = mmap_bcm_register(chip, GP_OFFSET, GP_SIZE_BYTES)?;
        let clr0 = MmapPtr::new(map.clone(), GP_CLR0);
        let set0 = MmapPtr::new(map.clone(), GP_SET0);
        let lvl0 = MmapPtr::new(map.clone(), GP_LEV0);
        #[cfg(feature = "wide-gpio")]
        let (clr1, set1, lvl1) = (
            MmapPtr::new(map.clone(), GP_CLR0 + 4),
            MmapPtr::new(map.clone(), GP_SET0 + 4),
            MmapPtr::new(map.clone(), GP_LEV0 + 4),
        );
        let function_select = GPIOFunctionSelectRegisters::new(map, GP_FSEL0);
        Ok(Self {
            clr0,
            set0,
            lvl0,
            #[cfg(feature = "wide-gpio")]
            clr1,
            #[cfg(feature = "wide-gpio")]
            set1,
            #[cfg(feature = "wide-gpio")]
            lvl1,
            #[cfg(feature = "wide-gpio")]
            uses_high_bits: false,
            function_select,
        })
    }

    /// Marks the pins as used, which might need the registers for the pins above 31.
    pub(crate) fn use_bits(&mut self, _bits: GpioBits) {
        #[cfg(feature = "wide-gpio")]
        {
            self.uses_high_bits |= _bits >> 32 != 0;
        }
    }

    pub(crate) fn write_clr_bits(&mut self, value: GpioBits) {
        self.clr0.write(low_pins(value));
        #[cfg(feature = "wide-gpio")]
        if self.uses_high_bits {
            self.clr1.write((value >> 32) as u32);
        }
    }

    /// Writes nothing, which takes as long as one real write. This only writes the register of the low pins, so
    /// that a delay takes as long with the pins above 31 in use as without.
    #[inline(always)]
    pub(crate) fn write_nothing(&mut self) {
        self.clr0.write(0);
    }

    pub(crate) fn write_set_bits(&mut self, value: GpioBits) {
        self.set0.write(low_pins(value));
        #[cfg(feature = "wide-gpio")]
        if self.uses_high_bits {
            self.set1.write((value >> 32) as u32);
        }
    }

    pub(crate) fn select_function(&mut self, pin: u8, function: GPIOFunction) {
        self.function_select.set_function(pin, function);
    }

    pub(crate) fn read_pin_levels(&self) -> GpioBits {
        #[cfg(feature = "wide-gpio")]
        return GpioBits::from(self.lvl0.read()) | (GpioBits::from(self.lvl1.read()) << 32);
        #[cfg(not(feature = "wide-gpio"))]
        self.lvl0.read()
    }
}

/// The GPIO set and clear registers for the outputs that the user controls. Unlike [`GPIORegisters`], these
/// can be used from any thread.
pub(crate) struct UserGpioOutputs {
    map: Mutex<MmapMut>,
}

impl UserGpioOutputs {
    /// Maps the registers from `dev_mem`, which can be opened before the privileges are dropped.
    pub(crate) fn new(dev_mem: &File, chip: PiChip) -> io::Result<Self> {
        Ok(Self {
            map: Mutex::new(mmap_bcm_register_from(
                dev_mem,
                chip,
                GP_OFFSET,
                GP_SIZE_BYTES,
            )?),
        })
    }

    /// Sets the bits in `mask` to the values in `value`.
    pub(crate) fn write_masked_bits(&self, value: u32, mask: u32) {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        let base = map.as_mut_ptr();
        unsafe {
            base.add(GP_CLR0)
                .cast::<u32>()
                .write_volatile(!value & mask);
            base.add(GP_SET0).cast::<u32>().write_volatile(value & mask);
        }
    }
}

/// Waits until all previous writes to the GPIO registers are done.
#[inline(always)]
pub(crate) fn store_barrier() {
    #[cfg(any(
        target_arch = "aarch64",
        all(target_arch = "arm", target_feature = "v7")
    ))]
    unsafe {
        std::arch::asm!("dsb st", options(nostack, preserves_flags));
    }
}

/// Whether [`store_barrier`] does anything on this architecture.
pub(crate) const STORE_BARRIER_SUPPORTED: bool = cfg!(any(
    target_arch = "aarch64",
    all(target_arch = "arm", target_feature = "v7")
));

// System Timer
const ST_OFFSET: u64 = 0x3000;
const ST_SIZE_BYTES: usize = 28;
const ST_CLO: usize = 0x4;
const ST_CHI: usize = 0x8;

// Time measurement.
pub(crate) struct TimeRegisters {
    low: MmapPtr<u32>,
    high: MmapPtr<u32>,
}

impl TimeRegisters {
    pub(crate) fn new(chip: PiChip) -> io::Result<Self> {
        let map = mmap_bcm_register(chip, ST_OFFSET, ST_SIZE_BYTES)?;
        let low = MmapPtr::new(map.clone(), ST_CLO);
        let high = MmapPtr::new(map, ST_CHI);
        Ok(Self { low, high })
    }

    /// Time in microseconds from the free running 64 bit system timer.
    #[inline]
    pub(crate) fn time(&self) -> u64 {
        // The two halves can't be read at once. If the low half overflowed in between the reads, the high half
        // changes and we read again.
        loop {
            let high = self.high.read();
            let low = self.low.read();
            if self.high.read() == high {
                return (u64::from(high) << u32::BITS) | u64::from(low);
            }
        }
    }
}

// Pulse Width Modulator
const PWM_OFFSET: u64 = 0x0020_C000;
const PWM_SIZE_BYTES: usize = 32;
const PWM_CTL: usize = 0x00;
const PWM_STA: usize = 0x04;
const PWM_RNG1: usize = 0x10;
const PWM_FIF1: usize = 0x18;
const PWM_STA_EMPT1: u32 = 0x1 << 1;

/// CH1 Enable (0=disable 1=enable)
const PWM_CTL_PWEN1: u32 = 1 << 0;
/// CH1 Polarity (0: 0=low 1=high, 1: 1=low 0=high)
const PWM_CTL_POLA1: u32 = 1 << 4;
/// CH1 Use FIFO (0=data reg transmit 1=FIFO used for transmission)
const PWM_CTL_USEF1: u32 = 1 << 5;
/// CH1 Clear FIFO (1 Clears FIFO 0 has no effect)
const PWM_CTL_CLRF1: u32 = 1 << 6;

pub(crate) struct PWMRegisters {
    ctl: MmapPtr<u32>,
    rng1: MmapPtr<u32>,
    fif1: MmapPtr<u32>,
    sta: MmapPtr<u32>,
}

impl PWMRegisters {
    pub(crate) fn new(chip: PiChip) -> io::Result<Self> {
        let map = mmap_bcm_register(chip, PWM_OFFSET, PWM_SIZE_BYTES)?;
        let ctl = MmapPtr::new(map.clone(), PWM_CTL);
        let rng1 = MmapPtr::new(map.clone(), PWM_RNG1);
        let fif1 = MmapPtr::new(map.clone(), PWM_FIF1);
        let sta = MmapPtr::new(map, PWM_STA);
        Ok(Self {
            ctl,
            rng1,
            fif1,
            sta,
        })
    }

    /// Channel 1: Use FIFO | Polarity (1=low, 0=high) | Enable Channel
    pub(crate) fn enable_pwm(&mut self) {
        self.set_pwm_ctl(PWM_CTL_USEF1 | PWM_CTL_POLA1 | PWM_CTL_PWEN1);
    }

    /// Channel 1: Use FIFO | Polarity (1=low, 0=high) | Clear FIFO
    pub(crate) fn reset_pwm(&mut self) {
        self.set_pwm_ctl(PWM_CTL_USEF1 | PWM_CTL_POLA1 | PWM_CTL_CLRF1);
    }

    fn set_pwm_ctl(&mut self, value: u32) {
        self.ctl.write(value);
    }

    pub(crate) fn set_pwm_pulse_period(&mut self, value: u32) {
        self.rng1.write(value);
    }

    pub(crate) fn push_fifo(&mut self, value: u32) {
        self.fif1.write(value);
    }

    pub(crate) fn fifo_empty(&self) -> bool {
        (self.sta.read() & PWM_STA_EMPT1) != 0
    }
}

// Clock Manager
const CM_OFFSET: u64 = 0x0010_1000;
const CM_SIZE_BYTES: usize = 452;
const CM_PASSWD: u32 = 0x5A << 24;
const CM_PWMCTL: usize = 0xA0;
const CM_PWMDIV: usize = 0xA4;
const CM_PWMCTL_ENAB: u32 = 0x1 << 4;
const CM_PWMCTL_KILL: u32 = 0x1 << 5;
/// PLLD, which runs at 500 MHz on the BCM2835/6/7. The PWM divider in `HardwarePinPulser` assumes 500 MHz.
///
/// Known limitation: on the BCM2711 (Raspberry Pi 4) PLLD (`plld_per`) is reported to run at 750 MHz, which would
/// make the hardware pulses about 2/3 as long as `pwm_lsb_nanoseconds` asks for (as in upstream, which assumes
/// 500 MHz as well). This is not corrected here: the frequency depends on the firmware and could not be verified
/// on hardware, and the panels may be tuned to the current behaviour. The Raspberry Pi 1 to 3 (and Zero 2 W) are
/// not affected.
const CM_SRC_PLLD: u32 = 6;

/// Set the clock source.
const fn cm_ctl_src(x: u32) -> u32 {
    // bits 3-0
    let offset = 0;
    x << offset
}

/// Integer part of divider.
const fn cm_div_divi(x: u32) -> u32 {
    // bits 23-12
    let offset = 12;
    x << offset
}

pub(crate) struct ClkRegisters {
    pwm_ctl: MmapPtr<u32>,
    pwm_div: MmapPtr<u32>,
}

impl ClkRegisters {
    pub(crate) fn new(chip: PiChip) -> io::Result<Self> {
        let map = mmap_bcm_register(chip, CM_OFFSET, CM_SIZE_BYTES)?;
        let pwm_ctl = MmapPtr::new(map.clone(), CM_PWMCTL);
        let pwm_div = MmapPtr::new(map, CM_PWMDIV);
        Ok(Self { pwm_ctl, pwm_div })
    }

    pub(crate) fn init_pwm_divider(&mut self, divider: u32) {
        assert!(divider < (1 << 12)); // we only have 12 bits.

        // reset PWM clock
        self.pwm_ctl.write(CM_PASSWD | CM_PWMCTL_KILL);

        // set PWM clock source as 500 MHz PLLD
        self.pwm_ctl.write(CM_PASSWD | cm_ctl_src(CM_SRC_PLLD));

        // set PWM clock divider
        self.pwm_div
            .write(CM_PASSWD | cm_div_divi(divider));

        // enable PWM clock
        self.pwm_ctl
            .write(CM_PASSWD | CM_PWMCTL_ENAB | cm_ctl_src(CM_SRC_PLLD));
    }
}

use std::{fs::OpenOptions, rc::Rc, thread::sleep, time::Duration};

use memmap2::{MmapMut, MmapOptions};

use crate::chip::PiChip;

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
    registers_by_function: [MmapPtr<u32>; 6],
}

impl GPIOFunctionSelectRegisters {
    fn new(map: Rc<MmapMut>, byte_offset: usize) -> Self {
        let registers_by_function = [
            MmapPtr::new(map.clone(), byte_offset),
            MmapPtr::new(map.clone(), byte_offset + 4),
            MmapPtr::new(map.clone(), byte_offset + 2 * 4),
            MmapPtr::new(map.clone(), byte_offset + 3 * 4),
            MmapPtr::new(map.clone(), byte_offset + 4 * 4),
            MmapPtr::new(map, byte_offset + 5 * 4),
        ];
        Self {
            registers_by_function,
        }
    }

    fn set_function(&mut self, pin: u8, function: GPIOFunction) {
        let function_index = (pin / 10) as usize;
        let register = &self.registers_by_function[function_index];
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

pub fn mmap_bcm_register(chip: PiChip, offset: u64, size_bytes: usize) -> Rc<MmapMut> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/mem")
        .expect("Failed to open '/dev/mem'");
    let base = chip.get_peripherals_base();
    let map = unsafe {
        MmapOptions::new()
            .offset(base + offset)
            .len(size_bytes)
            .map_mut(&file)
            .unwrap()
    };
    Rc::new(map)
}

pub(crate) struct GPIORegisters {
    clr0: MmapPtr<u32>,
    set0: MmapPtr<u32>,
    lvl0: MmapPtr<u32>,
    function_select: GPIOFunctionSelectRegisters,
}

impl GPIORegisters {
    pub(crate) fn new(chip: PiChip) -> Self {
        let map = mmap_bcm_register(chip, GP_OFFSET, GP_SIZE_BYTES);
        let clr0 = MmapPtr::new(map.clone(), GP_CLR0);
        let set0 = MmapPtr::new(map.clone(), GP_SET0);
        let lvl0 = MmapPtr::new(map.clone(), GP_LEV0);
        let function_select = GPIOFunctionSelectRegisters::new(map, GP_FSEL0);
        Self {
            clr0,
            set0,
            lvl0,
            function_select,
        }
    }

    pub(crate) fn write_clr_bits(&mut self, value: u32) {
        self.clr0.write(value);
    }

    pub(crate) fn write_set_bits(&mut self, value: u32) {
        self.set0.write(value);
    }

    pub(crate) fn select_function(&mut self, pin: u8, function: GPIOFunction) {
        self.function_select.set_function(pin, function);
    }

    pub(crate) fn read_pin_level0(&self) -> u32 {
        self.lvl0.read()
    }
}

// System Timer
const ST_OFFSET: u64 = 0x3000;
const ST_SIZE_BYTES: usize = 28;
const ST_CLO: usize = 0x4;

const MIN_SYS_SLEEP_TIME_US: u64 = 100;

/// Required to read `ST_CLO` and the adjacent `ST_CHI`.
/// This has to be a struct so that we can have a fixed memory layout.
#[repr(C)]
struct TimeRegister {
    low: u32,
    high: u32,
}

impl TimeRegister {
    fn get_u64(&self) -> u64 {
        (u64::from(self.high) << u32::BITS) | u64::from(self.low)
    }
}

// Time measurement.
pub(crate) struct TimeRegisters {
    time: MmapPtr<TimeRegister>,
    sleep_factor: f32,
}

impl TimeRegisters {
    pub(crate) fn new(chip: PiChip) -> Self {
        let map = mmap_bcm_register(chip, ST_OFFSET, ST_SIZE_BYTES);
        let time = MmapPtr::new(map, ST_CLO);
        Self {
            time,
            sleep_factor: 0.4,
        }
    }

    pub(crate) fn get_time(&self) -> u64 {
        self.time.read().get_u64()
    }

    pub(crate) fn sleep(&mut self, duration_us: u64) {
        let end_time = self.get_time() + duration_us;
        self.sleep_at_most(duration_us);
        while self.get_time() < end_time {
            std::hint::spin_loop();
        }
    }

    pub(crate) fn sleep_at_most(&mut self, duration_us: u64) {
        if duration_us > MIN_SYS_SLEEP_TIME_US {
            let sys_sleep_time = (duration_us as f32 * self.sleep_factor) as u64;
            sleep(Duration::from_micros(sys_sleep_time));
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
pub(crate) const PWM_CTL_PWEN1: u32 = 1 << 0;
/// CH1 Polarity (0: 0=low 1=high, 1: 1=low 0=high)
pub(crate) const PWM_CTL_POLA1: u32 = 1 << 4;
/// CH1 Use FIFO (0=data reg transmit 1=FIFO used for transmission)
pub(crate) const PWM_CTL_USEF1: u32 = 1 << 5;
/// CH1 Clear FIFO (1 Clears FIFO 0 has no effect)
pub(crate) const PWM_CTL_CLRF1: u32 = 1 << 6;

pub(crate) struct PWMRegisters {
    ctl: MmapPtr<u32>,
    rng1: MmapPtr<u32>,
    fif1: MmapPtr<u32>,
    sta: MmapPtr<u32>,
}

impl PWMRegisters {
    pub(crate) fn new(chip: PiChip) -> Self {
        let map = mmap_bcm_register(chip, PWM_OFFSET, PWM_SIZE_BYTES);
        let ctl = MmapPtr::new(map.clone(), PWM_CTL);
        let rng1 = MmapPtr::new(map.clone(), PWM_RNG1);
        let fif1 = MmapPtr::new(map.clone(), PWM_FIF1);
        let sta = MmapPtr::new(map, PWM_STA);
        Self {
            ctl,
            rng1,
            fif1,
            sta,
        }
    }

    /// Channel 1: Use FIFO | Polarity (1=low, 0=high) | Enable Channel
    pub(crate) fn enable_pwm(&mut self) {
        self.set_pwm_ctl(PWM_CTL_USEF1 | PWM_CTL_POLA1 | PWM_CTL_PWEN1);
    }

    /// Channel 1: Use FIFO | Polarity (1=low, 0=high) | Clear FIFO
    pub(crate) fn reset_pwm(&mut self) {
        self.set_pwm_ctl(PWM_CTL_USEF1 | PWM_CTL_POLA1 | PWM_CTL_CLRF1);
    }

    pub(crate) fn set_pwm_ctl(&mut self, value: u32) {
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
const CM_SRC_PLLD: u32 = 6; /* 500.0 MHz */

/// Set the clock source.
const fn cm_ctl_src(x: u32) -> u32 {
    // bits 3-0
    let offset = 0;
    x << offset
}
/// Fractional part of divider.
const fn cm_div_divf(x: u32) -> u32 {
    // bits 11-0
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
    pub(crate) fn new(chip: PiChip) -> Self {
        let map = mmap_bcm_register(chip, CM_OFFSET, CM_SIZE_BYTES);
        let pwm_ctl = MmapPtr::new(map.clone(), CM_PWMCTL);
        let pwm_div = MmapPtr::new(map, CM_PWMDIV);
        Self { pwm_ctl, pwm_div }
    }

    pub(crate) fn init_pwm_divider(&mut self, divider: u32) {
        assert!(divider < (1 << 12)); // we only have 12 bits.

        // reset PWM clock
        self.pwm_ctl.write(CM_PASSWD | CM_PWMCTL_KILL);

        // set PWM clock source as 500 MHz PLLD
        self.pwm_ctl.write(CM_PASSWD | cm_ctl_src(CM_SRC_PLLD));

        // set PWM clock divider
        self.pwm_div
            .write(CM_PASSWD | cm_div_divi(divider) | cm_div_divf(0));

        // enable PWM clock
        self.pwm_ctl
            .write(CM_PASSWD | CM_PWMCTL_ENAB | cm_ctl_src(CM_SRC_PLLD));
    }
}

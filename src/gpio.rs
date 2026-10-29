use std::{
    error::Error,
    fmt::{Display, Formatter},
    time::Duration,
};

use crate::{
    RGBMatrixConfig,
    chip::PiChip,
    config::K_BIT_PLANES,
    gpio_bits,
    pin_pulser::{PinPulser, pwm_pin_function},
    registers::{ClkRegisters, GPIOFunction, GPIORegisters, PWMRegisters, TimeRegisters},
    row_address_setter::RowAddressSetter,
    utils::{drop_privs, linux_has_module_loaded, sleep_exactly},
};

#[derive(Debug)]
pub enum GpioInitializationError {
    OneWireProtocolEnabled,
    SoundModuleLoaded,
    FailedPrivilegeDrop(String),
}

impl Error for GpioInitializationError {}

impl Display for GpioInitializationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            GpioInitializationError::OneWireProtocolEnabled => f.write_str(
                "The Raspberry Pi has the one-wire protocol enabled.\n\
                This will mess with the display if GPIO pins overlap.\n\
                Disable 1-wire in raspi-config (Interface Options).",
            ),
            GpioInitializationError::SoundModuleLoaded => f.write_str(
                "The sound module is loaded. Disable on-board sound first.\n\
                \t* Raspberry PI OS: Set `dtparam=audio=off` in `/boot/config.txt`\n\
                \t* Other distributions: Add `blacklist snd_bcm2835` in \
                `/etc/modprobe.d/alsa-blacklist.conf`\n\
                Finally, reboot the system and try again.",
            ),
            GpioInitializationError::FailedPrivilegeDrop(e) => {
                f.write_str(&format!("Failed to drop privileges. Reason: {}", e))
            }
        }
    }
}

pub(crate) struct Gpio {
    gpio_registers: GPIORegisters,
    time_registers: TimeRegisters,
    pin_pulser: PinPulser,
    input_bits: u32,
    output_bits: u32,
    reserved_bits: u32,
    gpio_slowdown: u32,
    min_sys_sleep_time: Duration,
}

impl Gpio {
    /// Initialize GPIO and loads all registers. Needs root privileges.
    /// Will result in the application dropping those privileges if privilege dropping is enabled in the config.
    pub(crate) fn new(
        chip: PiChip,
        config: &RGBMatrixConfig,
        address_setter: &dyn RowAddressSetter,
    ) -> Result<Self, GpioInitializationError> {
        // The sound module uses the PWM peripheral, which we need for hardware pulsing.
        let hardware_pulsing = pwm_pin_function(config.hardware_mapping.output_enable).is_some();
        if hardware_pulsing && linux_has_module_loaded("snd_bcm2835") {
            return Err(GpioInitializationError::SoundModuleLoaded);
        }

        let mut gpio_registers = GPIORegisters::new(chip);
        let time_registers = TimeRegisters::new(chip);
        let pwm_registers = PWMRegisters::new(chip);
        let mut clk_registers = ClkRegisters::new(chip);

        // Drop privileges here as we no longer need root.
        if config.drop_privs {
            let res = drop_privs(&config.drop_priv_user, &config.drop_priv_group);
            if let Err(s) = res {
                return Err(GpioInitializationError::FailedPrivilegeDrop(s));
            }
        }

        // All bits we intend to use are outputs.
        let output_bits = config.hardware_mapping.used_bits() | address_setter.used_bits();

        // Hack: for the PWM mod, the user soldered together GPIO 18 (new OE)
        // with GPIO 4 (old OE).
        // Since they are connected inside the HAT, want to make extra sure that,
        // whatever the outside system set as pinmux, the old OE is _not_ also
        // set as output so that these GPIO outputs don't fight each other.
        //
        // So explicitly set both of these pins as input initially, so the user
        // can switch between the two modes "adafruit-hat" and "adafruit-hat-pwm"
        // without trouble.
        gpio_registers.select_function(4, GPIOFunction::Input);
        gpio_registers.select_function(18, GPIOFunction::Input);
        // Even with PWM enabled, GPIO4 still can not be used, because it is
        // now connected to the GPIO18 and thus must stay an input.
        // So reserve this bit if it is not set in outputs.
        let reserved_bits = gpio_bits!(4) & !output_bits;

        if output_bits & gpio_bits!(4) != 0 && linux_has_module_loaded("w1_gpio") {
            return Err(GpioInitializationError::OneWireProtocolEnabled);
        }

        gpio_registers.select_function_for_bits(output_bits, GPIOFunction::Output);

        // Each bit plane is shown twice as long as the previous one, except for the dithered ones.
        let bitplane_timings: Vec<u32> = (0..K_BIT_PLANES)
            .map(|b| config.pwm_lsb_nanoseconds << b.saturating_sub(config.dither_bits))
            .collect();

        let min_sys_sleep_time = Duration::from_micros(config.min_sys_sleep_microseconds);

        let pin_pulser = PinPulser::new(
            config.hardware_mapping.output_enable,
            &bitplane_timings,
            min_sys_sleep_time,
            pwm_registers,
            &mut gpio_registers,
            &mut clk_registers,
        );

        let gpio_slowdown = config.slowdown.unwrap_or_else(|| chip.gpio_slowdown());

        Ok(Self {
            gpio_registers,
            time_registers,
            pin_pulser,
            input_bits: 0,
            output_bits,
            reserved_bits,
            gpio_slowdown,
            min_sys_sleep_time,
        })
    }

    pub(crate) fn write_masked_bits(&mut self, value: u32, mask: u32) {
        self.clear_bits(!value & mask);
        self.set_bits(value & mask);
    }

    pub(crate) fn clear_bits(&mut self, value: u32) {
        if value == 0 {
            return;
        };
        for _ in 0..=self.gpio_slowdown {
            self.gpio_registers.write_clr_bits(value);
        }
    }

    pub(crate) fn set_bits(&mut self, value: u32) {
        if value == 0 {
            return;
        };
        for _ in 0..=self.gpio_slowdown {
            self.gpio_registers.write_set_bits(value);
        }
    }

    pub(crate) fn send_pulse(&mut self, bitplane: usize) {
        match &mut self.pin_pulser {
            PinPulser::Hardware(pin_pulser) => {
                pin_pulser.send_pulse(bitplane, &self.time_registers)
            }
            PinPulser::Software {
                pins,
                pulse_durations,
            } => {
                let (pins, duration) = (*pins, pulse_durations[bitplane]);
                // The output enable is active low. Unlike the hardware pulse, this blocks until it is
                // finished.
                self.clear_bits(pins);
                sleep_exactly(duration, self.min_sys_sleep_time);
                self.set_bits(pins);
            }
        }
    }

    pub(crate) fn wait_pulse_finished(&mut self) {
        if let PinPulser::Hardware(pin_pulser) = &mut self.pin_pulser {
            pin_pulser.wait_pulse_finished(&self.time_registers);
        }
    }

    pub(crate) fn request_enabled_inputs(&mut self, mut enabled_bits: u32) -> u32 {
        // Remove the bits that are already used otherwise.
        enabled_bits &= !(self.output_bits | self.input_bits | self.reserved_bits);

        self.gpio_registers
            .select_function_for_bits(enabled_bits, GPIOFunction::Input);
        self.input_bits |= enabled_bits;
        enabled_bits
    }

    pub(crate) fn read(&mut self) -> u32 {
        self.gpio_registers.read_pin_level0() & self.input_bits
    }

    /// Time instant in microseconds.
    pub(crate) fn get_time(&self) -> u64 {
        self.time_registers.get_time()
    }

    /// Sleep for exactly this many microseconds.
    pub(crate) fn sleep(&mut self, duration_us: u64) {
        self.time_registers
            .sleep(duration_us, self.min_sys_sleep_time);
    }
}

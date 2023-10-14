use std::{
    error::Error,
    fmt::{Display, Formatter},
};

use crate::{
    chip::PiChip,
    config::K_BIT_PLANES,
    gpio_bits,
    pin_pulser::PinPulser,
    registers::{ClkRegisters, GPIOFunction, GPIORegisters, PWMRegisters, TimeRegisters},
    row_address_setter::RowAddressSetter,
    utils::linux_has_module_loaded,
    RGBMatrixConfig,
};

#[derive(Debug)]
pub enum GpioInitializationError {
    OneWireProtocolEnabled,
    SoundModuleLoaded,
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
        }
    }
}

pub(crate) struct Gpio {
    gpio_registers: GPIORegisters,
    time_registers: TimeRegisters,
    pwm_registers: PWMRegisters,
    pin_pulser: PinPulser,
    input_bits: u32,
    output_bits: u32,
    reserved_bits: u32,
    gpio_slowdown: u32,
}

impl Gpio {
    /// Initialize GPIO and loads all registers. Needs root privileges.
    pub(crate) fn new(
        chip: PiChip,
        config: &RGBMatrixConfig,
        address_setter: &dyn RowAddressSetter,
    ) -> Result<Self, GpioInitializationError> {
        if linux_has_module_loaded("snd_bcm2835") {
            return Err(GpioInitializationError::SoundModuleLoaded);
        }

        let mut gpio_registers = GPIORegisters::new(chip);
        let time_registers = TimeRegisters::new(chip);
        let mut pwm_registers = PWMRegisters::new(chip);
        let mut clk_registers = ClkRegisters::new(chip);
        // TODO: We can drop privileges here.

        // Tell GPIO about all bits we intend to use.
        let mut all_used_bits: u32 = 0;
        all_used_bits |= config.hardware_mapping.used_bits();
        all_used_bits |= address_setter.used_bits();

        let input_bits = 0;
        let mut output_bits = all_used_bits;
        let mut reserved_bits = 0;

        // Initialize outputs
        {
            // Hack: for the PWM mod, the user soldered together GPIO 18 (new OE)
            // with GPIO 4 (old OE).
            // Since they are connected inside the HAT, want to make extra sure that,
            // whatever the outside system set as pinmux, the old OE is _not_ also
            // set as output so that these GPIO outputs don't fight each other.
            //
            // So explicitly set both of these pins as input initially, so the user
            // can switch between the two modes "adafruit-hat" and "adafruit-hat-pwm"
            // without trouble.
            {
                gpio_registers.select_function(4, GPIOFunction::Input);
                gpio_registers.select_function(18, GPIOFunction::Input);
                // Even with PWM enabled, GPIO4 still can not be used, because it is
                // now connected to the GPIO18 and thus must stay an input.
                // So reserve this bit if it is not set in outputs.
                reserved_bits |= gpio_bits!(4) & !output_bits;
            }

            output_bits &= !(input_bits | reserved_bits);

            if output_bits & gpio_bits!(4) != 0 && linux_has_module_loaded("w1_gpio") {
                return Err(GpioInitializationError::OneWireProtocolEnabled);
            }

            let k_max_available_bit = 31;
            (0..=k_max_available_bit).for_each(|b| {
                if output_bits & gpio_bits!(b) != 0 {
                    gpio_registers.select_function(b, GPIOFunction::Output);
                }
            });
        }
        assert!(output_bits == all_used_bits);

        let mut bitplane_timings = Vec::new();
        let mut timing_ns = config.pwm_lsb_nanoseconds;
        (0..K_BIT_PLANES).for_each(|b| {
            bitplane_timings.push(timing_ns);
            if b >= config.dither_bits {
                timing_ns *= 2;
            };
        });

        let pin_pulser = PinPulser::new(
            config.hardware_mapping.output_enable,
            &bitplane_timings,
            &mut pwm_registers,
            &mut gpio_registers,
            &mut clk_registers,
        );

        let gpio_slowdown = config.slowdown.unwrap_or_else(|| chip.gpio_slowdown());

        Ok(Self {
            gpio_registers,
            time_registers,
            pwm_registers,
            pin_pulser,
            input_bits,
            output_bits,
            reserved_bits,
            gpio_slowdown,
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
        let Gpio {
            time_registers,
            pwm_registers,
            pin_pulser,
            ..
        } = self;
        pin_pulser.send_pulse(bitplane, pwm_registers, time_registers);
    }

    pub(crate) fn wait_pulse_finished(&mut self) {
        let Gpio {
            time_registers,
            pwm_registers,
            pin_pulser,
            ..
        } = self;
        pin_pulser.wait_pulse_finished(time_registers, pwm_registers);
    }

    pub(crate) fn request_enabled_inputs(&mut self, mut enabled_bits: u32) -> u32 {
        // Remove the bits that are already used otherwise.
        enabled_bits &= !(self.output_bits | self.input_bits | self.reserved_bits);

        let k_max_available_bit = 31;
        (0..=k_max_available_bit).for_each(|b| {
            if (enabled_bits & gpio_bits!(b)) != 0 {
                self.gpio_registers.select_function(b, GPIOFunction::Input);
            }
        });
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
        self.time_registers.sleep(duration_us);
    }
}

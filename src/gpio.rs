use std::{
    error::Error,
    fmt::{Display, Formatter},
    time::Duration,
};

use crate::{
    RGBMatrixConfig,
    chip::PiChip,
    gpio_bits,
    hardware_mapping::GpioBits,
    pin_pulser::{HardwarePinPulser, PinPulser, TimerPinPulser, uses_hardware_pulse},
    registers::{
        ClkRegisters, GPIOFunction, GPIORegisters, PWMRegisters, TimeRegisters, store_barrier,
    },
    rgb_matrix::MatrixCreationError,
    row_address_setter::RowAddressSetter,
    utils::{linux_has_module_loaded, sleep_nanos},
};

/// The highest GPIO pin that can be used.
#[cfg(not(feature = "wide-gpio"))]
const MAX_AVAILABLE_BIT: u8 = 31;
#[cfg(feature = "wide-gpio")]
const MAX_AVAILABLE_BIT: u8 = 45;

/// An error while setting up the GPIO pins.
#[derive(Debug)]
#[non_exhaustive]
pub enum GpioInitializationError {
    /// The one-wire kernel module is loaded, which conflicts with a hardware mapping that uses GPIO 4.
    OneWireProtocolEnabled,
    /// The sound module is loaded, which conflicts with the PWM peripheral used for hardware pulsing.
    SoundModuleLoaded,
    /// Privileges could not be dropped, for the given reason.
    FailedPrivilegeDrop(String),
    /// The PIO state machine of the Raspberry Pi 5 could not be set up or used.
    Rp1Pio(String),
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
                \t* Raspberry Pi OS: Set `dtparam=audio=off` in `/boot/config.txt`\n\
                \t* Other distributions: Add `blacklist snd_bcm2835` in \
                `/etc/modprobe.d/alsa-blacklist.conf`\n\
                Finally, reboot the system and try again.",
            ),
            GpioInitializationError::FailedPrivilegeDrop(e) => {
                f.write_str(&format!("Failed to drop privileges. Reason: {}", e))
            }
            GpioInitializationError::Rp1Pio(message) => f.write_str(message),
        }
    }
}

pub(crate) struct Gpio {
    gpio_registers: GPIORegisters,
    time_registers: TimeRegisters,
    pwm_registers: PWMRegisters,
    pin_pulser: PinPulser,
    input_bits: GpioBits,
    output_bits: GpioBits,
    reserved_bits: GpioBits,
    /// The pins that drive the matrix, which are turned back into inputs on drop (except the output enable).
    matrix_bits: GpioBits,
    /// The (active low) output enable pins.
    output_enable: GpioBits,
    /// How many extra writes to wait after each GPIO change, or -1 to wait for a memory barrier.
    gpio_slowdown: i32,
    /// Waits for the pulses wake up this early and busy wait the rest.
    min_sys_sleep_time: Duration,
}

impl Gpio {
    /// Initialize GPIO and loads all registers. Needs root privileges.
    pub(crate) fn new(
        chip: PiChip,
        config: &RGBMatrixConfig,
        address_setter: &RowAddressSetter,
    ) -> Result<Self, MatrixCreationError> {
        let hardware_pulse = uses_hardware_pulse(config);
        // The sound driver uses the PWM peripheral, which is only a problem if we use it as well.
        if hardware_pulse && linux_has_module_loaded("snd_bcm2835") {
            return Err(MatrixCreationError::GpioError(
                GpioInitializationError::SoundModuleLoaded,
            ));
        }

        let memory_error = MatrixCreationError::MemoryAccessError;
        let mut gpio_registers = GPIORegisters::new(chip).map_err(memory_error)?;
        let time_registers = TimeRegisters::new(chip).map_err(memory_error)?;
        let mut pwm_registers = PWMRegisters::new(chip).map_err(memory_error)?;
        let mut clk_registers = ClkRegisters::new(chip).map_err(memory_error)?;

        // Tell GPIO about all bits we intend to use.
        let mut all_used_bits: GpioBits = 0;
        all_used_bits |= config.hardware_mapping.used_bits(config.parallel);
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
                return Err(MatrixCreationError::GpioError(
                    GpioInitializationError::OneWireProtocolEnabled,
                ));
            }

            (0..=MAX_AVAILABLE_BIT).for_each(|b| {
                if output_bits & gpio_bits!(b) != 0 {
                    gpio_registers.select_function(b, GPIOFunction::Output);
                }
            });
            gpio_registers.use_bits(output_bits);
        }
        assert!(output_bits == all_used_bits);

        let bitplane_timings = config.bitplane_timings_ns();

        let output_enable = config.hardware_mapping.output_enable;
        let pin_pulser = if hardware_pulse {
            PinPulser::Hardware(HardwarePinPulser::new(
                output_enable,
                &bitplane_timings,
                &mut pwm_registers,
                &mut gpio_registers,
                &mut clk_registers,
            ))
        } else {
            // Output enable is active low, so keep the LEDs off until the first pulse.
            gpio_registers.write_set_bits(output_enable);
            PinPulser::Timer(TimerPinPulser::new(output_enable, &bitplane_timings))
        };

        let gpio_slowdown = config.slowdown.unwrap_or_else(|| chip.gpio_slowdown());

        Ok(Self {
            gpio_registers,
            time_registers,
            pwm_registers,
            pin_pulser,
            input_bits,
            output_bits,
            reserved_bits,
            matrix_bits: all_used_bits,
            output_enable,
            gpio_slowdown,
            min_sys_sleep_time: Duration::from_micros(config.min_sys_sleep_microseconds),
        })
    }

    /// Stops the hardware pulses, switches the LEDs off with the output enable, and turns the other pins of the
    /// matrix back into inputs. The output enable stays an output that is driven high (LEDs off): as an input,
    /// the default pull-down of e.g. GPIO 18 would enable the LEDs again.
    pub(crate) fn release(&mut self) {
        if let PinPulser::Hardware(_) = self.pin_pulser {
            self.wait_pulse_finished();
            self.pwm_registers.reset_pwm();
        }
        // Drive the output enable inactive (high) from the GPIO instead of the PWM before letting go of it.
        // Set the level latch first so the pin does not briefly drive low when it becomes an output.
        self.gpio_registers.write_set_bits(self.output_enable);
        store_barrier();
        for b in 0..=MAX_AVAILABLE_BIT {
            if self.output_enable & gpio_bits!(b) != 0 {
                self.gpio_registers.select_function(b, GPIOFunction::Output);
            }
        }
        self.set_bits(self.output_enable);
        store_barrier();
        let released = self.matrix_bits & !self.output_enable;
        for b in 0..=MAX_AVAILABLE_BIT {
            if released & gpio_bits!(b) != 0 {
                self.gpio_registers.select_function(b, GPIOFunction::Input);
            }
        }
        self.output_bits &= !released;
    }

    pub(crate) fn write_masked_bits(&mut self, value: GpioBits, mask: GpioBits) {
        self.clear_bits(!value & mask);
        self.set_bits(value & mask);
    }

    pub(crate) fn clear_bits(&mut self, value: GpioBits) {
        if value == 0 {
            return;
        };
        self.gpio_registers.write_clr_bits(value);
        self.delay();
    }

    pub(crate) fn set_bits(&mut self, value: GpioBits) {
        if value == 0 {
            return;
        };
        self.gpio_registers.write_set_bits(value);
        self.delay();
    }

    /// Gives slow panels time to see the change.
    #[inline]
    fn delay(&mut self) {
        if self.gpio_slowdown < 0 {
            store_barrier();
            return;
        }
        // Writing nothing to a register takes as long as a real write. With the pins above 31 in use, clearing
        // writes two registers, which would double the delay, so only one is written.
        for _ in 0..self.gpio_slowdown {
            self.gpio_registers.write_nothing();
        }
    }

    pub(crate) fn send_pulse(&mut self, bitplane: usize) {
        let Gpio {
            time_registers,
            pwm_registers,
            pin_pulser,
            ..
        } = self;
        match pin_pulser {
            PinPulser::Hardware(pulser) => {
                pulser.send_pulse(bitplane, pwm_registers, time_registers);
            }
            PinPulser::Timer(pulser) => {
                let (pins, duration_ns) = pulser.pulse(bitplane);
                self.clear_bits(pins);
                sleep_nanos(duration_ns, self.min_sys_sleep_time);
                self.set_bits(pins);
            }
        }
    }

    pub(crate) fn wait_pulse_finished(&mut self) {
        let Gpio {
            time_registers,
            pwm_registers,
            pin_pulser,
            min_sys_sleep_time,
            ..
        } = self;
        // The timer based pulses are already finished when `send_pulse` returns.
        if let PinPulser::Hardware(pulser) = pin_pulser {
            pulser.wait_pulse_finished(time_registers, pwm_registers, *min_sys_sleep_time);
        }
    }

    pub(crate) fn request_enabled_inputs(&mut self, mut enabled_bits: GpioBits) -> GpioBits {
        // Remove the bits that are already used otherwise.
        enabled_bits &= !(self.output_bits | self.input_bits | self.reserved_bits);

        (0..=MAX_AVAILABLE_BIT).for_each(|b| {
            if (enabled_bits & gpio_bits!(b)) != 0 {
                self.gpio_registers.select_function(b, GPIOFunction::Input);
            }
        });
        self.input_bits |= enabled_bits;
        enabled_bits
    }

    /// Turns the requested pins that aren't used otherwise into outputs and returns them.
    pub(crate) fn request_outputs(&mut self, mut requested_bits: GpioBits) -> GpioBits {
        requested_bits &= !(self.output_bits | self.input_bits | self.reserved_bits);
        (0..=MAX_AVAILABLE_BIT).for_each(|b| {
            if requested_bits & gpio_bits!(b) != 0 {
                self.gpio_registers.select_function(b, GPIOFunction::Output);
            }
        });
        self.gpio_registers.use_bits(requested_bits);
        self.output_bits |= requested_bits;
        requested_bits
    }

    pub(crate) fn read(&mut self) -> GpioBits {
        self.gpio_registers.read_pin_levels() & self.input_bits
    }
}

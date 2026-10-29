use std::time::Duration;

use crate::{
    gpio_bits,
    registers::{ClkRegisters, GPIOFunction, GPIORegisters, PWMRegisters, TimeRegisters},
    utils::sleep_at_most,
};

const PWM_BASE_TIME_NS: u32 = 2;

/// The pin and function that connect the output enable to PWM0, if it is on such a pin.
pub(crate) fn pwm_pin_function(pins: u32) -> Option<(u8, GPIOFunction)> {
    if pins == gpio_bits!(18) {
        Some((18, GPIOFunction::Alt5))
    } else if pins == gpio_bits!(12) {
        Some((12, GPIOFunction::Alt0))
    } else {
        None
    }
}

pub(crate) enum PinPulser {
    /// The output enable is driven by the PWM peripheral, which times the pulses in hardware.
    Hardware(HardwarePinPulser),
    /// Fallback for output enable pins that are not connected to the PWM peripheral. The pulses are timed in
    /// software, which is less accurate and keeps the CPU busy for the whole on-time.
    Software {
        /// The output enable pins.
        pins: u32,
        /// Pulse duration for each bit plane.
        pulse_durations: Vec<Duration>,
    },
}

impl PinPulser {
    pub(crate) fn new(
        pins: u32,
        bitplane_timings_ns: &[u32],
        min_sys_sleep_time: Duration,
        pwm_registers: PWMRegisters,
        gpio_registers: &mut GPIORegisters,
        clk_registers: &mut ClkRegisters,
    ) -> Self {
        if let Some((pin, function)) = pwm_pin_function(pins) {
            gpio_registers.select_function(pin, function);
            Self::Hardware(HardwarePinPulser::new(
                bitplane_timings_ns,
                min_sys_sleep_time,
                pwm_registers,
                clk_registers,
            ))
        } else {
            // The output enable is active low, so keep the panel dark until the first pulse.
            gpio_registers.write_set_bits(pins);
            let pulse_durations = bitplane_timings_ns
                .iter()
                .map(|&timing| Duration::from_nanos(u64::from(timing)))
                .collect();
            Self::Software {
                pins,
                pulse_durations,
            }
        }
    }
}

pub(crate) struct HardwarePinPulser {
    pwm_registers: PWMRegisters,
    /// Hints how long to sleep for each bit plane.
    sleep_hints_us: Vec<u64>,
    /// Pulse period for each bit plane.
    pulse_periods: Vec<u32>,
    /// When the current pulse is expected to end.
    pulse_end_hint_us: Option<u64>,
    /// Shorter waits are busy waited and longer ones wake up this early.
    min_sys_sleep_time: Duration,
}

impl HardwarePinPulser {
    fn new(
        bitplane_timings_ns: &[u32],
        min_sys_sleep_time: Duration,
        mut pwm_registers: PWMRegisters,
        clk_registers: &mut ClkRegisters,
    ) -> Self {
        let sleep_hints_us = bitplane_timings_ns
            .iter()
            .map(|&t| u64::from(t / 1000))
            .collect();

        let time_base = bitplane_timings_ns[0];

        pwm_registers.reset_pwm();
        clk_registers.init_pwm_divider((time_base / 2) / PWM_BASE_TIME_NS);
        let pulse_periods = bitplane_timings_ns
            .iter()
            .map(|timing| 2 * timing / time_base)
            .collect();

        Self {
            pwm_registers,
            sleep_hints_us,
            pulse_periods,
            pulse_end_hint_us: None,
            min_sys_sleep_time,
        }
    }

    pub(crate) fn send_pulse(&mut self, bitplane: usize, time_registers: &TimeRegisters) {
        let pwm_registers = &mut self.pwm_registers;
        if self.pulse_periods[bitplane] < 16 {
            pwm_registers.set_pwm_pulse_period(self.pulse_periods[bitplane]);
            pwm_registers.push_fifo(self.pulse_periods[bitplane]);
        } else {
            // Keep the actual range as short as possible, as we have to wait for one full period of these in
            // the zero phase. The hardware can't deal with values < 2, so only do this when we have have
            // enough of these.
            let period_fraction = self.pulse_periods[bitplane] / 8;
            pwm_registers.set_pwm_pulse_period(period_fraction);
            for _ in 0..8 {
                pwm_registers.push_fifo(period_fraction);
            }
        }

        // We need one sentinel value at the end to have it go back to default state (otherwise it just
        // repeats the last value, so will be constantly 'on').
        pwm_registers.push_fifo(0);

        // For some reason, we need a second empty sentinel in the FIFO, otherwise our way to detect the end
        // of the pulse, which relies on "is the queue empty" does not work. It is not entirely clear why that
        // is from the data sheet, but probably there is some buffering register in which data elements are
        // kept after the FIFO is emptied.
        pwm_registers.push_fifo(0);

        self.pulse_end_hint_us = Some(time_registers.get_time() + self.sleep_hints_us[bitplane]);
        pwm_registers.enable_pwm();
    }

    pub(crate) fn wait_pulse_finished(&mut self, time_registers: &TimeRegisters) {
        let Some(pulse_end_hint_us) = self.pulse_end_hint_us.take() else {
            return;
        };

        let remaining_time_us = pulse_end_hint_us.saturating_sub(time_registers.get_time());
        sleep_at_most(
            Duration::from_micros(remaining_time_us),
            self.min_sys_sleep_time,
        );

        while !self.pwm_registers.fifo_empty() {
            // busy wait until done.
            std::hint::spin_loop();
        }

        self.pwm_registers.reset_pwm();
    }
}

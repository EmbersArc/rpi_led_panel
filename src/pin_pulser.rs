use crate::{
    gpio_bits,
    registers::{ClkRegisters, GPIOFunction, GPIORegisters, PWMRegisters, TimeRegisters},
};

const PWM_BASE_TIME_NS: u32 = 2;

struct Pulse {
    start_time: u64,
    sleep_hint_us: u32,
}

pub(crate) struct PinPulser {
    /// Hints how long to sleep.
    sleep_hints_us: Vec<u32>,
    /// Pulse period for each bit plane.
    pulse_periods: Vec<u32>,
    /// The current pulse.
    current_pulse: Option<Pulse>,
}

impl PinPulser {
    pub(crate) fn new(
        pins: u32,
        bitplane_timings_ns: &[u32],
        pwm_registers: &mut PWMRegisters,
        gpio_registers: &mut GPIORegisters,
        clk_registers: &mut ClkRegisters,
    ) -> Self {
        let sleep_hints_us = bitplane_timings_ns.iter().map(|t| t / 1000).collect();

        let time_base = bitplane_timings_ns[0];

        if pins == gpio_bits!(18) {
            // Set GPIO 18 to PWM0 mode
            gpio_registers.select_function(18, GPIOFunction::Alt5);
        } else if pins == gpio_bits!(12) {
            // Set GPIO 12 to PWM0 mode
            gpio_registers.select_function(12, GPIOFunction::Alt0);
        } else {
            unreachable!()
        }

        pwm_registers.reset_pwm();
        clk_registers.init_pwm_divider((time_base / 2) / PWM_BASE_TIME_NS);
        let pulse_periods = bitplane_timings_ns
            .iter()
            .map(|timing| 2 * timing / time_base)
            .collect();

        Self {
            sleep_hints_us,
            pulse_periods,
            current_pulse: None,
        }
    }

    pub(crate) fn send_pulse(
        &mut self,
        bitplane: usize,
        pwm_registers: &mut PWMRegisters,
        time_registers: &mut TimeRegisters,
    ) {
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

        self.current_pulse = Some(Pulse {
            start_time: time_registers.get_time(),
            sleep_hint_us: self.sleep_hints_us[bitplane],
        });
        pwm_registers.enable_pwm();
    }

    pub(crate) fn wait_pulse_finished(
        &mut self,
        time_registers: &mut TimeRegisters,
        pwm_registers: &mut PWMRegisters,
    ) {
        let pulse = match self.current_pulse.take() {
            Some(t) => t,
            None => {
                return;
            }
        };

        let already_elapsed_us = time_registers.get_time() - pulse.start_time;
        let remaining_time_us = (pulse.sleep_hint_us as u64).saturating_sub(already_elapsed_us);
        time_registers.sleep_at_most(remaining_time_us);

        while !pwm_registers.fifo_empty() {
            // busy wait until done.
            std::thread::yield_now();
        }

        pwm_registers.reset_pwm();
    }
}

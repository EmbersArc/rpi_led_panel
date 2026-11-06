//! The hardware that refreshes the panels: the GPIO peripherals of the Raspberry Pi 1 to 4, or the RP1 I/O
//! controller of the Raspberry Pi 5.

use crate::{
    RGBMatrixConfig,
    canvas::Canvas,
    chip::PiChip,
    config::BIT_PLANES,
    gpio::Gpio,
    hardware_mapping::{GpioBits, HardwareMapping, from_low_pins, low_pins},
    init_sequence::PanelInit,
    rgb_matrix::MatrixCreationError,
    row_address_setter::RowAddressSetter,
    rp1::is_pi5_model,
    rp1_pio::Rp1Pio,
    rp1_rio::Rp1Rio,
    utils::drop_privs,
};

// Only one backend exists at a time, so the size difference doesn't matter.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Backend {
    Bcm(BcmBackend),
    Rp1Rio(Rp1Rio),
    Rp1Pio(Rp1Pio),
}

impl Backend {
    /// Sets up the hardware and drops the privileges if configured.
    pub(crate) fn new(chip: PiChip, config: &RGBMatrixConfig) -> Result<Self, MatrixCreationError> {
        let backend = if chip.uses_rp1() || is_pi5_model() {
            if config.rp1_pio {
                Self::Rp1Pio(Rp1Pio::new(config)?)
            } else {
                Self::Rp1Rio(Rp1Rio::new(config)?)
            }
        } else {
            Self::Bcm(BcmBackend::new(chip, config)?)
        };

        // Drop privileges here as we no longer need root. Without root, e.g. with /dev/gpiomem0 on the
        // Raspberry Pi 5, there is nothing to drop.
        let is_root = unsafe { libc::geteuid() } == 0;
        if !config.no_drop_privs && is_root {
            drop_privs(&config.drop_priv_user, &config.drop_priv_group).map_err(|e| {
                MatrixCreationError::GpioError(
                    crate::gpio::GpioInitializationError::FailedPrivilegeDrop(e),
                )
            })?;
        }
        Ok(backend)
    }

    pub(crate) fn init_panels(&mut self, init: &PanelInit) -> Result<(), MatrixCreationError> {
        match self {
            Self::Bcm(backend) => backend.init_panels(init),
            Self::Rp1Rio(rio) => rio.init_panels(init),
            Self::Rp1Pio(pio) => pio.init_panels(init)?,
        }
        Ok(())
    }

    /// Shows one frame. Only the PIO backend can fail, if the kernel driver rejects the transfer.
    pub(crate) fn dump(
        &mut self,
        canvas: &Canvas,
        pwm_low_bit: usize,
    ) -> Result<(), MatrixCreationError> {
        match self {
            Self::Bcm(backend) => backend.dump(canvas, pwm_low_bit),
            Self::Rp1Rio(rio) => rio.dump(canvas, pwm_low_bit),
            Self::Rp1Pio(pio) => return pio.dump(canvas, pwm_low_bit),
        }
        Ok(())
    }

    /// Turns the requested pins into inputs, as far as they are available. GPIO inputs and outputs are not
    /// supported on the Raspberry Pi 5.
    pub(crate) fn request_inputs(&mut self, bits: u32) -> u32 {
        match self {
            Self::Bcm(backend) => {
                low_pins(backend.gpio.request_enabled_inputs(from_low_pins(bits)))
            }
            Self::Rp1Rio(_) | Self::Rp1Pio(_) => 0,
        }
    }

    /// Turns the requested pins into outputs that the user controls, as far as they are available.
    pub(crate) fn request_outputs(&mut self, bits: u32) -> u32 {
        match self {
            Self::Bcm(backend) => low_pins(backend.gpio.request_outputs(from_low_pins(bits))),
            Self::Rp1Rio(_) | Self::Rp1Pio(_) => 0,
        }
    }

    pub(crate) fn read_inputs(&mut self) -> u32 {
        match self {
            Self::Bcm(backend) => low_pins(backend.gpio.read()),
            Self::Rp1Rio(_) | Self::Rp1Pio(_) => 0,
        }
    }
}

/// Drives the panels with the GPIO peripherals of the BCM283x and BCM2711.
pub(crate) struct BcmBackend {
    gpio: Gpio,
    row_setter: Box<dyn RowAddressSetter>,
    hardware_mapping: HardwareMapping,
    /// Mask of bits while clocking in.
    color_clk_mask: GpioBits,
}

impl BcmBackend {
    fn new(chip: PiChip, config: &RGBMatrixConfig) -> Result<Self, MatrixCreationError> {
        let row_setter = config.row_setter.create(config);
        let gpio = Gpio::new(chip, config, row_setter.as_ref())?;
        Ok(Self {
            gpio,
            row_setter,
            hardware_mapping: config.hardware_mapping,
            color_clk_mask: config
                .hardware_mapping
                .color_clock_mask(config.parallel),
        })
    }

    fn init_panels(&mut self, init: &PanelInit) {
        let hm = self.hardware_mapping;
        self.gpio.clear_bits(hm.clock | hm.strobe);
        for register in &init.registers {
            for &word in register {
                self.gpio.write_masked_bits(word, init.mask);
                self.gpio.set_bits(hm.clock);
                self.gpio.clear_bits(hm.clock);
            }
            self.gpio.clear_bits(hm.strobe);
        }
    }

    fn dump(&mut self, canvas: &Canvas, pwm_low_bit: usize) {
        let Self {
            gpio,
            row_setter,
            hardware_mapping,
            color_clk_mask,
        } = self;
        let start_bit = canvas.start_bit(pwm_low_bit);

        for row_loop in 0..canvas.double_rows() {
            let d_row = canvas.display_row(row_loop);

            // Rows can't be switched very quickly without ghosting, so we do the
            // full PWM of one row before switching rows.
            for b in start_bit..BIT_PLANES {
                // While the output enable is still on, we can already clock in the next data.
                let row = canvas.row_at(d_row, 0, b);
                row.iter().for_each(|col| {
                    gpio.write_masked_bits(*col, *color_clk_mask); // col + reset clock
                    gpio.set_bits(hardware_mapping.clock); // Rising edge: clock color in.
                });

                gpio.clear_bits(*color_clk_mask); // clock back to normal.

                // OE of the previous row-data must be finished before strobe.
                gpio.wait_pulse_finished();

                // Setting address and strobing needs to happen in dark time.
                row_setter.set_row_address(gpio, d_row);

                // Strobe in the previously clocked in row.
                gpio.set_bits(hardware_mapping.strobe);
                gpio.clear_bits(hardware_mapping.strobe);

                // Now switch on for the sleep time necessary for that bit-plane.
                gpio.send_pulse(b);
            }
        }
    }
}

impl Drop for BcmBackend {
    /// Like [`Rp1Rio`], leaves the LEDs off and the pins as inputs, except the output enable, see
    /// [`Gpio::release`].
    fn drop(&mut self) {
        self.gpio.release();
    }
}

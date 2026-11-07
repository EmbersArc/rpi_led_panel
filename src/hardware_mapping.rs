use crate::error::InvalidVariantError;

use std::{ops::BitOr, str::FromStr};

use crate::gpio_bits;

/// A set of GPIO pins, one bit per pin. With the `wide-gpio` feature, the pins above 31 of the compute module can be
/// used as well.
#[cfg(not(feature = "wide-gpio"))]
pub(crate) type GpioBits = u32;
#[cfg(feature = "wide-gpio")]
pub(crate) type GpioBits = u64;

/// The pins below 32, e.g. for the 32 bit GPIO registers.
#[allow(clippy::unnecessary_cast)]
pub(crate) const fn low_pins(bits: GpioBits) -> u32 {
    bits as u32
}

/// The pins below 32 as [`GpioBits`].
#[allow(clippy::useless_conversion)]
pub(crate) fn from_low_pins(bits: u32) -> GpioBits {
    GpioBits::from(bits)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ColorBits {
    pub(crate) r1: GpioBits,
    pub(crate) g1: GpioBits,
    pub(crate) b1: GpioBits,
    pub(crate) r2: GpioBits,
    pub(crate) g2: GpioBits,
    pub(crate) b2: GpioBits,
}

impl ColorBits {
    pub(crate) const fn unused() -> Self {
        Self {
            r1: 0,
            g1: 0,
            b1: 0,
            r2: 0,
            g2: 0,
            b2: 0,
        }
    }

    pub(crate) fn used_bits(&self) -> GpioBits {
        self.r1 | self.r2 | self.g1 | self.g2 | self.b1 | self.b2
    }

    fn red_bits(&self) -> GpioBits {
        self.r1 | self.r2
    }

    fn green_bits(&self) -> GpioBits {
        self.g1 | self.g2
    }

    fn blue_bits(&self) -> GpioBits {
        self.b1 | self.b2
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Panels {
    pub(crate) color_bits: [ColorBits; 6],
}

impl Panels {
    pub(crate) fn used_bits(&self) -> GpioBits {
        self.red_bits() | self.green_bits() | self.blue_bits()
    }

    pub(crate) fn red_bits(&self) -> GpioBits {
        self.color_bits
            .iter()
            .map(ColorBits::red_bits)
            .fold(0, BitOr::bitor)
    }

    pub(crate) fn green_bits(&self) -> GpioBits {
        self.color_bits
            .iter()
            .map(ColorBits::green_bits)
            .fold(0, BitOr::bitor)
    }

    pub(crate) fn blue_bits(&self) -> GpioBits {
        self.color_bits
            .iter()
            .map(ColorBits::blue_bits)
            .fold(0, BitOr::bitor)
    }
}

/// The GPIO pins that the panels are connected to. There is a constructor for each supported wiring, which can
/// also be selected by name when parsing, e.g. `"AdafruitHat"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HardwareMapping {
    pub(crate) output_enable: GpioBits,
    pub(crate) clock: GpioBits,
    pub(crate) strobe: GpioBits,

    pub(crate) a: GpioBits,
    pub(crate) b: GpioBits,
    pub(crate) c: GpioBits,
    pub(crate) d: GpioBits,
    pub(crate) e: GpioBits,

    pub(crate) panels: Panels,
}

impl FromStr for HardwareMapping {
    type Err = InvalidVariantError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        #[derive(strum::EnumString, strum::VariantNames)]
        #[strum(
            parse_err_fn = InvalidVariantError::new::<Self>,
            parse_err_ty = InvalidVariantError
        )]
        enum HardwareMappingNames {
            AdafruitHat,
            AdafruitHatPwm,
            Regular,
            RegularPi1,
            Classic,
            ClassicPi1,
            ComputeModule,
        }
        let mapping = HardwareMappingNames::from_str(s)?;
        match mapping {
            HardwareMappingNames::AdafruitHat => Ok(Self::adafruit_hat()),
            HardwareMappingNames::AdafruitHatPwm => Ok(Self::adafruit_hat_pwm()),
            HardwareMappingNames::Regular => Ok(Self::regular()),
            HardwareMappingNames::RegularPi1 => Ok(Self::regular_pi1()),
            HardwareMappingNames::Classic => Ok(Self::classic()),
            HardwareMappingNames::ClassicPi1 => Ok(Self::classic_pi1()),
            HardwareMappingNames::ComputeModule => Ok(Self::compute_module()),
        }
    }
}

impl HardwareMapping {
    /// All bits used for output when driving `parallel` chains, except for the row address.
    pub(crate) fn used_bits(&self, parallel: usize) -> GpioBits {
        self.output_enable | self.strobe | self.color_clock_mask(parallel)
    }

    /// Mask of bits while clocking in.
    pub(crate) fn color_clock_mask(&self, parallel: usize) -> GpioBits {
        self.panels
            .color_bits
            .iter()
            .take(parallel)
            .map(ColorBits::used_bits)
            .fold(self.clock, BitOr::bitor)
    }

    pub(crate) fn max_parallel_chains(&self) -> usize {
        self.panels
            .color_bits
            .iter()
            .filter(|p| p.used_bits() > 0)
            .count()
    }
}

impl HardwareMapping {
    /// The regular hardware mapping used by the adapter PCBs.
    #[must_use]
    pub const fn regular() -> Self {
        Self {
            output_enable: gpio_bits!(18),
            clock: gpio_bits!(17),
            strobe: gpio_bits!(4),

            a: gpio_bits!(22),
            b: gpio_bits!(23),
            c: gpio_bits!(24),
            d: gpio_bits!(25),
            e: gpio_bits!(15), // RxD kept free unless 1:64

            panels: Panels {
                color_bits: [
                    /* Parallel chain 0, RGB for both sub-panels */
                    ColorBits {
                        r1: gpio_bits!(11), // masks: SPI0_SCLK
                        g1: gpio_bits!(27), // Not on RPi1, Rev1; use "regular-pi1" instead
                        b1: gpio_bits!(7),  // masks: SPI0_CE1
                        r2: gpio_bits!(8),  // masks: SPI0_CE0
                        g2: gpio_bits!(9),  // masks: SPI0_MISO
                        b2: gpio_bits!(10), // masks: SPI0_MOSI
                    },
                    /* All the following are only available with 40 GPIO pins, on A+/B+/Pi2,3 */
                    /* Chain 1 */
                    ColorBits {
                        r1: gpio_bits!(12),
                        g1: gpio_bits!(5),
                        b1: gpio_bits!(6),
                        r2: gpio_bits!(19),
                        g2: gpio_bits!(13),
                        b2: gpio_bits!(20),
                    },
                    /* Chain 2 */
                    ColorBits {
                        r1: gpio_bits!(14), // masks TxD when parallel=3
                        g1: gpio_bits!(2),  // masks SDA when parallel=3
                        b1: gpio_bits!(3),  // masks SCL when parallel=3
                        r2: gpio_bits!(26),
                        g2: gpio_bits!(16),
                        b2: gpio_bits!(21),
                    },
                    ColorBits::unused(),
                    ColorBits::unused(),
                    ColorBits::unused(),
                ],
            },
        }
    }

    /// An unmodified Adafruit HAT.
    #[must_use]
    pub const fn adafruit_hat() -> Self {
        Self {
            output_enable: gpio_bits!(4),
            clock: gpio_bits!(17),
            strobe: gpio_bits!(21),

            a: gpio_bits!(22),
            b: gpio_bits!(26),
            c: gpio_bits!(27),
            d: gpio_bits!(20),
            e: gpio_bits!(24), // Needs manual wiring

            panels: Panels {
                color_bits: [
                    ColorBits {
                        r1: gpio_bits!(5),
                        g1: gpio_bits!(13),
                        b1: gpio_bits!(6),
                        r2: gpio_bits!(12),
                        g2: gpio_bits!(16),
                        b2: gpio_bits!(23),
                    },
                    ColorBits::unused(),
                    ColorBits::unused(),
                    ColorBits::unused(),
                    ColorBits::unused(),
                    ColorBits::unused(),
                ],
            },
        }
    }

    /// An Adafruit HAT with the PWM modification.
    #[must_use]
    pub const fn adafruit_hat_pwm() -> Self {
        Self {
            output_enable: gpio_bits!(18),
            ..Self::adafruit_hat()
        }
    }

    /// The regular pin-out, but for Raspberry Pi1. The very first Pi1 Rev1 uses the same pin for GPIO-21 as
    /// later Pis use GPIO-27. Make it work for both.
    #[must_use]
    pub const fn regular_pi1() -> Self {
        Self {
            output_enable: gpio_bits!(18),
            clock: gpio_bits!(17),
            strobe: gpio_bits!(4),

            a: gpio_bits!(22),
            b: gpio_bits!(23),
            c: gpio_bits!(24),
            d: gpio_bits!(25),
            e: gpio_bits!(15), // RxD kept free unless 1:64

            /* Parallel chain 0, RGB for both sub-panels */
            panels: Panels {
                color_bits: [
                    ColorBits {
                        r1: gpio_bits!(11), // masks: SPI0_SCLK
                        // On Pi1 Rev1, the pin other Pis have GPIO27, these have GPIO21. So make this work for
                        // both Rev1 and Rev2.
                        g1: gpio_bits!(21, 27),
                        b1: gpio_bits!(7),  // masks: SPI0_CE1
                        r2: gpio_bits!(8),  // masks: SPI0_CE0
                        g2: gpio_bits!(9),  // masks: SPI0_MISO
                        b2: gpio_bits!(10), // masks: SPI0_MOSI
                    },
                    // No more chains - there are not enough GPIO
                    ColorBits::unused(),
                    ColorBits::unused(),
                    ColorBits::unused(),
                    ColorBits::unused(),
                    ColorBits::unused(),
                ],
            },
        }
    }

    /// Early forms of this library had this as default mapping, mostly derived from the 26 GPIO-header
    /// version so that it also can work on 40 Pin GPIO headers with more parallel chains. Not used anymore.
    #[must_use]
    pub const fn classic() -> Self {
        Self {
            output_enable: gpio_bits!(27), // Not available on RPi1, Rev 1
            clock: gpio_bits!(11),
            strobe: gpio_bits!(4),

            a: gpio_bits!(7),
            b: gpio_bits!(8),
            c: gpio_bits!(9),
            d: gpio_bits!(10),
            e: 0,

            panels: Panels {
                color_bits: [
                    ColorBits {
                        r1: gpio_bits!(17),
                        g1: gpio_bits!(18),
                        b1: gpio_bits!(22),
                        r2: gpio_bits!(23),
                        g2: gpio_bits!(24),
                        b2: gpio_bits!(25),
                    },
                    ColorBits {
                        r1: gpio_bits!(12),
                        g1: gpio_bits!(5),
                        b1: gpio_bits!(6),
                        r2: gpio_bits!(19),
                        g2: gpio_bits!(13),
                        b2: gpio_bits!(20),
                    },
                    ColorBits {
                        r1: gpio_bits!(14), // masks TxD if parallel = 3
                        g1: gpio_bits!(2),  // masks SDA if parallel = 3
                        b1: gpio_bits!(3),  // masks SCL if parallel = 3
                        r2: gpio_bits!(15),
                        g2: gpio_bits!(26),
                        b2: gpio_bits!(21),
                    },
                    ColorBits::unused(),
                    ColorBits::unused(),
                    ColorBits::unused(),
                ],
            },
        }
    }

    /// Classic pin-out for Rev-A Raspberry Pi.
    #[must_use]
    pub const fn classic_pi1() -> Self {
        Self {
            // The Revision-1 and Revision-2 boards have different GPIO mapping on the P1-3 and P1-5. So we
            // use both interpretations. To keep the I2C pins free, we avoid these in later mappings.
            output_enable: gpio_bits!(0, 2),
            clock: gpio_bits!(1, 3),
            strobe: gpio_bits!(4),

            a: gpio_bits!(7),
            b: gpio_bits!(8),
            c: gpio_bits!(9),
            d: gpio_bits!(10),
            e: 0,

            panels: Panels {
                color_bits: [
                    ColorBits {
                        r1: gpio_bits!(17),
                        g1: gpio_bits!(18),
                        b1: gpio_bits!(22),
                        r2: gpio_bits!(23),
                        g2: gpio_bits!(24),
                        b2: gpio_bits!(25),
                    },
                    ColorBits::unused(),
                    ColorBits::unused(),
                    ColorBits::unused(),
                    ColorBits::unused(),
                    ColorBits::unused(),
                ],
            },
        }
    }

    /// Pin-out for the official Compute Module I/O development board. No pin is left free when using 6 parallel
    /// chains.
    ///
    /// Chains 3 to 5 use the GPIO pins above 31, which needs the `wide-gpio` feature. Without it, up to three
    /// parallel chains are supported.
    #[must_use]
    pub const fn compute_module() -> Self {
        Self {
            output_enable: gpio_bits!(18),
            clock: gpio_bits!(16),
            strobe: gpio_bits!(17),

            a: gpio_bits!(2),
            b: gpio_bits!(3),
            c: gpio_bits!(4),
            d: gpio_bits!(5),
            e: gpio_bits!(6),

            panels: Panels {
                color_bits: [
                    ColorBits {
                        r1: gpio_bits!(7),
                        g1: gpio_bits!(8),
                        b1: gpio_bits!(9),
                        r2: gpio_bits!(10),
                        g2: gpio_bits!(11),
                        b2: gpio_bits!(12),
                    },
                    ColorBits {
                        r1: gpio_bits!(13),
                        g1: gpio_bits!(14),
                        b1: gpio_bits!(15),
                        r2: gpio_bits!(19),
                        g2: gpio_bits!(20),
                        b2: gpio_bits!(21),
                    },
                    ColorBits {
                        r1: gpio_bits!(22),
                        g1: gpio_bits!(23),
                        b1: gpio_bits!(24),
                        r2: gpio_bits!(25),
                        g2: gpio_bits!(26),
                        b2: gpio_bits!(27),
                    },
                    Self::COMPUTE_MODULE_WIDE_CHAINS[0],
                    Self::COMPUTE_MODULE_WIDE_CHAINS[1],
                    Self::COMPUTE_MODULE_WIDE_CHAINS[2],
                ],
            },
        }
    }

    /// Chains 3 to 5 of the compute module, on the pins above 31.
    #[cfg(feature = "wide-gpio")]
    const COMPUTE_MODULE_WIDE_CHAINS: [ColorBits; 3] = [
        ColorBits {
            r1: gpio_bits!(28),
            g1: gpio_bits!(29),
            b1: gpio_bits!(30),
            r2: gpio_bits!(31),
            g2: gpio_bits!(32),
            b2: gpio_bits!(33),
        },
        ColorBits {
            r1: gpio_bits!(34),
            g1: gpio_bits!(35),
            b1: gpio_bits!(36),
            r2: gpio_bits!(37),
            g2: gpio_bits!(38),
            b2: gpio_bits!(39),
        },
        ColorBits {
            r1: gpio_bits!(40),
            g1: gpio_bits!(41),
            b1: gpio_bits!(42),
            r2: gpio_bits!(43),
            g2: gpio_bits!(44),
            b2: gpio_bits!(45),
        },
    ];
    #[cfg(not(feature = "wide-gpio"))]
    const COMPUTE_MODULE_WIDE_CHAINS: [ColorBits; 3] = [ColorBits::unused(); 3];
}

#[cfg(test)]
mod tests {
    use super::HardwareMapping;
    use crate::gpio_bits;

    #[test]
    fn used_bits_only_include_parallel_chains() {
        let regular = HardwareMapping::regular();
        // Chain 2 uses the I2C and UART pins, which must stay free for a single chain.
        let i2c_uart = gpio_bits!(2, 3, 14);
        assert_eq!(regular.used_bits(1) & i2c_uart, 0);
        assert_eq!(regular.used_bits(3) & i2c_uart, i2c_uart);
    }

    #[test]
    fn compute_module_has_six_chains_with_wide_gpio() {
        let chains = HardwareMapping::compute_module().max_parallel_chains();
        assert_eq!(chains, if cfg!(feature = "wide-gpio") { 6 } else { 3 });
    }

    #[test]
    fn regular_pi1_does_not_overlap_row_address() {
        let mapping = HardwareMapping::regular_pi1();
        assert_eq!(mapping.used_bits(1) & mapping.e, 0);
    }

    #[test]
    fn test_signals_use_distinct_pins() {
        for (name, h) in [
            ("Regular", HardwareMapping::regular()),
            ("RegularPi1", HardwareMapping::regular_pi1()),
            ("AdafruitHat", HardwareMapping::adafruit_hat()),
            ("AdafruitHatPwm", HardwareMapping::adafruit_hat_pwm()),
            ("Classic", HardwareMapping::classic()),
            ("ClassicPi1", HardwareMapping::classic_pi1()),
        ] {
            let mut signals = vec![h.output_enable, h.clock, h.strobe, h.a, h.b, h.c, h.d, h.e];
            for c in h.panels.color_bits {
                signals.extend([c.r1, c.g1, c.b1, c.r2, c.g2, c.b2]);
            }
            // A signal can drive several pins, but no pin may be driven by two signals.
            let mut used_bits = 0;
            for bits in signals {
                assert_eq!(used_bits & bits, 0, "{name} uses a pin twice");
                used_bits |= bits;
            }
        }
    }
}

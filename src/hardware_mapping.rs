use std::{error::Error, ops::BitOr, str::FromStr};

use crate::gpio_bits;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ColorBits {
    pub(crate) r1: u32,
    pub(crate) g1: u32,
    pub(crate) b1: u32,
    pub(crate) r2: u32,
    pub(crate) g2: u32,
    pub(crate) b2: u32,
}

impl ColorBits {
    pub const fn unused() -> Self {
        Self {
            r1: 0,
            g1: 0,
            b1: 0,
            r2: 0,
            g2: 0,
            b2: 0,
        }
    }

    pub(crate) fn used_bits(&self) -> u32 {
        self.r1 | self.r2 | self.g1 | self.g2 | self.b1 | self.b2
    }

    fn red_bits(&self) -> u32 {
        self.r1 | self.r2
    }

    fn green_bits(&self) -> u32 {
        self.g1 | self.g2
    }

    fn blue_bits(&self) -> u32 {
        self.b1 | self.b2
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Panels {
    pub(crate) color_bits: [ColorBits; 6],
}

impl Panels {
    pub(crate) fn used_bits(&self) -> u32 {
        self.red_bits() | self.green_bits() | self.blue_bits()
    }

    pub(crate) fn red_bits(&self) -> u32 {
        self.color_bits
            .iter()
            .map(ColorBits::red_bits)
            .fold(0, BitOr::bitor)
    }

    pub(crate) fn green_bits(&self) -> u32 {
        self.color_bits
            .iter()
            .map(ColorBits::green_bits)
            .fold(0, BitOr::bitor)
    }

    pub(crate) fn blue_bits(&self) -> u32 {
        self.color_bits
            .iter()
            .map(ColorBits::blue_bits)
            .fold(0, BitOr::bitor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HardwareMapping {
    pub(crate) output_enable: u32,
    pub(crate) clock: u32,
    pub(crate) strobe: u32,

    pub(crate) a: u32,
    pub(crate) b: u32,
    pub(crate) c: u32,
    pub(crate) d: u32,
    pub(crate) e: u32,

    pub(crate) panels: Panels,
}

impl FromStr for HardwareMapping {
    type Err = Box<dyn Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "AdafruitHat" => Ok(Self::adafruit_hat()),
            "AdafruitHatPwm" => Ok(Self::adafruit_hat_pwm()),
            "Regular" => Ok(Self::regular()),
            "RegularPi1" => Ok(Self::regular_pi1()),
            "Classic" => Ok(Self::classic()),
            "ClassicPi1" => Ok(Self::classic_pi1()),
            _ => Err(format!("'{s}' is not a valid GPIO mapping.").into()),
        }
    }
}

impl HardwareMapping {
    pub(crate) fn used_bits(&self) -> u32 {
        self.output_enable | self.clock | self.strobe | self.panels.used_bits()
    }

    /// Mask of bits while clocking in.
    pub(crate) fn get_color_clock_mask(&self, parallel: usize) -> u32 {
        let mut color_clk_mask: u32 = 0;
        (0..6).for_each(|panel| {
            if parallel > panel {
                color_clk_mask |= &self.panels.color_bits[panel].used_bits();
            }
        });
        color_clk_mask |= self.clock;
        color_clk_mask
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
                        r1: gpio_bits!(11), // masks: SPI0_SCKL
                        g1: gpio_bits!(27), // Not on RPi1, Rev1; use "regular-pi1" instead
                        b1: gpio_bits!(7),  // masks: SPI0_CE1
                        r2: gpio_bits!(8),  // masks: SPI0_CE0
                        g2: gpio_bits!(9),  // masks: SPI0_MISO
                        b2: gpio_bits!(10), // masks: SPI0_MOSI
                    },
                    /* All the following are only available with 40 GPIP pins, on A+/B+/Pi2,3 */
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
                        g1: gpio_bits!(2),  // masks SCL when parallel=3
                        b1: gpio_bits!(3),  // masks SDA when parallel=3
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

    // An unmodified Adafruit HAT
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

    // An Adafruit HAT with the PWM modification
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
                        // On Pi1 Rev1, the pin other Pis have GPIO27, these have GPIO21. So make this work for
                        // both Rev1 and Rev2.
                        r1: gpio_bits!(15, 27),
                        g1: gpio_bits!(21),
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
}

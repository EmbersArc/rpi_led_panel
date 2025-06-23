use std::{error::Error, fs::read_to_string, str::FromStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PiChip {
    /// Model 0 and 1
    BCM2708,
    /// Models 2 and 3
    BCM2709,
    /// Model 4
    BCM2711,
}

impl FromStr for PiChip {
    type Err = Box<dyn Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "BCM2708" | "BCM2835" => Ok(Self::BCM2708),
            "BCM2709" | "BCM2836" | "BCM2837" => Ok(Self::BCM2709),
            "BCM2711" => Ok(Self::BCM2711),
            _ => Err(format!("'{s}' is not a valid chip model.").into()),
        }
    }
}

impl PiChip {
    /// Try to automatically determine the model.
    #[must_use]
    pub fn determine() -> Option<Self> {
        let cpuinfo = read_to_string("/proc/cpuinfo").ok()?;

        let revision_number =
            if let Some(line) = cpuinfo.lines().find(|line| line.starts_with("Revision")) {
                // https://www.raspberrypi.org/documentation/hardware/raspberrypi/revision-codes/README.md
                let revision_str = line.split(' ').next_back()?;
                let old_style = revision_str.len() == 4;
                if old_style {
                    return Some(Self::BCM2708);
                }
                let revision = u32::from_str_radix(revision_str, 16).ok()?;
                // Bits: NOQuuuWuFMMMCCCCPPPPTTTTTTTTRRRR
                //                       ^^^^ processor model
                (revision >> 12) & 0b1111
            } else if let Some(line) = cpuinfo
                .lines()
                .find(|line| line.starts_with("CPU revision"))
            {
                let revision_str = line.split(' ').next_back()?;
                revision_str.parse().ok()?
            } else {
                return None;
            };
        match revision_number {
            // BCM2835
            0 => Some(Self::BCM2708),
            // BCM2836
            1 => Some(Self::BCM2709),
            // BCM2837
            2 => Some(Self::BCM2709),
            // BCM2711
            3 => Some(Self::BCM2711),
            _ => None,
        }
    }

    pub(crate) const fn num_cores(self) -> usize {
        match self {
            PiChip::BCM2708 => 1,
            PiChip::BCM2709 | PiChip::BCM2711 => 4,
        }
    }

    // All peripherals can be described by an offset from the Peripheral Base Address.
    pub(crate) const fn get_peripherals_base(self) -> u64 {
        match self {
            PiChip::BCM2708 => 0x2000_0000,
            PiChip::BCM2709 => 0x3F00_0000,
            PiChip::BCM2711 => 0xFE00_0000,
        }
    }

    pub(crate) fn gpio_slowdown(self) -> u32 {
        match self {
            PiChip::BCM2708 | PiChip::BCM2709 => 1,
            PiChip::BCM2711 => 3,
        }
    }
}

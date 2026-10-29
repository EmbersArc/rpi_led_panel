use std::fs::{read, read_to_string};

use crate::error::InvalidVariantError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, strum::EnumString, strum::VariantNames)]
#[strum(
    parse_err_fn = InvalidVariantError::new::<Self>,
    parse_err_ty = InvalidVariantError
)]
pub enum PiChip {
    /// Model 0 and 1
    #[strum(serialize = "BCM2708", serialize = "BCM2835")]
    BCM2708,
    /// Models 2 and 3
    #[strum(serialize = "BCM2709", serialize = "BCM2836", serialize = "BCM2837")]
    BCM2709,
    /// Model 4
    #[strum(serialize = "BCM2711")]
    BCM2711,
}

impl PiChip {
    /// Try to automatically determine the model.
    #[must_use]
    pub fn determine() -> Option<Self> {
        // Provided by the Raspberry Pi kernel.
        if let Ok(cpuinfo) = read_to_string("/proc/cpuinfo") {
            if let Some(chip) = Self::from_cpuinfo(&cpuinfo) {
                return Some(chip);
            }
        }
        // Provided by mainline kernels (Arch Linux ARM, Debian, Fedora, etc.).
        if let Ok(compatible) = read("/proc/device-tree/compatible") {
            if let Some(chip) = Self::from_device_tree_compatible(&compatible) {
                return Some(chip);
            }
        }
        None
    }

    fn from_cpuinfo(cpuinfo: &str) -> Option<Self> {
        let line = cpuinfo.lines().find(|line| line.starts_with("Revision"))?;
        // https://www.raspberrypi.com/documentation/computers/raspberry-pi.html#raspberry-pi-revision-codes
        let revision_str = line.split(' ').next_back()?;
        let revision = u32::from_str_radix(revision_str, 16).ok()?;
        // Bits: NOQuuuWuFMMMCCCCPPPPTTTTTTTTRRRR
        //               ^ new-style flag
        //                       ^^^^ processor model
        let new_style = (revision >> 23) & 1 == 1;
        if !new_style {
            // Old-style codes (e.g. `000e`, or `100000e` with the warranty bit set) are all BCM2835 boards.
            return Some(Self::BCM2708);
        }
        match (revision >> 12) & 0b1111 {
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

    /// The `compatible` property is a list of NUL-terminated strings, e.g.
    /// `raspberrypi,4-model-b\0brcm,bcm2711\0`. Older Raspberry Pi kernels use the
    /// `bcm2708`/`bcm2709`/`bcm2710` names.
    fn from_device_tree_compatible(compatible: &[u8]) -> Option<Self> {
        compatible.split(|&b| b == 0).find_map(|entry| match entry {
            b"brcm,bcm2835" | b"brcm,bcm2708" => Some(Self::BCM2708),
            b"brcm,bcm2836" | b"brcm,bcm2837" | b"brcm,bcm2709" | b"brcm,bcm2710" => {
                Some(Self::BCM2709)
            }
            b"brcm,bcm2711" => Some(Self::BCM2711),
            _ => None,
        })
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

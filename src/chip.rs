use std::fs::{read, read_to_string};

use crate::error::InvalidVariantError;

/// The chip of the Raspberry Pi, which determines the peripheral addresses and the default GPIO slowdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, strum::EnumString, strum::VariantNames)]
#[strum(
    parse_err_fn = InvalidVariantError::new::<Self>,
    parse_err_ty = InvalidVariantError
)]
#[non_exhaustive]
pub enum PiChip {
    /// Models 0 and 1
    #[strum(serialize = "BCM2708", serialize = "BCM2835")]
    BCM2708,
    /// Models 2 and 3
    #[strum(serialize = "BCM2709", serialize = "BCM2836", serialize = "BCM2837")]
    BCM2709,
    /// Model 4
    #[strum(serialize = "BCM2711")]
    BCM2711,
    /// Model 5, whose GPIO pins are connected to the RP1 I/O controller.
    #[strum(serialize = "BCM2712")]
    BCM2712,
}

/// Reads the hexadecimal revision code from the "Revision" line of /proc/cpuinfo.
fn revision_from_cpuinfo(cpuinfo: &str) -> Option<u32> {
    let line = cpuinfo.lines().find(|line| line.starts_with("Revision"))?;
    let (_, revision) = line.split_once(':')?;
    u32::from_str_radix(revision.trim(), 16).ok()
}

impl PiChip {
    /// Try to automatically determine the model.
    #[must_use]
    pub fn determine() -> Option<Self> {
        read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|cpuinfo| revision_from_cpuinfo(&cpuinfo))
            // Kernels that don't list the revision in /proc/cpuinfo still have it in the device tree.
            .or_else(|| {
                let bytes = read("/proc/device-tree/system/linux,revision").ok()?;
                Some(u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?))
            })
            .and_then(Self::from_revision)
            // Provided by mainline kernels (Arch Linux ARM, Debian, Fedora, etc.), which have neither.
            .or_else(|| {
                Self::from_device_tree_compatible(&read("/proc/device-tree/compatible").ok()?)
            })
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
            b"brcm,bcm2712" => Some(Self::BCM2712),
            _ => None,
        })
    }

    /// Get the chip from the revision code, see
    /// <https://www.raspberrypi.com/documentation/computers/raspberry-pi.html#raspberry-pi-revision-codes>.
    fn from_revision(revision: u32) -> Option<Self> {
        // Bits: NOQuuuWuFMMMCCCCPPPPTTTTTTTTRRRR
        //               ^ new style flag
        const NEW_STYLE_FLAG: u32 = 1 << 23;
        if revision & NEW_STYLE_FLAG == 0 {
            // Old style revision codes are only used by the original models.
            return Some(Self::BCM2708);
        }
        // Bits: NOQuuuWuFMMMCCCCPPPPTTTTTTTTRRRR
        //                       ^^^^ processor model
        match (revision >> 12) & 0b1111 {
            // BCM2835
            0 => Some(Self::BCM2708),
            // BCM2836
            1 => Some(Self::BCM2709),
            // BCM2837
            2 => Some(Self::BCM2709),
            // BCM2711
            3 => Some(Self::BCM2711),
            // BCM2712
            4 => Some(Self::BCM2712),
            _ => None,
        }
    }

    pub(crate) const fn num_cores(self) -> usize {
        match self {
            PiChip::BCM2708 => 1,
            PiChip::BCM2709 | PiChip::BCM2711 | PiChip::BCM2712 => 4,
        }
    }

    /// Whether the GPIO pins are driven through the RP1 I/O controller instead of the chip's own peripherals.
    pub(crate) const fn uses_rp1(self) -> bool {
        matches!(self, PiChip::BCM2712)
    }

    /// All peripherals can be described by an offset from the Peripheral Base Address. The GPIO pins of the
    /// BCM2712 are not among them.
    pub(crate) const fn get_peripherals_base(self) -> Option<u64> {
        match self {
            PiChip::BCM2708 => Some(0x2000_0000),
            PiChip::BCM2709 => Some(0x3F00_0000),
            PiChip::BCM2711 => Some(0xFE00_0000),
            PiChip::BCM2712 => None,
        }
    }

    pub(crate) fn gpio_slowdown(self) -> i32 {
        match self {
            PiChip::BCM2708 | PiChip::BCM2709 | PiChip::BCM2712 => 1,
            PiChip::BCM2711 => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PiChip, revision_from_cpuinfo};

    #[test]
    fn determines_chip_from_revision() {
        let chip = |cpuinfo| PiChip::from_revision(revision_from_cpuinfo(cpuinfo).unwrap());
        assert_eq!(chip("Revision\t: 000e"), Some(PiChip::BCM2708));
        // Old style revision of an overvolted Pi.
        assert_eq!(chip("Revision\t: 1000002"), Some(PiChip::BCM2708));
        assert_eq!(chip("Revision\t: 9000c1"), Some(PiChip::BCM2708)); // Zero W
        assert_eq!(chip("Revision\t: a01041"), Some(PiChip::BCM2709)); // 2B
        assert_eq!(chip("Revision\t: a02082"), Some(PiChip::BCM2709)); // 3B
        assert_eq!(chip("Revision\t: 902120"), Some(PiChip::BCM2709)); // Zero 2 W
        assert_eq!(chip("Revision\t: c03111"), Some(PiChip::BCM2711)); // 4B
        assert_eq!(chip("Revision\t: c04170"), Some(PiChip::BCM2712)); // 5B
        assert_eq!(chip("Revision\t: d04190"), Some(PiChip::BCM2712)); // 500
        assert_eq!(chip("Revision\t: c05170"), None); // Unknown processor
        assert_eq!(revision_from_cpuinfo("CPU revision\t: 3"), None);
    }

    #[test]
    fn determines_chip_from_device_tree_compatible() {
        let chip = PiChip::from_device_tree_compatible;
        assert_eq!(
            chip(b"raspberrypi,4-model-b\0brcm,bcm2711\0"),
            Some(PiChip::BCM2711)
        );
        assert_eq!(
            chip(b"raspberrypi,3-model-b\0brcm,bcm2837\0"),
            Some(PiChip::BCM2709)
        );
        assert_eq!(
            chip(b"raspberrypi,5-model-b\0brcm,bcm2712\0"),
            Some(PiChip::BCM2712)
        );
        assert_eq!(
            chip(b"raspberrypi,model-zero\0brcm,bcm2835\0"),
            Some(PiChip::BCM2708)
        );
        assert_eq!(chip(b"pine64,rock64\0"), None);
    }
}

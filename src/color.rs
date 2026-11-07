use std::sync::LazyLock;

use crate::config::BIT_PLANES;

/// The lookup table is the same for every canvas, so it is only computed once.
pub(crate) static CIE1931_LOOKUP: LazyLock<ColorLookup> = LazyLock::new(ColorLookup::new_cie1931);

// Do CIE1931 luminance correction and scale to output bitplanes
fn luminance_cie1931(c: u8, brightness: u8) -> u16 {
    let out_factor = ((1 << BIT_PLANES) - 1) as f32;
    let v = f32::from(c) * f32::from(brightness) / 255.0;
    (out_factor
        * (if v <= 8.0 {
            v / 902.3
        } else {
            ((v + 16.0) / 116.0).powi(3)
        }))
    .round() as u16
}

/// Scales the color linearly, without luminance correction.
fn direct_map(c: u8, brightness: u8) -> u16 {
    let c = u16::from(c) * u16::from(brightness) / 100;
    // Align with the top-most bit planes.
    c << (BIT_PLANES - 8)
}

pub(crate) struct ColorLookup {
    per_brightness: [[u16; 256]; 100],
}

impl ColorLookup {
    pub(crate) fn new_cie1931() -> Self {
        let mut per_brightness = [[0; 256]; 100];
        for c in 0..=255u8 {
            for b in 0..100u8 {
                per_brightness[usize::from(b)][usize::from(c)] = luminance_cie1931(c, b + 1);
            }
        }
        Self { per_brightness }
    }

    /// Maps the color to the bit plane values. The brightness is between 1 and 100.
    pub(crate) fn map_rgb(
        &self,
        brightness: u8,
        luminance_correct: bool,
        inverse: bool,
        [r, g, b]: [u8; 3],
    ) -> [u16; 3] {
        let mapped = if luminance_correct {
            let for_brightness = &self.per_brightness[brightness as usize - 1];
            [r, g, b].map(|c| for_brightness[c as usize])
        } else {
            [r, g, b].map(|c| direct_map(c, brightness))
        };
        if inverse { mapped.map(|c| !c) } else { mapped }
    }
}

#[cfg(test)]
mod tests {
    use super::CIE1931_LOOKUP;

    #[test]
    fn maps_colors() {
        let map = |luminance_correct, inverse, c| {
            CIE1931_LOOKUP.map_rgb(100, luminance_correct, inverse, [c, 0, 0])[0]
        };
        assert_eq!(map(true, false, 255), 2047);
        assert_eq!(map(true, false, 0), 0);
        // Rounded like upstream: 2047 * 1 / 902.3 * 100 / 255 = 0.89.
        assert_eq!(map(true, false, 1), 1);
        assert_eq!(map(false, false, 255), 255 << 3);
        assert_eq!(map(false, false, 128), 128 << 3);
        assert_eq!(map(true, true, 0), !0);
        assert_eq!(map(true, true, 255), !2047);
    }
}

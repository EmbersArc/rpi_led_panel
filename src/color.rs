use std::sync::LazyLock;

use crate::config::BIT_PLANES;

static CIE1931: LazyLock<ColorLookup> = LazyLock::new(ColorLookup::new_cie1931);

/// CIE1931 lightness L* (0-100) to relative luminance (0-1).
fn cie1931(l: f32) -> f32 {
    if l <= 8.0 {
        l / 902.3
    } else {
        ((l + 16.0) / 116.0).powi(3)
    }
}

// Luminance-correct `c`, then scale linearly by brightness so blended colors keep their ratios when dimmed.
fn luminance_cie1931(c: u8, brightness: u8) -> u16 {
    let out_factor = ((1 << BIT_PLANES) - 1) as f32;
    let color = cie1931(f32::from(c) * 100.0 / 255.0);
    let brightness = cie1931(f32::from(brightness));
    (out_factor * color * brightness) as u16
}

pub(crate) struct ColorLookup {
    per_brightness: [[u16; 256]; 100],
}

impl ColorLookup {
    /// The shared CIE1931 lookup table, computed on first use.
    pub(crate) fn cie1931() -> &'static Self {
        &CIE1931
    }

    fn new_cie1931() -> Self {
        let mut per_brightness = [[0; 256]; 100];
        for c in 0..=255u8 {
            for b in 0..100u8 {
                per_brightness[usize::from(b)][usize::from(c)] = luminance_cie1931(c, b + 1);
            }
        }
        Self { per_brightness }
    }

    pub(crate) fn lookup_rgb(&self, brightness: u8, r: u8, g: u8, b: u8) -> [u16; 3] {
        let for_brightness = &self.per_brightness[brightness as usize - 1];
        [
            for_brightness[r as usize],
            for_brightness[g as usize],
            for_brightness[b as usize],
        ]
    }
}

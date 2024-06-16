use crate::config::K_BIT_PLANES;

// Do CIE1931 luminance correction and scale to output bitplanes
fn luminance_cie1931(c: u8, brightness: u8) -> u16 {
    let out_factor = ((1 << K_BIT_PLANES) - 1) as f32;
    let v = f32::from(c) * f32::from(brightness) / 255.0;
    (out_factor
        * (if v <= 8.0 {
            v / 902.3
        } else {
            ((v + 16.0) / 116.0).powi(3)
        })) as u16
}

#[derive(Clone)]
pub(crate) struct ColorLookup {
    per_brightness: [[u16; 256]; 100],
}

impl ColorLookup {
    pub(crate) fn new_cie1931() -> Self {
        let mut per_brightness = [[0; 256]; 100];
        (0..=255u8).for_each(|c| {
            (0..100u8).for_each(|b| {
                per_brightness[b as usize][c as usize] = luminance_cie1931(c, b + 1);
            });
        });
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

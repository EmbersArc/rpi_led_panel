use std::io::Write;

use rpi_led_panel::embedded_graphics::{
    Drawable,
    image::{Image, ImageRawBE},
    pixelcolor::Rgb888,
    prelude::*,
};
use rpi_led_panel::{RGBMatrix, RGBMatrixConfig};

const IMAGE_DATA: &[u8] = include_bytes!("../assets/ferris_test_card.rgb");
const IMAGE_SIZE: i32 = 64;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let config: RGBMatrixConfig = argh::from_env();
    let (mut matrix, mut canvas) = RGBMatrix::new(config).expect("Matrix initialization failed");
    let rows = canvas.height() as i32;
    let cols = canvas.width() as i32;

    let image_data = ImageRawBE::<Rgb888>::new(IMAGE_DATA, IMAGE_SIZE as u32);
    let image = Image::new(
        &image_data,
        Point::new(cols / 2 - IMAGE_SIZE / 2, rows / 2 - IMAGE_SIZE / 2),
    );

    for step in 0..=u64::MAX {
        image.draw(&mut canvas).unwrap();
        canvas = matrix
            .update_on_vsync(canvas)
            .expect("The display update thread stopped");

        if step % 120 == 0 {
            print!("\r{:>100}\rFramerate: {}", "", matrix.framerate());
            std::io::stdout().flush().unwrap();
        }
    }
}

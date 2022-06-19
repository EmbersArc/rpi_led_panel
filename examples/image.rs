use std::io::Write;

use embedded_graphics::{
    image::{Image, ImageRawBE},
    pixelcolor::Rgb888,
    prelude::*,
    Drawable,
};
use rpi_led_matrix::{RGBMatrix, RGBMatrixConfig};

const IMAGE_DATA: &[u8] = include_bytes!("../assets/ferris_test_card.rgb");
const IMAGE_SIZE: usize = 64;

fn main() {
    let config: RGBMatrixConfig = argh::from_env();
    let rows = config.rows;
    let cols = config.cols;
    let (mut matrix, mut canvas) = RGBMatrix::new(config, 0);

    let image_data = ImageRawBE::<Rgb888>::new(IMAGE_DATA, IMAGE_SIZE as u32);
    let image = Image::new(
        &image_data,
        Point::new(
            (cols / 2 - IMAGE_SIZE / 2) as i32,
            (rows / 2 - IMAGE_SIZE / 2) as i32,
        ),
    );

    for step in 0.. {
        canvas.fill(0, 0, 0);
        image.draw(canvas.as_mut()).unwrap();
        canvas = matrix.update_on_vsync(canvas);

        if step % 120 == 0 {
            print!("\r{:>100}\rFramerate: {}", "", matrix.get_framerate());
            std::io::stdout().flush().unwrap();
        }
    }
}

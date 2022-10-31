use std::io::Write;

use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle},
    text::{Alignment, Text},
    Drawable,
};
use rpi_led_panel::{RGBMatrix, RGBMatrixConfig};

fn main() {
    let config: RGBMatrixConfig = argh::from_env();
    let rows = config.rows as i32;
    let cols = config.cols as i32;

    let (mut matrix, mut canvas) = RGBMatrix::new(config, 0).expect("Matrix initialization failed");

    let circle = {
        let thin_stroke = PrimitiveStyle::with_stroke(Rgb888::CSS_GRAY, 1);
        Circle::with_center(
            Point::new(rows / 2 - 1, cols / 2 - 1),
            rows.min(cols) as u32 - 2,
        )
        .into_styled(thin_stroke)
    };
    let top = Line::new(Point::new(0, 0), Point::new(cols - 1, 0))
        .into_styled(PrimitiveStyle::with_stroke(Rgb888::GREEN, 1));
    let bottom = Line::new(Point::new(0, rows - 1), Point::new(cols - 1, rows - 1))
        .into_styled(PrimitiveStyle::with_stroke(Rgb888::CYAN, 1));
    let left = Line::new(Point::new(0, 0), Point::new(0, rows - 1))
        .into_styled(PrimitiveStyle::with_stroke(Rgb888::RED, 1));
    let right = Line::new(Point::new(cols - 1, 0), Point::new(cols - 1, rows - 1))
        .into_styled(PrimitiveStyle::with_stroke(Rgb888::BLUE, 1));
    let diagonal1 = Line::new(Point::new(0, 0), Point::new(cols - 1, rows - 1))
        .into_styled(PrimitiveStyle::with_stroke(Rgb888::YELLOW, 1));
    let diagonal2 = Line::new(Point::new(cols - 1, 0), Point::new(0, rows - 1))
        .into_styled(PrimitiveStyle::with_stroke(Rgb888::MAGENTA, 1));

    let text = Text::with_alignment(
        "Hello\nWorld",
        Point::new(cols / 2, rows / 2),
        MonoTextStyle::new(&FONT_6X10, Rgb888::WHITE),
        Alignment::Center,
    );

    for step in 0.. {
        canvas.fill(0, 0, 0);
        circle.draw(canvas.as_mut()).unwrap();
        [diagonal1, diagonal2, top, bottom, left, right]
            .iter()
            .for_each(|line| line.draw(canvas.as_mut()).unwrap());
        if (step / 100) % 2 == 0 {
            text.draw(canvas.as_mut()).unwrap();
        }
        canvas = matrix.update_on_vsync(canvas);

        if step % 120 == 0 {
            print!("\r{:>100}\rFramerate: {}", "", matrix.get_framerate());
            std::io::stdout().flush().unwrap();
        }
    }
}

use std::io::Write;

use rpi_led_matrix::{RGBMatrix, RGBMatrixConfig};

fn scale_col(value: isize, low: isize, high: isize) -> u8 {
    if value < low {
        return 0;
    }
    if value > high {
        return 255;
    }
    (255 * (value - low) / (high - low)) as u8
}

fn rotate([x, y]: [isize; 2], angle: f64) -> [f64; 2] {
    [
        x as f64 * angle.cos() - y as f64 * angle.sin(),
        x as f64 * angle.sin() + y as f64 * angle.cos(),
    ]
}

fn main() {
    let config: RGBMatrixConfig = argh::from_env();
    let rows = config.rows as isize;
    let cols = config.cols as isize;
    let (mut matrix, mut canvas) = RGBMatrix::new(config, 0);

    let [center_x, center_y] = [cols / 2, rows / 2];

    let rotate_square = (rows.min(cols) as f64 * 1.41) as isize;
    let min_rotate = center_x - rotate_square / 2;
    let max_rotate = center_x + rotate_square / 2;

    let display_square = (rows.min(cols) as f64 * 0.7) as isize;
    let min_display = center_x - display_square / 2;
    let max_display = center_x + display_square / 2;

    for step in 0.. {
        let rotation_deg = step as f64 / 2.0;
        for x in min_rotate..max_rotate {
            for y in min_rotate..max_rotate {
                let [rot_x, rot_y] =
                    rotate([x - center_x, y - center_x], rotation_deg.to_radians());
                let canvas_x = rot_x + center_x as f64;
                let canvas_y = rot_y + center_y as f64;
                if (min_display..max_display).contains(&x)
                    && (min_display..max_display).contains(&y)
                {
                    canvas.set_pixel(
                        canvas_x as usize,
                        canvas_y as usize,
                        scale_col(x, min_display, max_display),
                        255 - scale_col(y, min_display, max_display),
                        scale_col(y, min_display, max_display),
                    )
                } else {
                    canvas.set_pixel(canvas_x as usize, canvas_y as usize, 0, 0, 0)
                }
            }
        }

        canvas = matrix.update_on_vsync(canvas);

        if step % 120 == 0 {
            print!("\r{:>100}\rFramerate: {}", "", matrix.get_framerate());
            std::io::stdout().flush().unwrap();
        }
    }
}

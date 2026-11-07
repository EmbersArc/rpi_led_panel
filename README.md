<p align="center">
    <img src="assets/logo.png" width="350">
</p>

[![Latest Version]][crates.io]

[latest version]: https://img.shields.io/crates/v/rpi_led_panel.svg
[crates.io]: https://crates.io/crates/rpi_led_panel

# 🦀 Raspberry Pi LED Panel

A Rust rewrite of the fantastic [rpi-rgb-led-matrix](https://github.com/hzeller/rpi-rgb-led-matrix) library. Use this if you want something that is easy to integrate into your Rust project but don't mind it being mostly untested. Contributions are welcome.

See the [examples](examples) to get started. You can run any of them with `--help` to get a list of command line arguments. See the [original readme](https://github.com/hzeller/rpi-rgb-led-matrix) for more information.

## Raspberry Pi 5

On a Raspberry Pi 5, the GPIO pins are driven through its RP1 I/O controller, which is detected automatically. By default, the CPU sets the pins directly through RP1's registered I/O block, which works without root privileges through `/dev/gpiomem0`. With `--rp1-pio true`, a PIO state machine of the RP1 clocks out the data instead, which needs the kernel's PIO driver at `/dev/pio0`. On the Raspberry Pi 5, `--slowdown` divides the pixel clock.

Like upstream, the Raspberry Pi 5 supports the `Regular`, `Classic`, `AdafruitHat` and `AdafruitHatPwm` hardware mappings (and `RegularPi1` with PIO), up to 3 parallel chains, and no GPIO inputs or outputs for the user. The CPU backend only changes the pins of the matrix, so other outputs of the GPIO bank keep their state.

## Compute Module

The `ComputeModule` hardware mapping supports up to 3 parallel chains. Enable the `wide-gpio` feature to use all 6, which needs the GPIO pins above 31 and doubles the memory per canvas.

## Refresh thread

The panels are refreshed by a thread named `led-refresh`, pinned to the last core. By default it runs at the highest priority of the normal scheduler (nice -20). With `--realtime-priority <1-99>` it uses the real-time `SCHED_FIFO` policy instead and most of the kernel's real-time throttling is turned off, which can reduce flicker. Only do this with the refresh core isolated (`isolcpus=3` on a 4 core Pi, in `/boot/firmware/cmdline.txt`): otherwise the thread can starve everything else on that core, including kernel threads, and stall the system.

`--uncapped-refresh-rate true` refreshes as fast as possible instead of at `--refresh-rate`. `--clear-on-vsync false` skips clearing the canvas returned by `update_on_vsync`, for programs that redraw every pixel anyway. `update_on_vsync` returns an error if the refresh thread has stopped, and `suspend` blanks the display and pauses the thread until the next `update_on_vsync`. Warnings and suggestions are reported through the `log` crate, so a logger such as `env_logger` is needed to see them.

## Known limitations

On the Raspberry Pi 4, the PWM clock source (PLLD) is reported to run at 750 MHz instead of the 500 MHz the hardware pulses are timed for, as in upstream. The on-times of the bit planes may therefore be shorter than `--pwm-lsb-nanoseconds` asks for. This is not corrected, as it could not be verified on hardware.

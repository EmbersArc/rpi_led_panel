use std::time::Duration;

use argh::FromArgs;

use crate::{
    HardwareMapping, PiChip, canvas::LedSequence, init_sequence::PanelType,
    multiplex_mapper::MultiplexMapperType, named_pixel_mapper::NamedPixelMapperType,
    row_address_setter::RowAddressSetterType,
};

/// Typically, a HUB75 panel is split in two half displays, so that a 1:16 multiplexing actually multiplexes
/// over two half displays and gives 32 lines.
pub(crate) const SUB_PANELS: usize = 2;

/// Maximum usable bit planes. 11 bits seems to be a sweet spot in which we still get somewhat useful refresh
/// rate and have good color richness. This is the default setting. However, in low-light situations, we want
/// to be able to scale down brightness more by having more bits at the bottom.
pub(crate) const BIT_PLANES: usize = 11;

/// Configuration for an RGB matrix panel controller.
#[derive(FromArgs, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RGBMatrixConfig {
    /// the display wiring e.g. "Regular", "AdafruitHat", "AdafruitHatPwm", etc. Default: "Regular"
    #[argh(option, default = "HardwareMapping::regular()")]
    pub hardware_mapping: HardwareMapping,
    /// the number of display rows. Default: 64
    #[argh(option, default = "64")]
    pub rows: usize,
    /// the number of display columns. Default: 64
    #[argh(option, default = "64")]
    pub cols: usize,
    /// the display refresh rate, between 1 and 100000. Default: 120
    #[argh(option, default = "120")]
    pub refresh_rate: usize,
    /// refresh the panels as fast as possible instead of waiting for the next frame of the refresh rate, which
    /// is then ignored. This uses the whole core and makes the refresh rate depend on the configuration.
    /// Default: false
    #[argh(option, default = "false")]
    pub uncapped_refresh_rate: bool,
    /// the Raspberry Pi chip model e.g. "BCM2711", Default: automatic
    #[argh(option)]
    pub pi_chip: Option<PiChip>,
    /// the LEDs can only be switched on or off, so the shaded brightness perception is achieved via PWM
    /// (Pulse Width Modulation). In order to get a good 8 bit per color resolution (24 bit RGB), the 11 bits
    /// default per color are good because our eyes are actually perceiving brightness logarithmically, so we
    /// need a lot more physical resolution to get 24Bit sRGB. This flag sets the bits used for this; lowering
    /// it means the lower bits (=more subtle color nuances) are omitted. Typically you might be mostly
    /// interested in the extremes: 1 Bit for situations that only require 8 colors (e.g. for high contrast
    /// text displays) or 11 Bit for everything else (e.g. showing images or videos). Lower number of bits use
    /// slightly less CPU and result in a higher refresh rate. Default: 11
    #[argh(option, default = "11")]
    pub pwm_bits: usize,
    /// base time-unit for the on-time in the lowest significant bit in nanoseconds. Lower values will allow
    /// higher frame rate, but will also negatively impact quality in some panels. Good values for full-color
    /// display (pwm_bits=11) are somewhere between 100 and 300. Default: 130
    #[argh(option, default = "130")]
    pub pwm_lsb_nanoseconds: u32,
    /// the Raspberry Pis starting with Pi2 are putting out data too fast for almost all LED panels. In this
    /// case, you want to slow down writing to GPIO. Zero for this parameter means 'no slowdown'. The default
    /// 1 typically works fine, but often you have to even go further by setting it to 2. If you have a
    /// Raspberry Pi with a slower processor (Model A, A+, B+, Zero), then a value of 0 might work and is
    /// desirable. A Raspberry Pi3 or Pi4 might even need higher values for the panels to be happy. On ARMv7 and
    /// newer, -1 waits for a memory barrier instead, which can be faster than 1 while still being stable. On a
    /// Raspberry Pi 5, this divides the pixel clock instead. Values between -1 and 60 are allowed.
    /// Default: automatic
    #[argh(option)]
    pub slowdown: Option<i32>,
    /// use the PWM hardware for exact timing of the LED on-time. This requires the output enable to be on GPIO 18
    /// or 12, otherwise the timing is done in software. Turn it off if the hardware PWM is needed for something
    /// else, like sound. Default: true
    #[argh(option, default = "true")]
    pub hardware_pulse: bool,
    /// lower values reduce CPU usage but can make flickers more likely: waits wake up this early and busy wait
    /// the rest. Default: 100
    #[argh(option, default = "100")]
    pub min_sys_sleep_microseconds: u64,
    /// use interlaced scan instead of progressive scan
    #[argh(switch)]
    pub interlaced: bool,
    /// the lower bits can be time dithered, i.e. their brightness contribution is achieved by only showing
    /// them some frames (this is possible, because the PWM is implemented as binary code modulation). This
    /// will allow higher refresh rate (or same refresh rate with increased --pwm-lsb-nanoseconds). The
    /// disadvantage could be slightly lower brightness, in particular for longer chains, and higher CPU use.
    /// CPU use is not of concern for Raspberry Pi 2, 3 and 4 (as we run on a dedicated core anyway) but
    /// probably for Raspberry Pi 1 or Pi Zero. Default: 0 (no dithering)
    #[argh(option, default = "0")]
    pub dither_bits: usize,
    /// number of daisy-chained panels. Default: 1
    #[argh(option, default = "1")]
    pub chain_length: usize,
    /// how many chains to run in parallel. Default: 1
    #[argh(option, default = "1")]
    pub parallel: usize,
    /// typically left empty, but some panels need a particular initialization sequence. This can be e.g.
    /// "FM6126A" for that particular panel type.
    #[argh(option)]
    pub panel_type: Option<PanelType>,
    /// the kind of multiplexing mapper.
    #[argh(option)]
    pub multiplexing: Option<MultiplexMapperType>,
    /// the kind of pixel mapper.
    #[argh(option)]
    pub pixelmapper: Vec<NamedPixelMapperType>,
    /// the row address setter.
    #[argh(option, default = "RowAddressSetterType::Direct")]
    pub row_setter: RowAddressSetterType,
    /// the LED sequence, Default: "RGB"
    #[argh(option, default = "LedSequence::Rgb")]
    pub led_sequence: LedSequence,
    /// brightness in percent. Default: 100
    #[argh(option, default = "100")]
    pub led_brightness: u8,
    /// invert all colors, for panels that light up the LEDs on a low signal. Default: false
    #[argh(option, default = "false")]
    pub inverse_colors: bool,
    /// print the refresh rate to the terminal. Default: false
    #[argh(option, default = "false")]
    pub show_refresh: bool,
    /// busy-wait for the end of the frame to get exact refresh rates. Turning this off reduces the CPU usage, but
    /// makes the refresh rate less stable. Default: true
    #[argh(option, default = "true")]
    pub busy_waiting: bool,
    /// run the refresh thread with the real-time SCHED_FIFO policy at this priority (1 to 99) instead of the
    /// highest normal priority (nice -20). This also disables most of the kernel's real-time throttling, so
    /// only use it with the refresh core isolated (`isolcpus=3` in /boot/firmware/cmdline.txt on a 4 core Pi):
    /// otherwise the thread can starve everything else scheduled on that core, including kernel threads, which
    /// can stall the system or trigger the watchdog. Needs root or CAP_SYS_NICE. Default: off
    #[argh(option)]
    pub realtime_priority: Option<u8>,
    /// clear the canvas that `RGBMatrix::update_on_vsync` returns. Turn it off if every frame redraws all
    /// pixels anyway: the returned canvas then still holds an older frame. Default: true
    #[argh(option, default = "true")]
    pub clear_on_vsync: bool,
    /// on a Raspberry Pi 5, use the PIO state machine of the RP1 I/O controller to clock out the data, instead of
    /// setting the GPIO pins from the CPU. This needs the kernel's PIO driver (/dev/pio0). Default: false
    #[argh(option, default = "false")]
    pub rp1_pio: bool,
    /// GPIO pins to read as inputs, as a decimal bitmask in which bit n stands for GPIO n. Pins that the
    /// matrix uses itself are skipped. GPIO inputs are not supported on the Raspberry Pi 5. Default: 0
    #[argh(option, default = "0")]
    pub input_bits: u32,
    /// keep running as root instead of dropping privileges after initialization
    #[argh(switch)]
    pub no_drop_privs: bool,
    /// username or UID to drop privileges to. Default: daemon
    #[argh(option, default = "String::from(\"daemon\")")]
    pub drop_priv_user: String,
    /// group name or GID to drop privileges to. Default: daemon
    #[argh(option, default = "String::from(\"daemon\")")]
    pub drop_priv_group: String,
}

/// The highest refresh rate that is accepted. Faster frames than 10 µs are not possible anyway.
pub(crate) const MAX_REFRESH_RATE: usize = 100_000;

impl RGBMatrixConfig {
    /// How long each frame should take, or `None` to refresh as fast as possible.
    pub(crate) fn frame_time_target(&self) -> Option<Duration> {
        if self.uncapped_refresh_rate {
            return None;
        }
        // Validated to be between 1 and `MAX_REFRESH_RATE`, so this neither truncates nor divides by zero.
        let rate = self.refresh_rate.clamp(1, MAX_REFRESH_RATE) as u64;
        Some(Duration::from_nanos(1_000_000_000 / rate))
    }

    pub(crate) const fn double_rows(&self) -> usize {
        self.rows / SUB_PANELS
    }

    /// How long the LEDs are on for each bit plane. The lowest bits are dithered and show for the same time.
    pub(crate) fn bitplane_timings_ns(&self) -> [u32; BIT_PLANES] {
        let mut timing_ns = self.pwm_lsb_nanoseconds;
        std::array::from_fn(|b| {
            let timing = timing_ns;
            if b >= self.dither_bits {
                timing_ns = timing_ns.saturating_mul(2);
            }
            timing
        })
    }
}

impl Default for RGBMatrixConfig {
    fn default() -> Self {
        Self {
            hardware_mapping: HardwareMapping::regular(),
            rows: 64,
            cols: 64,
            refresh_rate: 120,
            uncapped_refresh_rate: false,
            pi_chip: None,
            pwm_bits: 11,
            pwm_lsb_nanoseconds: 130,
            slowdown: None,
            hardware_pulse: true,
            min_sys_sleep_microseconds: 100,
            interlaced: false,
            dither_bits: 0,
            chain_length: 1,
            parallel: 1,
            panel_type: None,
            multiplexing: None,
            pixelmapper: vec![],
            row_setter: RowAddressSetterType::Direct,
            led_sequence: LedSequence::Rgb,
            led_brightness: 100,
            inverse_colors: false,
            show_refresh: false,
            busy_waiting: true,
            realtime_priority: None,
            clear_on_vsync: true,
            rp1_pio: false,
            input_bits: 0,
            no_drop_privs: false,
            drop_priv_user: String::from("daemon"),
            drop_priv_group: String::from("daemon"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FromArgs, RGBMatrixConfig};

    #[test]
    fn test_argument_defaults_match_default() {
        let parsed = RGBMatrixConfig::from_args(&["test"], &[]).unwrap();
        assert_eq!(parsed, RGBMatrixConfig::default());
    }
}

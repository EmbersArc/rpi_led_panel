use argh::FromArgs;

use crate::{
    canvas::LedSequence, init_sequence::PanelType, multiplex_mapper::MultiplexMapperType,
    named_pixel_mapper::NamedPixelMapperType, row_address_setter::RowAddressSetterType,
    HardwareMapping, PiChip,
};

/// Typically, a Hub75 panel is split in two half displays, so that a 1:16 multiplexing actually multiplexes
/// over two half displays and gives 32 lines.
pub(crate) const SUB_PANELS: usize = 2;

/// Maximum usable bit planes. 11 bits seems to be a sweet spot in which we still get somewhat useful refresh
/// rate and have good color richness. This is the default setting. However, in low-light situations, we want
/// to be able to scale down brightness more by having more bits at the bottom.
pub(crate) const K_BIT_PLANES: usize = 11;

/// Configuration for an RGB matrix panel controller.
#[derive(FromArgs, Debug, PartialEq, Eq, Hash)]
pub struct RGBMatrixConfig {
    /// the display wiring e.g. "AdafruitHat" or "AdafruitHatPwm". Default: "AdafruitHatPwm"
    #[argh(option, default = "HardwareMapping::adafruit_hat_pwm()")]
    pub hardware_mapping: HardwareMapping,
    /// the number of display rows. Default: 64
    #[argh(option, default = "64")]
    pub rows: usize,
    /// the number of display columns. Default: 64
    #[argh(option, default = "64")]
    pub cols: usize,
    /// the display refresh rate. Default: 120
    #[argh(option, default = "120")]
    pub refresh_rate: usize,
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
    /// the Raspberry Pi starting with Pi2 are putting out data too fast for almost all LED panels. In this
    /// case, you want to slow down writing to GPIO. Zero for this parameter means 'no slowdown'. The default
    /// 1 typically works fine, but often you have to even go further by setting it to 2. If you have a
    /// Raspberry Pi with a slower processor (Model A, A+, B+, Zero), then a value of 0 might work and is
    /// desirable. A Raspberry Pi3 or Pi4 might even need higher values for the panels to be happy.
    /// Default: automatic
    #[argh(option)]
    pub slowdown: Option<u32>,
    /// interlaced scan mode. Default: false
    #[argh(option, default = "false")]
    pub interlaced: bool,
    /// the lower bits can be time dithered, i.e. their brightness contribution is achieved by only showing
    /// them some frames (this is possible, because the PWM is implemented as binary code modulation). This
    /// will allow higher refresh rate (or same refresh rate with increased --pwm_lsb_nanoseconds). The
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
}

impl RGBMatrixConfig {
    pub(crate) const fn double_rows(&self) -> usize {
        self.rows / SUB_PANELS
    }
}

impl Default for RGBMatrixConfig {
    fn default() -> Self {
        Self {
            hardware_mapping: HardwareMapping::adafruit_hat_pwm(),
            rows: 64,
            cols: 64,
            refresh_rate: 120,
            pi_chip: None,
            pwm_bits: 11,
            pwm_lsb_nanoseconds: 130,
            slowdown: None,
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
        }
    }
}

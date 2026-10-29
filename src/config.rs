use argh::FromArgs;

use crate::{
    HardwareMapping, PiChip, canvas::LedSequence, init_sequence::PanelType,
    multiplex_mapper::MultiplexMapperType, named_pixel_mapper::NamedPixelMapperType,
    rgb_matrix::MatrixCreationError, row_address_setter::RowAddressSetterType,
};

/// Typically, a Hub75 panel is split in two half displays, so that a 1:16 multiplexing actually multiplexes
/// over two half displays and gives 32 lines.
pub(crate) const SUB_PANELS: usize = 2;

/// The row address lines A to E can select up to 32 double rows.
const MAX_DOUBLE_ROWS: usize = 32;

/// The PWM clock divider is a quarter of the LSB time and needs to be at least 1.
const MIN_PWM_LSB_NANOSECONDS: u32 = 4;

/// Well above any useful value, and below the 16384 at which the PWM clock divider overflows its 12 bits.
const MAX_PWM_LSB_NANOSECONDS: u32 = 10_000;

/// The display update has dither sequences for up to this many bits.
const MAX_DITHER_BITS: usize = 2;

/// Maximum usable bit planes. 11 bits seems to be a sweet spot in which we still get somewhat useful refresh
/// rate and have good color richness. This is the default setting. However, in low-light situations, we want
/// to be able to scale down brightness more by having more bits at the bottom.
pub(crate) const K_BIT_PLANES: usize = 11;

/// Configuration for an RGB matrix panel controller.
#[derive(FromArgs, Debug, PartialEq, Eq, Hash)]
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
    /// lower values reduce CPU usage but can make flickers more likely.
    /// Default: 100
    #[argh(option, default = "100")]
    pub min_sys_sleep_microseconds: u64,
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
    /// "FM6126" or "FM6127" for those panel types.
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
    /// drop privileges from root after initialization. Default: true
    #[argh(option, default = "true")]
    pub drop_privs: bool,
    /// username or UID to drop privileges to. Default: daemon
    #[argh(option, default = "String::from(\"daemon\")")]
    pub drop_priv_user: String,
    /// group name or GID to drop privileges to. Default: daemon
    #[argh(option, default = "String::from(\"daemon\")")]
    pub drop_priv_group: String,
}

impl RGBMatrixConfig {
    pub(crate) const fn double_rows(&self) -> usize {
        self.rows / SUB_PANELS
    }

    /// Reject options that the display update or the multiplex mapper can't handle. Multiplexing divides the
    /// rows by its `stretch_factor`, so they need to stay a multiple of [`SUB_PANELS`] after that.
    pub(crate) fn validate(&self, stretch_factor: usize) -> Result<(), MatrixCreationError> {
        let rows_step = SUB_PANELS * stretch_factor;
        let max_rows = MAX_DOUBLE_ROWS * rows_step;
        if !(rows_step..=max_rows).contains(&self.rows) || !self.rows.is_multiple_of(rows_step) {
            return Err(MatrixCreationError::InvalidConfig(format!(
                "rows needs to be a multiple of {rows_step} from {rows_step} to {max_rows}, but is {}.",
                self.rows
            )));
        }

        for (name, value) in [
            ("cols", self.cols),
            ("chain_length", self.chain_length),
            ("parallel", self.parallel),
            ("refresh_rate", self.refresh_rate),
        ] {
            if value == 0 {
                return Err(MatrixCreationError::InvalidConfig(format!(
                    "{name} needs to be at least 1."
                )));
            }
        }

        if let Some(multiplexing) = self.multiplexing {
            let [rows, cols] = multiplexing.supported_panel_size();
            for (name, value, supported) in [("rows", self.rows, rows), ("cols", self.cols, cols)] {
                if !supported.contains(value) {
                    return Err(MatrixCreationError::InvalidConfig(format!(
                        "{name} needs to be {supported} for {multiplexing:?} multiplexing, but is {value}."
                    )));
                }
            }
        }

        let max_parallel = self.hardware_mapping.max_parallel_chains();
        if self.parallel > max_parallel {
            return Err(MatrixCreationError::TooManyParallelChains(max_parallel));
        }

        if !(MIN_PWM_LSB_NANOSECONDS..=MAX_PWM_LSB_NANOSECONDS).contains(&self.pwm_lsb_nanoseconds)
        {
            return Err(MatrixCreationError::InvalidConfig(format!(
                "pwm_lsb_nanoseconds needs to be from {MIN_PWM_LSB_NANOSECONDS} to {MAX_PWM_LSB_NANOSECONDS}, but is {}.",
                self.pwm_lsb_nanoseconds
            )));
        }

        if !(1..=K_BIT_PLANES).contains(&self.pwm_bits) {
            return Err(MatrixCreationError::InvalidConfig(format!(
                "pwm_bits needs to be from 1 to {K_BIT_PLANES}, but is {}.",
                self.pwm_bits
            )));
        }

        if !(1..=100).contains(&self.led_brightness) {
            return Err(MatrixCreationError::InvalidConfig(format!(
                "led_brightness needs to be from 1 to 100, but is {}.",
                self.led_brightness
            )));
        }

        if self.dither_bits > MAX_DITHER_BITS {
            return Err(MatrixCreationError::InvalidDitherBits(self.dither_bits));
        }

        Ok(())
    }
}

impl Default for RGBMatrixConfig {
    fn default() -> Self {
        Self {
            hardware_mapping: HardwareMapping::regular(),
            rows: 64,
            cols: 64,
            refresh_rate: 120,
            pi_chip: None,
            pwm_bits: 11,
            pwm_lsb_nanoseconds: 130,
            slowdown: None,
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
            drop_privs: true,
            drop_priv_user: String::from("daemon"),
            drop_priv_group: String::from("daemon"),
        }
    }
}

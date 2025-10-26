/// Custom strum error that contains the variants in the message.
#[derive(Debug)]
pub struct InvalidVariantError {
    variants: &'static [&'static str],
}

impl InvalidVariantError {
    pub fn new<V: strum::VariantNames>(_s: &str) -> InvalidVariantError {
        InvalidVariantError {
            variants: V::VARIANTS,
        }
    }
}

impl std::fmt::Display for InvalidVariantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Options are: {:?}", self.variants)
    }
}

impl std::error::Error for InvalidVariantError {}

/// Custom strum error that contains the variants in the message.
#[derive(Debug)]
pub struct InvalidVariantError {
    input: String,
    variants: &'static [&'static str],
}

impl InvalidVariantError {
    pub(crate) fn new<V: strum::VariantNames>(s: &str) -> InvalidVariantError {
        InvalidVariantError {
            input: s.to_string(),
            variants: V::VARIANTS,
        }
    }
}

impl std::fmt::Display for InvalidVariantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "'{}' is not valid. Options are: {:?}",
            self.input, self.variants
        )
    }
}

impl std::error::Error for InvalidVariantError {}

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

/// A string that doesn't describe a [`NamedPixelMapperType`](crate::NamedPixelMapperType).
#[derive(Debug)]
pub struct InvalidPixelMapperError(String);

impl InvalidPixelMapperError {
    pub(crate) fn new(message: impl Into<String>) -> InvalidPixelMapperError {
        InvalidPixelMapperError(message.into())
    }
}

impl std::fmt::Display for InvalidPixelMapperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for InvalidPixelMapperError {}

//! `PptxError`: why a presentation could not be read. `Display` keeps the wording the viewer always showed.

#[derive(Debug, thiserror::Error)]
pub enum PptxError {
    /// The file could not be opened or read.
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
    /// Not a presentation, damaged, or too large to open.
    #[error("{0}")]
    Invalid(String),
}

impl From<String> for PptxError {
    fn from(message: String) -> Self {
        Self::Invalid(message)
    }
}

impl From<&str> for PptxError {
    fn from(message: &str) -> Self {
        Self::Invalid(message.to_string())
    }
}

impl From<PptxError> for String {
    fn from(error: PptxError) -> Self {
        error.to_string()
    }
}

pub type PptxResult<T> = Result<T, PptxError>;

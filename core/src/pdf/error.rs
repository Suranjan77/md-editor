//! `PdfError`: what `PdfRenderer` calls fail with. The worker thread still reports strings internally; they are classified here, at the public boundary.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PdfError {
    /// A render that was dropped because its page left the visible range.
    #[error("Skipped")]
    Skipped,
    #[error("Password required to open PDF")]
    PasswordRequired,
    /// The PDFium worker thread is gone (it failed to bind PDFium, or panicked).
    #[error("The PDF engine is not running")]
    WorkerGone,
    #[error("{0}")]
    Other(String),
}

impl PdfError {
    /// Classify a worker's error string.
    pub(crate) fn from_worker(message: String) -> Self {
        match message.as_str() {
            "Skipped" => Self::Skipped,
            "Password required to open PDF" => Self::PasswordRequired,
            _ => Self::Other(message),
        }
    }
}

impl From<String> for PdfError {
    fn from(message: String) -> Self {
        Self::from_worker(message)
    }
}

impl From<PdfError> for String {
    fn from(error: PdfError) -> Self {
        error.to_string()
    }
}

pub type PdfResult<T> = Result<T, PdfError>;

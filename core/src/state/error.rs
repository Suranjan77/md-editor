//! `StateError`: what the settings/annotation/tracker database layer fails with. `Display` keeps the wording callers always saw.

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Pdf(#[from] crate::pdf::PdfError),
    /// A lock was poisoned by a panic elsewhere.
    #[error("{0}")]
    Lock(String),
    #[error("{0}")]
    Other(String),
}

impl<T> From<std::sync::PoisonError<T>> for StateError {
    fn from(error: std::sync::PoisonError<T>) -> Self {
        Self::Lock(error.to_string())
    }
}

impl From<String> for StateError {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

impl From<&str> for StateError {
    fn from(message: &str) -> Self {
        Self::Other(message.to_string())
    }
}

impl From<StateError> for String {
    fn from(error: StateError) -> Self {
        error.to_string()
    }
}

pub type StateResult<T> = Result<T, StateError>;

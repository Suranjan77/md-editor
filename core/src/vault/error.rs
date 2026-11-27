//! `VaultError`: what vault operations fail with. `Display` keeps the user-facing wording the toasts always showed; the kind lets callers react differently (a missing file vs a permission problem).

use std::io;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("No vault root set")]
    NoVault,
    /// A user-supplied path resolves outside the vault (`..`, symlinks).
    #[error("Path escapes the vault root: {0}")]
    OutsideVault(String),
    #[error("Target already exists: {}", .0.display())]
    AlreadyExists(PathBuf),
    #[error("{} is read-only", .0.display())]
    ReadOnly(PathBuf),
    #[error("Not an image: {}", .0.display())]
    NotAnImage(PathBuf),
    #[error("{0}")]
    InvalidName(String),
    /// A filesystem call failed; `context` says what was being done.
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    State(#[from] crate::state::StateError),
    /// Anything else (the index, a poisoned lock).
    #[error("{0}")]
    Other(String),
}

impl VaultError {
    /// Wrap an I/O error with what was being attempted.
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    /// The file or folder does not exist (any more).
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Io { source, .. } if source.kind() == io::ErrorKind::NotFound)
    }

    /// The OS refused access, or the file is marked read-only.
    pub fn is_permission_denied(&self) -> bool {
        matches!(self, Self::ReadOnly(_))
            || matches!(self, Self::Io { source, .. } if source.kind() == io::ErrorKind::PermissionDenied)
    }
}

impl From<String> for VaultError {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

impl From<&str> for VaultError {
    fn from(message: &str) -> Self {
        Self::Other(message.to_string())
    }
}

/// Callers that still speak `String` (most of the native shell, for toasts)
/// convert with `?` or `.into()`.
impl From<VaultError> for String {
    fn from(error: VaultError) -> Self {
        error.to_string()
    }
}

pub type VaultResult<T> = Result<T, VaultError>;

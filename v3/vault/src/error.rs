//! Typed errors (plan §3.4): `Result<_, String>` is banned in v3 crates.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("i/o error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("not a directory: {0}")]
    NotADirectory(PathBuf),
    #[error("file changed on disk since it was read (mtime conflict): {0}")]
    Conflict(PathBuf),
    #[error("path escapes the vault root: {0}")]
    OutsideVault(PathBuf),
}

impl VaultError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> VaultError {
        VaultError::Io {
            path: path.into(),
            source,
        }
    }
}

//! Vault file/note listing types.

use serde::{Deserialize, Serialize};

/// A file entry in the vault listing.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileEntry {
    /// Vault-relative path of the entry.
    pub path: String,
    /// Display name (file or directory name).
    pub name: String,
    /// Whether the entry is a directory.
    pub is_dir: bool,
}

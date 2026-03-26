use serde::Serialize;

/// A file entry in the vault listing.
#[derive(Serialize, Clone, Debug)]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
}

//! Atomic saves (plan §3.4 "vault safety"): write to a temp file in the same
//! directory, fsync, then rename over the destination. A crash mid-save
//! leaves either the old file or the new file — never a torn one.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::error::VaultError;

/// Atomically replace `path` with `contents`.
pub fn atomic_save(path: &Path, contents: &[u8]) -> Result<(), VaultError> {
    let dir = path
        .parent()
        .ok_or_else(|| VaultError::NotADirectory(path.to_path_buf()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| VaultError::NotADirectory(path.to_path_buf()))?
        .to_string_lossy();
    // Same-directory temp file so the rename cannot cross filesystems.
    let tmp_path = dir.join(format!(".{file_name}.md3-tmp"));

    let mut tmp = fs::File::create(&tmp_path).map_err(|e| VaultError::io(&tmp_path, e))?;
    tmp.write_all(contents)
        .map_err(|e| VaultError::io(&tmp_path, e))?;
    tmp.sync_all().map_err(|e| VaultError::io(&tmp_path, e))?;
    drop(tmp);

    fs::rename(&tmp_path, path).map_err(|e| {
        // Best effort cleanup; the original file is untouched either way.
        let _ = fs::remove_file(&tmp_path);
        VaultError::io(path, e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_creates_and_replaces_without_leaving_temp_files() {
        let dir = match tempfile::tempdir() {
            Ok(d) => d,
            Err(e) => panic!("tempdir: {e}"),
        };
        let target = dir.path().join("note.md");

        if let Err(e) = atomic_save(&target, b"first") {
            panic!("save: {e}");
        }
        assert_eq!(fs::read(&target).ok().as_deref(), Some(b"first".as_ref()));

        if let Err(e) = atomic_save(&target, b"second") {
            panic!("resave: {e}");
        }
        assert_eq!(fs::read(&target).ok().as_deref(), Some(b"second".as_ref()));

        let leftovers: Vec<_> = match fs::read_dir(dir.path()) {
            Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.file_name()).collect(),
            Err(e) => panic!("read_dir: {e}"),
        };
        assert_eq!(leftovers.len(), 1, "no temp files remain: {leftovers:?}");
    }

    #[test]
    fn save_into_missing_directory_is_a_typed_error() {
        let err = atomic_save(Path::new("/nonexistent-md3/dir/note.md"), b"x");
        assert!(matches!(err, Err(VaultError::Io { .. })));
    }
}

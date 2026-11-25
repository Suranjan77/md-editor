//! The vault: indexing, opening/saving/creating/deleting files, and module re-exports.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::file_index::FileIndex;
use crate::state::AppState;

mod backlinks;
mod error;
mod files;
mod graph;
mod indexing;
mod relink;
mod rename;

use crate::types::{
    BacklinkItem, BacklinkTarget, FileEntry, GraphEdge, GraphEdgeKind, GraphNode, GraphNodeKind,
    GraphSnapshot, SearchResult,
};
pub use backlinks::*;
pub use error::{VaultError, VaultResult};
pub use files::*;
pub use graph::*;
pub use indexing::index_vault_for_generation;
pub use rename::*;

pub const IMAGE_EXTENSIONS: [&str; 7] = ["jpeg", "jpg", "png", "gif", "bmp", "svg", "webp"];
const MARKDOWN_EXTENSIONS: [&str; 2] = ["md", "markdown"];
const PDF_EXTENSIONS: [&str; 1] = ["pdf"];
/// Handwritten pages written by the native ink canvas.
const INK_EXTENSIONS: [&str; 1] = ["ink"];

/// Directory names that are never worth indexing or showing in the tree.
const EXCLUDED_DIRS: [&str; 6] = [
    "node_modules",
    "target",
    "build",
    "dist",
    "__pycache__",
    ".trash",
];

/// Maximum directory depth the vault walkers descend.
const MAX_WALK_DEPTH: usize = 32;

/// Folder names the user excluded (Settings → Files), on top of [`EXCLUDED_DIRS`].
static USER_EXCLUDED_DIRS: std::sync::RwLock<Vec<String>> = std::sync::RwLock::new(Vec::new());

/// Parse the comma-separated excluded-folders setting into folder names.
pub fn parse_excluded_dirs(raw: &str) -> Vec<String> {
    let mut names: Vec<String> = raw
        .split(',')
        .map(|name| name.trim().trim_matches('/').to_string())
        .filter(|name| !name.is_empty() && !name.contains('/'))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Replace the user's excluded folder names. Returns whether the set changed,
/// i.e. whether the vault needs re-indexing.
pub fn set_user_excluded_dirs(names: Vec<String>) -> bool {
    match USER_EXCLUDED_DIRS.write() {
        Ok(mut current) if *current != names => {
            *current = names;
            true
        }
        Ok(_) => false,
        Err(poisoned) => {
            *poisoned.into_inner() = names;
            true
        }
    }
}

fn is_excluded_dir(name: &str) -> bool {
    name.starts_with('.')
        || EXCLUDED_DIRS.contains(&name)
        || USER_EXCLUDED_DIRS
            .read()
            .is_ok_and(|names| names.iter().any(|excluded| excluded == name))
}

/// Whether `rel_path` (vault-relative), or any folder on the way to it, is one
/// the vault skips: dot-names, heavy build folders and the user's exclusions.
/// For filesystem watchers, which see changes the walk never visits.
pub fn is_excluded_rel_path(rel_path: &str) -> bool {
    rel_path
        .split('/')
        .filter(|part| !part.is_empty())
        .any(is_excluded_dir)
}

/// Whether any folder on the way to `rel_path` (vault-relative) is excluded.
fn is_in_excluded_dir(rel_path: &str) -> bool {
    let mut parts: Vec<&str> = rel_path.split('/').filter(|p| !p.is_empty()).collect();
    parts.pop();
    parts.into_iter().any(is_excluded_dir)
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

// ── Public API ──────────────────────────────────────────────────────

/// Set the vault root directory and index all markdown files.
/// Returns the file listing for the vault.
pub fn set_vault_root(state: &AppState, path: &str) -> VaultResult<Vec<FileEntry>> {
    let root = PathBuf::from(path);
    if !root.is_dir() {
        return Err(VaultError::from(format!("Not a directory: {}", path)));
    }
    let generation = state.begin_vault_change(root)?;
    loop {
        match index_vault_for_generation(state, path, generation)? {
            Some(entries) => return Ok(entries),
            None if state.current_vault_generation() == generation => {}
            None => return Err(VaultError::from("Vault changed while indexing")),
        }
    }
}

/// Open a file from the vault. Returns raw bytes.
pub fn open_file(state: &AppState, path: &str) -> VaultResult<Vec<u8>> {
    let _mutation = state.vault_mutation_guard()?;
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or(VaultError::NoVault)?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;

    if is_image_path(&abs_path) {
        read_image(&abs_path)
    } else {
        let content = read_file(&abs_path)?;
        let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
        index.update_file(&abs_path, &content);
        Ok(content.into_bytes())
    }
}

/// Save file content.
pub fn save_file(state: &AppState, path: &str, content: &str) -> VaultResult<()> {
    let _mutation = state.vault_mutation_guard()?;
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or(VaultError::NoVault)?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;
    write_file(&abs_path, content)?;

    let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
    index.update_file(&abs_path, content);

    let db = state.db.lock().map_err(|e| e.to_string())?;
    if let Err(e) = db.execute(
        "DELETE FROM file_search WHERE path = ?1",
        rusqlite::params![path],
    ) {
        log::warn!("Failed to clear stale search index for {path}: {e}");
    }
    if let Err(e) = db.execute(
        "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
        rusqlite::params![path, content],
    ) {
        log::warn!("Failed to update search index for {path}: {e}");
    }

    Ok(())
}

/// Identity of a note's text, used to notice that a file changed on disk
/// since the editor last read or wrote it. Stable within one process.
pub fn content_hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Vault-relative path for a conflict copy of `path`.
pub fn conflict_copy_path(vault_root: &Path, path: &str, stamp: &str) -> String {
    let rel = Path::new(path);
    let stem = rel
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    let ext = rel
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let parent = rel
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| format!("{}/", p.to_string_lossy().replace('\\', "/")))
        .unwrap_or_default();
    let mut n = 1;
    loop {
        let suffix = if n == 1 {
            String::new()
        } else {
            format!(" {n}")
        };
        let candidate = format!("{parent}{stem} (conflict {stamp}{suffix}){ext}");
        if !vault_root.join(&candidate).exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Save `content` to `path`, preserving an externally modified on-disk file as
/// a conflict copy first when `last_known` disagrees with disk.
pub fn save_file_guarded(
    state: &AppState,
    path: &str,
    content: &str,
    last_known: Option<u64>,
    stamp: &str,
) -> VaultResult<Option<String>> {
    let vault_root = {
        let guard = state.vault_root.lock().map_err(|e| e.to_string())?;
        guard.as_ref().ok_or(VaultError::NoVault)?.clone()
    };
    let abs = resolve_vault_path_checked(&vault_root, path)?;
    let mut conflict = None;
    if let (Some(expected), Ok(on_disk)) = (last_known, read_file(&abs))
        && content_hash(&on_disk) != expected
        && on_disk != content
    {
        let copy = conflict_copy_path(&vault_root, path, stamp);
        save_file(state, &copy, &on_disk)?;
        log::warn!(
            "{path} changed on disk while it had unsaved edits; kept that version as {copy}"
        );
        conflict = Some(copy);
    }
    save_file(state, path, content)?;
    Ok(conflict)
}

/// Reconcile the link/search index with the on-disk state of a single
/// vault-relative markdown path after an external change.
pub fn sync_path_from_disk(state: &AppState, rel_path: &str) -> VaultResult<()> {
    let _mutation = state.vault_mutation_guard()?;
    let vault_root = {
        let guard = state.vault_root.lock().map_err(|e| e.to_string())?;
        guard.as_ref().ok_or(VaultError::NoVault)?.clone()
    };
    let abs = resolve_vault_path_checked(&vault_root, rel_path)?;
    if !is_markdown_path(&abs) || is_in_excluded_dir(rel_path) {
        return Ok(());
    }

    if abs.is_file() {
        let Ok(content) = read_file(&abs) else {
            return Ok(());
        };
        {
            let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
            index.update_file(&abs, &content);
        }
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let _ = db.execute(
            "DELETE FROM file_search WHERE path = ?1",
            rusqlite::params![rel_path],
        );
        let _ = db.execute(
            "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
            rusqlite::params![rel_path, content],
        );
    } else {
        {
            let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
            index.remove_file(&abs);
        }
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let _ = db.execute(
            "DELETE FROM file_search WHERE path = ?1",
            rusqlite::params![rel_path],
        );
    }
    Ok(())
}

/// Create a new empty file.
pub fn create_file(state: &AppState, path: &str) -> VaultResult<()> {
    let _mutation = state.vault_mutation_guard()?;
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or(VaultError::NoVault)?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;
    if abs_path.exists() {
        return Err(VaultError::from(format!(
            "File already exists: {}",
            abs_path.display()
        )));
    }
    write_file(&abs_path, "")?;

    if is_markdown_path(&abs_path) {
        let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
        index.update_file(&abs_path, "");
        drop(index);
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let _ = db.execute(
            "INSERT INTO file_search (path, content) VALUES (?1, '')",
            rusqlite::params![path],
        );
    }
    Ok(())
}

/// Create a new directory.
pub fn create_dir(state: &AppState, path: &str) -> VaultResult<()> {
    let _mutation = state.vault_mutation_guard()?;
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or(VaultError::NoVault)?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;
    if abs_path.exists() {
        return Err(VaultError::from(format!(
            "Directory already exists: {}",
            abs_path.display()
        )));
    }
    fs::create_dir_all(&abs_path).map_err(|e| {
        VaultError::io(
            format!("Failed to create directory {}", abs_path.display()),
            e,
        )
    })
}

/// Delete a file or directory.
pub fn delete_entry(state: &AppState, path: &str) -> VaultResult<()> {
    let _mutation = state.vault_mutation_guard()?;
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or(VaultError::NoVault)?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;

    if abs_path.is_file() {
        // Delete first: a file that cannot be removed (permissions, a
        // read-only mount) must stay indexed and searchable.
        fs::remove_file(&abs_path).map_err(|e| {
            VaultError::io(format!("Failed to delete file {}", abs_path.display()), e)
        })?;
        state
            .file_index
            .lock()
            .map_err(|e| e.to_string())?
            .remove_file(&abs_path);
        let db = state.db.lock().map_err(|e| e.to_string())?;
        if let Err(e) = db.execute(
            "DELETE FROM file_search WHERE path = ?1",
            rusqlite::params![path],
        ) {
            log::warn!("Failed to remove {path} from search index: {e}");
        }
        Ok(())
    } else if abs_path.is_dir() {
        let indexed_files = list_all_md_files(&abs_path)?;
        let removed = fs::remove_dir_all(&abs_path);
        // `remove_dir_all` can fail part-way; unindex exactly what is gone.
        let gone: Vec<PathBuf> = indexed_files
            .into_iter()
            .filter(|file| !file.exists())
            .collect();
        {
            let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
            for file in &gone {
                index.remove_file(file);
            }
        }
        let db = state.db.lock().map_err(|e| e.to_string())?;
        if let Err(e) = removed {
            for file in &gone {
                let _ = db.execute(
                    "DELETE FROM file_search WHERE path = ?1",
                    rusqlite::params![path_to_relative_string(file, vault_root)],
                );
            }
            return Err(VaultError::io(
                format!("Failed to delete directory {}", abs_path.display()),
                e,
            ));
        }
        let prefix = format!("{}/", path.trim_end_matches('/'));
        db.execute(
            "DELETE FROM file_search
             WHERE path = ?1 OR substr(path, 1, length(?2)) = ?2",
            rusqlite::params![path, prefix],
        )
        .map_err(|e| {
            VaultError::from(format!(
                "Failed to remove {path} subtree from search index: {e}"
            ))
        })?;
        Ok(())
    } else {
        Err(VaultError::from(format!(
            "Path does not exist: {}",
            abs_path.display()
        )))
    }
}

/// List all entries in the vault.
pub fn list_vault(state: &AppState) -> VaultResult<Vec<FileEntry>> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or(VaultError::NoVault)?;
    list_vault_entries(vault_root)
}

/// Read raw image bytes from the vault.
pub fn read_vault_image(state: &AppState, path: &str) -> VaultResult<Vec<u8>> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or(VaultError::NoVault)?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;
    read_image(&abs_path)
}

#[cfg(test)]
mod tests;

//! Attachments: images pasted or dropped into notes are stored in the vault, named, deduplicated by content, and linked relative to the note.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::state::AppState;
use crate::vault::{VaultError, VaultResult, resolve_vault_path_checked, write_bytes};

/// Where new attachments go.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AttachmentFolder {
    /// `attachments/` at the vault root.
    #[default]
    VaultRoot,
    /// `attachments/` beside the note they are added to.
    BesideNote,
    /// A vault-relative folder.
    Custom(String),
}

impl AttachmentFolder {
    /// Parse the stored setting: `root`, `beside-note`, or a folder path.
    pub fn from_setting(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            None | Some("") | Some("root") => Self::VaultRoot,
            Some("beside-note") => Self::BesideNote,
            Some(folder) => Self::Custom(folder.trim_matches('/').to_string()),
        }
    }
}

/// A stored attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    /// Vault-relative path, `/`-separated.
    pub rel_path: String,
    /// True when identical bytes were already in the folder and were reused.
    pub reused: bool,
}

/// Vault-relative folder for attachments of `note` (vault-relative, if any).
pub fn attachments_dir(folder: &AttachmentFolder, note: Option<&str>) -> String {
    match folder {
        AttachmentFolder::VaultRoot => "attachments".to_string(),
        AttachmentFolder::BesideNote => match note.and_then(|n| Path::new(n).parent()) {
            Some(parent) if !parent.as_os_str().is_empty() => {
                format!("{}/attachments", to_slash(parent))
            }
            _ => "attachments".to_string(),
        },
        AttachmentFolder::Custom(dir) if dir.is_empty() => "attachments".to_string(),
        AttachmentFolder::Custom(dir) => dir.clone(),
    }
}

/// File name for a pasted bitmap: `Pasted image 2026-11-23 14-05-31.png`.
pub fn pasted_image_name(stamp: &str) -> String {
    format!("Pasted image {stamp}.png")
}

/// Store `bytes` as `file_name` in the attachments folder. Identical bytes
/// already in that folder are reused rather than duplicated; a different file
/// with the same name gets ` 2`, ` 3`… before the extension.
pub fn store_bytes(
    state: &AppState,
    folder: &AttachmentFolder,
    note: Option<&str>,
    file_name: &str,
    bytes: &[u8],
) -> VaultResult<Stored> {
    let root = vault_root(state)?;
    let dir_rel = attachments_dir(folder, note);
    let dir_abs = resolve_vault_path_checked(&root, &dir_rel)?;

    if let Some(existing) = find_identical(&dir_abs, bytes) {
        return Ok(Stored {
            rel_path: format!("{dir_rel}/{existing}"),
            reused: true,
        });
    }

    let name = unique_name(&dir_abs, &sanitize_file_name(file_name));
    let rel_path = format!("{dir_rel}/{name}");
    let abs = resolve_vault_path_checked(&root, &rel_path)?;
    write_bytes(&abs, bytes)?;
    Ok(Stored {
        rel_path,
        reused: false,
    })
}

/// Encode an RGBA bitmap (a pasted screenshot) as PNG and store it.
pub fn store_bitmap(
    state: &AppState,
    folder: &AttachmentFolder,
    note: Option<&str>,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    stamp: &str,
) -> VaultResult<Stored> {
    let image = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or("Clipboard image has an unexpected size")?;
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| VaultError::from(format!("Could not encode the image: {e}")))?;
    store_bytes(state, folder, note, &pasted_image_name(stamp), &png)
}

/// Bring an existing file into the vault: a file already inside the vault is
/// linked where it is; one outside is copied into the attachments folder
/// under its own name.
pub fn import_file(
    state: &AppState,
    folder: &AttachmentFolder,
    note: Option<&str>,
    source: &Path,
) -> VaultResult<Stored> {
    let root = vault_root(state)?;
    let source = fs::canonicalize(source)
        .map_err(|e| VaultError::from(format!("Cannot read {}: {e}", source.display())))?;
    let canonical_root = fs::canonicalize(&root).unwrap_or(root);
    if let Ok(rel) = source.strip_prefix(&canonical_root) {
        return Ok(Stored {
            rel_path: to_slash(rel),
            reused: true,
        });
    }
    let bytes = fs::read(&source)
        .map_err(|e| VaultError::from(format!("Cannot read {}: {e}", source.display())))?;
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".to_string());
    store_bytes(state, folder, note, &name, &bytes)
}

/// Markdown that embeds `target` (vault-relative) from `note`
/// (vault-relative): `![](../attachments/a.png)`. Paths with spaces are
/// wrapped in `<…>`, as CommonMark requires.
pub fn image_markdown(note: Option<&str>, target: &str) -> String {
    let from_dir = note
        .and_then(|n| Path::new(n).parent())
        .map(to_slash)
        .unwrap_or_default();
    let rel = relative_path(&from_dir, target);
    if rel.contains(' ') {
        format!("![](<{rel}>)")
    } else {
        format!("![]({rel})")
    }
}

/// `target` relative to the folder `from_dir` (both vault-relative, `/`).
fn relative_path(from_dir: &str, target: &str) -> String {
    let from: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend(&to[common..]);
    parts.join("/")
}

fn vault_root(state: &AppState) -> VaultResult<PathBuf> {
    let guard = state.vault_root.lock().map_err(|e| e.to_string())?;
    guard
        .clone()
        .ok_or_else(|| VaultError::from("Open a vault first"))
}

fn to_slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// A name of a file in `dir` with exactly `bytes`, if any.
fn find_identical(dir: &Path, bytes: &[u8]) -> Option<String> {
    let want = Sha256::digest(bytes);
    fs::read_dir(dir).ok()?.flatten().find_map(|entry| {
        let path = entry.path();
        let meta = entry.metadata().ok()?;
        if !meta.is_file() || meta.len() != bytes.len() as u64 {
            return None;
        }
        let have = Sha256::digest(fs::read(&path).ok()?);
        (have == want).then(|| entry.file_name().to_string_lossy().into_owned())
    })
}

/// `name` if free in `dir`, else `stem 2.ext`, `stem 3.ext`…
fn unique_name(dir: &Path, name: &str) -> String {
    if !dir.join(name).exists() {
        return name.to_string();
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| format!("{stem} {n}{ext}"))
        .find(|candidate| !dir.join(candidate).exists())
        .expect("an unbounded counter finds a free name")
}

/// Drop path separators and characters that break Markdown link targets.
fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | '<' | '>' | '(' | ')' | '[' | ']' | ':' | '*' | '?' | '"' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    if trimmed.is_empty() {
        "image".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests;

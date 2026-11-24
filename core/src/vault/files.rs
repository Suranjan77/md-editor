//! Filesystem helpers: path resolution and containment, atomic writes, file-type checks and vault listing.

use super::*;

// ── Internal helpers ────────────────────────────────────────────────

/// Resolve a vault-relative path to an absolute path, guaranteed to stay
/// within `vault_root`.
///
/// The relative path is normalized lexically: `.` is dropped, `..` pops a
/// component, and any attempt to escape the root (leading `..` or an absolute
/// path) is clamped so the result can never point outside the vault. Use
/// [`resolve_vault_path_checked`] when an escape attempt should be a hard
/// error rather than silently clamped.
pub fn resolve_vault_path(vault_root: &Path, relative_path: &str) -> PathBuf {
    normalize_within_root(vault_root, relative_path).0
}

/// Like [`resolve_vault_path`] but returns an error if `relative_path` tries to
/// escape the vault root. Use for filesystem mutations (save/create/delete/
/// rename) and reads where operating on the wrong file would be harmful.
pub fn resolve_vault_path_checked(
    vault_root: &Path,
    relative_path: &str,
) -> Result<PathBuf, String> {
    let (resolved, escaped) = normalize_within_root(vault_root, relative_path);
    if escaped {
        return Err(format!("Path escapes the vault root: {relative_path}"));
    }
    let canonical_root = fs::canonicalize(vault_root)
        .map_err(|e| format!("Failed to resolve vault root {}: {e}", vault_root.display()))?;
    let mut existing = resolved.as_path();
    while fs::symlink_metadata(existing).is_err() {
        existing = existing.parent().ok_or_else(|| {
            format!(
                "Could not resolve an existing ancestor for {}",
                resolved.display()
            )
        })?;
    }
    let canonical_existing = fs::canonicalize(existing).map_err(|e| {
        format!(
            "Failed to resolve path inside vault {}: {e}",
            existing.display()
        )
    })?;
    if !canonical_existing.starts_with(&canonical_root) {
        return Err(format!(
            "Path resolves outside the vault root: {relative_path}"
        ));
    }
    Ok(resolved)
}

/// Lexically normalize `relative_path` against `vault_root`. Returns the
/// clamped absolute path and whether the input attempted to escape the root.
pub(super) fn normalize_within_root(vault_root: &Path, relative_path: &str) -> (PathBuf, bool) {
    use std::path::Component;

    let mut stack: Vec<std::ffi::OsString> = Vec::new();
    let mut escaped = false;

    for component in Path::new(relative_path).components() {
        match component {
            Component::Normal(c) => stack.push(c.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                if stack.pop().is_none() {
                    escaped = true;
                }
            }
            // An absolute path supplied where a relative one was expected is
            // an escape attempt; ignore the anchor and keep building under root.
            Component::RootDir | Component::Prefix(_) => escaped = true,
        }
    }

    let mut resolved = vault_root.to_path_buf();
    for c in stack {
        resolved.push(c);
    }
    (resolved, escaped)
}

/// File size in bytes and modified time as a Unix timestamp (seconds), used as
/// the PDF text cache key. Returns `(0, 0)` when the file can't be stat'd, which
/// simply forces a fresh extraction.
pub(super) fn file_size_and_mtime(path: &Path) -> (u64, i64) {
    let Ok(meta) = fs::metadata(path) else {
        return (0, 0);
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    (meta.len(), mtime)
}

pub(super) fn read_file(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("Failed to read file {}: {}", path.display(), e))
}

pub(super) fn read_image(path: &Path) -> Result<Vec<u8>, String> {
    if !is_image_path(path) {
        return Err(format!("Not an image: {}", path.display()));
    }
    fs::read(path).map_err(|e| format!("Failed to read image {}: {}", path.display(), e))
}

/// Write `content` to `path` atomically.
///
/// A plain `fs::write` truncates the destination before it writes, so a crash
/// or power loss part-way through leaves a half-written note where a good one
/// used to be. Instead the content goes to a temporary file in the *same*
/// directory (so the final step is a same-filesystem rename), is flushed to
/// disk with `sync_all`, and only then replaces the destination. `rename` is
/// atomic, so a reader either sees the whole old file or the whole new one —
/// never a truncated mix.
///
/// The parent directory is fsynced afterwards where possible so the rename
/// itself survives a power loss, not just the file contents.
///
/// Two properties a plain `fs::write` gave for free have to be restored by
/// hand, because replacing a directory entry is not the same as writing
/// through one:
///
/// - A symlinked note is resolved first, so the target is rewritten rather
///   than the link being replaced by a regular file.
/// - The destination's existing permissions are copied onto the replacement,
///   so a note kept at `0600` does not come back world-readable.
/// - A read-only destination is refused. The rename would otherwise succeed
///   (it needs only a writable folder) and silently replace a file the user
///   protected.
pub fn write_file(path: &Path, content: &str) -> Result<(), String> {
    use std::io::Write;

    // Write through a symlink, not over it.
    let path = &fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    fs::create_dir_all(&parent).map_err(|e| format!("Failed to create directory: {}", e))?;

    // Same directory as the destination, so the rename below cannot cross a
    // filesystem boundary. The name is unique per write so concurrent saves of
    // different files never collide on it.
    let tmp_path = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unnamed".to_string()),
        uuid::Uuid::new_v4().simple()
    ));

    // Inherit the destination's permissions where it already exists; a fresh
    // file keeps the default the platform would have given it.
    let existing_permissions = fs::metadata(path).map(|m| m.permissions()).ok();
    if existing_permissions.as_ref().is_some_and(|p| p.readonly()) {
        return Err(format!("{} is read-only", path.display()));
    }

    let write_result = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(content.as_bytes())?;
        if let Some(permissions) = existing_permissions.clone() {
            file.set_permissions(permissions)?;
        }
        file.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("Failed to write file {}: {}", path.display(), e));
    }

    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("Failed to replace file {}: {}", path.display(), e));
    }

    // Best-effort: persist the directory entry itself. Not all platforms allow
    // opening a directory for sync, so a failure here is not fatal.
    if let Ok(dir) = fs::File::open(&parent) {
        let _ = dir.sync_all();
    }

    Ok(())
}

pub fn is_image(ext: &str) -> bool {
    IMAGE_EXTENSIONS
        .iter()
        .any(|candidate| ext.eq_ignore_ascii_case(candidate))
}

pub fn ext_matches(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extensions
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

pub fn is_markdown_path(path: &Path) -> bool {
    ext_matches(path, &MARKDOWN_EXTENSIONS)
}

pub fn is_pdf_path(path: &Path) -> bool {
    ext_matches(path, &PDF_EXTENSIONS)
}

pub fn is_image_path(path: &Path) -> bool {
    ext_matches(path, &IMAGE_EXTENSIONS)
}

pub fn is_pptx_path(path: &Path) -> bool {
    ext_matches(path, &PPTX_EXTENSIONS)
}

pub fn is_ink_path(path: &Path) -> bool {
    ext_matches(path, &INK_EXTENSIONS)
}

pub fn is_supported_vault_path(path: &Path) -> bool {
    is_markdown_path(path)
        || is_pdf_path(path)
        || is_pptx_path(path)
        || is_image_path(path)
        || is_ink_path(path)
}

/// Every supported file and folder under `root`, as the sidebar lists them
/// (dot/heavy folders and symlinks skipped). Directories first, then by name.
pub fn list_vault_entries(root: &Path) -> Result<Vec<FileEntry>, String> {
    let mut entries = Vec::new();
    list_vault_recursive(root, root, &mut entries, 0)?;
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(entries)
}

pub(super) fn list_vault_recursive(
    root: &Path,
    dir: &Path,
    entries: &mut Vec<FileEntry>,
    depth: usize,
) -> Result<(), String> {
    if depth >= MAX_WALK_DEPTH {
        return Ok(());
    }
    let read_dir = fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            // Skip excluded/dot directories and never follow directory
            // symlinks (which could form a cycle).
            if is_excluded_dir(&name) || is_symlink(&path) {
                continue;
            }
            entries.push(FileEntry {
                path: path_to_relative_string(&path, root),
                name,
                is_dir: true,
            });
            list_vault_recursive(root, &path, entries, depth + 1)?;
        } else if name.starts_with('.') || is_symlink(&path) {
            continue;
        } else if is_supported_vault_path(&path) {
            entries.push(FileEntry {
                path: path_to_relative_string(&path, root),
                name,
                is_dir: false,
            });
        }
    }

    Ok(())
}

pub(super) fn path_to_relative_string(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Convert a FileIndex/disk path to the stable slash-separated graph id.
/// Paths outside the active vault are ignored rather than leaking an absolute
/// filesystem path into the snapshot.
pub(super) fn graph_relative_path(path: &Path, root: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    Some(relative.to_string_lossy().replace('\\', "/"))
}

/// Normalize paths persisted in SQLite. Current records are vault-relative,
/// but accepting absolute in-vault paths and legacy backslashes makes graph
/// snapshots resilient to older sidecars and cross-platform vault moves.
pub(super) fn normalize_stored_graph_path(root: &Path, stored: &str) -> Option<String> {
    let stored = stored.trim().replace('\\', "/");
    if stored.is_empty() {
        return None;
    }
    let path = Path::new(&stored);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let (resolved, escaped) = normalize_within_root(root, &stored);
        if escaped {
            return None;
        }
        resolved
    };
    graph_relative_path(&absolute, root)
}

pub(super) fn graph_node_kind(
    path: &str,
    markdown_paths: &BTreeSet<String>,
    pdf_paths: &BTreeSet<String>,
) -> GraphNodeKind {
    if markdown_paths.contains(path) {
        GraphNodeKind::Markdown
    } else if pdf_paths.contains(path) {
        GraphNodeKind::Pdf
    } else {
        GraphNodeKind::Missing
    }
}

pub(super) fn graph_node_label(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .or_else(|| Path::new(path).file_name())
        .map(|name| name.to_string_lossy().to_string())
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| path.to_string())
}

pub fn list_all_md_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    list_all_md_files_recursive(root, &mut files, 0)?;
    Ok(files)
}

pub(super) fn list_all_md_files_recursive(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    depth: usize,
) -> Result<(), String> {
    list_files_matching(dir, files, depth, &is_markdown_path)
}

/// List every `.pdf` file in the vault, applying the same exclusion/symlink/
/// depth guards as the markdown walker.
pub fn list_all_pdf_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    list_files_matching(root, &mut files, 0, &is_pdf_path)?;
    Ok(files)
}

/// Recursively collect files whose extension satisfies `keep`, skipping
/// dotfiles, excluded/dot directories, and directory symlinks, bounded by
/// [`MAX_WALK_DEPTH`].
pub(super) fn list_files_matching(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    depth: usize,
    keep: &dyn Fn(&Path) -> bool,
) -> Result<(), String> {
    if depth >= MAX_WALK_DEPTH {
        return Ok(());
    }
    let read_dir = fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            if is_excluded_dir(&name) || is_symlink(&path) {
                continue;
            }
            list_files_matching(&path, files, depth + 1, keep)?;
        } else if name.starts_with('.') || is_symlink(&path) {
            continue;
        } else if keep(&path) {
            files.push(path);
        }
    }
    Ok(())
}

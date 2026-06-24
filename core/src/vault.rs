use std::fs;
use std::path::{Path, PathBuf};

use crate::file_index::FileIndex;
use crate::state::AppState;
use crate::types::{BacklinkItem, BacklinkTarget, FileEntry, SearchResult};

const IMAGE_EXTENSIONS: [&str; 6] = ["jpeg", "jpg", "png", "svg", "webp", "avif"];

/// Directory names that are never worth indexing or showing in the tree. These
/// are heavy or irrelevant in note vaults and would otherwise blow up indexing
/// time and the file listing. Dotfiles (`.git`, `.obsidian`, …) are skipped
/// separately by the `starts_with('.')` check in the walkers.
const EXCLUDED_DIRS: [&str; 6] = [
    "node_modules",
    "target",
    "build",
    "dist",
    "__pycache__",
    ".trash",
];

/// Maximum directory depth the vault walkers descend. Guards against
/// pathologically deep trees (and, together with the symlink check, against
/// runaway recursion) without affecting any realistic vault layout.
const MAX_WALK_DEPTH: usize = 32;

/// Whether a directory should be skipped during indexing/listing: dotfolders
/// and the well-known heavy directories in [`EXCLUDED_DIRS`].
fn is_excluded_dir(name: &str) -> bool {
    name.starts_with('.') || EXCLUDED_DIRS.contains(&name)
}

/// Whether a directory entry is a symlink. Symlinked directories are not
/// followed during walks so a symlink cycle can never make indexing hang.
fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).map_or(false, |m| m.file_type().is_symlink())
}

// ── Public API ──────────────────────────────────────────────────────

/// Set the vault root directory and index all markdown files.
/// Returns the file listing for the vault.
pub fn set_vault_root(state: &AppState, path: &str) -> Result<Vec<FileEntry>, String> {
    let root = PathBuf::from(path);
    if !root.is_dir() {
        return Err(format!("Not a directory: {}", path));
    }

    // Phase 1: read every file and build the link index into local structures
    // WITHOUT holding any shared lock. The disk I/O here is the slow part; if
    // it ran while holding `file_index`/`db`, the UI thread would block the
    // moment it touched the index (e.g. opening a file), reintroducing the
    // freeze this off-thread indexing is meant to avoid.
    let md_files = list_all_md_files(&root)?;
    let mut index = FileIndex::new(root.clone());
    // (relative path, content) for the FTS rebuild.
    let mut indexed: Vec<(String, String)> = Vec::with_capacity(md_files.len());
    // (absolute path, content) for the two-pass link-graph rebuild, which needs
    // every file's name registered before it can resolve bare links to files in
    // other subfolders.
    let mut for_graph: Vec<(PathBuf, String)> = Vec::with_capacity(md_files.len());
    for file_path in md_files {
        if let Ok(content) = read_file(&file_path) {
            let rel_path = file_path
                .strip_prefix(&root)
                .unwrap_or(&file_path)
                .to_string_lossy()
                .to_string();
            indexed.push((rel_path, content.clone()));
            for_graph.push((file_path, content));
        }
    }
    index.rebuild(&for_graph);

    // Phase 1b: extract PDF text for full-text search. Best-effort and done
    // without holding any lock (pdfium extraction is slow); skipped entirely
    // when no renderer is available (e.g. headless/test builds).
    let mut pdf_indexed: Vec<(String, String)> = Vec::new();
    if let Some(renderer) = state.pdf_renderer.as_ref() {
        if let Ok(pdf_files) = list_all_pdf_files(&root) {
            for pdf_path in pdf_files {
                let rel_path = pdf_path
                    .strip_prefix(&root)
                    .unwrap_or(&pdf_path)
                    .to_string_lossy()
                    .to_string();
                let (file_size, modified_at) = file_size_and_mtime(&pdf_path);

                // Reuse cached text when the PDF is unchanged; only fall back to
                // the (slow) pdfium extraction when size/mtime differ.
                let text = if let Some(cached) =
                    state.get_cached_pdf_text(&rel_path, file_size, modified_at)
                {
                    cached
                } else {
                    match renderer.extract_document_text(&pdf_path.to_string_lossy()) {
                        Ok(text) => {
                            // Cache even empty results (e.g. scanned PDFs) so we
                            // don't re-extract them on every open.
                            state.put_cached_pdf_text(&rel_path, file_size, modified_at, &text);
                            text
                        }
                        Err(_) => continue,
                    }
                };
                if !text.trim().is_empty() {
                    pdf_indexed.push((rel_path, text));
                }
            }
        }
    }

    // Phase 2: publish results under short-lived locks only.
    {
        let mut vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
        *vault_root = Some(root.clone());
    }
    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        // Rebuild the FTS index atomically: a single transaction is far faster
        // than per-row autocommit and prevents a half-rebuilt index if
        // indexing is interrupted partway through.
        let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM file_search", [])
            .map_err(|e| e.to_string())?;
        for (rel_path, content) in indexed.iter().chain(pdf_indexed.iter()) {
            if let Err(e) = tx.execute(
                "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
                rusqlite::params![rel_path, content],
            ) {
                eprintln!("Failed to index {rel_path} for search: {e}");
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
    }
    {
        let mut file_index = state.file_index.lock().map_err(|e| e.to_string())?;
        *file_index = index;
    }

    list_vault_entries(&root)
}

/// Open a file from the vault. Returns raw bytes.
pub fn open_file(state: &AppState, path: &str) -> Result<Vec<u8>, String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;

    if abs_path
        .extension()
        .map_or(false, |e| is_image(e.to_str().unwrap_or("")))
    {
        read_image(&abs_path)
    } else {
        let content = read_file(&abs_path)?;
        let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
        index.update_file(&abs_path, &content);
        Ok(content.into_bytes())
    }
}

/// Save file content.
pub fn save_file(state: &AppState, path: &str, content: &str) -> Result<(), String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;
    write_file(&abs_path, content)?;

    let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
    index.update_file(&abs_path, content);

    let db = state.db.lock().map_err(|e| e.to_string())?;
    if let Err(e) = db.execute(
        "DELETE FROM file_search WHERE path = ?1",
        rusqlite::params![path],
    ) {
        eprintln!("Failed to clear stale search index for {path}: {e}");
    }
    if let Err(e) = db.execute(
        "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
        rusqlite::params![path, content],
    ) {
        eprintln!("Failed to update search index for {path}: {e}");
    }

    Ok(())
}

/// Reconcile the link/search index with the on-disk state of a single
/// vault-relative markdown path after an *external* change (a filesystem watcher
/// event, a git pull, another editor). Existing files are re-read and
/// re-indexed; vanished files are removed from the index and search. Non-
/// markdown paths are ignored. Safe to call redundantly — it's idempotent.
pub fn sync_path_from_disk(state: &AppState, rel_path: &str) -> Result<(), String> {
    let vault_root = {
        let guard = state.vault_root.lock().map_err(|e| e.to_string())?;
        guard.as_ref().ok_or("No vault root set")?.clone()
    };
    let abs = resolve_vault_path_checked(&vault_root, rel_path)?;
    let is_md = abs.extension().map_or(false, |e| e == "md" || e == "markdown");
    if !is_md {
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
pub fn create_file(state: &AppState, path: &str) -> Result<(), String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;
    if abs_path.exists() {
        return Err(format!("File already exists: {}", abs_path.display()));
    }
    write_file(&abs_path, "")
}

/// Create a new directory.
pub fn create_dir(state: &AppState, path: &str) -> Result<(), String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;
    if abs_path.exists() {
        return Err(format!("Directory already exists: {}", abs_path.display()));
    }
    fs::create_dir_all(&abs_path)
        .map_err(|e| format!("Failed to create directory {}: {}", abs_path.display(), e))
}

/// Rename (or move) a file or directory.
///
/// When a markdown file is renamed, every `[[wikilink]]` that pointed at it is
/// rewritten in the files that linked to it, so backlinks survive the rename.
/// The link graph and full-text index are updated for both the renamed file and
/// each file whose links were rewritten.
pub fn rename_entry(state: &AppState, old_path: &str, new_path: &str) -> Result<(), String> {
    let vault_root = {
        let guard = state.vault_root.lock().map_err(|e| e.to_string())?;
        guard.as_ref().ok_or("No vault root set")?.clone()
    };
    let abs_old = resolve_vault_path_checked(&vault_root, old_path)?;
    let abs_new = resolve_vault_path_checked(&vault_root, new_path)?;

    if abs_new.exists() {
        return Err(format!("Target already exists: {}", abs_new.display()));
    }

    let is_md_file = abs_old.is_file() && abs_old.extension().map_or(false, |e| e == "md");

    // Snapshot the files that link to this note *before* mutating the index —
    // these are the ones whose `[[wikilinks]]` need rewriting.
    let backlinks: Vec<PathBuf> = if is_md_file {
        let index = state.file_index.lock().map_err(|e| e.to_string())?;
        index.get_backlinks(&abs_old)
    } else {
        Vec::new()
    };

    // Do the physical rename first; if it fails, nothing else has changed.
    fs::rename(&abs_old, &abs_new)
        .map_err(|e| format!("Failed to rename {}: {}", abs_old.display(), e))?;

    if !is_md_file {
        return Ok(());
    }

    // New link target: vault-relative, forward slashes, no extension.
    let new_rel = abs_new
        .strip_prefix(&vault_root)
        .unwrap_or(&abs_new)
        .with_extension("");
    let new_rel_str = new_rel.to_string_lossy().replace('\\', "/");

    // Rewrite links in each backlinking file that resolved to the old path.
    for bl in &backlinks {
        if bl == &abs_old {
            continue;
        }
        let Ok(content) = read_file(bl) else { continue };
        let Some(updated) =
            crate::file_index::rewrite_links_to(&content, &vault_root, bl, &abs_old, &new_rel_str)
        else {
            continue;
        };
        if write_file(bl, &updated).is_err() {
            continue;
        }
        {
            let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
            index.update_file(bl, &updated);
        }
        let bl_rel = path_to_relative_string(bl, &vault_root);
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let _ = db.execute(
            "DELETE FROM file_search WHERE path = ?1",
            rusqlite::params![bl_rel],
        );
        if let Err(e) = db.execute(
            "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
            rusqlite::params![bl_rel, updated],
        ) {
            eprintln!("Failed to reindex {bl_rel} after rename: {e}");
        }
    }

    // Re-key the renamed file in the link graph and search index.
    {
        let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
        index.remove_file(&abs_old);
        if let Ok(content) = read_file(&abs_new) {
            index.update_file(&abs_new, &content);
            drop(index);
            let db = state.db.lock().map_err(|e| e.to_string())?;
            let _ = db.execute(
                "DELETE FROM file_search WHERE path = ?1",
                rusqlite::params![old_path],
            );
            let _ = db.execute(
                "DELETE FROM file_search WHERE path = ?1",
                rusqlite::params![new_path],
            );
            if let Err(e) = db.execute(
                "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
                rusqlite::params![new_path, content],
            ) {
                eprintln!("Failed to index renamed file {new_path}: {e}");
            }
        }
    }

    Ok(())
}

/// Delete a file or directory.
pub fn delete_entry(state: &AppState, path: &str) -> Result<(), String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;

    if abs_path.is_file() {
        let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
        index.remove_file(&abs_path);
        let db = state.db.lock().map_err(|e| e.to_string())?;
        if let Err(e) = db.execute(
            "DELETE FROM file_search WHERE path = ?1",
            rusqlite::params![path],
        ) {
            eprintln!("Failed to remove {path} from search index: {e}");
        }
    }

    if abs_path.is_dir() {
        fs::remove_dir_all(&abs_path)
            .map_err(|e| format!("Failed to delete directory {}: {}", abs_path.display(), e))
    } else {
        fs::remove_file(&abs_path)
            .map_err(|e| format!("Failed to delete file {}: {}", abs_path.display(), e))
    }
}

/// List all entries in the vault.
pub fn list_vault(state: &AppState) -> Result<Vec<FileEntry>, String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    list_vault_entries(vault_root)
}

/// Full-text search across the vault using FTS5.
pub fn search_vault(state: &AppState, query: &str) -> Result<Vec<SearchResult>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let fts_query = format!("\"{}\"", query.replace('"', "\"\""));

    let mut stmt = db
        .prepare(
            "SELECT path, snippet(file_search, 1, '<b>', '</b>', '...', 15) FROM file_search WHERE content MATCH ?1 ORDER BY rank LIMIT 100",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(rusqlite::params![&fts_query], |row| {
            Ok(SearchResult {
                path: row.get(0)?,
                line: 1,
                context: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    for row in rows {
        if let Ok(r) = row {
            results.push(r);
        }
    }
    Ok(results)
}

/// Get backlinks for a file.
pub fn get_backlinks(state: &AppState, path: &str) -> Result<Vec<String>, String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path(vault_root, path);

    let index = state.file_index.lock().map_err(|e| e.to_string())?;
    let backlinks = index.get_backlinks(&abs_path);

    Ok(backlinks
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect())
}

/// Get mixed backlinks (markdown files, PDF documents, and PDF annotations).
pub fn get_mixed_backlinks(state: &AppState, path: &str) -> Result<Vec<BacklinkItem>, String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;

    let lower_path = path.to_lowercase();
    let mut results = Vec::new();

    if lower_path.ends_with(".pdf") {
        // PDF Case:
        // 1. Get incoming backlinks from FileIndex (markdown files linking to this PDF)
        let abs_path = resolve_vault_path(vault_root, path);
        let index = state.file_index.lock().map_err(|e| e.to_string())?;
        let backlinks = index.get_backlinks(&abs_path);
        for bl in backlinks {
            let rel_path = path_to_relative_string(&bl, vault_root);
            let name = bl
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| rel_path.clone());
            results.push(BacklinkItem {
                source: BacklinkTarget::MarkdownFile { path: rel_path },
                label: name,
                context: None,
            });
        }

        // 2. Query notes linked from PDF annotations of this PDF document
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let mut stmt = db
            .prepare("SELECT document_id FROM pdf_documents WHERE vault_relative_path = ?1")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query([path]).map_err(|e| e.to_string())?;
        if let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let doc_id: String = row.get(0).map_err(|e| e.to_string())?;

            let mut stmt2 = db
                .prepare(
                    "SELECT linked_note_path, selected_text FROM pdf_annotations
                     WHERE document_id = ?1 AND linked_note_path IS NOT NULL AND linked_note_path != ''",
                )
                .map_err(|e| e.to_string())?;
            let mut rows2 = stmt2.query([doc_id]).map_err(|e| e.to_string())?;
            while let Some(row2) = rows2.next().map_err(|e| e.to_string())? {
                let note_path: String = row2.get(0).map_err(|e| e.to_string())?;
                let selected_text: String = row2.get(1).map_err(|e| e.to_string())?;

                let note_name = Path::new(&note_path)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| note_path.clone());

                results.push(BacklinkItem {
                    source: BacklinkTarget::MarkdownFile { path: note_path },
                    label: note_name,
                    context: Some(selected_text),
                });
            }
        }
    } else {
        // Markdown Case:
        // 1. Standard incoming backlinks from FileIndex
        let abs_path = resolve_vault_path(vault_root, path);
        let index = state.file_index.lock().map_err(|e| e.to_string())?;
        let backlinks = index.get_backlinks(&abs_path);
        for bl in backlinks {
            let rel_path = path_to_relative_string(&bl, vault_root);
            let name = bl
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| rel_path.clone());
            results.push(BacklinkItem {
                source: BacklinkTarget::MarkdownFile { path: rel_path },
                label: name,
                context: None,
            });
        }

        // 2. Query annotations from SQLite referencing this note (linked_note_path)
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let mut stmt = db
            .prepare(
                "SELECT a.id, a.page_index, a.selected_text, d.vault_relative_path
                 FROM pdf_annotations a
                 JOIN pdf_documents d ON a.document_id = d.document_id
                 WHERE a.linked_note_path = ?1",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query([path]).map_err(|e| e.to_string())?;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let ann_id: String = row.get(0).map_err(|e| e.to_string())?;
            let page_idx: i32 = row.get(1).map_err(|e| e.to_string())?;
            let selected_text: String = row.get(2).map_err(|e| e.to_string())?;
            let doc_path: String = row.get(3).map_err(|e| e.to_string())?;

            results.push(BacklinkItem {
                source: BacklinkTarget::PdfAnnotation {
                    document_path: doc_path,
                    annotation_id: ann_id,
                    page: (page_idx + 1) as u16,
                },
                label: format!("Page {} highlight", page_idx + 1),
                context: Some(selected_text),
            });
        }
    }

    Ok(results)
}

/// Read raw image bytes from the vault.
pub fn read_vault_image(state: &AppState, path: &str) -> Result<Vec<u8>, String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path_checked(vault_root, path)?;
    read_image(&abs_path)
}

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
    Ok(resolved)
}

/// Lexically normalize `relative_path` against `vault_root`. Returns the
/// clamped absolute path and whether the input attempted to escape the root.
fn normalize_within_root(vault_root: &Path, relative_path: &str) -> (PathBuf, bool) {
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
fn file_size_and_mtime(path: &Path) -> (u64, i64) {
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

fn read_file(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("Failed to read file {}: {}", path.display(), e))
}

fn read_image(path: &Path) -> Result<Vec<u8>, String> {
    if !path
        .extension()
        .map_or(false, |e| is_image(e.to_str().unwrap_or("")))
    {
        return Err(format!("Not an image: {}", path.display()));
    }
    fs::read(path).map_err(|e| format!("Failed to read image {}: {}", path.display(), e))
}

fn write_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {}", e))?;
    }
    fs::write(path, content).map_err(|e| format!("Failed to write file {}: {}", path.display(), e))
}

pub fn is_image(ext: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&ext)
}

fn list_vault_entries(root: &Path) -> Result<Vec<FileEntry>, String> {
    let mut entries = Vec::new();
    list_vault_recursive(root, root, &mut entries, 0)?;
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(entries)
}

fn list_vault_recursive(
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
        } else if name.starts_with('.') {
            continue;
        } else if path
            .extension()
            .map(|e| {
                e == "md" || e == "markdown" || e == "pdf" || is_image(e.to_str().unwrap_or(""))
            })
            .unwrap_or(false)
        {
            entries.push(FileEntry {
                path: path_to_relative_string(&path, root),
                name,
                is_dir: false,
            });
        }
    }

    Ok(())
}

fn path_to_relative_string(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn list_all_md_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    list_all_md_files_recursive(root, &mut files, 0)?;
    Ok(files)
}

fn list_all_md_files_recursive(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    depth: usize,
) -> Result<(), String> {
    list_files_matching(dir, files, depth, &|ext| ext == "md" || ext == "markdown")
}

/// List every `.pdf` file in the vault, applying the same exclusion/symlink/
/// depth guards as the markdown walker.
pub fn list_all_pdf_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    list_files_matching(root, &mut files, 0, &|ext| ext == "pdf")?;
    Ok(files)
}

/// Recursively collect files whose extension satisfies `keep`, skipping
/// dotfiles, excluded/dot directories, and directory symlinks, bounded by
/// [`MAX_WALK_DEPTH`].
fn list_files_matching(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    depth: usize,
    keep: &dyn Fn(&str) -> bool,
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
        } else if name.starts_with('.') {
            continue;
        } else if path.extension().and_then(|e| e.to_str()).map_or(false, keep) {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_normal_paths_stay_within_root() {
        let root = Path::new("/vault");
        assert_eq!(resolve_vault_path(root, "notes/a.md"), PathBuf::from("/vault/notes/a.md"));
        assert_eq!(resolve_vault_path(root, "./a.md"), PathBuf::from("/vault/a.md"));
        // Interior `..` that stays inside the vault is allowed.
        assert_eq!(
            resolve_vault_path(root, "notes/../a.md"),
            PathBuf::from("/vault/a.md")
        );
    }

    #[test]
    fn checked_rejects_traversal_escapes() {
        let root = Path::new("/vault");
        assert!(resolve_vault_path_checked(root, "../etc/passwd").is_err());
        assert!(resolve_vault_path_checked(root, "notes/../../secret").is_err());
        assert!(resolve_vault_path_checked(root, "/etc/passwd").is_err());
        assert!(resolve_vault_path_checked(root, "notes/a.md").is_ok());
    }

    #[test]
    fn walk_skips_excluded_and_dot_dirs() {
        let base = std::env::temp_dir().join(format!("md_walk_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(base.join("notes")).unwrap();
        fs::create_dir_all(base.join("node_modules/pkg")).unwrap();
        fs::create_dir_all(base.join(".obsidian")).unwrap();
        fs::write(base.join("notes/a.md"), "a").unwrap();
        fs::write(base.join("node_modules/pkg/b.md"), "b").unwrap();
        fs::write(base.join(".obsidian/c.md"), "c").unwrap();

        let files = list_all_md_files(&base).unwrap();
        assert_eq!(files.len(), 1, "only notes/a.md should be indexed");
        assert!(files[0].ends_with("notes/a.md"));

        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn walk_does_not_follow_symlink_cycles() {
        let base = std::env::temp_dir().join(format!("md_symlink_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(base.join("real")).unwrap();
        fs::write(base.join("real/a.md"), "a").unwrap();
        // A symlink pointing back at the vault root would loop forever if followed.
        std::os::unix::fs::symlink(&base, base.join("real/loop")).unwrap();

        // Must terminate and find the single real file.
        let files = list_all_md_files(&base).unwrap();
        assert_eq!(files.len(), 1);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn resolve_clamps_escapes_into_root() {
        let root = Path::new("/vault");
        // Even the non-checked variant must never escape the root.
        let resolved = resolve_vault_path(root, "../../etc/passwd");
        assert!(resolved.starts_with("/vault"));
        let resolved_abs = resolve_vault_path(root, "/etc/passwd");
        assert!(resolved_abs.starts_with("/vault"));
    }
}

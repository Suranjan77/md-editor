use std::fs;
use std::path::{Path, PathBuf};

use crate::file_index::FileIndex;
use crate::state::AppState;
use crate::types::{BacklinkItem, BacklinkTarget, FileEntry, SearchResult, SearchResultGroup, UnifiedSearchResult};

const IMAGE_EXTENSIONS: [&str; 6] = ["jpeg", "jpg", "png", "svg", "webp", "avif"];

// ── Public API ──────────────────────────────────────────────────────

/// Set the vault root directory and index all markdown files.
/// Returns the file listing for the vault.
pub fn set_vault_root(state: &AppState, path: &str) -> Result<Vec<FileEntry>, String> {
    let root = PathBuf::from(path);
    if !root.is_dir() {
        return Err(format!("Not a directory: {}", path));
    }

    {
        let mut vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
        *vault_root = Some(root.clone());
    }

    {
        let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
        *index = FileIndex::new(root.clone());
        let md_files = list_all_md_files(&root)?;

        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.execute("DELETE FROM file_search", []).ok();

        for file_path in md_files {
            if let Ok(content) = read_file(&file_path) {
                index.update_file(&file_path, &content);
                let rel_path = file_path
                    .strip_prefix(&root)
                    .unwrap_or(&file_path)
                    .to_string_lossy()
                    .to_string();
                db.execute(
                    "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
                    rusqlite::params![&rel_path, &content],
                )
                .ok();
            }
        }
    }

    list_vault_entries(&root)
}

/// Open a file from the vault. Returns raw bytes.
pub fn open_file(state: &AppState, path: &str) -> Result<Vec<u8>, String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path(vault_root, path);

    if abs_path
        .extension()
        .is_some_and(|e| is_image(e.to_str().unwrap_or("")))
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
    let abs_path = resolve_vault_path(vault_root, path);
    write_file(&abs_path, content)?;

    let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
    index.update_file(&abs_path, content);

    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.execute(
        "DELETE FROM file_search WHERE path = ?1",
        rusqlite::params![path],
    )
    .ok();
    db.execute(
        "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
        rusqlite::params![path, content],
    )
    .ok();

    Ok(())
}

/// Save file content and update backlinks from pre-parsed local markdown link targets.
pub fn save_file_with_markdown_link_targets(
    state: &AppState,
    path: &str,
    content: &str,
    markdown_link_targets: &[String],
) -> Result<(), String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path(vault_root, path);
    write_file(&abs_path, content)?;

    let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
    index.update_file_targets(&abs_path, markdown_link_targets.iter().map(String::as_str));

    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.execute(
        "DELETE FROM file_search WHERE path = ?1",
        rusqlite::params![path],
    )
    .ok();
    db.execute(
        "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
        rusqlite::params![path, content],
    )
    .ok();

    Ok(())
}

/// Create a new empty file.
pub fn create_file(state: &AppState, path: &str) -> Result<(), String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path(vault_root, path);
    if abs_path.exists() {
        return Err(format!("File already exists: {}", abs_path.display()));
    }
    write_file(&abs_path, "")
}

/// Create a new directory.
pub fn create_dir(state: &AppState, path: &str) -> Result<(), String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path(vault_root, path);
    if abs_path.exists() {
        return Err(format!("Directory already exists: {}", abs_path.display()));
    }
    fs::create_dir_all(&abs_path)
        .map_err(|e| format!("Failed to create directory {}: {}", abs_path.display(), e))
}

/// Rename a file or directory.
pub fn rename_entry(state: &AppState, old_path: &str, new_path: &str) -> Result<(), String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_old = resolve_vault_path(vault_root, old_path);
    let abs_new = resolve_vault_path(vault_root, new_path);

    if abs_old.is_file() && is_markdown_path(&abs_old) {
        let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
        index.remove_file(&abs_old);
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.execute(
            "DELETE FROM file_search WHERE path = ?1",
            rusqlite::params![old_path],
        )
        .ok();
    }

    if abs_new.exists() {
        return Err(format!("Target already exists: {}", abs_new.display()));
    }
    fs::rename(&abs_old, &abs_new)
        .map_err(|e| format!("Failed to rename {}: {}", abs_old.display(), e))?;

    if abs_new.is_file() && is_markdown_path(&abs_new) {
        let content = fs::read_to_string(&abs_new)
            .map_err(|e| format!("Failed to read renamed file {}: {}", abs_new.display(), e))?;

        let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
        index.update_file(&abs_new, &content);

        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.execute(
            "DELETE FROM file_search WHERE path = ?1",
            rusqlite::params![new_path],
        )
        .ok();
        db.execute(
            "INSERT INTO file_search (path, content) VALUES (?1, ?2)",
            rusqlite::params![new_path, &content],
        )
        .ok();
    }

    Ok(())
}

/// Delete a file or directory.
pub fn delete_entry(state: &AppState, path: &str) -> Result<(), String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = resolve_vault_path(vault_root, path);

    if abs_path.is_file() {
        let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
        index.remove_file(&abs_path);
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.execute(
            "DELETE FROM file_search WHERE path = ?1",
            rusqlite::params![path],
        )
        .ok();
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
    for r in rows.flatten() {
        results.push(r);
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
    let abs_path = resolve_vault_path(vault_root, path);
    read_image(&abs_path)
}

// ── Internal helpers ────────────────────────────────────────────────

pub fn resolve_vault_path(vault_root: &Path, relative_path: &str) -> PathBuf {
    vault_root.join(relative_path)
}

fn read_file(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("Failed to read file {}: {}", path.display(), e))
}

fn read_image(path: &Path) -> Result<Vec<u8>, String> {
    if !path
        .extension()
        .is_some_and(|e| is_image(e.to_str().unwrap_or("")))
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

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext == "md" || ext == "markdown")
}

fn list_vault_entries(root: &Path) -> Result<Vec<FileEntry>, String> {
    let mut entries = Vec::new();
    list_vault_recursive(root, root, &mut entries)?;
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
) -> Result<(), String> {
    let read_dir = fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            entries.push(FileEntry {
                path: path_to_relative_string(&path, root),
                name,
                is_dir: true,
            });
            list_vault_recursive(root, &path, entries)?;
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
    list_all_md_files_recursive(root, &mut files)?;
    Ok(files)
}

fn list_all_md_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let read_dir = fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            list_all_md_files_recursive(&path, files)?;
        } else if path
            .extension()
            .is_some_and(|e| e == "md" || e == "markdown")
        {
            files.push(path);
        }
    }
    Ok(())
}

/// Perform unified global search across markdown content, headings, filenames, annotations & notes.
pub fn search_vault_unified(
    state: &AppState,
    query: &str,
    active_markdown_path: Option<&str>,
    active_pdf_path: Option<&str>,
) -> Result<Vec<UnifiedSearchResult>, String> {
    let query_lower = query.to_lowercase();
    let query_trimmed = query.trim();
    if query_trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let index_locked = state.file_index.lock().ok();
    let vault_root_locked = state.vault_root.lock().ok();
    let vault_root = vault_root_locked.as_ref().and_then(|r| r.as_ref());

    let is_linked = |p1: &str, p2: &str| -> bool {
        if let (Some(index), Some(root)) = (index_locked.as_ref(), vault_root) {
            let path1 = resolve_vault_path(root, p1);
            let path2 = resolve_vault_path(root, p2);
            index.outgoing.get(&path1).map_or(false, |set| set.contains(&path2))
                || index.incoming.get(&path1).map_or(false, |set| set.contains(&path2))
        } else {
            false
        }
    };

    let mut results = Vec::new();
    let db = state.db.lock().map_err(|e| e.to_string())?;

    // 1. Search Filenames
    // Markdown files
    let mut stmt_md_paths = db.prepare("SELECT DISTINCT path FROM file_search").map_err(|e| e.to_string())?;
    let mut rows_md_paths = stmt_md_paths.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = rows_md_paths.next().map_err(|e| e.to_string())? {
        let path: String = row.get(0).map_err(|e| e.to_string())?;
        let filename = std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        if filename.to_lowercase().contains(&query_lower) {
            let mut score = 10.0;
            if Some(path.as_str()) == active_markdown_path {
                score *= 1.5;
            }
            let file_stem = std::path::Path::new(&filename)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| filename.clone());
            if file_stem.trim().to_lowercase() == query_trimmed.to_lowercase() {
                score *= 2.0;
            }
            if let Some(active) = active_markdown_path {
                if is_linked(&path, active) {
                    score *= 1.3;
                }
            }
            results.push(UnifiedSearchResult {
                group: SearchResultGroup::Filename,
                path,
                line: 1,
                context: filename,
                score,
                page_index: None,
                annotation_id: None,
            });
        }
    }

    // PDF files
    let mut stmt_pdf_paths = db.prepare("SELECT vault_relative_path FROM pdf_documents").map_err(|e| e.to_string())?;
    let mut rows_pdf_paths = stmt_pdf_paths.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = rows_pdf_paths.next().map_err(|e| e.to_string())? {
        let path: String = row.get(0).map_err(|e| e.to_string())?;
        let filename = std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        if filename.to_lowercase().contains(&query_lower) {
            let mut score = 10.0;
            if Some(path.as_str()) == active_pdf_path {
                score *= 1.5;
            }
            let file_stem = std::path::Path::new(&filename)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| filename.clone());
            if file_stem.trim().to_lowercase() == query_trimmed.to_lowercase() {
                score *= 2.0;
            }
            if let Some(active) = active_markdown_path {
                if is_linked(&path, active) {
                    score *= 1.3;
                }
            }
            results.push(UnifiedSearchResult {
                group: SearchResultGroup::Filename,
                path,
                line: 1,
                context: filename,
                score,
                page_index: None,
                annotation_id: None,
            });
        }
    }

    // 2. Search PDF Annotations & Quick Notes
    let mut stmt_ann = db.prepare(
        "SELECT d.vault_relative_path, a.id, a.page_index, a.selected_text, a.note
         FROM pdf_annotations a
         JOIN pdf_documents d ON a.document_id = d.document_id
         WHERE a.selected_text LIKE ?1 OR a.note LIKE ?1"
    ).map_err(|e| e.to_string())?;
    let like_query = format!("%{}%", query_trimmed);
    let mut rows_ann = stmt_ann.query([&like_query]).map_err(|e| e.to_string())?;
    while let Some(row) = rows_ann.next().map_err(|e| e.to_string())? {
        let path: String = row.get(0).map_err(|e| e.to_string())?;
        let ann_id: String = row.get(1).map_err(|e| e.to_string())?;
        let page_idx: i32 = row.get(2).map_err(|e| e.to_string())?;
        let selected_text: String = row.get(3).map_err(|e| e.to_string())?;
        let note: Option<String> = row.get(4).map_err(|e| e.to_string())?;

        let note_text = note.unwrap_or_default();
        let context = format!("Highlight: \"{}\" | Note: \"{}\"", selected_text, note_text);

        let mut score = 6.0;
        if Some(path.as_str()) == active_pdf_path {
            score *= 1.5;
        }
        if selected_text.to_lowercase().contains(&query_lower) || note_text.to_lowercase().contains(&query_lower) {
            if selected_text.trim().to_lowercase() == query_trimmed.to_lowercase()
                || note_text.trim().to_lowercase() == query_trimmed.to_lowercase() {
                score *= 2.0;
            }
        }
        if let Some(active) = active_markdown_path {
            if is_linked(&path, active) {
                score *= 1.3;
            }
        }

        results.push(UnifiedSearchResult {
            group: SearchResultGroup::Annotation,
            path,
            line: (page_idx + 1) as usize,
            context,
            score,
            page_index: Some(page_idx as u16),
            annotation_id: Some(ann_id),
        });
    }

    // 3. Search Markdown Content & Headings
    let fts_query = format!("\"{}\"", query_trimmed.replace('"', "\"\""));
    let mut stmt_fts = db.prepare("SELECT path, content, rank FROM file_search WHERE content MATCH ?1").map_err(|e| e.to_string())?;
    let mut rows_fts = stmt_fts.query([&fts_query]).map_err(|e| e.to_string())?;
    while let Some(row) = rows_fts.next().map_err(|e| e.to_string())? {
        let path: String = row.get(0).map_err(|e| e.to_string())?;
        let content: String = row.get(1).map_err(|e| e.to_string())?;
        let rank: f64 = row.get(2).map_err(|e| e.to_string())?;

        for (idx, line) in content.lines().enumerate() {
            if line.to_lowercase().contains(&query_lower) {
                let is_heading = line.trim_start().starts_with('#');
                let group = if is_heading {
                    SearchResultGroup::Heading
                } else {
                    SearchResultGroup::MarkdownContent
                };

                let mut score = if is_heading { 8.0 } else { 5.0 };
                score += (10.0 - rank).max(0.0) as f32 * 0.1;

                if Some(path.as_str()) == active_markdown_path {
                    score *= 1.5;
                }
                if line.trim().to_lowercase() == query_trimmed.to_lowercase() {
                    score *= 2.0;
                }
                if let Some(active) = active_markdown_path {
                    if is_linked(&path, active) {
                        score *= 1.3;
                    }
                }

                results.push(UnifiedSearchResult {
                    group,
                    path: path.clone(),
                    line: idx + 1,
                    context: line.to_string(),
                    score,
                    page_index: None,
                    annotation_id: None,
                });
            }
        }
    }

    results.sort_by(|a, b| {
        b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.group.cmp(&b.group))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.cmp(&b.line))
    });

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("md_editor_{name}_{nanos}"))
    }

    #[test]
    fn rename_markdown_reindexes_links_and_search_under_new_path() {
        let root = unique_temp_dir("rename_reindex");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("source.md"), "Link to [[target]]. UniqueNeedle").unwrap();
        fs::write(root.join("target.md"), "Target").unwrap();

        let state = AppState::new_in_memory();
        set_vault_root(&state, root.to_str().unwrap()).unwrap();

        rename_entry(&state, "source.md", "renamed.md").unwrap();

        let backlinks = get_backlinks(&state, "target.md").unwrap();
        assert!(
            backlinks.iter().any(|p| p.ends_with("renamed.md")),
            "renamed markdown file should remain an incoming backlink: {backlinks:?}"
        );
        assert!(
            !backlinks.iter().any(|p| p.ends_with("source.md")),
            "old markdown path should be removed from backlinks: {backlinks:?}"
        );

        let results = search_vault(&state, "UniqueNeedle").unwrap();
        assert!(
            results.iter().any(|result| result.path == "renamed.md"),
            "FTS index should contain renamed markdown path: {results:?}"
        );
        assert!(
            !results.iter().any(|result| result.path == "source.md"),
            "FTS index should not retain old markdown path: {results:?}"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn save_file_with_markdown_link_targets_uses_parser_supplied_links() {
        let root = unique_temp_dir("save_parser_targets");
        fs::create_dir_all(&root).unwrap();

        let state = AppState::new_in_memory();
        set_vault_root(&state, root.to_str().unwrap()).unwrap();

        save_file_with_markdown_link_targets(
            &state,
            "source.md",
            "Parser saw a code-block-safe link set.",
            &["target".to_string()],
        )
        .unwrap();

        let backlinks = get_backlinks(&state, "target.md").unwrap();
        assert!(
            backlinks.iter().any(|path| path.ends_with("source.md")),
            "parser-supplied target should create backlink: {backlinks:?}"
        );

        save_file_with_markdown_link_targets(
            &state,
            "source.md",
            "Parser now reports a different link set.",
            &["other".to_string()],
        )
        .unwrap();

        let old_backlinks = get_backlinks(&state, "target.md").unwrap();
        assert!(
            old_backlinks.is_empty(),
            "old parser-supplied backlink should be removed: {old_backlinks:?}"
        );
        let new_backlinks = get_backlinks(&state, "other.md").unwrap();
        assert!(
            new_backlinks.iter().any(|path| path.ends_with("source.md")),
            "new parser-supplied backlink should be indexed: {new_backlinks:?}"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn test_search_vault_unified() {
        let root = unique_temp_dir("search_unified_test");
        fs::create_dir_all(&root).unwrap();

        let state = AppState::new_in_memory();
        set_vault_root(&state, root.to_str().unwrap()).unwrap();

        let note_content = "# Welcome to the Vault\nThis is a test note about Rust programming.\n";
        save_file(&state, "source.md", note_content).unwrap();

        {
            let db = state.db.lock().unwrap();
            db.execute("INSERT OR REPLACE INTO file_search (path, content) VALUES (?1, ?2)",
                       ["source.md", note_content]).unwrap();
        }

        let results = search_vault_unified(&state, "Vault", Some("source.md"), None).unwrap();
        assert!(!results.is_empty());
        let groups = results.iter().map(|r| r.group).collect::<Vec<_>>();
        assert!(groups.contains(&SearchResultGroup::Heading));

        let results_filename = search_vault_unified(&state, "source", Some("source.md"), None).unwrap();
        let groups_filename = results_filename.iter().map(|r| r.group).collect::<Vec<_>>();
        assert!(groups_filename.contains(&SearchResultGroup::Filename));

        let results2 = search_vault_unified(&state, "Rust", Some("source.md"), None).unwrap();
        let groups2 = results2.iter().map(|r| r.group).collect::<Vec<_>>();
        assert!(groups2.contains(&SearchResultGroup::MarkdownContent));

        let active_match = results2.iter().find(|r| r.path == "source.md").unwrap();
        let results_non_active = search_vault_unified(&state, "Rust", None, None).unwrap();
        let non_active_match = results_non_active.iter().find(|r| r.path == "source.md").unwrap();
        assert!(active_match.score > non_active_match.score);

        let _ = fs::remove_dir_all(root);
    }
}

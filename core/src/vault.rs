use std::fs;
use std::path::{Path, PathBuf};

use crate::file_index::FileIndex;
use crate::state::AppState;
use crate::types::{
    BacklinkItem, BacklinkTarget, FileEntry, SearchResult, SearchResultGroup, UnifiedSearchQuery,
    UnifiedSearchResult, UnifiedSearchSource,
};

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
    let vault_root_path = {
        let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
        vault_root.as_ref().ok_or("No vault root set")?.clone()
    };
    let abs_old = resolve_vault_path(&vault_root_path, old_path);
    let abs_new = resolve_vault_path(&vault_root_path, new_path);

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

    repair_rename_references(state, &vault_root_path, old_path, new_path)?;

    Ok(())
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub fn repair_rename_references(
    state: &AppState,
    vault_root: &Path,
    old_path: &str,
    new_path: &str,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    // Update pdf_documents
    db.execute(
        "UPDATE pdf_documents SET vault_relative_path = ?1 WHERE vault_relative_path = ?2",
        rusqlite::params![new_path, old_path],
    )
    .ok();

    // Update pdf_text_search
    db.execute(
        "UPDATE pdf_text_search SET path = ?1 WHERE path = ?2",
        rusqlite::params![new_path, old_path],
    )
    .ok();

    // Update pdf_annotations linked_note_path
    db.execute(
        "UPDATE pdf_annotations SET linked_note_path = ?1 WHERE linked_note_path = ?2",
        rusqlite::params![new_path, old_path],
    )
    .ok();

    // Update links inside markdown files
    let md_files = list_all_md_files(vault_root)?;
    let old_encoded = percent_encode(old_path);
    let new_encoded = percent_encode(new_path);

    let old_path_stem = if old_path.ends_with(".md") {
        old_path.strip_suffix(".md").unwrap_or(old_path)
    } else if old_path.ends_with(".markdown") {
        old_path.strip_suffix(".markdown").unwrap_or(old_path)
    } else {
        old_path
    };

    let new_path_stem = if new_path.ends_with(".md") {
        new_path.strip_suffix(".md").unwrap_or(new_path)
    } else if new_path.ends_with(".markdown") {
        new_path.strip_suffix(".markdown").unwrap_or(new_path)
    } else {
        new_path
    };

    for md_path in md_files {
        let content = match fs::read_to_string(&md_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let mut new_content = content.clone();

        // Replace pdf:// encoded and raw
        let old_pdf_encoded = format!("pdf://{}", old_encoded);
        let new_pdf_encoded = format!("pdf://{}", new_encoded);
        new_content = new_content.replace(&old_pdf_encoded, &new_pdf_encoded);

        let old_pdf_raw = format!("pdf://{}", old_path);
        let new_pdf_raw = format!("pdf://{}", new_path);
        new_content = new_content.replace(&old_pdf_raw, &new_pdf_raw);

        // Replace inline markdown links
        new_content = new_content.replace(&format!("({old_path})"), &format!("({new_path})"));
        new_content = new_content.replace(&format!("(./{old_path})"), &format!("(./{new_path})"));

        if old_path_stem != old_path {
            // Replace wiki links
            new_content = new_content.replace(
                &format!("[[{old_path_stem}]]"),
                &format!("[[{new_path_stem}]]"),
            );
            new_content = new_content.replace(
                &format!("[[{old_path_stem}#"),
                &format!("[[{new_path_stem}#"),
            );
            new_content = new_content.replace(
                &format!("[[{old_path_stem}|"),
                &format!("[[{new_path_stem}|"),
            );
            new_content =
                new_content.replace(&format!("[[{old_path}]]"), &format!("[[{new_path}]]"));
            new_content = new_content.replace(&format!("[[{old_path}#"), &format!("[[{new_path}#"));
            new_content = new_content.replace(&format!("[[{old_path}|"), &format!("[[{new_path}|"));

            // Replace inline link without extension
            new_content =
                new_content.replace(&format!("({old_path_stem})"), &format!("({new_path_stem})"));
            new_content = new_content.replace(
                &format!("(./{old_path_stem})"),
                &format!("(./{new_path_stem})"),
            );
        }

        if new_content != content {
            fs::write(&md_path, &new_content).map_err(|e| {
                format!(
                    "Failed to write updated links to {}: {}",
                    md_path.display(),
                    e
                )
            })?;
        }
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

pub fn path_to_relative_string(path: &Path, root: &Path) -> String {
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
pub fn list_all_pdf_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    list_all_pdf_files_recursive(root, &mut files)?;
    Ok(files)
}

fn list_all_pdf_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
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
            list_all_pdf_files_recursive(&path, files)?;
        } else if path.extension().is_some_and(|e| e == "pdf") {
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
    let query_model = UnifiedSearchQuery::all_sources(query)
        .with_active_paths(active_markdown_path, active_pdf_path);
    search_vault_unified_query(state, &query_model)
}

pub fn search_vault_unified_query(
    state: &AppState,
    query: &UnifiedSearchQuery,
) -> Result<Vec<UnifiedSearchResult>, String> {
    let query_lower = query.text.to_lowercase();
    let query_trimmed = query.text.trim();
    if query_trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let active_markdown_path = query.active_markdown_path.as_deref();
    let active_pdf_path = query.active_pdf_path.as_deref();

    let index_locked = state.file_index.lock().ok();
    let vault_root_locked = state.vault_root.lock().ok();
    let vault_root = vault_root_locked.as_ref().and_then(|r| r.as_ref());

    let is_linked = |p1: &str, p2: &str| -> bool {
        if let (Some(index), Some(root)) = (index_locked.as_ref(), vault_root) {
            let path1 = resolve_vault_path(root, p1);
            let path2 = resolve_vault_path(root, p2);
            index
                .outgoing
                .get(&path1)
                .is_some_and(|set| set.contains(&path2))
                || index
                    .incoming
                    .get(&path1)
                    .is_some_and(|set| set.contains(&path2))
        } else {
            false
        }
    };
    let search_context = AnnotationSearchContext {
        query_trimmed,
        active_pdf_path,
        active_markdown_path,
        ranking: &query.ranking,
        is_linked: &is_linked,
    };

    let mut results = Vec::new();
    let db = state.db.lock().map_err(|e| e.to_string())?;

    if query.includes(UnifiedSearchSource::Filename) {
        // Markdown files
        let mut stmt_md_paths = db
            .prepare("SELECT DISTINCT path FROM file_search")
            .map_err(|e| e.to_string())?;
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
                    score *= query.ranking.current_document_boost;
                }
                let file_stem = std::path::Path::new(&filename)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| filename.clone());
                if file_stem.trim().to_lowercase() == query_trimmed.to_lowercase() {
                    score *= query.ranking.exact_phrase_boost;
                }
                if let Some(active) = active_markdown_path
                    && is_linked(&path, active)
                {
                    score *= query.ranking.linked_note_boost;
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
        let mut stmt_pdf_paths = db
            .prepare("SELECT vault_relative_path FROM pdf_documents")
            .map_err(|e| e.to_string())?;
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
                    score *= query.ranking.current_document_boost;
                }
                let file_stem = std::path::Path::new(&filename)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| filename.clone());
                if file_stem.trim().to_lowercase() == query_trimmed.to_lowercase() {
                    score *= query.ranking.exact_phrase_boost;
                }
                if let Some(active) = active_markdown_path
                    && is_linked(&path, active)
                {
                    score *= query.ranking.linked_note_boost;
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
    }

    if query.includes(UnifiedSearchSource::Annotation)
        || query.includes(UnifiedSearchSource::QuickNote)
    {
        let mut stmt_ann = db
            .prepare(
                "SELECT d.vault_relative_path, a.id, a.page_index, a.selected_text, a.note
             FROM pdf_annotations a
             JOIN pdf_documents d ON a.document_id = d.document_id
             WHERE a.selected_text LIKE ?1 OR a.note LIKE ?1",
            )
            .map_err(|e| e.to_string())?;
        let like_query = format!("%{}%", query_trimmed);
        let mut rows_ann = stmt_ann.query([&like_query]).map_err(|e| e.to_string())?;
        while let Some(row) = rows_ann.next().map_err(|e| e.to_string())? {
            let path: String = row.get(0).map_err(|e| e.to_string())?;
            let ann_id: String = row.get(1).map_err(|e| e.to_string())?;
            let page_idx: i32 = row.get(2).map_err(|e| e.to_string())?;
            let selected_text: String = row.get(3).map_err(|e| e.to_string())?;
            let note: Option<String> = row.get(4).map_err(|e| e.to_string())?;

            if query.includes(UnifiedSearchSource::Annotation)
                && selected_text.to_lowercase().contains(&query_lower)
            {
                results.push(annotation_search_result(
                    AnnotationSearchInput {
                        group: SearchResultGroup::Annotation,
                        path: &path,
                        annotation_id: &ann_id,
                        page_index: page_idx,
                        context: search_result_preview(
                            &selected_text,
                            query_trimmed,
                            Some("Highlight"),
                        ),
                        matched_text: &selected_text,
                    },
                    &search_context,
                ));
            }

            let note_text = note.unwrap_or_default();
            if query.includes(UnifiedSearchSource::QuickNote)
                && note_text.to_lowercase().contains(&query_lower)
            {
                results.push(annotation_search_result(
                    AnnotationSearchInput {
                        group: SearchResultGroup::QuickNote,
                        path: &path,
                        annotation_id: &ann_id,
                        page_index: page_idx,
                        context: search_result_preview(&note_text, query_trimmed, Some("Note")),
                        matched_text: &note_text,
                    },
                    &search_context,
                ));
            }
        }
    }

    if query.includes(UnifiedSearchSource::MarkdownContent)
        || query.includes(UnifiedSearchSource::Heading)
    {
        let fts_query = format!("\"{}\"", query_trimmed.replace('"', "\"\""));
        let mut stmt_fts = db
            .prepare("SELECT path, content, rank FROM file_search WHERE content MATCH ?1")
            .map_err(|e| e.to_string())?;
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
                    let source = if is_heading {
                        UnifiedSearchSource::Heading
                    } else {
                        UnifiedSearchSource::MarkdownContent
                    };
                    if !query.includes(source) {
                        continue;
                    }

                    let mut score = if is_heading { 8.0 } else { 5.0 };
                    score += (10.0 - rank).max(0.0) as f32 * 0.1;

                    if Some(path.as_str()) == active_markdown_path {
                        score *= query.ranking.current_document_boost;
                    }
                    if line.trim().to_lowercase() == query_trimmed.to_lowercase() {
                        score *= query.ranking.exact_phrase_boost;
                    }
                    if let Some(active) = active_markdown_path
                        && is_linked(&path, active)
                    {
                        score *= query.ranking.linked_note_boost;
                    }

                    results.push(UnifiedSearchResult {
                        group,
                        path: path.clone(),
                        line: idx + 1,
                        context: search_result_preview(line, query_trimmed, None),
                        score,
                        page_index: None,
                        annotation_id: None,
                    });
                }
            }
        }
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.group.cmp(&b.group))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.cmp(&b.line))
    });

    Ok(results)
}

pub fn list_registered_pdf_paths(state: &AppState) -> Result<Vec<String>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = db
        .prepare("SELECT vault_relative_path FROM pdf_documents ORDER BY vault_relative_path")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn search_cached_pdf_text(
    state: &AppState,
    query: &str,
    paths: &[String],
) -> Result<Vec<UnifiedSearchResult>, String> {
    let query_trimmed = query.trim();
    if query_trimmed.is_empty() || paths.is_empty() {
        return Ok(Vec::new());
    }

    // Invalidate stale caches before locking db
    for path in paths {
        let _ = state.validate_and_invalidate_pdf_cache(path);
    }

    let db = state.db.lock().map_err(|e| e.to_string())?;
    let fts_query = format!("\"{}\"", query_trimmed.replace('"', "\"\""));
    let mut results = Vec::new();
    for path in paths {
        let mut stmt = db
            .prepare(
                "SELECT page_index, content, rank
                 FROM pdf_text_search
                 WHERE path = ?1 AND content MATCH ?2",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![path, &fts_query], |row| {
                let page_index: i64 = row.get(0)?;
                let content: String = row.get(1)?;
                let rank: f64 = row.get(2)?;
                Ok((page_index, content, rank))
            })
            .map_err(|e| e.to_string())?;

        for row in rows {
            let (page_index, content, rank) = row.map_err(|e| e.to_string())?;
            let mut score = 4.2;
            score += (10.0 - rank).max(0.0) as f32 * 0.1;
            if content.trim().eq_ignore_ascii_case(query_trimmed) {
                score *= 2.0;
            }
            results.push(UnifiedSearchResult {
                group: SearchResultGroup::PdfContent,
                path: path.clone(),
                line: page_index.saturating_add(1) as usize,
                context: format!(
                    "Cached PDF text: {}",
                    search_result_preview(&content, query_trimmed, None)
                ),
                score,
                page_index: Some(page_index.max(0) as u16),
                annotation_id: None,
            });
        }
    }
    Ok(results)
}

pub fn search_result_preview(text: &str, query: &str, label: Option<&str>) -> String {
    let clean_text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let query = query.trim();
    let max_chars = 120;
    let radius = 48;

    let preview = if clean_text.chars().count() <= max_chars {
        clean_text
    } else if let Some((start, end)) = find_case_insensitive_char_range(&clean_text, query) {
        let snippet_start = start.saturating_sub(radius);
        let snippet_end = (end + radius).min(clean_text.chars().count());
        let mut snippet = clean_text
            .chars()
            .skip(snippet_start)
            .take(snippet_end.saturating_sub(snippet_start))
            .collect::<String>();
        if snippet_start > 0 {
            snippet.insert_str(0, "...");
        }
        if snippet_end < clean_text.chars().count() {
            snippet.push_str("...");
        }
        snippet
    } else {
        let mut snippet = clean_text.chars().take(max_chars).collect::<String>();
        if clean_text.chars().count() > max_chars {
            snippet.push_str("...");
        }
        snippet
    };

    if let Some(label) = label {
        format!("{label}: \"{preview}\"")
    } else {
        preview
    }
}

fn find_case_insensitive_char_range(text: &str, query: &str) -> Option<(usize, usize)> {
    if query.is_empty() {
        return None;
    }
    let text_chars = text.chars().collect::<Vec<_>>();
    let query_chars = query.chars().collect::<Vec<_>>();
    if query_chars.is_empty() || query_chars.len() > text_chars.len() {
        return None;
    }

    for start in 0..=text_chars.len() - query_chars.len() {
        let candidate = text_chars[start..start + query_chars.len()]
            .iter()
            .collect::<String>();
        if candidate.eq_ignore_ascii_case(query) {
            return Some((start, start + query_chars.len()));
        }
    }
    None
}

struct AnnotationSearchInput<'a> {
    group: SearchResultGroup,
    path: &'a str,
    annotation_id: &'a str,
    page_index: i32,
    context: String,
    matched_text: &'a str,
}

struct AnnotationSearchContext<'a, F: Fn(&str, &str) -> bool> {
    query_trimmed: &'a str,
    active_pdf_path: Option<&'a str>,
    active_markdown_path: Option<&'a str>,
    ranking: &'a crate::types::UnifiedSearchRanking,
    is_linked: &'a F,
}

fn annotation_search_result<F>(
    input: AnnotationSearchInput<'_>,
    search_context: &AnnotationSearchContext<'_, F>,
) -> UnifiedSearchResult
where
    F: Fn(&str, &str) -> bool,
{
    let mut score = if input.group == SearchResultGroup::QuickNote {
        6.5
    } else {
        6.0
    };
    if Some(input.path) == search_context.active_pdf_path {
        score *= search_context.ranking.current_document_boost;
    }
    if input
        .matched_text
        .trim()
        .eq_ignore_ascii_case(search_context.query_trimmed)
    {
        score *= search_context.ranking.exact_phrase_boost;
    }
    if let Some(active) = search_context.active_markdown_path
        && (search_context.is_linked)(input.path, active)
    {
        score *= search_context.ranking.linked_note_boost;
    }

    UnifiedSearchResult {
        group: input.group,
        path: input.path.to_string(),
        line: (input.page_index + 1) as usize,
        context: input.context,
        score,
        page_index: Some(input.page_index as u16),
        annotation_id: Some(input.annotation_id.to_string()),
    }
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
    fn test_rename_pdf_and_markdown_repairs_references() {
        let root = unique_temp_dir("rename_repair_test");
        fs::create_dir_all(&root).unwrap();

        // 1. Create a PDF doc, save some content and index it
        let pdf_path = "subfolder/document.pdf";
        fs::create_dir_all(root.join("subfolder")).unwrap();
        fs::write(root.join(pdf_path), "PDF dummy content").unwrap();

        // 2. Create markdown note with references to the PDF
        let note_path = "note.md";
        let note_content = "Check [pdf annotation](pdf://subfolder/document.pdf?page=2&annotation=ann-123) and raw link pdf://subfolder/document.pdf.";
        fs::write(root.join(note_path), note_content).unwrap();

        let state = AppState::new_in_memory();
        set_vault_root(&state, root.to_str().unwrap()).unwrap();

        // Save doc metadata in db
        state
            .save_pdf_document("doc-id-123", pdf_path, 100, Some(12345))
            .unwrap();

        // Cache some text search content for this PDF
        state
            .save_pdf_page_text(pdf_path, 2, "Important cached text needle")
            .unwrap();

        // Save annotation with linked note path pointing to note.md
        let ann = crate::pdf::PdfAnnotation {
            id: "ann-123".to_string(),
            document_id: "doc-id-123".to_string(),
            page_index: 2,
            kind: crate::pdf::PdfAnnotationKind::Highlight,
            color: crate::pdf::PdfAnnotationColor::Yellow,
            selected_text: "Important highlight".to_string(),
            ranges: vec![],
            rects: vec![],
            note: None,
            linked_note_path: Some(note_path.to_string()),
            markdown_anchor: None,
            tags: vec!["tag1".to_string()],
            status: crate::pdf::PdfAnnotationStatus::Unresolved,
            created_at: 0,
            updated_at: 0,
        };
        state.save_pdf_annotation(&ann).unwrap();

        // 3. Rename the PDF file
        let new_pdf_path = "subfolder/new_document.pdf";
        rename_entry(&state, pdf_path, new_pdf_path).unwrap();

        // Check if database updated vault_relative_path in pdf_documents
        {
            let db = state.db.lock().unwrap();
            let mut stmt = db.prepare("SELECT vault_relative_path FROM pdf_documents WHERE document_id = 'doc-id-123'").unwrap();
            let db_path: String = stmt.query_row([], |r| r.get(0)).unwrap();
            assert_eq!(db_path, new_pdf_path);

            // Check if pdf_text_search path updated
            let mut stmt2 = db
                .prepare("SELECT path FROM pdf_text_search WHERE content LIKE '%needle%'")
                .unwrap();
            let fts_path: String = stmt2.query_row([], |r| r.get(0)).unwrap();
            assert_eq!(fts_path, new_pdf_path);
        }

        // Check if note.md links got updated to new PDF path!
        let updated_note = fs::read_to_string(root.join(note_path)).unwrap();
        assert!(
            updated_note.contains("pdf://subfolder/new_document.pdf?page=2&annotation=ann-123")
        );
        assert!(updated_note.contains("pdf://subfolder/new_document.pdf."));

        // 4. Rename the markdown note
        let new_note_path = "new_note.md";
        rename_entry(&state, note_path, new_note_path).unwrap();

        // Check if pdf_annotations linked_note_path was updated to new_note.md
        let anns = state.get_pdf_annotations("doc-id-123", None).unwrap();
        assert_eq!(anns.len(), 1);
        assert_eq!(anns[0].linked_note_path.as_deref(), Some(new_note_path));

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
            db.execute(
                "INSERT OR REPLACE INTO file_search (path, content) VALUES (?1, ?2)",
                ["source.md", note_content],
            )
            .unwrap();
        }

        let results = search_vault_unified(&state, "Vault", Some("source.md"), None).unwrap();
        assert!(!results.is_empty());
        let groups = results.iter().map(|r| r.group).collect::<Vec<_>>();
        assert!(groups.contains(&SearchResultGroup::Heading));

        let results_filename =
            search_vault_unified(&state, "source", Some("source.md"), None).unwrap();
        let groups_filename = results_filename.iter().map(|r| r.group).collect::<Vec<_>>();
        assert!(groups_filename.contains(&SearchResultGroup::Filename));

        let results2 = search_vault_unified(&state, "Rust", Some("source.md"), None).unwrap();
        let groups2 = results2.iter().map(|r| r.group).collect::<Vec<_>>();
        assert!(groups2.contains(&SearchResultGroup::MarkdownContent));

        let active_match = results2.iter().find(|r| r.path == "source.md").unwrap();
        let results_non_active = search_vault_unified(&state, "Rust", None, None).unwrap();
        let non_active_match = results_non_active
            .iter()
            .find(|r| r.path == "source.md")
            .unwrap();
        assert!(active_match.score > non_active_match.score);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unified_search_query_filters_sources_and_splits_quick_notes() {
        let root = unique_temp_dir("search_query_model_test");
        fs::create_dir_all(&root).unwrap();

        let state = AppState::new_in_memory();
        set_vault_root(&state, root.to_str().unwrap()).unwrap();

        let note_content = "# QueryModel\nNeedle appears in markdown.\n";
        save_file(&state, "source.md", note_content).unwrap();

        {
            let db = state.db.lock().unwrap();
            db.execute(
                "INSERT OR REPLACE INTO file_search (path, content) VALUES (?1, ?2)",
                ["source.md", note_content],
            )
            .unwrap();
            db.execute(
                "INSERT INTO pdf_documents
                 (document_id, vault_relative_path, file_size, modified_at, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                ("doc-1", "paper.pdf", 0_i64, 0_i64, 0_i64, 0_i64),
            )
            .unwrap();
            db.execute(
                "INSERT INTO pdf_annotations
                 (id, document_id, page_index, kind, color, ranges_json, rects_json, selected_text, note, created_at, updated_at, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                [
                    "ann-1",
                    "doc-1",
                    "2",
                    "highlight",
                    "yellow",
                    "[]",
                    "[]",
                    "Needle annotation",
                    "Needle quick note",
                    "0",
                    "0",
                    "unresolved",
                ],
            )
            .unwrap();
        }

        let query = UnifiedSearchQuery {
            text: "Needle".to_string(),
            sources: vec![UnifiedSearchSource::QuickNote],
            active_markdown_path: None,
            active_pdf_path: Some("paper.pdf".to_string()),
            ranking: crate::types::UnifiedSearchRanking::default(),
        };

        let results = search_vault_unified_query(&state, &query).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].group, SearchResultGroup::QuickNote);
        assert_eq!(results[0].path, "paper.pdf");
        assert_eq!(results[0].page_index, Some(2));
        assert_eq!(results[0].annotation_id.as_deref(), Some("ann-1"));

        let annotation_query = UnifiedSearchQuery {
            sources: vec![UnifiedSearchSource::Annotation],
            ..query
        };
        let annotation_results = search_vault_unified_query(&state, &annotation_query).unwrap();
        assert_eq!(annotation_results.len(), 1);
        assert_eq!(annotation_results[0].group, SearchResultGroup::Annotation);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn search_result_preview_centers_match_and_preserves_label() {
        let text = format!("{} needle {}", "alpha ".repeat(40), "omega ".repeat(40));
        let preview = search_result_preview(&text, "needle", Some("Note"));

        assert!(preview.starts_with("Note: \""));
        assert!(preview.contains("needle"));
        assert!(preview.contains("..."));
        assert!(preview.len() < text.len() + 10);
    }

    #[test]
    fn unified_search_markdown_results_use_context_preview() {
        let root = unique_temp_dir("search_preview_test");
        fs::create_dir_all(&root).unwrap();

        let state = AppState::new_in_memory();
        set_vault_root(&state, root.to_str().unwrap()).unwrap();

        let note_content = format!(
            "# Preview\n{} needle {}\n",
            "before ".repeat(40),
            "after ".repeat(40)
        );
        save_file(&state, "preview.md", &note_content).unwrap();

        let results = search_vault_unified(&state, "needle", None, None).unwrap();
        let markdown = results
            .iter()
            .find(|result| result.group == SearchResultGroup::MarkdownContent)
            .unwrap();

        assert!(markdown.context.contains("needle"));
        assert!(markdown.context.starts_with("..."));
        assert!(markdown.context.ends_with("..."));
        assert!(markdown.context.len() < note_content.len());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cached_pdf_text_search_returns_page_results() {
        let state = AppState::new_in_memory();
        state
            .save_pdf_page_text("paper.pdf", 2, "cached needle content")
            .unwrap();

        let results = search_cached_pdf_text(&state, "needle", &["paper.pdf".to_string()]).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].group, SearchResultGroup::PdfContent);
        assert_eq!(results[0].path, "paper.pdf");
        assert_eq!(results[0].page_index, Some(2));
        assert!(results[0].context.contains("Cached PDF text"));
        assert!(results[0].context.contains("needle"));
    }

    #[test]
    fn test_pdf_cache_freshness_and_invalidation() {
        let root = unique_temp_dir("pdf_cache_freshness");
        fs::create_dir_all(&root).unwrap();

        let state = AppState::new_in_memory();
        set_vault_root(&state, root.to_str().unwrap()).unwrap();

        let pdf_path = "sample.pdf";
        let abs_path = root.join(pdf_path);

        // 1. Create file on disk
        fs::write(&abs_path, "Initial Content").unwrap();
        let metadata = fs::metadata(&abs_path).unwrap();
        let size = metadata.len();
        let mtime = metadata
            .modified()
            .unwrap()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Save PDF document details in DB
        state
            .save_pdf_document("doc-hash-1", pdf_path, size, Some(mtime))
            .unwrap();

        // Initially no page is cached, so cache is empty (not fresh)
        assert!(!state.validate_and_invalidate_pdf_cache(pdf_path).unwrap());

        // Cache page text
        state
            .save_pdf_page_text(pdf_path, 0, "Initial Page Text Content")
            .unwrap();

        // Cache should now be fresh
        assert!(state.validate_and_invalidate_pdf_cache(pdf_path).unwrap());

        // Verify search finds it
        let results = search_cached_pdf_text(&state, "Content", &[pdf_path.to_string()]).unwrap();
        assert_eq!(results.len(), 1);

        // 2. Modify the file (size changes)
        fs::write(&abs_path, "Newer Content with different length").unwrap();

        // Cache should be detected as stale and invalidated
        assert!(!state.validate_and_invalidate_pdf_cache(pdf_path).unwrap());

        // Verify search now finds nothing since cache was cleared
        let results = search_cached_pdf_text(&state, "Content", &[pdf_path.to_string()]).unwrap();
        assert_eq!(results.len(), 0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn test_list_all_pdf_files_discovers_unregistered_pdfs() {
        let root = unique_temp_dir("list_pdf_test");
        fs::create_dir_all(&root).unwrap();

        // Write a PDF file
        let pdf_path = root.join("unopened.pdf");
        fs::write(&pdf_path, "PDF Content").unwrap();

        // Discover
        let files = list_all_pdf_files(&root).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name().unwrap(), "unopened.pdf");

        let _ = fs::remove_dir_all(root);
    }
}

use std::fs;
use std::path::{Path, PathBuf};

use crate::file_index::FileIndex;
use crate::state::AppState;
use crate::types::{BacklinkItem, BacklinkTarget, FileEntry};

mod paths;
mod reference_repair;
mod search;

pub use paths::{
    is_image, list_all_md_files, list_all_pdf_files, path_to_relative_string, resolve_vault_path,
};
use paths::{is_markdown_path, list_vault_entries, read_file, read_image, write_file};
pub use reference_repair::repair_rename_references;
pub use search::{
    list_registered_pdf_paths, search_cached_pdf_text, search_result_preview, search_vault,
    search_vault_unified, search_vault_unified_query,
};

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

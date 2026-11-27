//! Full-text vault search and incoming/mixed backlink queries across notes and PDF annotations.

use super::*;

/// Most results a vault search returns.
const SEARCH_LIMIT: usize = 100;

/// Full-text search across the vault: note and PDF text through FTS5, plus
/// PDF quick notes. Every term must match; the last also matches as a prefix,
/// so results come while the word is still being typed. A hit in a file's
/// path ranks above one in its body.
///
/// `line` is the 1-based line of the first match in a note, the 1-based page
/// of a PDF quick note, and 0 for PDF text (the page is found by PDF search).
pub fn search_vault(state: &AppState, query: &str) -> VaultResult<crate::types::SearchResults> {
    let terms: Vec<&str> = query.split_whitespace().collect();
    if terms.is_empty() {
        return Ok(crate::types::SearchResults::default());
    }
    let fts_query = terms
        .iter()
        .enumerate()
        .map(|(i, term)| {
            let quoted = format!("\"{}\"", term.replace('"', "\"\""));
            if i + 1 == terms.len() {
                format!("{quoted}*")
            } else {
                quoted
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let scope = state.pdf_scope_prefix()?;
    let lowered: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();

    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = db
        .prepare(
            "SELECT path, snippet(file_search, 1, '<b>', '</b>', '...', 15), content
             FROM file_search WHERE file_search MATCH ?1
             ORDER BY bm25(file_search, 10.0, 1.0) LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            rusqlite::params![&fts_query, (SEARCH_LIMIT + 1) as i64],
            |row| {
                let path: String = row.get(0)?;
                let content: String = row.get(2)?;
                let line = if is_markdown_path(Path::new(&path)) {
                    first_match_line(&content, &lowered)
                } else {
                    0
                };
                Ok(SearchResult {
                    path,
                    line,
                    context: row.get(1)?,
                })
            },
        )
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    for row in rows {
        match row {
            Ok(r) => results.push(r),
            Err(e) => log::warn!("Skipping malformed file_search row: {e}"),
        }
    }
    drop(stmt);
    if let Some(scope) = scope {
        results.extend(search_pdf_notes(&db, &scope, &lowered)?);
    }
    let truncated = results.len() > SEARCH_LIMIT;
    results.truncate(SEARCH_LIMIT);
    Ok(crate::types::SearchResults {
        items: results,
        truncated,
    })
}

/// 1-based line of the first occurrence of the first term, or 1.
fn first_match_line(content: &str, terms: &[String]) -> usize {
    let lower = content.to_lowercase();
    terms
        .first()
        .and_then(|term| lower.find(term.as_str()))
        .map_or(1, |at| lower[..at].matches('\n').count() + 1)
}

/// PDF quick notes in this vault containing every term.
fn search_pdf_notes(
    db: &rusqlite::Connection,
    scope: &str,
    terms: &[String],
) -> VaultResult<Vec<SearchResult>> {
    let mut stmt = db
        .prepare(
            "SELECT d.vault_relative_path, a.page_index, a.note
             FROM pdf_annotations a JOIN pdf_documents d ON d.document_id = a.document_id
             WHERE substr(a.document_id, 1, length(?1)) = ?1
               AND a.note IS NOT NULL AND a.note != ''
             ORDER BY d.vault_relative_path, a.page_index",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![scope], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut hits = Vec::new();
    for (path, page, note) in rows.flatten() {
        let lower = note.to_lowercase();
        if terms.iter().all(|term| lower.contains(term.as_str())) {
            hits.push(SearchResult {
                path: path.replace('\\', "/"),
                line: (page.max(0) + 1) as usize,
                context: format!("Note, page {}: {}", page.max(0) + 1, note_excerpt(&note)),
            });
        }
    }
    Ok(hits)
}

fn note_excerpt(note: &str) -> String {
    const MAX: usize = 120;
    let flat = note.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX {
        flat
    } else {
        format!("{}...", flat.chars().take(MAX).collect::<String>())
    }
}

/// Get backlinks for a file.
pub fn get_backlinks(state: &AppState, path: &str) -> VaultResult<Vec<String>> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or(VaultError::NoVault)?;
    let abs_path = resolve_vault_path(vault_root, path);

    let index = state.file_index.lock().map_err(|e| e.to_string())?;
    let backlinks = index.get_backlinks(&abs_path);

    Ok(backlinks
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect())
}

/// Get mixed backlinks (markdown files, PDF documents, and PDF annotations).
pub fn get_mixed_backlinks(state: &AppState, path: &str) -> VaultResult<Vec<BacklinkItem>> {
    let (vault_root, pdf_scope_prefix, generation) = {
        let _publish = state.vault_publish_lock.lock().map_err(|e| e.to_string())?;
        let root = state
            .vault_root
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            .ok_or(VaultError::NoVault)?
            .clone();
        let scope_prefix = AppState::pdf_scope_prefix_for_root(&root);
        (root, scope_prefix, state.current_vault_generation())
    };

    let mut results = file_index_backlinks(state, &vault_root, path)?;
    if is_pdf_path(Path::new(path)) {
        append_pdf_annotation_note_links(state, path, &pdf_scope_prefix, &mut results)?;
    } else {
        append_note_pdf_annotation_backlinks(state, path, &pdf_scope_prefix, &mut results)?;
    }

    dedupe_annotation_backlinks(&mut results);

    let _publish = state.vault_publish_lock.lock().map_err(|e| e.to_string())?;
    if state.current_vault_generation() != generation
        || state.vault_root.lock().map_err(|e| e.to_string())?.as_ref() != Some(&vault_root)
    {
        return Err(VaultError::from("Vault changed while reading backlinks"));
    }

    Ok(results)
}

fn file_index_backlinks(
    state: &AppState,
    vault_root: &Path,
    path: &str,
) -> VaultResult<Vec<BacklinkItem>> {
    let abs_path = resolve_vault_path(vault_root, path);
    let index = state.file_index.lock().map_err(|e| e.to_string())?;
    let backlinks = index.get_backlinks(&abs_path);
    Ok(backlinks
        .into_iter()
        .map(|bl| {
            let rel_path = path_to_relative_string(&bl, vault_root);
            let name = bl
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| rel_path.clone());
            BacklinkItem {
                source: BacklinkTarget::MarkdownFile { path: rel_path },
                label: name,
                context: None,
            }
        })
        .collect())
}

fn append_pdf_annotation_note_links(
    state: &AppState,
    path: &str,
    pdf_scope_prefix: &str,
    results: &mut Vec<BacklinkItem>,
) -> VaultResult<()> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = db
        .prepare(
            "SELECT document_id FROM pdf_documents
             WHERE vault_relative_path = ?1
               AND substr(document_id, 1, length(?2)) = ?2
             ORDER BY updated_at DESC LIMIT 1",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query(rusqlite::params![path, pdf_scope_prefix])
        .map_err(|e| e.to_string())?;
    let Some(row) = rows.next().map_err(|e| e.to_string())? else {
        return Ok(());
    };
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
    Ok(())
}

fn append_note_pdf_annotation_backlinks(
    state: &AppState,
    path: &str,
    pdf_scope_prefix: &str,
    results: &mut Vec<BacklinkItem>,
) -> VaultResult<()> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = db
        .prepare(
            "SELECT a.id, a.page_index, a.selected_text, d.vault_relative_path
             FROM pdf_annotations a
             JOIN pdf_documents d ON a.document_id = d.document_id
             WHERE a.linked_note_path = ?1
               AND substr(d.document_id, 1, length(?2)) = ?2",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query(rusqlite::params![path, pdf_scope_prefix])
        .map_err(|e| e.to_string())?;
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
    Ok(())
}

fn dedupe_annotation_backlinks(results: &mut Vec<BacklinkItem>) {
    let annotation_note_paths: std::collections::HashSet<String> = results
        .iter()
        .filter(|item| item.context.is_some())
        .filter_map(|item| match &item.source {
            BacklinkTarget::MarkdownFile { path } => Some(path.clone()),
            _ => None,
        })
        .collect();
    results.retain(|item| {
        item.context.is_some()
            || match &item.source {
                BacklinkTarget::MarkdownFile { path } => !annotation_note_paths.contains(path),
                _ => true,
            }
    });
}

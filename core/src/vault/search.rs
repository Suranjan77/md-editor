use crate::state::AppState;
use crate::types::{
    SearchResult, SearchResultGroup, UnifiedSearchQuery, UnifiedSearchResult, UnifiedSearchSource,
};

use super::resolve_vault_path;

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
    for result in rows.flatten() {
        results.push(result);
    }
    Ok(results)
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
    let vault_root = vault_root_locked.as_ref().and_then(|root| root.as_ref());

    let is_linked = |path: &str, active_path: &str| -> bool {
        if let (Some(index), Some(root)) = (index_locked.as_ref(), vault_root) {
            let path = resolve_vault_path(root, path);
            let active_path = resolve_vault_path(root, active_path);
            index
                .outgoing
                .get(&path)
                .is_some_and(|set| set.contains(&active_path))
                || index
                    .incoming
                    .get(&path)
                    .is_some_and(|set| set.contains(&active_path))
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
        let mut stmt_md_paths = db
            .prepare("SELECT DISTINCT path FROM file_search")
            .map_err(|e| e.to_string())?;
        let mut rows_md_paths = stmt_md_paths.query([]).map_err(|e| e.to_string())?;
        while let Some(row) = rows_md_paths.next().map_err(|e| e.to_string())? {
            let path: String = row.get(0).map_err(|e| e.to_string())?;
            let filename = std::path::Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone());
            if filename.to_lowercase().contains(&query_lower) {
                let mut score = 10.0;
                if Some(path.as_str()) == active_markdown_path {
                    score *= query.ranking.current_document_boost;
                }
                let file_stem = std::path::Path::new(&filename)
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().to_string())
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

        let mut stmt_pdf_paths = db
            .prepare("SELECT vault_relative_path FROM pdf_documents")
            .map_err(|e| e.to_string())?;
        let mut rows_pdf_paths = stmt_pdf_paths.query([]).map_err(|e| e.to_string())?;
        while let Some(row) = rows_pdf_paths.next().map_err(|e| e.to_string())? {
            let path: String = row.get(0).map_err(|e| e.to_string())?;
            let filename = std::path::Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone());
            if filename.to_lowercase().contains(&query_lower) {
                let mut score = 10.0;
                if Some(path.as_str()) == active_pdf_path {
                    score *= query.ranking.current_document_boost;
                }
                let file_stem = std::path::Path::new(&filename)
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().to_string())
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
            let annotation_id: String = row.get(1).map_err(|e| e.to_string())?;
            let page_index: i32 = row.get(2).map_err(|e| e.to_string())?;
            let selected_text: String = row.get(3).map_err(|e| e.to_string())?;
            let note: Option<String> = row.get(4).map_err(|e| e.to_string())?;

            if query.includes(UnifiedSearchSource::Annotation)
                && selected_text.to_lowercase().contains(&query_lower)
            {
                results.push(annotation_search_result(
                    AnnotationSearchInput {
                        group: SearchResultGroup::Annotation,
                        path: &path,
                        annotation_id: &annotation_id,
                        page_index,
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
                        annotation_id: &annotation_id,
                        page_index,
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

            for (line_index, line) in content.lines().enumerate() {
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
                        line: line_index + 1,
                        context: search_result_preview(line, query_trimmed, None),
                        score,
                        page_index: None,
                        annotation_id: None,
                    });
                }
            }
        }
    }

    results.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.group.cmp(&right.group))
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.line.cmp(&right.line))
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
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::vault::{save_file, set_vault_root};

    fn unique_temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("md_editor_{name}_{nanos}"))
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
        let groups = results
            .iter()
            .map(|result| result.group)
            .collect::<Vec<_>>();
        assert!(groups.contains(&SearchResultGroup::Heading));

        let results_filename =
            search_vault_unified(&state, "source", Some("source.md"), None).unwrap();
        let groups_filename = results_filename
            .iter()
            .map(|result| result.group)
            .collect::<Vec<_>>();
        assert!(groups_filename.contains(&SearchResultGroup::Filename));

        let results = search_vault_unified(&state, "Rust", Some("source.md"), None).unwrap();
        let groups = results
            .iter()
            .map(|result| result.group)
            .collect::<Vec<_>>();
        assert!(groups.contains(&SearchResultGroup::MarkdownContent));

        let active_match = results
            .iter()
            .find(|result| result.path == "source.md")
            .unwrap();
        let results_non_active = search_vault_unified(&state, "Rust", None, None).unwrap();
        let non_active_match = results_non_active
            .iter()
            .find(|result| result.path == "source.md")
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

        fs::write(&abs_path, "Initial Content").unwrap();
        let metadata = fs::metadata(&abs_path).unwrap();
        let size = metadata.len();
        let modified_at = metadata
            .modified()
            .unwrap()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        state
            .save_pdf_document("doc-hash-1", pdf_path, size, Some(modified_at))
            .unwrap();
        assert!(!state.validate_and_invalidate_pdf_cache(pdf_path).unwrap());

        state
            .save_pdf_page_text(pdf_path, 0, "Initial Page Text Content")
            .unwrap();
        assert!(state.validate_and_invalidate_pdf_cache(pdf_path).unwrap());

        let results = search_cached_pdf_text(&state, "Content", &[pdf_path.to_string()]).unwrap();
        assert_eq!(results.len(), 1);

        fs::write(&abs_path, "Newer Content with different length").unwrap();
        assert!(!state.validate_and_invalidate_pdf_cache(pdf_path).unwrap());

        let results = search_cached_pdf_text(&state, "Content", &[pdf_path.to_string()]).unwrap();
        assert_eq!(results.len(), 0);

        let _ = fs::remove_dir_all(root);
    }
}

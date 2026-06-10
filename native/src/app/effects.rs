use iced::Task;
use iced::widget::operation::{self};

use std::sync::Arc;
use std::time::Instant;

use crate::editor::parser;
use crate::features::pdf::navigation::parse_pdf_link;
use crate::messages::{EditorMessage, Message, PdfMessage, SearchMessage};
use crate::views;

use super::model::*;

pub(crate) fn plain_highlight_placeholders(text: &str) -> Vec<parser::StyledLine> {
    text.split('\n')
        .enumerate()
        .map(|(idx, line)| {
            let mut styled = parser::StyledLine::new();
            styled.block_id = idx;
            styled.spans.push(parser::StyledSpan::plain(line));
            styled
        })
        .collect()
}

pub fn pdf_companion_note_key(pdf_path: &str) -> String {
    format!("pdf_companion_note:{}", pdf_path.replace('\\', "/"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FocusTarget {
    FileSearch,
    GlobalSearch,
    PdfSearch,
    CommandPalette,
    CitationPalette,
}

impl FocusTarget {
    pub(crate) fn widget_id(self) -> &'static str {
        match self {
            Self::FileSearch => views::search::FILE_SEARCH_INPUT_ID,
            Self::GlobalSearch => views::search::GLOBAL_SEARCH_INPUT_ID,
            Self::PdfSearch => views::pdf_viewer::PDF_SEARCH_INPUT_ID,
            Self::CommandPalette => views::command_palette::COMMAND_PALETTE_INPUT_ID,
            Self::CitationPalette => views::citation_palette::CITATION_PALETTE_INPUT_ID,
        }
    }
}

pub(crate) fn focus_target(target: FocusTarget) -> Task<Message> {
    operation::focus(iced::advanced::widget::Id::new(target.widget_id()))
}

pub(crate) fn focus_file_search_input() -> Task<Message> {
    focus_target(FocusTarget::FileSearch)
}

pub(crate) fn focus_global_search_input() -> Task<Message> {
    focus_target(FocusTarget::GlobalSearch)
}

pub(crate) fn focus_command_palette_input() -> Task<Message> {
    focus_target(FocusTarget::CommandPalette)
}

pub(crate) fn focus_citation_palette_input() -> Task<Message> {
    focus_target(FocusTarget::CitationPalette)
}

pub(crate) fn search_registered_pdf_text_results(
    state: &Arc<md_editor_core::state::AppState>,
    query: &md_editor_core::types::UnifiedSearchQuery,
    active_pdf_path: Option<&str>,
) -> md_editor_core::types::UnifiedPdfTextSearchResultBatch {
    let Some(renderer) = state.pdf_renderer() else {
        return empty_pdf_text_batch();
    };
    let vault_root = match state.vault_root_path() {
        Ok(Some(path)) => path,
        _ => return empty_pdf_text_batch(),
    };
    let pdf_paths = match md_editor_core::vault::list_all_pdf_files(&vault_root) {
        Ok(files) => files
            .into_iter()
            .map(|p| md_editor_core::vault::path_to_relative_string(&p, &vault_root))
            .collect::<Vec<_>>(),
        Err(_) => return empty_pdf_text_batch(),
    };
    let total_candidates = pdf_paths
        .iter()
        .filter(|path| active_pdf_path != Some(path.as_str()))
        .count();
    let targets = registered_pdf_search_targets(
        pdf_paths,
        active_pdf_path,
        GLOBAL_PDF_TEXT_SEARCH_MAX_DOCUMENTS,
    );
    let document_cap_reached = total_candidates > targets.len();

    let mut results =
        md_editor_core::vault::search_cached_pdf_text(state, query.text.trim(), &targets)
            .unwrap_or_default();
    let cached_paths = results
        .iter()
        .map(|result| result.path.clone())
        .collect::<std::collections::HashSet<_>>();
    if results.len() >= GLOBAL_PDF_TEXT_SEARCH_MAX_RESULTS {
        results.truncate(GLOBAL_PDF_TEXT_SEARCH_MAX_RESULTS);
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.line.cmp(&b.line))
        });
        return md_editor_core::types::UnifiedPdfTextSearchResultBatch {
            results,
            searched_documents: cached_paths.len(),
            total_candidates,
            result_cap_reached: true,
            document_cap_reached,
        };
    }

    let mut searched_documents = 0;
    let mut result_cap_reached = false;
    for vault_path in targets {
        if cached_paths.contains(&vault_path) {
            searched_documents += 1;
            continue;
        }
        searched_documents += 1;
        let Ok(abs_path) = md_editor_core::vault::resolve_vault_path(&vault_root, &vault_path)
        else {
            continue;
        };
        let abs_path = abs_path.to_string_lossy().to_string();
        let Ok(matches) = renderer.search_text(&abs_path, &query.text, false, false) else {
            continue;
        };

        for search_match in matches {
            let mut score = 4.0;
            if search_match
                .context
                .trim()
                .eq_ignore_ascii_case(query.text.trim())
            {
                score *= query.ranking.exact_phrase_boost;
            }
            results.push(md_editor_core::types::UnifiedSearchResult {
                group: md_editor_core::types::SearchResultGroup::PdfContent,
                path: vault_path.clone(),
                line: (search_match.page_index + 1) as usize,
                context: format!(
                    "PDF text ({} areas): {}",
                    search_match.rects.len(),
                    md_editor_core::vault::search_result_preview(
                        &search_match.context,
                        query.text.trim(),
                        None,
                    )
                ),
                score,
                page_index: Some(search_match.page_index),
                annotation_id: None,
            });
            if results.len() >= GLOBAL_PDF_TEXT_SEARCH_MAX_RESULTS {
                result_cap_reached = true;
                break;
            }
        }
        if results.len() >= GLOBAL_PDF_TEXT_SEARCH_MAX_RESULTS {
            result_cap_reached = true;
            break;
        }
    }
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.cmp(&b.line))
    });
    md_editor_core::types::UnifiedPdfTextSearchResultBatch {
        results,
        searched_documents,
        total_candidates,
        result_cap_reached,
        document_cap_reached,
    }
}

pub(crate) fn empty_pdf_text_batch() -> md_editor_core::types::UnifiedPdfTextSearchResultBatch {
    md_editor_core::types::UnifiedPdfTextSearchResultBatch {
        results: Vec::new(),
        searched_documents: 0,
        total_candidates: 0,
        result_cap_reached: false,
        document_cap_reached: false,
    }
}

pub(crate) fn format_pdf_search_status(
    batch: &md_editor_core::types::UnifiedPdfTextSearchResultBatch,
) -> String {
    let mut status = format!(
        "PDF text: searched {} of {} registered PDFs",
        batch.searched_documents, batch.total_candidates
    );
    if batch.result_cap_reached {
        status.push_str("; result cap reached");
    } else if batch.document_cap_reached {
        status.push_str("; document cap reached");
    }
    status
}

pub(crate) fn index_registered_pdf_text_pages(
    state: &Arc<md_editor_core::state::AppState>,
) -> Result<usize, String> {
    let Some(renderer) = state.pdf_renderer() else {
        return Ok(0);
    };
    let vault_root = state.vault_root_path()?;
    let Some(vault_root) = vault_root else {
        return Ok(0);
    };
    let pdf_paths = md_editor_core::vault::list_all_pdf_files(&vault_root)?
        .into_iter()
        .map(|p| md_editor_core::vault::path_to_relative_string(&p, &vault_root))
        .collect::<Vec<_>>();
    let targets = registered_pdf_index_targets(pdf_paths, PDF_TEXT_INDEX_MAX_DOCUMENTS);

    let mut indexed_pages = 0;
    for vault_path in targets {
        if state
            .validate_and_invalidate_pdf_cache(&vault_path)
            .unwrap_or(false)
        {
            continue;
        }

        let Ok(abs_path) = md_editor_core::vault::resolve_vault_path(&vault_root, &vault_path)
        else {
            continue;
        };
        let abs_path = abs_path.to_string_lossy().to_string();

        if let Ok((hash, len, mtime)) =
            md_editor_core::infrastructure::pdfium::document::compute_provisional_id(
                std::path::Path::new(&abs_path),
            )
        {
            let _ = state.save_pdf_document(&hash, &vault_path, len, mtime);
        }

        let page_count = renderer.page_count(&abs_path).unwrap_or(0);
        let pages_to_index = page_count.min(PDF_TEXT_INDEX_MAX_PAGES_PER_DOCUMENT);
        for page_index in 0..pages_to_index {
            if let Ok(page_text) = renderer.get_page_text(
                &abs_path,
                md_editor_core::domain::PageIndex::from(page_index),
            ) {
                state.save_pdf_page_text(&vault_path, page_index, &page_text.text)?;
                indexed_pages += 1;
            }
        }
    }
    Ok(indexed_pages)
}

pub(crate) fn registered_pdf_index_targets(
    pdf_paths: Vec<String>,
    max_documents: usize,
) -> Vec<String> {
    pdf_paths.into_iter().take(max_documents).collect()
}

pub(crate) fn registered_pdf_search_targets(
    pdf_paths: Vec<String>,
    active_pdf_path: Option<&str>,
    max_documents: usize,
) -> Vec<String> {
    pdf_paths
        .into_iter()
        .filter(|path| active_pdf_path != Some(path.as_str()))
        .take(max_documents)
        .collect()
}

pub fn focus_pdf_search_input() -> Task<Message> {
    focus_target(FocusTarget::PdfSearch)
}

pub(crate) fn normalize_path(path: &std::path::Path) -> String {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::Normal(c) => {
                components.push(c);
            }
            std::path::Component::CurDir => {}
            _ => {
                components.push(component.as_os_str());
            }
        }
    }
    let normalized: std::path::PathBuf = components.into_iter().collect();
    normalized.to_string_lossy().to_string().replace('\\', "/")
}

pub(crate) fn resolve_relative_link_path(
    vault_root: Option<&str>,
    active_path: Option<&str>,
    link_path: &str,
) -> String {
    if link_path.starts_with('.') {
        if let Some(active_file) = active_path {
            let active_path_buf = std::path::Path::new(active_file);
            if let Some(parent) = active_path_buf.parent() {
                let resolved = parent.join(link_path);
                return normalize_path(&resolved);
            }
        }
    }
    // If it doesn't start with '.', check if there is an existing file relative to the active path's parent.
    if let (Some(vault), Some(active_file)) = (vault_root, active_path) {
        let active_path_buf = std::path::Path::new(active_file);
        if let Some(parent) = active_path_buf.parent() {
            let relative_candidate = parent.join(link_path);
            let abs_relative = std::path::Path::new(vault).join(&relative_candidate);
            if abs_relative.exists()
                || abs_relative.with_extension("md").exists()
                || abs_relative.with_extension("markdown").exists()
            {
                return normalize_path(&relative_candidate);
            }
        }
    }
    link_path.to_string()
}

pub(crate) fn slugify(s: &str) -> String {
    crate::editor::parser::markdown_anchor_slug(s)
}

pub fn save_markdown_file_with_parser_targets(
    state: &md_editor_core::state::AppState,
    path: &str,
    content: &str,
) -> Result<(), String> {
    let markdown_link_targets = parser_index_targets(content);
    md_editor_core::vault::save_file_with_markdown_link_targets(
        state,
        path,
        content,
        &markdown_link_targets,
    )
}

pub fn reindex_markdown_file_with_parser_targets(
    state: &md_editor_core::state::AppState,
    path: &str,
    content: &str,
) -> Result<(), String> {
    let targets = parser_index_targets(content);
    state.update_file_index_targets(path, &targets)
}

pub(crate) fn reindex_vault_with_parser_targets(
    state: &md_editor_core::state::AppState,
    vault_root: &std::path::Path,
) -> Result<(), String> {
    let md_files = md_editor_core::vault::list_all_md_files(vault_root)?;
    let mut files = Vec::with_capacity(md_files.len());
    for abs_path in md_files {
        let content = std::fs::read_to_string(&abs_path)
            .map_err(|err| format!("Failed to read file {}: {err}", abs_path.display()))?;
        let targets = parser_index_targets(&content);
        files.push((abs_path, targets));
    }
    state.rebuild_file_index_with_targets(vault_root, files)
}

pub(crate) fn parser_index_targets(content: &str) -> Vec<String> {
    let highlighted = parser::highlight_markdown(content);
    let metadata = parser::extract_document_metadata(&highlighted);
    metadata
        .links
        .iter()
        .filter_map(indexable_markdown_link_target)
        .collect()
}

pub(crate) fn indexable_markdown_link_target(
    link: &crate::editor::parser::MarkdownLinkEntry,
) -> Option<String> {
    if !matches!(
        link.kind,
        crate::editor::parser::MarkdownLinkKind::Wiki
            | crate::editor::parser::MarkdownLinkKind::Inline
            | crate::editor::parser::MarkdownLinkKind::ResolvedReference
    ) {
        return None;
    }

    let target = link.target.trim();
    if target.is_empty() || target.starts_with('#') {
        return None;
    }
    if let Some(pdf_target) = parse_pdf_link(target) {
        return Some(pdf_target.path);
    }
    if has_uri_scheme(target) {
        return None;
    }
    Some(target.to_string())
}

pub(crate) fn has_uri_scheme(target: &str) -> bool {
    let Some(colon_idx) = target.find(':') else {
        return false;
    };
    let first_separator = target
        .find('/')
        .into_iter()
        .chain(target.find('\\'))
        .min()
        .unwrap_or(usize::MAX);
    colon_idx < first_separator
        && target[..colon_idx]
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

pub(crate) fn find_heading_line(text: &str, target_slug: &str) -> Option<usize> {
    for (line_idx, line_content) in text.split('\n').enumerate() {
        let trimmed = line_content.trim_start();
        if trimmed.starts_with('#') {
            let mut level = 0;
            for c in trimmed.chars() {
                if c == '#' {
                    level += 1;
                } else {
                    break;
                }
            }
            if level > 0 && level <= 6 {
                let heading_text = trimmed[level..].trim();
                if slugify(heading_text) == target_slug {
                    return Some(line_idx);
                }
            }
        }
    }
    None
}

pub(crate) fn find_heading_or_widget_line(
    text: &str,
    highlighted_lines: &[crate::editor::parser::StyledLine],
    target_slug: &str,
) -> Option<usize> {
    // If target_slug is "listing-N", we also want to look for "code-N", and vice-versa
    let alternative_slug = if let Some(num_str) = target_slug.strip_prefix("listing-") {
        Some(format!("code-{}", num_str))
    } else if let Some(num_str) = target_slug.strip_prefix("code-") {
        Some(format!("listing-{}", num_str))
    } else {
        None
    };

    let metadata = crate::editor::parser::extract_document_metadata(highlighted_lines);
    for anchor in &metadata.anchors {
        if anchor.slug.eq_ignore_ascii_case(target_slug) {
            return Some(anchor.line);
        }
        if let Some(ref alt) = alternative_slug
            && anchor.slug.eq_ignore_ascii_case(alt)
        {
            return Some(anchor.line);
        }
    }

    if let Some(line_idx) = find_heading_line(text, target_slug) {
        return Some(line_idx);
    }
    let target_slug_underscored = target_slug.replace('-', "_");

    let re_slug_str = format!(
        r#"(?i)id\s*=\s*["']{}["']|name\s*=\s*["']{}["']|\\label\s*\{{\s*{}\s*\}}|\{{\s*#\s*{}\s*\}}"#,
        regex::escape(target_slug),
        regex::escape(target_slug),
        regex::escape(target_slug),
        regex::escape(target_slug)
    );
    let re_slug = regex::Regex::new(&re_slug_str).ok()?;

    let re_under_str = format!(
        r#"(?i)id\s*=\s*["']{}["']|name\s*=\s*["']{}["']|\\label\s*\{{\s*{}\s*\}}|\{{\s*#\s*{}\s*\}}"#,
        regex::escape(&target_slug_underscored),
        regex::escape(&target_slug_underscored),
        regex::escape(&target_slug_underscored),
        regex::escape(&target_slug_underscored)
    );
    let re_under = regex::Regex::new(&re_under_str).ok()?;

    for (line_idx, line_content) in text.split('\n').enumerate() {
        if re_slug.is_match(line_content) || re_under.is_match(line_content) {
            return Some(line_idx);
        }
    }
    None
}

pub(crate) fn format_citation_item_as_markdown(
    item: &crate::messages::CitationItem,
    active_pdf_path: Option<&str>,
) -> String {
    match item {
        crate::messages::CitationItem::Selection { text, page_index } => {
            let pdf_path = active_pdf_path.unwrap_or("document.pdf");
            let link = crate::features::pdf::navigation::build_pdf_link(
                pdf_path,
                Some(page_index + 1),
                None,
            );
            format!(
                "> {}\n> [Selection (Page {})]({})\n\n",
                text.trim().replace('\n', "\n> "),
                page_index + 1,
                link
            )
        }
        crate::messages::CitationItem::Annotation {
            id,
            text,
            page_index,
        } => {
            let pdf_path = active_pdf_path.unwrap_or("document.pdf");
            let link = crate::features::pdf::navigation::build_pdf_link(
                pdf_path,
                Some(page_index + 1),
                Some(id),
            );
            format!(
                "> {}\n> [Highlight (Page {})]({})\n\n",
                text.trim().replace('\n', "\n> "),
                page_index + 1,
                link
            )
        }
        crate::messages::CitationItem::SearchHit {
            path,
            page_index,
            snippet,
        } => {
            let link =
                crate::features::pdf::navigation::build_pdf_link(path, Some(page_index + 1), None);
            format!(
                "> {}\n> [PDF Text (Page {})]({})\n\n",
                snippet.trim().replace('\n', "\n> "),
                page_index + 1,
                link
            )
        }
    }
}

impl MdEditor {
    pub(crate) fn load_pdf_page_links(&mut self, page: u16) -> Task<Message> {
        if self.pdf.page_links.contains_key(&page) || self.pdf.pending_links.contains(&page) {
            return Task::none();
        }
        let Some(path) = &self.pdf.active_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        self.pdf.pending_links.insert(page);
        let path_str = abs_path.to_string_lossy().to_string();
        let generation = self.pdf.render_generation;
        let _state = self.state.clone();

        Task::perform(
            async move {
                let renderer = _state.pdf_renderer()?;
                renderer
                    .get_page_links(&path_str, md_editor_core::domain::PageIndex::from(page))
                    .ok()
            },
            move |res| {
                Message::Pdf(PdfMessage::PageLinksLoaded(
                    generation,
                    page,
                    res.unwrap_or_default(),
                ))
            },
        )
    }

    pub(crate) fn load_pdf_page_text(&mut self, page: u16) -> Task<Message> {
        if self.pdf.page_text.contains_key(&page) || self.pdf.pending_text.contains(&page) {
            return Task::none();
        }
        let Some(path) = &self.pdf.active_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        self.pdf.pending_text.insert(page);
        let path_str = abs_path.to_string_lossy().to_string();
        let generation = self.pdf.render_generation;
        let _state = self.state.clone();

        Task::perform(
            async move {
                let renderer = _state
                    .pdf_renderer()
                    .ok_or_else(|| "No PDF renderer".to_string())?;
                renderer.get_page_text(&path_str, md_editor_core::domain::PageIndex::from(page))
            },
            move |res| Message::Pdf(PdfMessage::PageTextLoaded(generation, page, res)),
        )
    }

    pub(crate) fn update_pdf_page_cache(&mut self) {
        let first = self.pdf_page_at_scroll(self.pdf.scroll_y);
        let viewport_height = if self.pdf.viewport_height > 0.0 {
            self.pdf.viewport_height
        } else {
            self.estimated_editor_viewport_height()
        };
        let last = self.pdf_page_at_scroll(self.pdf.scroll_y + viewport_height);

        // Clamp to document range
        let first = first.min(self.pdf.total_pages.saturating_sub(1));
        let last = last.min(self.pdf.total_pages.saturating_sub(1));

        let range = if self.pdf.total_pages > 0 {
            Some((first, last.max(first)))
        } else {
            None
        };
        self.pdf.view.page_cache.set_visible_range(range);
        self.pdf.view.page_cache.touch_visible();
    }

    pub(crate) fn sync_pdf_pages_to_cache(&mut self) {
        for (idx, page) in self.pdf.pages.iter_mut().enumerate() {
            if page.is_some() && !self.pdf.view.page_cache.contains(idx as u16) {
                *page = None;
                self.pdf.stale_pages.remove(&(idx as u16));
            }
        }
    }

    pub(crate) fn highlight_all(&mut self) -> Task<Message> {
        self.refresh_highlighting_for_current_buffer(false)
    }

    pub(crate) fn refresh_highlighting_for_current_buffer(
        &mut self,
        opened_file: bool,
    ) -> Task<Message> {
        let text = self.editor.buffer.text();
        let line_count = self.editor.buffer.line_count();
        self.editor.highlight_generation = self.editor.highlight_generation.wrapping_add(1);
        let generation = self.editor.highlight_generation;
        self.editor.pending_highlight_generation = None;
        self.editor.pending_highlight_requested_at = None;
        self.editor.pending_highlight_text = None;

        if opened_file && line_count > HUGE_DOC_LINE_THRESHOLD {
            self.editor.highlighted_lines = plain_highlight_placeholders(&text);
            self.editor.toc_entries = views::toc::get_toc(&self.editor.highlighted_lines);
            return Self::highlight_task(generation, text);
        }

        if !opened_file && line_count > LARGE_DOC_LINE_THRESHOLD {
            self.editor.pending_highlight_generation = Some(generation);
            self.editor.pending_highlight_requested_at = Some(Instant::now());
            self.editor.pending_highlight_text = Some(text);
            return Task::none();
        }

        self.editor.highlighted_lines = parser::highlight_markdown(&text);
        self.editor.toc_entries = views::toc::get_toc(&self.editor.highlighted_lines);
        Task::batch(vec![self.load_images(), self.load_math()])
    }

    pub(crate) fn highlight_task(generation: u64, text: String) -> Task<Message> {
        Task::perform(
            async move { parser::highlight_markdown(&text) },
            move |lines| Message::Editor(EditorMessage::HighlightReady(generation, lines)),
        )
    }

    pub(crate) fn search_registered_pdf_text_task(
        &self,
        search_id: u64,
        query: md_editor_core::types::UnifiedSearchQuery,
    ) -> Task<Message> {
        let state = self.state.clone();
        let active_pdf_path = self.pdf.active_path.clone();

        Task::perform(
            async move {
                let results = tokio::task::spawn_blocking(move || {
                    search_registered_pdf_text_results(&state, &query, active_pdf_path.as_deref())
                })
                .await
                .unwrap_or_default();
                (search_id, results)
            },
            |(id, results)| Message::Search(SearchMessage::UnifiedPdfMatchesFound(id, results)),
        )
    }

    pub(crate) fn index_registered_pdf_text_task(&self) -> Task<Message> {
        let state = self.state.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || index_registered_pdf_text_pages(&state))
                    .await
                    .unwrap_or_else(|err| Err(err.to_string()))
            },
            |result| Message::Search(SearchMessage::PdfTextIndexFinished(result)),
        )
    }

    pub(crate) fn search_pdf(&mut self) -> Task<Message> {
        let Some(path) = &self.pdf.active_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        let query = self.pdf.view.search.query.clone();
        if query.trim().is_empty() {
            self.pdf.view.search.matches.clear();
            self.pdf.view.search.page_index.clear();
            self.pdf.view.search.searching = false;
            return Task::none();
        }
        let regex = self.pdf.view.search.regex;
        let match_case = self.pdf.view.search.match_case;
        let path_str = abs_path.to_string_lossy().to_string();

        let Some(renderer) = self.state.pdf_renderer() else {
            return Task::none();
        };

        // Increment active search id and set searching = true
        self.pdf.view.search.searching = true;
        self.search.pdf_active_id = self.search.pdf_active_id.wrapping_add(1);
        let search_id = self.search.pdf_active_id;

        // Cancel previous search
        let _ = renderer.cancel_search(search_id.wrapping_sub(1));

        self.pdf.view.search.matches.clear();
        self.pdf.view.search.page_index.clear();

        match renderer.search_text_stream(path_str, query, regex, match_case, search_id) {
            Ok((res_rx, done_rx)) => {
                let (tokio_tx, tokio_rx) = tokio::sync::mpsc::channel(100);

                tokio::task::spawn_blocking(move || {
                    while let Ok(m) = res_rx.recv() {
                        if tokio_tx
                            .blocking_send(Message::Pdf(PdfMessage::SearchMatchesFound(
                                search_id,
                                vec![m],
                            )))
                            .is_err()
                        {
                            return;
                        }
                    }
                    let res = done_rx.recv().unwrap_or(Ok(()));
                    let _ = tokio_tx
                        .blocking_send(Message::Pdf(PdfMessage::SearchFinished(search_id, res)));
                });

                let stream = iced::futures::stream::unfold(tokio_rx, |mut rx| async move {
                    if let Some(msg) = rx.recv().await {
                        Some((msg, rx))
                    } else {
                        None
                    }
                });

                Task::stream(stream)
            }
            Err(err) => {
                self.search.pdf_error = Some(err);
                self.pdf.view.search.searching = false;
                Task::none()
            }
        }
    }

    pub(crate) fn load_images(&mut self) -> Task<Message> {
        let mut failures = Vec::new();
        let Some(active_path) = &self.workspace.active_path else {
            return Task::none();
        };
        let Some(vault_root) = &self.workspace.vault_root else {
            return Task::none();
        };
        let Some(base_path) = std::path::Path::new(vault_root)
            .join(active_path)
            .parent()
            .map(|path| path.to_path_buf())
        else {
            return Task::none();
        };

        for line in &self.editor.highlighted_lines {
            for span in &line.spans {
                if span.is_image {
                    if let Some(path) = &span.image_path {
                        if !self.editor.image_cache.contains_key(path)
                            && !self.editor.image_errors.contains_key(path)
                        {
                            let img_path = base_path.join(path);
                            match image::open(&img_path) {
                                Ok(img) => {
                                    self.editor.image_errors.remove(path);
                                    let width = img.width();
                                    let height = img.height();
                                    let handle = iced::widget::image::Handle::from_rgba(
                                        width,
                                        height,
                                        img.into_rgba8().into_raw(),
                                    );
                                    self.editor.image_cache.insert(
                                        path.clone(),
                                        (handle, width as f32, height as f32),
                                    );
                                }
                                Err(err) => failures.push(Task::done(Message::Editor(
                                    EditorMessage::ImageLoadFailed(path.clone(), err.to_string()),
                                ))),
                            }
                        }
                    }
                }
            }
        }
        Task::batch(failures)
    }

    pub(crate) fn load_math(&self) -> Task<Message> {
        let mut tasks = Vec::new();
        for line in &self.editor.highlighted_lines {
            for span in &line.spans {
                if span.is_math {
                    let tex = span
                        .visible_text(false)
                        .trim_matches('$')
                        .trim()
                        .to_string();
                    if !tex.is_empty()
                        && !self.editor.math_cache.contains_key(&tex)
                        && !self.editor.math_errors.contains_key(&tex)
                    {
                        let tex_clone = tex.clone();
                        tasks.push(Task::perform(
                            async move { (tex_clone.clone(), Self::render_latex_task(&tex_clone)) },
                            |(t, r)| Message::Editor(EditorMessage::MathRendered(t, r)),
                        ));
                    }
                }
            }
        }
        Task::batch(tasks)
    }

    pub(crate) fn render_latex_task(
        tex: &str,
    ) -> Result<(iced::widget::image::Handle, f32, f32), String> {
        use ratex_layout::{LayoutOptions, layout, to_display_list};
        use ratex_parser::parser::parse;
        use ratex_render::{RenderOptions, render_to_png};
        use ratex_types::color::Color as RatexColor;
        use ratex_types::math_style::MathStyle;

        let options = RenderOptions {
            font_size: 24.0,
            padding: 4.0,
            background_color: RatexColor {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.0,
            },
            font_dir: String::new(),
            device_pixel_ratio: 2.0,
        };

        let layout_opts = LayoutOptions::default()
            .with_style(MathStyle::Display)
            .with_color(RatexColor {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            });

        let ast = parse(tex).map_err(|e| format!("Parse error: {}", e))?;
        let lbox = layout(&ast, &layout_opts);
        let display_list = to_display_list(&lbox);
        let bytes =
            render_to_png(&display_list, &options).map_err(|e| format!("Render error: {:?}", e))?;

        let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
        let w = img.width();
        let h = img.height();
        Ok((
            iced::widget::image::Handle::from_bytes(bytes),
            w as f32 / 2.0,
            h as f32 / 2.0,
        ))
    }
}

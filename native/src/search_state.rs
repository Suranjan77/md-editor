//! Search sub-state.
//!
//! Owns the in-document find/replace fields, the vault-wide search results,
//! and the PDF search results, plus the memoized in-document match cache. The
//! shell still drives the cross-cutting effects (moving the editor cursor,
//! scrolling, launching PDF search tasks) but reads/writes search data through
//! this struct.
//!
//! Per `docs/refactor-mdeditor-decomposition.md`, the buffer revision that
//! invalidates the match cache stays owned by the editor side and is passed
//! into [`SearchState::ensure_matches`].

use std::collections::HashMap;

use crate::editor::buffer::DocBuffer;
use crate::search::DocumentMatch;

use md_editor_core::pdf::PdfSearchMatch;
use md_editor_core::types::SearchResult;

/// Identity of a computed in-document match set. When any component changes the
/// cached matches are stale and must be rebuilt.
#[derive(Clone, PartialEq, Eq)]
struct DocMatchKey {
    buffer_revision: u64,
    query: String,
    regex: bool,
    match_case: bool,
    active_path: Option<String>,
}

#[derive(Default)]
pub struct SearchState {
    pub visible: bool,
    pub file_visible: bool,
    pub query: String,
    pub replace: String,
    pub regex: bool,
    pub match_case: bool,
    pub match_index: Option<usize>,
    pub results: Vec<SearchResult>,
    pub pdf_results: Vec<PdfSearchMatch>,
    pub pdf_indices_by_page: HashMap<u16, Vec<usize>>,
    pub pdf_error: Option<String>,

    // Memoized in-document matches; rebuilt only when `doc_match_key` changes.
    doc_match_cache: Vec<DocumentMatch>,
    doc_match_key: Option<DocMatchKey>,
}

impl SearchState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn match_count(&self) -> usize {
        self.doc_match_cache.len()
    }

    pub fn matches(&self) -> &[DocumentMatch] {
        &self.doc_match_cache
    }

    pub fn active_match_position(&self) -> Option<(usize, usize)> {
        let index = self.match_index?;
        self.doc_match_cache
            .get(index.min(self.doc_match_cache.len().saturating_sub(1)))
            .map(|m| (m.line, m.start_col))
    }

    /// Rebuild the cached in-document matches only when the buffer or search
    /// parameters have changed. Called once per `update` so `view` can read the
    /// matches cheaply without rescanning the buffer.
    pub fn ensure_matches(
        &mut self,
        buffer: &DocBuffer,
        active_path: Option<&str>,
        buffer_revision: u64,
    ) {
        let key = DocMatchKey {
            buffer_revision,
            query: self.query.clone(),
            regex: self.regex,
            match_case: self.match_case,
            active_path: active_path.map(str::to_string),
        };
        if self.doc_match_key.as_ref() == Some(&key) {
            return;
        }

        self.doc_match_cache =
            compute_matches(buffer, &self.query, self.regex, self.match_case, active_path.is_some());
        self.doc_match_key = Some(key);
    }

    pub fn rebuild_pdf_page_index(&mut self) {
        self.pdf_indices_by_page.clear();
        for (idx, result) in self.pdf_results.iter().enumerate() {
            self.pdf_indices_by_page
                .entry(result.page_index)
                .or_default()
                .push(idx);
        }
    }
}

fn compute_matches(
    buffer: &DocBuffer,
    query: &str,
    regex: bool,
    match_case: bool,
    has_active_path: bool,
) -> Vec<DocumentMatch> {
    if query.is_empty() || !has_active_path {
        return Vec::new();
    }

    (0..buffer.line_count())
        .flat_map(|line| {
            let text = buffer.line_text(line);
            crate::search::line_matches(&text, query, regex, match_case)
                .into_iter()
                .map(move |line_match| DocumentMatch {
                    line,
                    start_col: line_match.start_col,
                    end_col: line_match.end_col,
                })
        })
        .collect()
}

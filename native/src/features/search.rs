use std::collections::HashMap;

use crate::messages::SearchWrapStatus;

pub(crate) type EditorMatch = md_editor_core::types::SearchResult;

#[derive(Debug, Clone, Default)]
pub(crate) struct EditorSearchState {
    pub(crate) query: String,
    pub(crate) replace: String,
    pub(crate) regex: bool,
    pub(crate) match_case: bool,
    pub(crate) matches: Vec<EditorMatch>,
    pub(crate) active_index: Option<usize>,
    pub(crate) visible: bool,
    pub(crate) wrap_status: Option<SearchWrapStatus>,
}

impl EditorSearchState {
    #[cfg(test)]
    pub(crate) fn reset_results(&mut self) {
        self.matches.clear();
        self.active_index = None;
        self.wrap_status = None;
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PdfSearchState {
    pub(crate) query: String,
    pub(crate) regex: bool,
    pub(crate) match_case: bool,
    pub(crate) matches: Vec<md_editor_core::application::pdf_service::PdfSearchMatch>,
    pub(crate) active_index: Option<usize>,
    pub(crate) page_index: HashMap<u16, Vec<usize>>,
    pub(crate) searching: bool,
    pub(crate) visible: bool,
}

impl PdfSearchState {
    #[cfg(test)]
    pub(crate) fn reset_results(&mut self) {
        self.matches.clear();
        self.active_index = None;
        self.page_index.clear();
        self.searching = false;
    }
}

#[derive(Debug)]
pub(crate) struct GlobalSearchState {
    pub(crate) id: u64,
    pub(crate) pdf_search_id: Option<u64>,
    pub(crate) pending_db: bool,
    pub(crate) pending_pdf: bool,
    pub(crate) pending_vault_pdf: bool,
    pub(crate) pdf_status: Option<String>,
    pub(crate) sources: Vec<md_editor_core::types::UnifiedSearchSource>,
    pub(crate) results: Vec<md_editor_core::types::UnifiedSearchResult>,
    pub(crate) searching: bool,
    pub(crate) error: Option<String>,
}

impl Default for GlobalSearchState {
    fn default() -> Self {
        Self {
            id: 0,
            pdf_search_id: None,
            pending_db: false,
            pending_pdf: false,
            pending_vault_pdf: false,
            pdf_status: None,
            sources: md_editor_core::types::UnifiedSearchQuery::all_sources("").sources,
            results: Vec::new(),
            searching: false,
            error: None,
        }
    }
}

impl GlobalSearchState {
    pub(crate) fn update_searching(&mut self) {
        self.searching = self.pending_db || self.pending_pdf || self.pending_vault_pdf;
    }

    #[cfg(test)]
    pub(crate) fn reset_results(&mut self) {
        self.results.clear();
        self.error = None;
        self.pdf_status = None;
        self.pdf_search_id = None;
        self.pending_db = false;
        self.pending_pdf = false;
        self.pending_vault_pdf = false;
        self.searching = false;
    }
}

#[derive(Debug, Default)]
pub(crate) struct SearchState {
    pub(crate) visible: bool,
    pub(crate) editor: EditorSearchState,
    pub(crate) global: GlobalSearchState,
    pub(crate) pdf_error: Option<String>,
    pub(crate) pdf_active_id: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_enables_all_global_sources() {
        let state = SearchState::default();
        let expected = md_editor_core::types::UnifiedSearchQuery::all_sources("").sources;

        assert!(!state.visible);
        assert_eq!(state.global.sources, expected);
        assert!(!state.global.searching);
        assert!(state.pdf_error.is_none());
        assert_eq!(state.pdf_active_id, 0);
    }

    #[test]
    fn editor_result_reset_preserves_query_and_options() {
        let mut state = EditorSearchState {
            query: "needle".to_string(),
            replace: "replacement".to_string(),
            regex: true,
            match_case: true,
            matches: vec![EditorMatch {
                path: "note.md".to_string(),
                line: 1,
                context: "needle".to_string(),
            }],
            active_index: Some(0),
            wrap_status: Some(SearchWrapStatus::WrappedForward),
            visible: true,
        };

        state.reset_results();

        assert_eq!(state.query, "needle");
        assert_eq!(state.replace, "replacement");
        assert!(state.regex);
        assert!(state.match_case);
        assert!(state.visible);
        assert!(state.matches.is_empty());
        assert!(state.active_index.is_none());
        assert!(state.wrap_status.is_none());
    }

    #[test]
    fn global_result_reset_clears_pending_work_but_preserves_sources_and_generation() {
        let mut state = GlobalSearchState {
            id: 7,
            pdf_search_id: Some(8),
            pending_db: true,
            pending_pdf: true,
            pending_vault_pdf: true,
            pdf_status: Some("Searching PDFs".to_string()),
            results: vec![md_editor_core::types::UnifiedSearchResult {
                group: md_editor_core::types::SearchResultGroup::Filename,
                path: "note.md".to_string(),
                line: 0,
                context: "note.md".to_string(),
                score: 1.0,
                page_index: None,
                annotation_id: None,
            }],
            searching: true,
            error: Some("failed".to_string()),
            ..GlobalSearchState::default()
        };
        let sources = state.sources.clone();

        state.reset_results();

        assert_eq!(state.id, 7);
        assert_eq!(state.sources, sources);
        assert!(state.results.is_empty());
        assert!(state.error.is_none());
        assert!(state.pdf_status.is_none());
        assert!(state.pdf_search_id.is_none());
        assert!(!state.pending_db);
        assert!(!state.pending_pdf);
        assert!(!state.pending_vault_pdf);
        assert!(!state.searching);
    }

    #[test]
    fn pdf_result_reset_preserves_query_and_options() {
        let mut state = PdfSearchState {
            query: "needle".to_string(),
            regex: true,
            match_case: true,
            active_index: Some(0),
            searching: true,
            visible: true,
            ..PdfSearchState::default()
        };
        state.page_index.insert(0, vec![0]);

        state.reset_results();

        assert_eq!(state.query, "needle");
        assert!(state.regex);
        assert!(state.match_case);
        assert!(state.visible);
        assert!(state.matches.is_empty());
        assert!(state.page_index.is_empty());
        assert!(state.active_index.is_none());
        assert!(!state.searching);
    }
}

use std::collections::HashMap;

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

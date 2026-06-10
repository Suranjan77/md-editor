use crate::state::AppState;
use crate::domain::{SearchResult, UnifiedSearchQuery, UnifiedSearchResult};

pub struct SearchService<'a> {
    state: &'a AppState,
}

impl<'a> SearchService<'a> {
    pub const fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub fn markdown(&self, query: &str) -> Result<Vec<SearchResult>, String> {
        crate::vault::search_vault(self.state, query)
    }

    pub fn unified(&self, query: &UnifiedSearchQuery) -> Result<Vec<UnifiedSearchResult>, String> {
        crate::vault::search_vault_unified_query(self.state, query)
    }
}

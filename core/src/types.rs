use serde::{Deserialize, Serialize};

/// A file entry in the vault listing.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SearchResult {
    pub path: String,
    pub line: usize,
    pub context: String,
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SearchResultGroup {
    MarkdownContent,
    PdfContent,
    Filename,
    Heading,
    Annotation,
    QuickNote,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UnifiedSearchResult {
    pub group: SearchResultGroup,
    pub path: String,
    pub line: usize,
    pub context: String,
    pub score: f32,
    pub page_index: Option<u16>,
    pub annotation_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct UnifiedPdfTextSearchResultBatch {
    pub results: Vec<UnifiedSearchResult>,
    pub searched_documents: usize,
    pub total_candidates: usize,
    pub result_cap_reached: bool,
    pub document_cap_reached: bool,
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnifiedSearchSource {
    MarkdownContent,
    PdfContent,
    Filename,
    Heading,
    Annotation,
    QuickNote,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UnifiedSearchRanking {
    pub current_document_boost: f32,
    pub exact_phrase_boost: f32,
    pub linked_note_boost: f32,
}

impl Default for UnifiedSearchRanking {
    fn default() -> Self {
        Self {
            current_document_boost: 1.5,
            exact_phrase_boost: 2.0,
            linked_note_boost: 1.3,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UnifiedSearchQuery {
    pub text: String,
    pub sources: Vec<UnifiedSearchSource>,
    pub active_markdown_path: Option<String>,
    pub active_pdf_path: Option<String>,
    pub ranking: UnifiedSearchRanking,
}

impl UnifiedSearchQuery {
    pub fn all_sources(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            sources: vec![
                UnifiedSearchSource::MarkdownContent,
                UnifiedSearchSource::PdfContent,
                UnifiedSearchSource::Filename,
                UnifiedSearchSource::Heading,
                UnifiedSearchSource::Annotation,
                UnifiedSearchSource::QuickNote,
            ],
            active_markdown_path: None,
            active_pdf_path: None,
            ranking: UnifiedSearchRanking::default(),
        }
    }

    pub fn with_active_paths(
        mut self,
        active_markdown_path: Option<&str>,
        active_pdf_path: Option<&str>,
    ) -> Self {
        self.active_markdown_path = active_markdown_path.map(str::to_string);
        self.active_pdf_path = active_pdf_path.map(str::to_string);
        self
    }

    pub fn includes(&self, source: UnifiedSearchSource) -> bool {
        self.sources.contains(&source)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum BacklinkTarget {
    MarkdownFile {
        path: String,
    },
    PdfDocument {
        path: String,
    },
    PdfAnnotation {
        document_path: String,
        annotation_id: String,
        page: u16,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BacklinkItem {
    pub source: BacklinkTarget,
    pub label: String,
    pub context: Option<String>,
}

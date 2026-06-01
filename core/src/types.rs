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

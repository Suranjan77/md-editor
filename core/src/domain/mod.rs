//! Validated domain units used at core boundaries.

pub mod id;
pub mod links;
pub mod note;
pub mod path;
pub mod pdf;
pub mod pdf_page;
pub mod search;
pub mod session;

pub use links::{BacklinkItem, BacklinkTarget};
pub use note::FileEntry;
pub use path::{AbsPath, AbsPathError, VaultPath, VaultPathError};
pub use pdf_page::{PageIndex, PageNumber, PageNumberError};
pub use search::{
    SearchResult, SearchResultGroup, UnifiedPdfTextSearchResultBatch, UnifiedSearchQuery,
    UnifiedSearchRanking, UnifiedSearchResult, UnifiedSearchSource,
};
pub use session::{StudySession, TrackerKv};

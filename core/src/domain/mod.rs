//! Validated domain units used at core boundaries.

pub mod path;
pub mod pdf;
pub mod pdf_page;

pub use path::{AbsPath, AbsPathError, VaultPath, VaultPathError};
pub use pdf_page::{PageIndex, PageNumber, PageNumberError};

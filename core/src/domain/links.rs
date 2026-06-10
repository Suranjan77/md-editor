//! Backlink graph types shared between vault notes and PDF documents.

use serde::{Deserialize, Serialize};

/// The source a backlink originates from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum BacklinkTarget {
    /// A markdown note in the vault.
    MarkdownFile {
        /// Vault-relative path of the note.
        path: String,
    },
    /// A PDF document in the vault.
    PdfDocument {
        /// Vault-relative path of the document.
        path: String,
    },
    /// A specific annotation inside a PDF document.
    PdfAnnotation {
        /// Vault-relative path of the document.
        document_path: String,
        /// Identifier of the annotation.
        annotation_id: String,
        /// 1-based page label the annotation lives on.
        page: u16,
    },
}

/// One entry in a backlinks listing.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BacklinkItem {
    /// Where the link comes from.
    pub source: BacklinkTarget,
    /// Human-readable label for the source.
    pub label: String,
    /// Optional surrounding context snippet.
    pub context: Option<String>,
}

//! Vault core for MD Editor: typed errors, atomic saves,
//! debounced fs watcher, FTS5 incremental index, link graph + rename
//! repair, hash-keyed PDF annotations. No UI dependencies; everything is
//! callable from tests.

pub mod annotations;
pub mod asset_sizes;
pub mod atomic;
pub mod error;
pub mod index;
pub mod links;
pub mod migrations;
pub mod session;
pub mod tracker;
pub mod watcher;

pub use annotations::{
    Annotation, AnnotationStore, KnownDocument, NewAnnotation, Quad, document_hash,
};
pub use asset_sizes::AssetSizeStore;
pub use atomic::atomic_save;
pub use error::VaultError;
pub use index::{Hit, SearchIndex, SyncReport, TextExtractor};
pub use links::{LinkGraph, WikiLink, extract_wikilinks, resolve_target, rewrite_links};
pub use session::SessionStore;
pub use tracker::{StudySession, TrackerKv, TrackerStore};
pub use watcher::{DEFAULT_DEBOUNCE, VaultWatcher};

//! Vault core for md-editor v3 (plan §3.4): typed errors, atomic saves,
//! debounced fs watcher, FTS5 incremental index, link graph + rename
//! repair. No UI dependencies; everything is callable from tests.

pub mod atomic;
pub mod error;
pub mod index;
pub mod links;
pub mod watcher;

pub use atomic::atomic_save;
pub use error::VaultError;
pub use index::{Hit, SearchIndex, SyncReport, TextExtractor};
pub use links::{LinkGraph, WikiLink, extract_wikilinks, resolve_target, rewrite_links};
pub use watcher::{DEFAULT_DEBOUNCE, VaultWatcher};

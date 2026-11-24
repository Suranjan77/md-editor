//! md-editor-core: the headless engine — vault, index, search, SQLite state, PDF and PPTX readers. No UI.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations, clippy::unwrap_used)]

pub mod attachments;
pub mod config;
pub mod file_index;
pub mod pdf;
pub mod pptx;
pub mod references;
pub mod state;
pub mod tracker;
pub mod types;
pub mod vault;

pub use state::AppState;

#[cfg(test)]
mod massive_tests;

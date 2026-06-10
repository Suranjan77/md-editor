pub mod config;
mod database;
pub mod domain;

pub mod state;
pub mod tracker;
pub mod vault;

/// Transitional alias for the dissolved `types` module (P2.T1); import from
/// `domain` instead. Removed in P2.T6.
pub use domain as types;

pub use database::DatabaseError;
pub use state::AppState;

#[cfg(test)]
#[path = "../pdfium_build_paths.rs"]
mod pdfium_build_paths;

pub mod application;
pub mod infrastructure;

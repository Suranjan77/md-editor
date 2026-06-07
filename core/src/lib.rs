pub mod config;
mod database;
pub mod domain;
pub mod file_index;

pub mod state;
pub mod tracker;
pub mod types;
pub mod vault;

pub use state::AppState;

#[cfg(test)]
#[path = "../pdfium_build_paths.rs"]
mod pdfium_build_paths;

pub mod application;
pub mod infrastructure;
#[cfg(test)]
mod massive_tests;

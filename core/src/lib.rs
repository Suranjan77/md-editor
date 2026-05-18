pub mod config;
pub mod file_index;
pub mod pdf;
pub mod state;
pub mod tracker;
pub mod types;
pub mod vault;

pub use state::AppState;

#[cfg(test)]
mod massive_tests;

//! Vault core for md-editor v3 (plan §3.4) — typed errors and vault safety
//! primitives. The watcher, FTS5 index, and link graph land in later sessions
//! (handoff §deferred); the error discipline and atomic-save contract land
//! first because everything else builds on them.

pub mod atomic;
pub mod error;

pub use atomic::atomic_save;
pub use error::VaultError;

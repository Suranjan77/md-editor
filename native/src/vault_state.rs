//! Vault navigation sub-state.
//!
//! Owns the vault root, the file-tree listing, sidebar selection/expansion,
//! and the backlinks panel. The shell still drives the file operations (open,
//! create, delete) but reads/writes vault navigation through `self.vault`.
//!
//! Note: `active_path` (the currently open document) stays on the shell — it's
//! a cross-cutting "current document" concern shared by the editor, search,
//! and PDF sides, not vault navigation.
//!
//! Part of the `MdEditor` decomposition; see
//! `docs/refactor-mdeditor-decomposition.md`.

use std::collections::BTreeSet;

use md_editor_core::types::{BacklinkItem, FileEntry};

pub struct VaultState {
    pub root: Option<String>,
    pub entries: Vec<FileEntry>,
    pub selected_path: Option<String>,
    pub expanded_folders: BTreeSet<String>,
    pub sidebar_visible: bool,
    pub backlinks_visible: bool,
    pub backlinks: Vec<BacklinkItem>,
}

impl VaultState {
    pub fn new() -> Self {
        Self {
            root: None,
            entries: Vec::new(),
            selected_path: None,
            expanded_folders: BTreeSet::new(),
            sidebar_visible: true,
            backlinks_visible: false,
            backlinks: Vec::new(),
        }
    }
}

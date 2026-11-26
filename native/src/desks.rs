//! Saved workspace layouts ("desks") persisted per vault in SQLite `settings`.

use serde::{Deserialize, Serialize};

use crate::app::workspace::{PaneId, SplitAxis};
use crate::tabs::Tabs;

#[cfg(test)]
mod tests;

/// A named snapshot of a vault's two-pane workspace, tabs, viewer state, and panel layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Desk {
    pub name: String,
    pub pane_a_tabs: String,
    pub pane_b_tabs: Option<String>,
    pub axis: SplitAxis,
    pub split_ratio: f32,
    pub focused: PaneId,
    pub sidebar_visible: bool,
    pub backlinks_visible: bool,
    pub toc_visible: bool,
}

impl Desk {
    /// Total valid tabs across Pane A and Pane B (if split).
    pub fn tab_count(&self) -> usize {
        let a = Tabs::json_tab_count(&self.pane_a_tabs);
        let b = self.pane_b_tabs.as_deref().map_or(0, Tabs::json_tab_count);
        a + b
    }

    /// Human-readable summary shown in the Desks dialog and Command Palette.
    pub fn summary(&self) -> String {
        let panes = if self.pane_b_tabs.is_some() {
            "Split"
        } else {
            "1 pane"
        };
        let count = self.tab_count();
        let noun = if count == 1 { "tab" } else { "tabs" };
        format!("{panes} · {count} {noun}")
    }
}

/// Collection of saved desks for one vault, ordered most-recently-saved first.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DeskStore {
    desks: Vec<Desk>,
}

impl DeskStore {
    pub fn list(&self) -> &[Desk] {
        &self.desks
    }

    pub fn get(&self, name: &str) -> Option<&Desk> {
        let trimmed = name.trim();
        self.desks
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(trimmed))
    }

    /// Save a desk, replacing any existing desk with the same name (case-insensitively)
    /// and moving it to the top of the list.
    pub fn save(&mut self, mut desk: Desk) -> Result<(), String> {
        let trimmed = desk.name.trim().to_string();
        if trimmed.is_empty() {
            return Err("Desk name cannot be empty".to_string());
        }
        desk.name = trimmed;
        if !desk.split_ratio.is_finite() {
            desk.split_ratio = 0.5;
        }
        desk.split_ratio = desk.split_ratio.clamp(0.2, 0.8);
        self.desks
            .retain(|existing| !existing.name.eq_ignore_ascii_case(&desk.name));
        self.desks.insert(0, desk);
        Ok(())
    }

    /// Delete a saved desk by name (case-insensitively). Returns `true` if removed.
    pub fn delete(&mut self, name: &str) -> bool {
        let trimmed = name.trim();
        let before = self.desks.len();
        self.desks
            .retain(|existing| !existing.name.eq_ignore_ascii_case(trimmed));
        self.desks.len() != before
    }

    /// Propagate a vault file or folder rename into all saved desks' tab snapshots.
    pub fn rename_path(&mut self, old_path: &str, new_path: &str) -> bool {
        let mut changed = false;
        for desk in &mut self.desks {
            if let Some(updated) = Tabs::rewrite_json_paths(&desk.pane_a_tabs, old_path, new_path) {
                desk.pane_a_tabs = updated;
                changed = true;
            }
            if let Some(ref raw_b) = desk.pane_b_tabs
                && let Some(updated) = Tabs::rewrite_json_paths(raw_b, old_path, new_path)
            {
                desk.pane_b_tabs = Some(updated);
                changed = true;
            }
        }
        changed
    }

    /// Prune a deleted vault file or folder from all saved desks' tab snapshots.
    pub fn remove_deleted_path(&mut self, deleted_path: &str) -> bool {
        let mut changed = false;
        for desk in &mut self.desks {
            if let Some(updated) = Tabs::remove_path_from_json(&desk.pane_a_tabs, deleted_path) {
                desk.pane_a_tabs = updated;
                changed = true;
            }
            if let Some(ref raw_b) = desk.pane_b_tabs
                && let Some(updated) = Tabs::remove_path_from_json(raw_b, deleted_path)
            {
                desk.pane_b_tabs = Some(updated);
                changed = true;
            }
        }
        changed
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_json(raw: &str) -> Self {
        match serde_json::from_str::<Self>(raw) {
            Ok(mut store) => {
                for desk in &mut store.desks {
                    if !desk.split_ratio.is_finite() {
                        desk.split_ratio = 0.5;
                    }
                    desk.split_ratio = desk.split_ratio.clamp(0.2, 0.8);
                }
                store
            }
            Err(err) => {
                log::warn!("Ignoring corrupt saved desks payload: {err}");
                Self::default()
            }
        }
    }

    pub fn load(state: &md_editor_core::state::AppState, vault_root: &str) -> Self {
        let key = storage_key(vault_root);
        match md_editor_core::config::get_sys_config(state, &key) {
            Ok(Some(raw)) if !raw.trim().is_empty() => Self::from_json(&raw),
            _ => Self::default(),
        }
    }

    pub fn persist(&self, state: &md_editor_core::state::AppState, vault_root: &str) {
        let key = storage_key(vault_root);
        let _ = md_editor_core::config::set_sys_config(state, &key, &self.to_json());
    }
}

fn storage_key(vault_root: &str) -> String {
    format!("desks:{vault_root}")
}

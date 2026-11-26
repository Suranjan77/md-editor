//! Saved named graph pinboards persisted per vault in SQLite `settings`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{GraphScope, MAX_ZOOM, MIN_ZOOM};

#[cfg(test)]
mod tests;

/// Saved coordinates of a pinned graph node.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PinPosition {
    pub x: f32,
    pub y: f32,
}

/// Named snapshot of pinned graph node positions, filters, scope, and camera.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pinboard {
    pub name: String,
    #[serde(default)]
    pub pinned_positions: BTreeMap<String, PinPosition>,
    pub scope: GraphScope,
    #[serde(default = "default_true")]
    pub show_pdfs: bool,
    #[serde(default = "default_true")]
    pub show_sketches: bool,
    #[serde(default = "default_true")]
    pub show_missing: bool,
    #[serde(default = "default_true")]
    pub show_orphans: bool,
    #[serde(default)]
    pub pan: (f32, f32),
    #[serde(default = "default_zoom")]
    pub zoom: f32,
}

fn default_true() -> bool {
    true
}

fn default_zoom() -> f32 {
    1.0
}

impl Pinboard {
    /// Human-readable summary shown in the Pinboards dialog.
    pub fn summary(&self) -> String {
        let count = self.pinned_positions.len();
        let pin_noun = if count == 1 { "pin" } else { "pins" };
        let scope_label = match self.scope {
            GraphScope::Global => "Global".to_string(),
            GraphScope::Local { depth } => {
                let hop = if depth == 1 { "hop" } else { "hops" };
                format!("Local {depth} {hop}")
            }
        };
        format!("{count} {pin_noun} · {scope_label}")
    }

    fn sanitize(&mut self) {
        self.pinned_positions
            .retain(|path, pos| !path.trim().is_empty() && pos.x.is_finite() && pos.y.is_finite());
        self.scope = self.scope.normalized();
        if !self.pan.0.is_finite() {
            self.pan.0 = 0.0;
        }
        if !self.pan.1.is_finite() {
            self.pan.1 = 0.0;
        }
        if !self.zoom.is_finite() {
            self.zoom = 1.0;
        }
        self.zoom = self.zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    }
}

/// Collection of saved pinboards for one vault, ordered most-recently-saved first.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PinboardStore {
    pinboards: Vec<Pinboard>,
}

impl PinboardStore {
    pub fn list(&self) -> &[Pinboard] {
        &self.pinboards
    }

    pub fn get(&self, name: &str) -> Option<&Pinboard> {
        let trimmed = name.trim();
        self.pinboards
            .iter()
            .find(|board| board.name.eq_ignore_ascii_case(trimmed))
    }

    /// Save a pinboard, replacing any existing one with the same name (case-insensitively)
    /// and moving it to the top of the list.
    pub fn save(&mut self, mut board: Pinboard) -> Result<(), String> {
        let trimmed = board.name.trim().to_string();
        if trimmed.is_empty() {
            return Err("Pinboard name cannot be empty".to_string());
        }
        board.name = trimmed;
        board.sanitize();
        self.pinboards
            .retain(|existing| !existing.name.eq_ignore_ascii_case(&board.name));
        self.pinboards.insert(0, board);
        Ok(())
    }

    /// Delete a saved pinboard by name (case-insensitively). Returns `true` if removed.
    pub fn delete(&mut self, name: &str) -> bool {
        let trimmed = name.trim();
        let before = self.pinboards.len();
        self.pinboards
            .retain(|existing| !existing.name.eq_ignore_ascii_case(trimmed));
        self.pinboards.len() != before
    }

    /// Propagate a vault file or folder rename into all saved pinboard node coordinates.
    pub fn rename_path(&mut self, old_path: &str, new_path: &str) -> bool {
        let mut changed = false;
        let child_prefix = format!("{}/", old_path.trim_end_matches('/'));
        for board in &mut self.pinboards {
            let keys: Vec<String> = board.pinned_positions.keys().cloned().collect();
            for old_key in keys {
                let updated_key = if old_key == old_path {
                    Some(new_path.to_string())
                } else {
                    old_key
                        .strip_prefix(&child_prefix)
                        .map(|rest| format!("{}/{rest}", new_path.trim_end_matches('/')))
                };
                if let Some(new_key) = updated_key
                    && let Some(pos) = board.pinned_positions.remove(&old_key)
                {
                    board.pinned_positions.insert(new_key, pos);
                    changed = true;
                }
            }
        }
        changed
    }

    /// Remove a deleted file or folder from all saved pinboards.
    pub fn remove_deleted_path(&mut self, deleted_path: &str) -> bool {
        let mut changed = false;
        let child_prefix = format!("{}/", deleted_path.trim_end_matches('/'));
        for board in &mut self.pinboards {
            let before = board.pinned_positions.len();
            board
                .pinned_positions
                .retain(|path, _| path != deleted_path && !path.starts_with(&child_prefix));
            if board.pinned_positions.len() != before {
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
                for board in &mut store.pinboards {
                    board.sanitize();
                }
                store
            }
            Err(err) => {
                log::warn!("Ignoring corrupt saved pinboards payload: {err}");
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
    format!("pinboards:{vault_root}")
}

//! Vault navigation sub-state: vault root, file tree, filter, selection, expansion and backlinks.

use std::collections::BTreeSet;

use iced::Task;

use md_editor_core::types::{BacklinkItem, FileEntry};

use crate::messages::{Message, VaultMessage};

pub struct VaultState {
    pub root: Option<String>,
    pub entries: Vec<FileEntry>,
    pub selected_path: Option<String>,
    pub expanded_folders: BTreeSet<String>,
    pub filter_query: String,
    pub indexing: bool,
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
            filter_query: String::new(),
            indexing: false,
            sidebar_visible: true,
            backlinks_visible: false,
            backlinks: Vec::new(),
        }
    }

    /// Flat list of visible `(path, is_dir)` rows in tree order for keyboard navigation.
    pub fn visible_rows(&self) -> Vec<(String, bool)> {
        let mut out = Vec::new();
        let filter = self.filter_query.trim().to_lowercase();
        if !filter.is_empty() {
            let mut filtered: Vec<(String, bool)> = self
                .entries
                .iter()
                .filter(|e| !e.is_dir && e.path.to_lowercase().contains(&filter))
                .map(|e| (e.path.clone(), false))
                .collect();
            filtered.sort_by_key(|a| a.0.to_lowercase());
            return filtered;
        }
        collect_visible_level(&self.entries, "", &self.expanded_folders, &mut out);
        out
    }

    /// Handle messages that mutate only this pane's own navigation state.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Vault(VaultMessage::SidebarToggle) => {
                self.sidebar_visible = !self.sidebar_visible;
                Task::none()
            }
            Message::Vault(VaultMessage::SidebarFilterChanged(query)) => {
                self.filter_query = query;
                Task::none()
            }
            Message::Vault(VaultMessage::SidebarFolderToggled(path)) => {
                self.selected_path = Some(path.clone());
                if self.expanded_folders.contains(&path) {
                    self.expanded_folders.remove(&path);
                } else {
                    self.expanded_folders.insert(path);
                }
                Task::none()
            }
            Message::Vault(VaultMessage::SidebarSelectionCleared) => {
                self.selected_path = None;
                Task::none()
            }
            _ => Task::none(),
        }
    }
}

fn collect_visible_level(
    entries: &[FileEntry],
    prefix: &str,
    expanded: &BTreeSet<String>,
    out: &mut Vec<(String, bool)>,
) {
    let mut immediate = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in entries {
        if !entry.path.starts_with(prefix) || entry.path == prefix {
            continue;
        }
        let Some(relative) = entry.path.strip_prefix(prefix) else {
            continue;
        };
        let relative = relative.trim_start_matches('/');
        let first_part = relative
            .split_once('/')
            .map_or(relative, |(first, _)| first);
        if !seen.insert(first_part.to_string()) {
            continue;
        }
        let is_dir = relative.contains('/') || entry.is_dir;
        let child_path = if prefix.is_empty() {
            first_part.to_string()
        } else {
            format!("{prefix}/{first_part}")
        };
        immediate.push((first_part.to_string(), child_path, is_dir));
    }
    immediate.sort_by(|a, b| match (a.2, b.2) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
    });
    for (_, path, is_dir) in immediate {
        out.push((path.clone(), is_dir));
        if is_dir && expanded.contains(&path) {
            collect_visible_level(entries, &path, expanded, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicking_a_folder_selects_and_toggles_it() {
        let mut vault = VaultState::new();
        vault.selected_path = Some("notes/a.md".to_string());

        let _ = vault.update(Message::Vault(VaultMessage::SidebarFolderToggled(
            "notes".to_string(),
        )));
        assert_eq!(vault.selected_path.as_deref(), Some("notes"));
        assert!(vault.expanded_folders.contains("notes"));

        let _ = vault.update(Message::Vault(VaultMessage::SidebarFolderToggled(
            "notes".to_string(),
        )));
        assert_eq!(vault.selected_path.as_deref(), Some("notes"));
        assert!(!vault.expanded_folders.contains("notes"));
    }

    #[test]
    fn clicking_empty_space_clears_selection() {
        let mut vault = VaultState::new();
        vault.expanded_folders.insert("notes".to_string());
        let _ = vault.update(Message::Vault(VaultMessage::SidebarFolderToggled(
            "notes".to_string(),
        )));
        assert_eq!(vault.selected_path.as_deref(), Some("notes"));

        let _ = vault.update(Message::Vault(VaultMessage::SidebarSelectionCleared));
        assert_eq!(vault.selected_path, None);
    }
}

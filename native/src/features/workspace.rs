use std::collections::BTreeSet;

use crate::features::pdf::navigation::NavigationHistory;

#[derive(Debug, Clone)]
pub(crate) enum WorkspaceMessage {
    OpenVaultDialog,
    CreateVaultDialog,
    OpenRecentVault(String),
    VaultOpened(Option<String>),
    FileClicked(String),
    FolderToggled(String),
    CreateFileDialog,
    CreateFolderDialog,
    DeleteFile(String),
    DeleteFileDialog(String),
}

#[derive(Debug, Default)]
pub(crate) struct WorkspaceState {
    pub(crate) vault_root: Option<String>,
    pub(crate) vault_entries: Vec<md_editor_core::domain::FileEntry>,
    pub(crate) selected_path: Option<String>,
    pub(crate) active_path: Option<String>,
    pub(crate) expanded_folders: BTreeSet<String>,
    pub(crate) backlinks_visible: bool,
    pub(crate) backlinks: Vec<md_editor_core::domain::BacklinkItem>,
    pub(crate) navigation_history: NavigationHistory,
}

impl WorkspaceState {
    pub(crate) fn update_local(&mut self, message: &WorkspaceMessage) -> bool {
        match message {
            WorkspaceMessage::FolderToggled(vault_path) => {
                self.toggle_folder(vault_path.clone());
                true
            }
            _ => false,
        }
    }

    pub(crate) fn toggle_folder(&mut self, vault_path: String) {
        if !self.expanded_folders.remove(&vault_path) {
            self.expanded_folders.insert(vault_path);
        }
    }

    pub(crate) fn clear_active_markdown(&mut self) {
        self.active_path = None;
        self.backlinks.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_no_open_vault_or_document() {
        let state = WorkspaceState::default();

        assert!(state.vault_root.is_none());
        assert!(state.vault_entries.is_empty());
        assert!(state.selected_path.is_none());
        assert!(state.active_path.is_none());
        assert!(state.expanded_folders.is_empty());
        assert!(!state.backlinks_visible);
        assert!(state.backlinks.is_empty());
        assert!(state.navigation_history.entries.is_empty());
    }

    #[test]
    fn toggling_folder_expands_then_collapses_same_path() {
        let mut state = WorkspaceState::default();

        assert!(state.update_local(&WorkspaceMessage::FolderToggled("notes".to_string())));
        assert!(state.expanded_folders.contains("notes"));

        assert!(state.update_local(&WorkspaceMessage::FolderToggled("notes".to_string())));
        assert!(!state.expanded_folders.contains("notes"));
    }

    #[test]
    fn clearing_active_markdown_preserves_vault_selection_and_history() {
        let mut state = WorkspaceState {
            vault_root: Some("/vault".to_string()),
            selected_path: Some("notes/current.md".to_string()),
            active_path: Some("notes/current.md".to_string()),
            backlinks: vec![md_editor_core::domain::BacklinkItem {
                source: md_editor_core::domain::BacklinkTarget::MarkdownFile {
                    path: "notes/source.md".to_string(),
                },
                label: "source".to_string(),
                context: Some("link".to_string()),
            }],
            ..WorkspaceState::default()
        };
        state.navigation_history.push(
            crate::features::pdf::navigation::NavigationTarget::Markdown {
                path: "notes/current.md".to_string(),
                line: 0,
                column: 0,
            },
        );

        state.clear_active_markdown();

        assert!(state.active_path.is_none());
        assert!(state.backlinks.is_empty());
        assert_eq!(state.vault_root.as_deref(), Some("/vault"));
        assert_eq!(state.selected_path.as_deref(), Some("notes/current.md"));
        assert_eq!(state.navigation_history.entries.len(), 1);
    }
}

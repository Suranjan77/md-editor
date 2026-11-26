//! Vault entry creation, renaming, deletion and modal validation.

use super::*;
use crate::messages::PdfMessage;

impl MdEditor {
    /// Submit the active create/rename/note modal, reporting validation errors inline.
    pub(super) fn on_name_modal_submit(&mut self, input: String) -> Task<Message> {
        if let Some(views::modals::ModalType::QuickNote(id)) = self.ui.active_modal.clone() {
            self.ui.active_modal = None;
            self.ui.modal_input.clear();
            self.ui.modal_error = None;
            self.ui.link_note_picker_search.clear();
            return Task::done(Message::Pdf(PdfMessage::AddQuickNote(id, input)));
        }
        if let Some(views::modals::ModalType::LinkNote(id)) = self.ui.active_modal.clone() {
            self.ui.active_modal = None;
            self.ui.modal_input.clear();
            self.ui.modal_error = None;
            self.ui.link_note_picker_search.clear();
            return Task::done(Message::Pdf(PdfMessage::LinkNote(id, input)));
        }

        let name = input.trim();
        if let Err(validation_err) = validate_entry_name(name) {
            self.ui.modal_error = Some(validation_err.clone());
            self.ui.toast = Some(validation_err);
            return Task::none();
        }

        if let Some(views::modals::ModalType::Rename(old_path)) = self.ui.active_modal.clone() {
            return self.submit_rename(&old_path, name);
        }

        let target_path = self.new_entry_path(name);
        match self.ui.active_modal.as_ref() {
            Some(views::modals::ModalType::CreateFile) => {
                let path = if target_path.ends_with(".md") || target_path.ends_with(".markdown") {
                    target_path
                } else {
                    format!("{target_path}.md")
                };
                if self.vault.entries.iter().any(|e| e.path == path) {
                    let msg = format!("'{path}' already exists");
                    self.ui.modal_error = Some(msg.clone());
                    self.ui.toast = Some(msg);
                    return Task::none();
                }
                let result =
                    md_editor_core::vault::create_file(&self.state, &path).map_err(String::from);
                if !self.reset_vault_ui(result) {
                    return Task::none();
                }
                self.reveal_in_sidebar(&path);
                self.showing_pdf = false;
                self.open_file(&path)
            }
            Some(views::modals::ModalType::CreateFolder) => {
                if self.vault.entries.iter().any(|e| e.path == target_path) {
                    let msg = format!("'{target_path}' already exists");
                    self.ui.modal_error = Some(msg.clone());
                    self.ui.toast = Some(msg);
                    return Task::none();
                }
                let result = md_editor_core::vault::create_dir(&self.state, &target_path)
                    .map_err(String::from);
                self.reset_vault_ui(result);
                self.invalidate_graph();
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn submit_rename(&mut self, old_path: &str, name: &str) -> Task<Message> {
        let parent = std::path::Path::new(old_path)
            .parent()
            .filter(|path| !path.as_os_str().is_empty());
        let new_path = parent
            .map(|path| path.join(name))
            .unwrap_or_else(|| std::path::PathBuf::from(name))
            .to_string_lossy()
            .replace('\\', "/");

        if new_path != old_path && self.vault.entries.iter().any(|e| e.path == new_path) {
            let msg = format!("'{new_path}' already exists");
            self.ui.modal_error = Some(msg.clone());
            self.ui.toast = Some(msg);
            return Task::none();
        }
        // Either pane may show something under the renamed path.
        if let Some(err) = self.save_all_panes() {
            self.ui.toast = Some(format!("Not renaming: {err}"));
            return Task::none();
        }
        match md_editor_core::vault::rename_entry(&self.state, old_path, &new_path) {
            Ok(()) => {
                self.vault.entries =
                    md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                rename_open_path(&mut self.vault.selected_path, old_path, &new_path);
                let mut graph_active = self.graph.active_path.clone();
                rename_open_path(&mut graph_active, old_path, &new_path);
                self.graph.set_active_path(graph_active);
                let reload = self.for_each_pane(|app| {
                    rename_open_path(&mut app.active_path, old_path, &new_path);
                    rename_open_path(&mut app.pdf.active_path, old_path, &new_path);
                    rename_open_path(&mut app.active_image_path, old_path, &new_path);
                    app.editor.forget_retained(old_path);
                    app.tabs.rename_path(old_path, &new_path);
                    app.persist_tabs();
                    app.reload_open_note_from_disk()
                });
                if let Some(ref active) = self.active_path {
                    let _ =
                        md_editor_core::config::set_sys_config(&self.state, "last_file", active);
                }
                self.ui.active_modal = None;
                self.ui.modal_input.clear();
                self.ui.modal_error = None;
                self.ui.toast = Some("Renamed".to_string());
                self.invalidate_graph();
                reload
            }
            Err(error) => {
                let msg = error.to_string();
                self.ui.modal_error = Some(msg.clone());
                self.ui.toast = Some(msg);
                Task::none()
            }
        }
    }

    /// Create and open a note directly from the Command Palette's no-match fallback.
    pub(super) fn create_note_from_palette(&mut self, raw_name: &str) -> Task<Message> {
        let trimmed = raw_name.trim().trim_matches('/');
        if trimmed.is_empty() || trimmed.contains('\\') || trimmed.contains("..") {
            return Task::none();
        }
        let path = if trimmed.ends_with(".md") || trimmed.ends_with(".markdown") {
            trimmed.to_string()
        } else {
            format!("{trimmed}.md")
        };
        self.ui.command_palette_visible = false;
        self.ui.command_palette_query.clear();
        if !self.vault.entries.iter().any(|e| e.path == path) {
            if let Err(err) = md_editor_core::vault::create_file(&self.state, &path) {
                self.ui.toast = Some(err.to_string());
                return Task::none();
            }
            self.vault.entries = md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
        }
        self.reveal_in_sidebar(&path);
        self.open_file(&path)
    }

    pub(super) fn on_delete_file(&mut self, path: String) -> Task<Message> {
        match md_editor_core::vault::delete_entry(&self.state, &path) {
            Ok(()) => {
                self.vault.entries =
                    md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                let child_prefix = format!("{path}/");
                let gone =
                    |p: Option<&str>| p.is_some_and(|p| p == path || p.starts_with(&child_prefix));
                if gone(self.vault.selected_path.as_deref()) {
                    self.vault.selected_path = None;
                }
                self.vault
                    .expanded_folders
                    .retain(|folder| !gone(Some(folder.as_str())));
                if gone(self.graph.active_path.as_deref()) {
                    self.graph.set_active_path(None);
                }
                let next = self.for_each_pane(|app| app.forget_deleted_in_pane(&path));
                self.ui.active_modal = None;
                self.ui.modal_error = None;
                self.ui.link_note_picker_search.clear();
                self.ui.toast = Some("Deleted".to_string());
                self.invalidate_graph();
                return next;
            }
            Err(err) => self.ui.toast = Some(err.to_string()),
        }
        Task::none()
    }

    /// `path` (a file, or a folder and everything in it) was deleted: drop it
    /// from the pane swapped in, and show the next tab if it was on screen.
    fn forget_deleted_in_pane(&mut self, path: &str) -> Task<Message> {
        let child_prefix = format!("{path}/");
        let gone = |p: Option<&str>| p.is_some_and(|p| p == path || p.starts_with(&child_prefix));
        self.editor.forget_retained(path);
        let was_active_deleted = gone(self.active_path.as_deref())
            || gone(self.pdf.active_path.as_deref())
            || gone(self.active_image_path.as_deref());
        if gone(self.active_path.as_deref()) {
            self.active_path = None;
            self.editor.buffer = DocBuffer::new();
            self.editor.autosave_pending_since = None;
            self.editor.highlight = Default::default();
            self.editor.toc_entries.clear();
        }
        if gone(self.pdf.active_path.as_deref()) {
            self.clear_pdf_pane();
        }
        if gone(self.active_image_path.as_deref()) {
            self.active_image_path = None;
            self.active_image = None;
        }
        self.tabs.remove_deleted_path(path);
        self.persist_tabs();
        match self.tabs.active_id() {
            Some(next_id) if was_active_deleted => self.activate_tab(next_id),
            _ => Task::none(),
        }
    }

    pub(super) fn reset_vault_ui(&mut self, result: Result<(), String>) -> bool {
        match result {
            Ok(()) => {
                self.vault.entries =
                    md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                self.ui.active_modal = None;
                self.ui.modal_input.clear();
                self.ui.modal_error = None;
                self.ui.modal_parent_override = None;
                self.ui.link_note_picker_search.clear();
                self.ui.toast = Some("Created".to_string());
                true
            }
            Err(err) => {
                self.ui.modal_error = Some(err.clone());
                self.ui.toast = Some(err);
                false
            }
        }
    }

    pub(super) fn new_entry_path(&self, name: &str) -> String {
        if let Some(ref folder) = self.ui.modal_parent_override {
            let trimmed = folder.trim_matches('/');
            return if trimmed.is_empty() {
                name.to_string()
            } else {
                format!("{trimmed}/{name}")
            };
        }
        Self::new_entry_parent(
            self.vault.selected_path.as_deref(),
            &self.vault.entries,
            &self.vault.expanded_folders,
        )
        .map(|dir| format!("{}/{}", dir.trim_end_matches('/'), name))
        .unwrap_or_else(|| name.to_string())
    }

    pub(super) fn new_entry_parent(
        selected: Option<&str>,
        entries: &[md_editor_core::types::FileEntry],
        expanded: &std::collections::BTreeSet<String>,
    ) -> Option<String> {
        let selected = selected?;
        let child_prefix = format!("{selected}/");
        let is_dir = entries.iter().any(|entry| {
            (entry.path == selected && entry.is_dir) || entry.path.starts_with(&child_prefix)
        });
        if !is_dir && !entries.iter().any(|entry| entry.path == selected) {
            return None;
        }
        let mut ancestor = selected;
        while let Some(idx) = ancestor.rfind('/') {
            ancestor = &ancestor[..idx];
            if !expanded.contains(ancestor) {
                return None;
            }
        }
        if is_dir {
            Some(selected.to_string())
        } else {
            selected.rfind('/').map(|idx| selected[..idx].to_string())
        }
    }
}

/// Validate a file or folder name entered in a modal dialog.
pub(crate) fn validate_entry_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Name cannot be empty".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("Name cannot contain path separators".to_string());
    }
    if name == "." || name == ".." {
        return Err("Invalid entry name".to_string());
    }
    Ok(())
}

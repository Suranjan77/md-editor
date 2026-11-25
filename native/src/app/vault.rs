//! Vault lifecycle, indexing, filesystem watcher reconciliation and sidebar actions.

use super::*;
use crate::messages::{ShellMessage, VaultMessage};

impl MdEditor {
    /// Handle a `VaultMessage` routed here by [`MdEditor::update_inner`].
    pub(super) fn update_vault(&mut self, message: VaultMessage) -> Task<Message> {
        match message {
            VaultMessage::OpenVaultDialog => Task::perform(
                async {
                    let folder = rfd::AsyncFileDialog::new()
                        .set_title("Open Vault Folder")
                        .pick_folder()
                        .await;
                    folder.map(|f| f.path().to_string_lossy().to_string())
                },
                |value| Message::Vault(VaultMessage::VaultOpened(value)),
            ),
            VaultMessage::CreateVaultDialog => Task::perform(
                async {
                    let folder = rfd::AsyncFileDialog::new()
                        .set_title("Select or Create Empty Vault Folder")
                        .pick_folder()
                        .await;
                    folder.map(|f| f.path().to_string_lossy().to_string())
                },
                |value| Message::Vault(VaultMessage::VaultCreated(value)),
            ),
            VaultMessage::VaultOpened(Some(path)) => self.open_vault_and_restore(&path),
            VaultMessage::VaultOpened(None) => Task::none(),
            VaultMessage::VaultCreated(Some(path)) => self.create_and_open_vault(&path),
            VaultMessage::VaultCreated(None) => Task::none(),
            VaultMessage::OpenRecentVault(path) => self.open_vault_and_restore(&path),
            VaultMessage::VaultIndexed(generation, root, result) => {
                self.on_vault_indexed(generation, root, result)
            }
            m @ (VaultMessage::SidebarToggle
            | VaultMessage::SidebarFilterChanged(_)
            | VaultMessage::SidebarFolderToggled(_)
            | VaultMessage::SidebarSelectionCleared) => self.vault.update(Message::Vault(m)),
            VaultMessage::SidebarKeyNav(nav) => self.on_sidebar_key_nav(nav),
            VaultMessage::SidebarFileClicked(path) => self.on_sidebar_file_clicked(path),
            VaultMessage::RevealInFileManager(rel_path) => {
                self.ui.context_menu = None;
                if let Some(abs) = self.resolve_active_path(&rel_path) {
                    crate::platform::reveal_in_file_manager(&abs);
                } else if let Some(ref root) = self.vault.root {
                    crate::platform::reveal_in_file_manager(std::path::Path::new(root));
                }
                Task::none()
            }
            VaultMessage::CopyRelativePath(rel_path) => {
                self.ui.context_menu = None;
                self.ui
                    .notify(crate::ui::ToastKind::Success, "Copied path", false);
                iced::clipboard::write(rel_path)
            }
            VaultMessage::CreateNoteFromPalette(raw_name) => {
                self.create_note_from_palette(&raw_name)
            }
            m @ (VaultMessage::CreateFileDialog
            | VaultMessage::CreateFolderDialog
            | VaultMessage::NewNoteInFolder(_)
            | VaultMessage::NewFolderInFolder(_)
            | VaultMessage::DeleteFileDialog(_)
            | VaultMessage::RenameEntryDialog(_)
            | VaultMessage::NameModalInputChanged(_)
            | VaultMessage::PdfLinkNoteFolderSelected(_)
            | VaultMessage::PdfLinkNoteFileSelected(_)
            | VaultMessage::PdfLinkNotePickerSearchChanged(_)
            | VaultMessage::NameModalCancel) => self.ui.update(Message::Vault(m)),
            VaultMessage::NameModalSubmit(input) => self.on_name_modal_submit(input),
            VaultMessage::DeleteFile(path) => self.on_delete_file(path),
            VaultMessage::NameModalSubmitCurrent => self.on_name_modal_submit_current(),
            VaultMessage::VaultFilesChanged(paths) => self.on_vault_files_changed(paths),
        }
    }

    fn open_vault_and_restore(&mut self, path: &str) -> Task<Message> {
        let index_task = self.open_vault(path);
        if self.vault.root.as_deref() == Some(path) {
            let restore_task = self.restore_vault_tabs(path, None);
            Task::batch(vec![index_task, restore_task])
        } else {
            index_task
        }
    }

    fn create_and_open_vault(&mut self, path: &str) -> Task<Message> {
        let dir = std::path::Path::new(path);
        if let Err(err) = std::fs::create_dir_all(dir) {
            self.ui.notify(
                crate::ui::ToastKind::Error,
                format!("Could not create vault folder: {err}"),
                false,
            );
            return Task::none();
        }
        let index_task = self.open_vault(path);
        if self.vault.root.as_deref() != Some(path) {
            return index_task;
        }
        let welcome_note = "Index.md";
        if !dir.join(welcome_note).exists() {
            if let Err(err) = md_editor_core::vault::save_file(
                &self.state,
                welcome_note,
                "# Welcome\n\nStart writing here.\n",
            ) {
                log::warn!("Could not create {welcome_note} in the new vault: {err}");
            }
            self.vault.entries = md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
        }
        let open_task = self.open_file(welcome_note);
        Task::batch(vec![index_task, open_task])
    }

    fn on_vault_indexed(
        &mut self,
        generation: u64,
        root: String,
        result: Result<Option<Vec<md_editor_core::types::FileEntry>>, String>,
    ) -> Task<Message> {
        if generation != self.state.current_vault_generation()
            || self.vault.root.as_deref() != Some(root.as_str())
        {
            return Task::none();
        }
        let entries = match result {
            Ok(Some(entries)) => entries,
            Ok(None) => {
                let state = self.state.clone();
                let index_root = root.clone();
                let message_root = root.clone();
                return Task::perform(
                    async move {
                        md_editor_core::vault::index_vault_for_generation(
                            &state,
                            &index_root,
                            generation,
                        )
                    },
                    move |result| {
                        Message::Vault(VaultMessage::VaultIndexed(
                            generation,
                            message_root.clone(),
                            result.map_err(String::from),
                        ))
                    },
                );
            }
            Err(error) => {
                self.vault.indexing = false;
                self.ui.toast = Some(format!("Could not index vault: {error}"));
                return Task::none();
            }
        };
        self.vault.indexing = false;
        self.vault.entries = entries;
        self.graph.mark_dirty();
        if let Some(path) = self
            .active_path
            .clone()
            .or_else(|| self.pdf.active_path.clone())
        {
            self.vault.backlinks =
                md_editor_core::vault::get_mixed_backlinks(&self.state, &path).unwrap_or_default();
        }
        Task::none()
    }

    fn on_name_modal_submit_current(&mut self) -> Task<Message> {
        if self.ui.command_palette_visible {
            let action = views::command_palette::action_at(
                &self.ui.command_palette_query,
                &self.ui.commands,
                &self.vault.entries,
                self.ui.palette_selected,
            );
            return match action {
                Some(views::command_palette::PaletteAction::Command(shortcut)) => Task::done(
                    Message::Shell(ShellMessage::CommandPaletteCommandClicked(shortcut)),
                ),
                Some(views::command_palette::PaletteAction::OpenFile(path)) => Task::done(
                    Message::Shell(ShellMessage::CommandPaletteFileClicked(path)),
                ),
                None => Task::none(),
            };
        }
        self.ui
            .update(Message::Vault(VaultMessage::NameModalSubmitCurrent))
    }

    fn on_vault_files_changed(&mut self, paths: Vec<String>) -> Task<Message> {
        let paths: Vec<String> = paths
            .into_iter()
            .filter(|path| !self.is_recent_self_write(path))
            .collect();
        if paths.is_empty() {
            return Task::none();
        }

        for path in &paths {
            let _ = md_editor_core::vault::sync_path_from_disk(&self.state, path);
        }
        if let Ok(entries) = md_editor_core::vault::list_vault(&self.state) {
            self.vault.entries = entries;
        }
        if let Some(ref root) = self.vault.root {
            let root_path = std::path::PathBuf::from(root);
            self.tabs.sync_missing(|rel| {
                md_editor_core::vault::resolve_vault_path(&root_path, rel).is_file()
            });
        }
        if let Some(active) = self
            .active_path
            .clone()
            .or_else(|| self.pdf.active_path.clone())
            .or_else(|| self.pptx.active_path.clone())
        {
            self.vault.backlinks = md_editor_core::vault::get_mixed_backlinks(&self.state, &active)
                .unwrap_or_default();
        }
        self.invalidate_graph();
        if let Some(active) = self.active_path.clone()
            && paths.contains(&active)
        {
            return self.reload_open_note_from_disk();
        }
        if let Some(deck) = self.pptx.active_path.clone()
            && paths.contains(&deck)
        {
            if self
                .resolve_active_path(&deck)
                .is_some_and(|path| path.is_file())
            {
                return self.load_pptx(&deck);
            }
            self.pptx.close();
        }
        Task::none()
    }

    /// Select `path` in the sidebar and expand every folder above it.
    pub(super) fn reveal_in_sidebar(&mut self, path: &str) {
        let mut ancestor = path;
        while let Some(idx) = ancestor.rfind('/') {
            ancestor = &ancestor[..idx];
            self.vault.expanded_folders.insert(ancestor.to_string());
        }
        self.vault.selected_path = Some(path.to_string());
    }

    pub(super) fn open_vault(&mut self, path: &str) -> Task<Message> {
        let root_path = std::path::PathBuf::from(path);
        if !root_path.is_dir() {
            self.ui.toast = Some(format!("Not a directory: {path}"));
            return Task::none();
        }
        if let SaveOutcome::Failed(err) = self.save_active_document() {
            self.ui.toast = Some(format!("Could not save before switching vaults: {err}"));
            return Task::none();
        }
        self.save_current_tab_view();
        self.persist_tabs();

        let generation = match self.state.begin_vault_change(root_path) {
            Ok(generation) => generation,
            Err(error) => {
                self.ui.toast = Some(format!("Could not open vault: {error}"));
                return Task::none();
            }
        };
        self.clear_all_document_panes();
        self.editor.forget_all_retained();
        self.tabs = crate::tabs::Tabs::new();
        self.graph.visible = false;
        self.graph.clear_for_vault_change();
        self.ui.settings_visible = false;
        self.ui.context_menu = None;
        self.vault.selected_path = None;
        self.vault.expanded_folders.clear();
        self.vault.filter_query.clear();
        self.vault.backlinks.clear();
        self.vault.indexing = true;
        self.search.visible = false;
        self.search.file_visible = false;
        self.search.query.clear();
        self.search.replace.clear();
        self.search.match_index = None;
        self.search.results.clear();
        self.search.results_truncated = false;
        self.search.pdf_results.clear();
        self.search.pdf_indices_by_page.clear();
        self.search.pdf_error = None;
        self.vault.root = Some(path.to_string());
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_vault", path);
        self.record_recent_vault(path);
        self.vault.entries = md_editor_core::vault::list_vault(&self.state).unwrap_or_default();

        let state = self.state.clone();
        let path = path.to_string();
        let result_path = path.clone();
        Task::perform(
            async move { md_editor_core::vault::index_vault_for_generation(&state, &path, generation) },
            move |result| {
                Message::Vault(VaultMessage::VaultIndexed(
                    generation,
                    result_path.clone(),
                    result.map_err(String::from),
                ))
            },
        )
    }

    /// Rebuild the index and file list of the open vault in place, e.g. after
    /// the excluded folders changed. Tabs, selection and search stay as they are.
    pub(super) fn reindex_vault(&mut self) -> Task<Message> {
        let Some(root) = self.vault.root.clone() else {
            return Task::none();
        };
        let generation = self.state.current_vault_generation();
        self.vault.entries = md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
        self.vault.indexing = true;
        let state = self.state.clone();
        let result_root = root.clone();
        Task::perform(
            async move { md_editor_core::vault::index_vault_for_generation(&state, &root, generation) },
            move |result| {
                Message::Vault(VaultMessage::VaultIndexed(
                    generation,
                    result_root.clone(),
                    result.map_err(String::from),
                ))
            },
        )
    }
}

//! The vault and file tree: opening vaults, indexing, creating, renaming and deleting entries.

use super::*;
use crate::messages::{PdfMessage, ShellMessage, VaultMessage};

impl MdEditor {
    /// Handle a message routed here by [`MdEditor::update_inner`].
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
            VaultMessage::VaultOpened(Some(path)) => self.open_vault(&path),
            // The folder picker was cancelled.
            VaultMessage::VaultOpened(None) => Task::none(),
            VaultMessage::VaultIndexed(generation, root, result) => {
                if generation != self.state.current_vault_generation()
                    || self.vault.root.as_deref() != Some(root.as_str())
                {
                    return Task::none();
                }
                let entries = match result {
                    Ok(Some(entries)) => entries,
                    Ok(None) => {
                        // The vault is still current, but a save/create/delete
                        // landed while this snapshot was being assembled. Retry
                        // from disk so the background build cannot overwrite it.
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
                        self.ui.toast = Some(format!("Could not index vault: {error}"));
                        return Task::none();
                    }
                };
                self.vault.entries = entries;
                self.graph.mark_dirty();
                // Backlinks for the active file depend on the freshly built
                // index; refresh them now that indexing has completed.
                if let Some(path) = self
                    .active_path
                    .clone()
                    .or_else(|| self.pdf.active_path.clone())
                {
                    self.vault.backlinks =
                        md_editor_core::vault::get_mixed_backlinks(&self.state, &path)
                            .unwrap_or_default();
                }
                Task::none()
            }
            // Vault navigation arms that mutate only `self.vault` are routed
            // to `VaultState::update`; see vault_state.rs.
            m @ (VaultMessage::SidebarToggle
            | VaultMessage::SidebarFolderToggled(_)
            | VaultMessage::SidebarSelectionCleared) => self.vault.update(Message::Vault(m)),
            VaultMessage::SidebarFileClicked(path) => {
                let path = path.trim().to_string();
                self.graph.visible = false;
                if let Some(url_str) = path.strip_prefix("pdf://") {
                    let (encoded_pdf_path, query) = if let Some(idx) = url_str.find('?') {
                        (&url_str[..idx], Some(&url_str[idx + 1..]))
                    } else {
                        (url_str, None)
                    };
                    let pdf_path = crate::pdf_notes::decode_pdf_link_path(encoded_pdf_path);

                    let mut page: Option<u16> = None;
                    let mut annotation_id: Option<String> = None;
                    if let Some(query_str) = query {
                        for pair in query_str.split('&') {
                            let mut parts = pair.splitn(2, '=');
                            if let (Some(key), Some(val)) = (parts.next(), parts.next()) {
                                if key == "page" {
                                    if let Ok(p) = val.parse::<u16>() {
                                        page = Some(p);
                                    }
                                } else if key == "annotation" {
                                    annotation_id = Some(val.to_string());
                                }
                            }
                        }
                    }

                    let resolved_pdf_path = resolve_relative_link_path(
                        self.vault.root.as_deref(),
                        self.active_path.as_deref(),
                        &pdf_path,
                    );

                    // Turning on split view shrinks the PDF pane; re-fit below if
                    // it actually changed so an already-open PDF isn't stretched.
                    let split_changed = !self.ui.split_view_active;
                    self.ui.split_view_active = true;
                    self.showing_pdf = true;

                    if self.pdf.active_path.as_deref() == Some(&resolved_pdf_path) {
                        self.pdf.focused_annotation_id = annotation_id;
                        let nav_task = if let Some(p) = page {
                            let p_0 = p.saturating_sub(1);
                            self.navigate_pdf_page(p_0)
                        } else if let Some(ref ann_id) = self.pdf.focused_annotation_id {
                            let mut target_page = None;
                            for (page_idx, page_anns) in &self.pdf.annotations {
                                if page_anns.iter().any(|a| &a.id == ann_id) {
                                    target_page = Some(*page_idx);
                                    break;
                                }
                            }
                            if let Some(target_page) = target_page {
                                self.navigate_pdf_page(target_page)
                            } else {
                                Task::none()
                            }
                        } else {
                            Task::none()
                        };
                        if split_changed {
                            Task::batch(vec![self.refit_pdf_if_needed(), nav_task])
                        } else {
                            nav_task
                        }
                    } else {
                        self.pdf.initial_target_page = page.map(|p| p.saturating_sub(1));
                        self.pdf.initial_target_annotation = annotation_id;
                        self.open_pdf(&resolved_pdf_path)
                    }
                } else {
                    let is_url = path.starts_with("http://")
                        || path.starts_with("https://")
                        || path.contains("://");

                    if is_url {
                        crate::platform::open_external(&path);
                        Task::none()
                    } else {
                        let (file_part, anchor_part) = if let Some(idx) = path.find('#') {
                            (&path[..idx], Some(&path[idx + 1..]))
                        } else {
                            (path.as_str(), None)
                        };

                        if let Some(anchor_part) = anchor_part {
                            let file_part = file_part.trim();
                            let anchor_part = anchor_part.trim();
                            if file_part.is_empty() {
                                let target_slug = slugify(anchor_part);
                                if let Some(line_idx) = find_heading_or_widget_line(
                                    &self.editor.buffer.text(),
                                    &self.editor.highlight.lines,
                                    &target_slug,
                                ) {
                                    self.editor.request_reveal(Reveal::Center);
                                    self.run_editor_command(EditorCommand::SetCursor {
                                        line: line_idx,
                                        col: 0,
                                        affinity: Affinity::Downstream,
                                    })
                                } else {
                                    self.ui.toast = Some(format!(
                                        "Heading or widget not found: #{}",
                                        anchor_part
                                    ));
                                    Task::none()
                                }
                            } else {
                                let resolved_file = self.resolve_internal_link_path(file_part);
                                self.vault.selected_path = Some(resolved_file.clone());
                                let open_task = self.open_file_extended(&resolved_file, false);

                                let target_slug = slugify(anchor_part);
                                if let Some(line_idx) = find_heading_or_widget_line(
                                    &self.editor.buffer.text(),
                                    &self.editor.highlight.lines,
                                    &target_slug,
                                ) {
                                    self.editor.request_reveal(Reveal::Center);
                                    let cmd_task =
                                        self.run_editor_command(EditorCommand::SetCursor {
                                            line: line_idx,
                                            col: 0,
                                            affinity: Affinity::Downstream,
                                        });
                                    Task::batch(vec![open_task, cmd_task])
                                } else {
                                    // If heading or widget not found in the new file, reset scroll to top!
                                    self.editor.scroll_y = 0.0;
                                    let scroll_task = operation::scroll_to(
                                        iced::advanced::widget::Id::new(EDITOR_SCROLLABLE_ID),
                                        AbsoluteOffset { x: 0.0, y: 0.0 },
                                    );
                                    Task::batch(vec![open_task, scroll_task])
                                }
                            }
                        } else {
                            let resolved_path = self.resolve_internal_link_path(&path);
                            self.vault.selected_path = Some(resolved_path.clone());
                            let lower = resolved_path.to_lowercase();
                            if lower.ends_with(".md") || lower.ends_with(".markdown") {
                                self.showing_pdf = false;
                                self.open_file(&resolved_path)
                            } else if lower.ends_with(".pdf") {
                                self.pdf.active_path = Some(resolved_path.clone());
                                self.showing_pdf = true;
                                self.open_pdf(&resolved_path)
                            } else if is_supported_image_path(&lower) {
                                self.open_image(&resolved_path)
                            } else if lower.ends_with(".pptx") {
                                self.open_pptx(&resolved_path)
                            } else if lower.ends_with(".ink") {
                                self.open_ink_page(&resolved_path)
                            } else {
                                Task::none()
                            }
                        }
                    }
                }
            }
            m @ (VaultMessage::CreateFileDialog
            | VaultMessage::CreateFolderDialog
            | VaultMessage::DeleteFileDialog(_)
            | VaultMessage::RenameEntryDialog(_)
            | VaultMessage::NameModalInputChanged(_)
            | VaultMessage::PdfLinkNoteFolderSelected(_)
            | VaultMessage::PdfLinkNoteFileSelected(_)
            | VaultMessage::PdfLinkNotePickerSearchChanged(_)
            | VaultMessage::NameModalCancel) => self.ui.update(Message::Vault(m)),
            VaultMessage::NameModalSubmit(input) => {
                if let Some(views::modals::ModalType::QuickNote(id)) = self.ui.active_modal.clone()
                {
                    self.ui.active_modal = None;
                    self.ui.modal_input.clear();
                    self.ui.link_note_picker_search.clear();
                    return Task::done(Message::Pdf(PdfMessage::AddQuickNote(id, input)));
                }
                if let Some(views::modals::ModalType::LinkNote(id)) = self.ui.active_modal.clone() {
                    self.ui.active_modal = None;
                    self.ui.modal_input.clear();
                    self.ui.link_note_picker_search.clear();
                    return Task::done(Message::Pdf(PdfMessage::LinkNote(id, input)));
                }

                let name = input.trim();
                if name.is_empty() {
                    self.ui.toast = Some("Name cannot be empty".to_string());
                    return Task::none();
                }
                if name.contains('/') || name.contains('\\') {
                    self.ui.toast = Some("Name cannot contain path separators".to_string());
                    return Task::none();
                }

                if let Some(views::modals::ModalType::Rename(old_path)) =
                    self.ui.active_modal.clone()
                {
                    let parent = std::path::Path::new(&old_path)
                        .parent()
                        .filter(|path| !path.as_os_str().is_empty());
                    let new_path = parent
                        .map(|path| path.join(name))
                        .unwrap_or_else(|| std::path::PathBuf::from(name))
                        .to_string_lossy()
                        .replace('\\', "/");
                    // A rename rewrites links in other notes — possibly the
                    // open one. Flush it first so the rewrite lands on our
                    // latest text and the buffer can simply follow the file.
                    if let SaveOutcome::Failed(err) = self.save_active_document() {
                        self.ui.toast = Some(format!("Not renaming: {err}"));
                        return Task::none();
                    }
                    match md_editor_core::vault::rename_entry(&self.state, &old_path, &new_path) {
                        Ok(()) => {
                            self.vault.entries =
                                md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                            rename_open_path(&mut self.active_path, &old_path, &new_path);
                            rename_open_path(&mut self.pdf.active_path, &old_path, &new_path);
                            rename_open_path(&mut self.active_image_path, &old_path, &new_path);
                            rename_open_path(&mut self.vault.selected_path, &old_path, &new_path);
                            let mut graph_active = self.graph.active_path.clone();
                            rename_open_path(&mut graph_active, &old_path, &new_path);
                            self.graph.set_active_path(graph_active);
                            self.ui.active_modal = None;
                            self.ui.modal_input.clear();
                            self.ui.toast = Some("Renamed".to_string());
                            self.invalidate_graph();
                            // Pick up links the rename rewrote in the open note.
                            return self.reload_open_note_from_disk();
                        }
                        Err(error) => self.ui.toast = Some(error.to_string()),
                    }
                    return Task::none();
                }

                let target_path = self.new_entry_path(name);
                match self.ui.active_modal.as_ref() {
                    Some(views::modals::ModalType::CreateFile) => {
                        let path =
                            if target_path.ends_with(".md") || target_path.ends_with(".markdown") {
                                target_path
                            } else {
                                format!("{}.md", target_path)
                            };
                        let result = md_editor_core::vault::create_file(&self.state, &path)
                            .map_err(String::from);
                        if !self.reset_vault_ui(result) {
                            return Task::none();
                        }
                        // A note you just named is the note you mean to write
                        // in: reveal it in the tree and open it straight away.
                        self.reveal_in_sidebar(&path);
                        self.showing_pdf = false;
                        self.open_file(&path)
                    }
                    Some(views::modals::ModalType::CreateFolder) => {
                        let result = md_editor_core::vault::create_dir(&self.state, &target_path)
                            .map_err(String::from);
                        self.reset_vault_ui(result);
                        self.invalidate_graph();
                        Task::none()
                    }
                    _ => Task::none(),
                }
            }
            VaultMessage::DeleteFile(path) => {
                match md_editor_core::vault::delete_entry(&self.state, &path) {
                    Ok(()) => {
                        self.vault.entries =
                            md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                        // Drop any parked buffer for the deleted path so a new
                        // file created under the same name cannot inherit the
                        // old document's undo history.
                        self.editor.forget_retained(&path);
                        // Deleting a folder takes everything under it too, so
                        // every check below matches the path or its children.
                        let child_prefix = format!("{path}/");
                        let gone = |p: Option<&str>| {
                            p.is_some_and(|p| p == path || p.starts_with(&child_prefix))
                        };
                        // A selection at or inside the deleted entry is gone
                        // from the tree; keeping it would steer new files into
                        // it if something with that name reappears later.
                        if gone(self.vault.selected_path.as_deref()) {
                            self.vault.selected_path = None;
                        }
                        self.vault
                            .expanded_folders
                            .retain(|folder| !gone(Some(folder.as_str())));
                        // An open note inside a deleted folder must be detached
                        // too, or autosave would write it back and recreate it.
                        if gone(self.active_path.as_deref()) {
                            self.active_path = None;
                            self.editor.buffer = DocBuffer::new();
                            self.editor.autosave_pending_since = None;
                            self.editor.highlight = Default::default();
                        }
                        if gone(self.pdf.active_path.as_deref()) {
                            self.pdf.active_path = None;
                            self.pdf.pages.clear();
                            self.pdf.dimensions.clear();
                        }
                        if gone(self.active_image_path.as_deref()) {
                            self.active_image_path = None;
                            self.active_image = None;
                        }
                        if gone(self.graph.active_path.as_deref()) {
                            self.graph.set_active_path(None);
                        }
                        if gone(self.pptx.active_path.as_deref()) {
                            self.pptx.close();
                        }
                        self.ui.active_modal = None;
                        self.ui.link_note_picker_search.clear();
                        self.ui.toast = Some("Deleted".to_string());
                        self.invalidate_graph();
                    }
                    Err(err) => self.ui.toast = Some(err.to_string()),
                }
                Task::none()
            }
            VaultMessage::NameModalSubmitCurrent => {
                // Enter belongs to the palette while it is open; otherwise it
                // falls through to whatever modal is showing.
                if self.ui.command_palette_visible {
                    let action = views::command_palette::action_at(
                        &self.ui.command_palette_query,
                        &self.ui.commands,
                        &self.vault.entries,
                        self.ui.palette_selected,
                    );
                    return match action {
                        Some(views::command_palette::PaletteAction::Command(shortcut)) => {
                            Task::done(Message::Shell(ShellMessage::CommandPaletteCommandClicked(
                                shortcut,
                            )))
                        }
                        Some(views::command_palette::PaletteAction::OpenFile(path)) => Task::done(
                            Message::Shell(ShellMessage::CommandPaletteFileClicked(path)),
                        ),
                        None => Task::none(),
                    };
                }
                self.ui
                    .update(Message::Vault(VaultMessage::NameModalSubmitCurrent))
            }
            VaultMessage::VaultFilesChanged(paths) => {
                // Reconcile the index/search for changed markdown files. The
                // open note follows the file only while it is clean
                // (`reload_open_note_from_disk`); with unsaved edits, the next
                // save keeps the disk version as a conflict copy.
                //
                // Our own saves are filtered out entirely: `save_file` already
                // updated the index and the search row, and the tree structure
                // cannot have changed. Without this, every autosave triggered a
                // full `list_vault` walk plus a backlink rebuild — a hitch on
                // every pause in typing, on a large vault.
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
                if let Some(active) = self
                    .active_path
                    .clone()
                    .or_else(|| self.pdf.active_path.clone())
                    .or_else(|| self.pptx.active_path.clone())
                {
                    self.vault.backlinks =
                        md_editor_core::vault::get_mixed_backlinks(&self.state, &active)
                            .unwrap_or_default();
                }
                self.invalidate_graph();
                if let Some(active) = self.active_path.clone()
                    && paths.contains(&active)
                {
                    return self.reload_open_note_from_disk();
                }
                // An open presentation follows its file: it is read again
                // when the file changes, and closed when the file is gone.
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
        }
    }

    /// Settle the vault UI after a create: on success refresh the tree and
    /// close the modal, on failure surface the error and keep the modal open
    /// so the name can be corrected. Returns whether the create succeeded.
    pub(super) fn reset_vault_ui(&mut self, result: Result<(), String>) -> bool {
        match result {
            Ok(()) => {
                self.vault.entries =
                    md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                self.ui.active_modal = None;
                self.ui.modal_input.clear();
                self.ui.link_note_picker_search.clear();
                self.ui.toast = Some("Created".to_string());
                true
            }
            Err(err) => {
                self.ui.toast = Some(err);
                false
            }
        }
    }

    /// Select `path` in the sidebar and expand every folder above it, so the
    /// row is actually visible rather than selected inside a collapsed folder.
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
        // Write the open note while its vault is still the active one; once
        // the root changes, its relative path would resolve into the new vault.
        if let SaveOutcome::Failed(err) = self.save_active_document() {
            self.ui.toast = Some(format!("Could not save before switching vaults: {err}"));
            return Task::none();
        }
        let generation = match self.state.begin_vault_change(root_path) {
            Ok(generation) => generation,
            Err(error) => {
                self.ui.toast = Some(format!("Could not open vault: {error}"));
                return Task::none();
            }
        };
        self.active_path = None;
        self.editor.buffer = DocBuffer::new();
        self.editor.autosave_pending_since = None;
        self.editor.highlight = Default::default();
        self.editor.forget_all_retained();
        self.editor.toc_entries.clear();
        self.pdf.active_path = None;
        self.pdf.pages.clear();
        self.pptx.close();
        self.active_image_path = None;
        self.active_image = None;
        self.showing_pdf = false;
        self.graph.visible = false;
        self.graph.clear_for_vault_change();
        self.vault.selected_path = None;
        self.vault.expanded_folders.clear();
        self.vault.backlinks.clear();
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
        // `begin_vault_change` publishes the requested root immediately so file
        // opens resolve correctly; show a cheap tree while indexing continues.
        self.vault.entries = md_editor_core::vault::list_vault(&self.state).unwrap_or_default();

        // Build the full-text and backlink indexes off the UI thread; a large
        // vault must not freeze startup. `set_vault_root` does its disk I/O
        // lock-free, so the UI stays responsive while it runs.
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

    pub(super) fn new_entry_path(&self, name: &str) -> String {
        Self::new_entry_parent(
            self.vault.selected_path.as_deref(),
            &self.vault.entries,
            &self.vault.expanded_folders,
        )
        .map(|dir| format!("{}/{}", dir.trim_end_matches('/'), name))
        .unwrap_or_else(|| name.to_string())
    }

    /// The folder a new file or folder lands in: the sidebar selection's
    /// folder, but only while that row is still on screen. A selection that
    /// was deleted, or hidden by collapsing a folder above it, no longer reads
    /// as selected, so new entries go to the vault root rather than following
    /// a target the user can't see.
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

        // The tree only renders a row when every folder above it is expanded.
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

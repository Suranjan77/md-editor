use iced::Task;
use iced::widget::operation::{self, AbsoluteOffset};

use crate::features::editor::EditorEffect;

use crate::editor::buffer::{DocBuffer, EditorCommand};
use crate::features::pdf::navigation::{build_pdf_link, parse_pdf_link};
use crate::features::search::SearchLocalEffect;
use crate::features::tracker::TrackerEffect;
use crate::messages::{
    CitationMessage, EditorBlockActionKind, EditorMessage, Message, OverlayMessage, PdfMessage,
    SearchMessage, SearchWrapStatus, ShellMessage, Shortcut, SystemMessage, TrackerMessage,
    WorkspaceMessage,
};
use crate::theme as app_theme;
use crate::views;
use std::collections::HashSet;

use super::model::*;
use crate::app::*;

impl MdEditor {
    pub(crate) fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            message @ Message::Shell(_) => self.coordinate(message),
            message @ Message::Workspace(_) => self.coordinate(message),
            message @ Message::Editor(_) => self.coordinate(message),
            Message::Pdf(message) => self.update_pdf_message(message),
            message @ Message::Search(_) => self.coordinate(message),
            message @ Message::Citation(_) => self.coordinate(message),
            message @ Message::Tracker(_) => self.coordinate(message),
            message @ Message::Overlay(_) => self.coordinate(message),
            message @ Message::System(_) => self.coordinate(message),
        }
    }

    fn coordinate(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Workspace(WorkspaceMessage::OpenVaultDialog) => Task::perform(
                async {
                    let folder = rfd::AsyncFileDialog::new()
                        .set_title("Open Vault Folder")
                        .pick_folder()
                        .await;
                    folder.map(|f| f.path().to_string_lossy().to_string())
                },
                |path| Message::Workspace(WorkspaceMessage::VaultOpened(path)),
            ),
            Message::Workspace(WorkspaceMessage::CreateVaultDialog) => Task::perform(
                async {
                    let folder = rfd::AsyncFileDialog::new()
                        .set_title("Create Or Open Vault Folder")
                        .pick_folder()
                        .await;
                    folder.map(|folder| folder.path().to_string_lossy().to_string())
                },
                |path| Message::Workspace(WorkspaceMessage::VaultOpened(path)),
            ),
            Message::Workspace(WorkspaceMessage::OpenRecentVault(path)) => {
                self.open_vault(&path);
                self.index_registered_pdf_text_task()
            }
            Message::Workspace(WorkspaceMessage::VaultOpened(Some(path))) => {
                self.open_vault(&path);
                self.index_registered_pdf_text_task()
            }
            Message::Workspace(WorkspaceMessage::VaultOpened(None)) => Task::none(),
            Message::Shell(ShellMessage::SidebarToggle) => {
                self.toggle_sidebar_visible();
                Task::none()
            }
            Message::Workspace(WorkspaceMessage::FileClicked(path)) => {
                if self.pdf.showing_pdf && self.pdf.active_path.is_some() {
                    self.push_pdf_navigation_history();
                } else if self.workspace.active_path.is_some() {
                    self.push_markdown_navigation_history();
                }
                let mut path = path.trim().to_string();
                if path.starts_with('<') && path.ends_with('>') {
                    path = path[1..path.len() - 1].to_string();
                }

                // Resolve markdown reference links if present in the document
                if !path.contains("://") && !path.contains('/') && !path.ends_with(".md") {
                    let ref_def = format!("[{}]:", path.to_lowercase());
                    for line in self.editor.buffer.text().lines() {
                        let trimmed = line.trim_start();
                        if trimmed.to_lowercase().starts_with(&ref_def) {
                            let mut resolved = trimmed[ref_def.len()..].trim().to_string();
                            if resolved.starts_with('<') && resolved.ends_with('>') {
                                resolved = resolved[1..resolved.len() - 1].to_string();
                            }
                            path = resolved;
                            break;
                        }
                    }
                }

                if let Some(target) = parse_pdf_link(&path) {
                    let resolved_pdf_path = resolve_relative_link_path(
                        self.workspace.vault_root.as_deref(),
                        self.workspace.active_path.as_deref(),
                        &target.path,
                    );

                    self.shell.split_view_active = true;
                    self.pdf.showing_pdf = true;
                    self.set_active_panel(ActivePanel::Pdf);

                    if self.pdf_paths_match(self.pdf.active_path.as_deref(), &resolved_pdf_path) {
                        if let Some(ann_id) = &target.annotation_id {
                            if let Some((target_page, _)) = self.find_pdf_annotation(ann_id) {
                                self.pdf.focused_annotation_id = Some(ann_id.to_string());
                                return self.navigate_pdf_page(target_page);
                            }

                            self.pdf.initial_target_annotation = Some(ann_id.to_string());
                            self.pdf.focused_annotation_id = Some(ann_id.to_string());
                        }

                        if let Some(p) = target.page {
                            self.navigate_pdf_page(p.saturating_sub(1))
                        } else {
                            Task::none()
                        }
                    } else {
                        self.pdf.initial_target_page = target.page.map(|p| p.saturating_sub(1));
                        self.pdf.initial_target_annotation = target.annotation_id;
                        self.open_pdf(&resolved_pdf_path)
                    }
                } else {
                    let is_url = path.starts_with("http://")
                        || path.starts_with("https://")
                        || path.contains("://");

                    if is_url {
                        #[cfg(target_os = "windows")]
                        let _ = std::process::Command::new("cmd")
                            .args(["/C", "start", "", &path])
                            .spawn();
                        #[cfg(target_os = "macos")]
                        let _ = std::process::Command::new("open").arg(&path).spawn();
                        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                        let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
                        Task::none()
                    } else {
                        let (file_part, anchor_part) = if let Some(idx) = path.find('#') {
                            let anchor = &path[idx + 1..];
                            if anchor
                                .chars()
                                .any(|c| matches!(c, '%' | '^' | '&' | '*' | '!' | '@' | '(' | ')'))
                            {
                                (path.as_str(), None)
                            } else {
                                (&path[..idx], Some(anchor))
                            }
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
                                    &self.editor.highlighted_lines,
                                    &target_slug,
                                ) {
                                    let scroll_task = self.center_editor_line(line_idx);
                                    let cmd_task =
                                        self.run_editor_command(EditorCommand::SetCursor {
                                            line: line_idx,
                                            col: 0,
                                        });
                                    Task::batch(vec![cmd_task, scroll_task])
                                } else {
                                    self.overlays.toast = Some(format!(
                                        "Heading or widget not found: #{}",
                                        anchor_part
                                    ));
                                    Task::none()
                                }
                            } else {
                                let mut resolved_file = resolve_relative_link_path(
                                    self.workspace.vault_root.as_deref(),
                                    self.workspace.active_path.as_deref(),
                                    file_part,
                                );
                                if std::path::Path::new(&resolved_file).extension().is_none() {
                                    resolved_file.push_str(".md");
                                }

                                let is_same_file =
                                    self.workspace.active_path.as_deref() == Some(&resolved_file);
                                if is_same_file {
                                    let target_slug = slugify(anchor_part);
                                    if let Some(line_idx) = find_heading_or_widget_line(
                                        &self.editor.buffer.text(),
                                        &self.editor.highlighted_lines,
                                        &target_slug,
                                    ) {
                                        let scroll_task = self.center_editor_line(line_idx);
                                        let cmd_task =
                                            self.run_editor_command(EditorCommand::SetCursor {
                                                line: line_idx,
                                                col: 0,
                                            });
                                        Task::batch(vec![cmd_task, scroll_task])
                                    } else {
                                        self.overlays.toast = Some(format!(
                                            "Heading or widget not found: #{}",
                                            anchor_part
                                        ));
                                        Task::none()
                                    }
                                } else {
                                    self.workspace.selected_path = Some(resolved_file.clone());
                                    let open_task = self.open_file_extended(&resolved_file, false);

                                    let target_slug = slugify(anchor_part);
                                    if let Some(line_idx) = find_heading_or_widget_line(
                                        &self.editor.buffer.text(),
                                        &self.editor.highlighted_lines,
                                        &target_slug,
                                    ) {
                                        let scroll_task = self.center_editor_line(line_idx);
                                        let cmd_task =
                                            self.run_editor_command(EditorCommand::SetCursor {
                                                line: line_idx,
                                                col: 0,
                                            });
                                        Task::batch(vec![open_task, cmd_task, scroll_task])
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
                            }
                        } else {
                            let mut resolved_path = resolve_relative_link_path(
                                self.workspace.vault_root.as_deref(),
                                self.workspace.active_path.as_deref(),
                                &path,
                            );
                            if std::path::Path::new(&resolved_path).extension().is_none() {
                                resolved_path.push_str(".md");
                            }
                            self.workspace.selected_path = Some(resolved_path.clone());
                            let lower = resolved_path.to_lowercase();
                            if lower.ends_with(".md") || lower.ends_with(".markdown") {
                                self.pdf.showing_pdf = false;
                                self.open_file(&resolved_path)
                            } else if lower.ends_with(".pdf") {
                                self.pdf.active_path = Some(resolved_path.clone());
                                self.pdf.showing_pdf = true;
                                self.open_pdf(&resolved_path)
                            } else if is_supported_image_path(&lower) {
                                self.open_image(&resolved_path)
                            } else {
                                Task::none()
                            }
                        }
                    }
                }
            }
            Message::Workspace(message @ WorkspaceMessage::FolderToggled(_)) => {
                self.workspace.update_local(&message);
                Task::none()
            }
            Message::Workspace(WorkspaceMessage::CreateFileDialog) => {
                self.overlays.active_modal = Some(views::modals::ModalType::CreateFile);
                self.overlays.modal_input.clear();
                self.overlays.link_note_picker_search.clear();
                Task::none()
            }
            Message::Workspace(WorkspaceMessage::CreateFolderDialog) => {
                self.overlays.active_modal = Some(views::modals::ModalType::CreateFolder);
                self.overlays.modal_input.clear();
                self.overlays.link_note_picker_search.clear();
                Task::none()
            }
            Message::Workspace(WorkspaceMessage::DeleteFileDialog(path)) => {
                self.overlays.active_modal = Some(views::modals::ModalType::Delete(path));
                Task::none()
            }
            Message::Overlay(OverlayMessage::NameModalInputChanged(input)) => {
                self.overlays.modal_input = input;
                Task::none()
            }
            Message::Overlay(OverlayMessage::NameModalCancel) => {
                self.overlays.close_modal();
                Task::none()
            }
            Message::Overlay(OverlayMessage::NameModalSubmitCurrent) => {
                if let Some(views::modals::ModalType::GoToPage { total, error: _ }) =
                    self.overlays.active_modal.clone()
                {
                    match self.overlays.modal_input.trim().parse::<u16>() {
                        Ok(page_num) if page_num >= 1 && page_num <= total => {
                            self.push_pdf_navigation_history();
                            self.overlays.active_modal = None;
                            let target_page = page_num.saturating_sub(1);
                            self.overlays.modal_input.clear();
                            return self.navigate_pdf_page(target_page);
                        }
                        _ => {
                            self.overlays.active_modal = Some(views::modals::ModalType::GoToPage {
                                total,
                                error: Some(format!("Page must be between 1 and {}", total)),
                            });
                            return Task::none();
                        }
                    }
                }
                if matches!(
                    self.overlays.active_modal,
                    Some(views::modals::ModalType::CreateFile)
                        | Some(views::modals::ModalType::CreateFolder)
                        | Some(views::modals::ModalType::QuickNote(_))
                        | Some(views::modals::ModalType::LinkNote(_))
                ) {
                    Task::done(Message::NameModalSubmit(self.overlays.modal_input.clone()))
                } else {
                    Task::none()
                }
            }
            Message::Overlay(OverlayMessage::NameModalSubmit(input)) => {
                if let Some(views::modals::ModalType::QuickNote(id)) =
                    self.overlays.active_modal.clone()
                {
                    self.overlays.close_modal();
                    return Task::done(Message::Pdf(PdfMessage::AddQuickNote(id, input)));
                }
                if let Some(views::modals::ModalType::LinkNote(id)) =
                    self.overlays.active_modal.clone()
                {
                    self.overlays.close_modal();
                    return Task::done(Message::Pdf(PdfMessage::LinkNote(id, input)));
                }
                if let Some(views::modals::ModalType::AnnotationTags(id)) =
                    self.overlays.active_modal.clone()
                {
                    self.overlays.close_modal();
                    return Task::done(Message::Pdf(PdfMessage::UpdateAnnotationTags(id, input)));
                }

                let name = input.trim();
                if name.is_empty() {
                    self.overlays.toast = Some("Name cannot be empty".to_string());
                    return Task::none();
                }

                let target_path = self.new_entry_path(name);
                let result = match self.overlays.active_modal.as_ref() {
                    Some(views::modals::ModalType::CreateFile) => {
                        let path =
                            if target_path.ends_with(".md") || target_path.ends_with(".markdown") {
                                target_path
                            } else {
                                format!("{}.md", target_path)
                            };
                        md_editor_core::vault::create_file(&self.state, &path)
                    }
                    Some(views::modals::ModalType::CreateFolder) => {
                        md_editor_core::vault::create_dir(&self.state, &target_path)
                    }
                    _ => Ok(()),
                };

                match result {
                    Ok(()) => {
                        self.workspace.vault_entries =
                            md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                        self.overlays.active_modal = None;
                        self.overlays.modal_input.clear();
                        self.overlays.link_note_picker_search.clear();
                        self.overlays.toast = Some("Created".to_string());
                    }
                    Err(err) => self.overlays.toast = Some(err),
                }
                Task::none()
            }
            Message::Workspace(WorkspaceMessage::DeleteFile(path)) => {
                match md_editor_core::vault::delete_entry(&self.state, &path) {
                    Ok(()) => {
                        self.workspace.vault_entries =
                            md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                        if self.workspace.active_path.as_deref() == Some(path.as_str()) {
                            self.workspace.active_path = None;
                            self.editor.buffer = DocBuffer::new();
                            self.editor.highlighted_lines.clear();
                        }
                        if self.pdf.active_path.as_deref() == Some(path.as_str()) {
                            self.pdf.active_path = None;
                            self.pdf.pages.clear();
                            self.pdf.dimensions.clear();
                            self.pdf.view.page_cache.clear();
                            self.pdf.toc_entries_flat = None;
                        }
                        self.overlays.active_modal = None;
                        self.overlays.link_note_picker_search.clear();
                        self.overlays.toast = Some("Deleted".to_string());
                    }
                    Err(err) => self.overlays.toast = Some(err),
                }
                Task::none()
            }

            Message::Editor(EditorMessage::Command(command)) => self.run_editor_command(command),
            Message::Editor(EditorMessage::CommandNoScroll(command)) => {
                self.run_editor_command_with_scroll(command, false)
            }
            Message::Editor(message @ EditorMessage::MathRendered(_, _))
            | Message::Editor(message @ EditorMessage::ImageLoadFailed(_, _))
            | Message::Editor(message @ EditorMessage::Scrolled { .. })
            | Message::Editor(message @ EditorMessage::HighlightReady(_, _)) => {
                match self.editor.update_local(message) {
                    EditorEffect::None => Task::none(),
                    EditorEffect::ActivateMarkdown => {
                        self.set_active_panel(ActivePanel::Markdown);
                        Task::none()
                    }
                    EditorEffect::LoadMedia => {
                        Task::batch(vec![self.load_images(), self.load_math()])
                    }
                    EditorEffect::ShowToast(message) => {
                        self.overlays.toast = Some(message);
                        Task::none()
                    }
                }
            }
            Message::Editor(EditorMessage::Save(is_autosave)) => {
                self.editor.pending_save = None;
                if let Some(path) = &self.workspace.active_path {
                    let content = self.editor.buffer.text();
                    let _ = save_markdown_file_with_parser_targets(&self.state, path, &content);
                    self.editor.buffer.dirty = false;
                    if !is_autosave {
                        self.overlays.toast = Some("File saved".to_string());
                    }
                }
                Task::none()
            }
            Message::Editor(EditorMessage::CheckboxToggle(line_idx)) => {
                self.run_editor_command(EditorCommand::ToggleCheckbox { line: line_idx })
            }
            Message::Editor(EditorMessage::BlockContextMenu {
                line_idx,
                absolute_pos,
            }) => {
                if let Some(items) = crate::editor::renderer::get_block_context_menu_items(
                    &self.editor.highlighted_lines,
                    line_idx,
                ) {
                    self.overlays.active_modal =
                        Some(views::modals::ModalType::EditorBlockContextMenu(
                            views::modals::EditorBlockContextMenuState {
                                absolute_pos,
                                line_idx,
                                items,
                            },
                        ));
                }
                Task::none()
            }
            Message::Editor(EditorMessage::BlockAction { line_idx, action }) => {
                self.overlays.active_modal = None;
                self.handle_editor_block_action(line_idx, action)
            }
            Message::Editor(EditorMessage::ContextMenu {
                line_idx,
                col,
                absolute_pos,
            }) => {
                // Build the link context-menu if the cursor lands on a link span.
                if let Some(line) = self.editor.highlighted_lines.get(line_idx) {
                    let existing_files: HashSet<String> = self
                        .workspace
                        .vault_entries
                        .iter()
                        .filter(|e| !e.is_dir)
                        .map(|e| e.path.clone())
                        .collect();

                    let mut x_acc = 0usize;
                    for span in &line.spans {
                        let span_len = span.text.chars().count();
                        let span_end = x_acc + span_len;
                        if span.is_link && col >= x_acc && col < span_end {
                            if let Some(target) = span.link_target.as_deref() {
                                let items = crate::views::modals::get_link_context_menu_items(
                                    target,
                                    self.workspace.vault_root.as_deref(),
                                    self.workspace.active_path.as_deref(),
                                    &existing_files,
                                );
                                let source_text = span.text.clone();
                                let display_text = span
                                    .display_text
                                    .clone()
                                    .unwrap_or_else(|| span.text.clone());
                                self.overlays.active_modal =
                                    Some(views::modals::ModalType::EditorLinkContextMenu(
                                        views::modals::EditorLinkContextMenuState {
                                            absolute_pos,
                                            line_idx,
                                            start_col: x_acc,
                                            end_col: span_end,
                                            link_target: target.to_string(),
                                            source_text,
                                            display_text,
                                            items,
                                        },
                                    ));
                            }
                            break;
                        }
                        x_acc = span_end;
                    }
                }
                Task::none()
            }
            Message::Editor(EditorMessage::LinkAction {
                line_idx,
                start_col,
                end_col,
                link_target,
                action,
            }) => {
                self.overlays.active_modal = None;
                self.handle_editor_link_action(line_idx, start_col, end_col, link_target, action)
            }
            Message::Editor(EditorMessage::CursorMove(line, col)) => {
                self.run_editor_command(EditorCommand::SetCursor { line, col })
            }
            Message::Editor(EditorMessage::ScrollToTarget(target_y)) => operation::scroll_to(
                iced::advanced::widget::Id::new(EDITOR_SCROLLABLE_ID),
                AbsoluteOffset {
                    x: 0.0,
                    y: target_y,
                },
            ),
            Message::Editor(EditorMessage::AutosaveElapsed) => {
                if let Some(requested) = self.editor.pending_save {
                    if requested.elapsed() >= EDITOR_AUTOSAVE_DELAY {
                        self.editor.pending_save = None;
                        return Task::done(Message::Editor(EditorMessage::Save(true)));
                    }
                }
                Task::none()
            }
            Message::Editor(EditorMessage::HighlightDebounceElapsed) => {
                if self
                    .editor
                    .pending_highlight_requested_at
                    .is_some_and(|requested| requested.elapsed() < HIGHLIGHT_DEBOUNCE)
                {
                    return Task::none();
                }
                let Some(generation) = self.editor.pending_highlight_generation else {
                    return Task::none();
                };
                let Some(text) = self.editor.pending_highlight_text.take() else {
                    self.editor.pending_highlight_generation = None;
                    self.editor.pending_highlight_requested_at = None;
                    return Task::none();
                };
                self.editor.pending_highlight_generation = None;
                self.editor.pending_highlight_requested_at = None;
                Self::highlight_task(generation, text)
            }
            Message::Shell(ShellMessage::KeyboardModifiersChanged(modifiers)) => {
                self.shell.keyboard_modifiers = modifiers;
                Task::none()
            }

            Message::Shell(ShellMessage::TocClicked(index)) => {
                if self.workspace.active_path.is_some() {
                    if self.pdf.showing_pdf && self.pdf.active_path.is_some() {
                        self.push_pdf_navigation_history();
                    } else if self.workspace.active_path.is_some() {
                        self.push_markdown_navigation_history();
                    }
                    self.set_active_panel(ActivePanel::Markdown);
                    Task::done(Message::Editor(EditorMessage::CursorMove(index, 0)))
                } else {
                    Task::none()
                }
            }

            Message::Tracker(message) => self.update_tracker(message),
            Message::Overlay(OverlayMessage::CommandPaletteOpen) => {
                self.overlays.command_palette_visible = true;
                self.overlays.command_palette_query.clear();
                focus_command_palette_input()
            }
            Message::Overlay(OverlayMessage::CommandPaletteQueryChanged(query)) => {
                self.overlays.command_palette_query = query;
                Task::none()
            }
            Message::Overlay(OverlayMessage::CommandPaletteCommandClicked(shortcut)) => {
                self.overlays.close_command_palette();
                Task::done(Message::KeyboardShortcut(shortcut))
            }
            Message::Citation(CitationMessage::PaletteToggle) => {
                self.overlays.citation_palette_visible = !self.overlays.citation_palette_visible;
                self.overlays.citation_palette_query.clear();
                if self.overlays.citation_palette_visible {
                    self.overlays.command_palette_visible = false;
                    self.search.visible = false;
                    return focus_citation_palette_input();
                }
                Task::none()
            }
            Message::Citation(CitationMessage::PaletteQueryChanged(query)) => {
                self.overlays.citation_palette_query = query;
                Task::none()
            }
            Message::Citation(CitationMessage::PaletteSubmitFirst) => {
                self.submit_first_citation_palette_item()
            }
            Message::Citation(CitationMessage::PaletteChoose(item)) => {
                self.choose_citation_item(item)
            }
            Message::Citation(CitationMessage::ExcerptModeToggle) => {
                self.overlays.excerpt_mode_active = !self.overlays.excerpt_mode_active;
                let status = if self.overlays.excerpt_mode_active {
                    "enabled"
                } else {
                    "disabled"
                };
                self.overlays.toast = Some(format!("Excerpt mode {status}"));
                Task::none()
            }
            Message::Citation(CitationMessage::ExcerptQueueAdd(item)) => {
                self.overlays.excerpts_queue.push(item);
                self.overlays.toast = Some("Excerpt added to queue".to_string());
                Task::none()
            }
            Message::Citation(CitationMessage::ExcerptQueueRemove(idx)) => {
                if idx < self.overlays.excerpts_queue.len() {
                    self.overlays.excerpts_queue.remove(idx);
                    self.overlays.toast = Some("Excerpt removed from queue".to_string());
                }
                Task::none()
            }
            Message::Citation(CitationMessage::ExcerptQueueClear) => {
                self.overlays.excerpts_queue.clear();
                self.overlays.toast = Some("Excerpt queue cleared".to_string());
                Task::none()
            }
            Message::Citation(CitationMessage::ExcerptQueueInsertBatch) => {
                if self.workspace.active_path.is_none() {
                    self.overlays.toast =
                        Some("Open a markdown file before inserting batch".to_string());
                    return Task::none();
                }
                if self.overlays.excerpts_queue.is_empty() {
                    self.overlays.toast = Some("Excerpt queue is empty".to_string());
                    return Task::none();
                }

                let mut batch_text = String::new();
                for item in &self.overlays.excerpts_queue {
                    batch_text.push_str(&format_citation_item_as_markdown(
                        item,
                        self.pdf.active_path.as_deref(),
                    ));
                }

                self.overlays.excerpts_queue.clear();
                self.set_active_panel(ActivePanel::Markdown);
                self.run_editor_command(crate::editor::buffer::EditorCommand::InsertText(
                    batch_text,
                ))
            }
            Message::Search(SearchMessage::Open) => {
                self.search.visible = true;
                if self.pdf.active_path.is_some() && !self.pdf.view.search.query.trim().is_empty() {
                    Task::batch(vec![self.search_pdf(), focus_global_search_input()])
                } else {
                    focus_global_search_input()
                }
            }
            Message::Search(SearchMessage::Close) => {
                self.search.visible = false;
                self.search.global.id = self.search.global.id.wrapping_add(1);
                self.search.editor.visible = false;
                self.pdf.view.search.visible = false;
                self.cancel_global_pdf_search();
                self.search.global.results.clear();
                self.search.global.error = None;
                self.restore_scroll_positions()
            }
            Message::Search(SearchMessage::QueryChanged(q)) => {
                if self.pdf_search_is_active() {
                    self.pdf.view.search.query = q.clone();
                    self.pdf.view.search.active_index = None;
                    self.search.pdf_error = None;
                    if q.len() > 1 {
                        self.search_pdf()
                    } else {
                        self.pdf.view.search.matches.clear();
                        self.pdf.view.search.page_index.clear();
                        Task::none()
                    }
                } else if self.search.visible {
                    self.search.editor.query = q.clone();
                    self.search.editor.active_index = None;
                    self.search.editor.wrap_status = None;
                    if q.trim().len() > 2 {
                        self.search.global.searching = true;
                        self.search.global.error = None;
                        self.search.global.id = self.search.global.id.wrapping_add(1);
                        let search_id = self.search.global.id;

                        let state = self.state.clone();
                        let query = self.build_global_search_query(q.clone());
                        let include_pdf_content =
                            query.includes(md_editor_core::types::UnifiedSearchSource::PdfContent);
                        let db_query = query.clone();

                        let db_task = Task::perform(
                            async move {
                                let res = md_editor_core::vault::search_vault_unified_query(
                                    &state, &db_query,
                                );
                                (search_id, res)
                            },
                            |(id, res)| match res {
                                Ok(matches) => {
                                    Message::Search(SearchMessage::UnifiedMatchesFound(id, matches))
                                }
                                Err(err) => {
                                    Message::Search(SearchMessage::UnifiedFinished(id, Err(err)))
                                }
                            },
                        );

                        self.search.global.results.clear();

                        let active_pdf_task = if self.pdf.active_path.is_some()
                            && include_pdf_content
                        {
                            self.pdf.view.search.query = q.clone();
                            self.pdf.view.search.active_index = None;
                            self.search.pdf_error = None;
                            let task = self.search_pdf();
                            if self.pdf.view.search.searching {
                                self.search.global.pdf_search_id = Some(self.search.pdf_active_id);
                                self.search.global.pending_pdf = true;
                            } else {
                                self.search.global.pdf_search_id = None;
                                self.search.global.pending_pdf = false;
                            }
                            task
                        } else {
                            self.cancel_global_pdf_search();
                            Task::none()
                        };
                        let vault_pdf_task = if include_pdf_content {
                            self.search.global.pending_vault_pdf = true;
                            self.search.global.pdf_status = Some(format!(
                                "PDF text: searching up to {} registered PDFs",
                                GLOBAL_PDF_TEXT_SEARCH_MAX_DOCUMENTS
                            ));
                            self.search_registered_pdf_text_task(search_id, query.clone())
                        } else {
                            self.search.global.pending_vault_pdf = false;
                            self.search.global.pdf_status = None;
                            Task::none()
                        };
                        self.search.global.pending_db = true;
                        self.update_global_search_searching();

                        Task::batch(vec![db_task, active_pdf_task, vault_pdf_task])
                    } else {
                        self.search.global.results.clear();
                        self.search.global.error = None;
                        self.search.global.pdf_status = None;
                        self.search.global.pending_db = false;
                        self.cancel_global_pdf_search();
                        self.search.global.id = self.search.global.id.wrapping_add(1);
                        if self.pdf.active_path.is_some() {
                            self.pdf.view.search.query = q.clone();
                            self.pdf.view.search.matches.clear();
                            self.pdf.view.search.page_index.clear();
                        }
                        Task::none()
                    }
                } else {
                    self.search.editor.query = q.clone();
                    self.search.editor.active_index = None;
                    self.search.editor.wrap_status = None;
                    if q.len() > 2 && !self.search.editor.regex {
                        if let Ok(res) = md_editor_core::vault::search_vault(&self.state, &q) {
                            self.search.editor.matches = res;
                        }
                    } else {
                        self.search.editor.matches.clear();
                    }
                    Task::none()
                }
            }
            Message::Search(message @ SearchMessage::ReplaceChanged(_)) => {
                self.search.update_local(&message);
                Task::none()
            }
            Message::Search(SearchMessage::RegexToggled(value)) => {
                if self.pdf_search_is_active() {
                    self.pdf.view.search.regex = value;
                    self.pdf.view.search.active_index = None;
                    if self.pdf.view.search.query.len() > 1 {
                        self.search_pdf()
                    } else {
                        Task::none()
                    }
                } else {
                    self.search.editor.regex = value;
                    self.search.editor.active_index = None;
                    self.search.editor.wrap_status = None;
                    Task::none()
                }
            }
            Message::Search(SearchMessage::MatchCaseToggled(value)) => {
                if self.pdf_search_is_active() {
                    self.pdf.view.search.match_case = value;
                    self.pdf.view.search.active_index = None;
                    if self.pdf.view.search.query.len() > 1 {
                        self.search_pdf()
                    } else {
                        Task::none()
                    }
                } else {
                    self.search.editor.match_case = value;
                    self.search.editor.active_index = None;
                    self.search.editor.wrap_status = None;
                    Task::none()
                }
            }
            Message::Search(message @ SearchMessage::SourceToggled(_, _)) => {
                if self.search.update_local(&message) == SearchLocalEffect::RestartVisibleSearch {
                    Task::done(Message::Search(SearchMessage::QueryChanged(
                        self.search.editor.query.clone(),
                    )))
                } else {
                    Task::none()
                }
            }
            Message::Search(SearchMessage::Previous) => {
                if self.pdf_search_is_active() {
                    self.navigate_pdf_search(false)
                } else if self.editor_search_is_active() {
                    self.navigate_file_search(false)
                } else {
                    Task::none()
                }
            }
            Message::Search(SearchMessage::Next) => {
                if self.pdf_search_is_active() {
                    self.navigate_pdf_search(true)
                } else if self.editor_search_is_active() {
                    self.navigate_file_search(true)
                } else {
                    Task::none()
                }
            }
            Message::Search(SearchMessage::ReplaceAll) => {
                match self.replace_all_in_current_document() {
                    Ok((count, task)) => {
                        self.overlays.toast = Some(format!("Replaced {} matches", count));
                        task
                    }
                    Err(err) => {
                        self.overlays.toast = Some(err);
                        Task::none()
                    }
                }
            }
            Message::Search(SearchMessage::Replace) => match self.replace_current_match() {
                Ok(task) => task,
                Err(err) => {
                    self.overlays.toast = Some(err);
                    Task::none()
                }
            },

            Message::Search(SearchMessage::UnifiedMatchesFound(search_id, matches)) => {
                if search_id == self.search.global.id {
                    self.search.global.results.retain(|r| {
                        r.group == md_editor_core::types::SearchResultGroup::PdfContent
                    });
                    self.search.global.results.extend(matches);
                    self.search.global.results.sort_by(|a, b| {
                        b.score
                            .partial_cmp(&a.score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then_with(|| a.group.cmp(&b.group))
                            .then_with(|| a.path.cmp(&b.path))
                            .then_with(|| a.line.cmp(&b.line))
                    });
                    self.search.global.pending_db = false;
                    self.update_global_search_searching();
                }
                Task::none()
            }
            Message::Search(SearchMessage::UnifiedPdfMatchesFound(search_id, batch)) => {
                if self.search.visible && search_id == self.search.global.id {
                    self.search.global.pdf_status = Some(format_pdf_search_status(&batch));
                    self.search.global.results.extend(batch.results);
                    self.search.global.results.sort_by(|a, b| {
                        b.score
                            .partial_cmp(&a.score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then_with(|| a.group.cmp(&b.group))
                            .then_with(|| a.path.cmp(&b.path))
                            .then_with(|| a.line.cmp(&b.line))
                    });
                    self.search.global.pending_vault_pdf = false;
                    self.update_global_search_searching();
                }
                Task::none()
            }
            Message::Search(SearchMessage::UnifiedFinished(search_id, result)) => {
                if search_id == self.search.global.id {
                    self.search.global.pending_db = false;
                    if let Err(err) = result {
                        self.search.global.error = Some(err);
                    }
                    self.update_global_search_searching();
                }
                Task::none()
            }
            Message::Search(SearchMessage::UnifiedResultClicked(result)) => {
                if self.pdf.showing_pdf && self.pdf.active_path.is_some() {
                    self.push_pdf_navigation_history();
                } else if self.workspace.active_path.is_some() {
                    self.push_markdown_navigation_history();
                }
                self.search.visible = false;

                match result.group {
                    md_editor_core::types::SearchResultGroup::MarkdownContent
                    | md_editor_core::types::SearchResultGroup::Heading => {
                        let open_task = self.open_file(&result.path);
                        let cursor_task = Task::done(Message::Editor(EditorMessage::CursorMove(
                            result.line.saturating_sub(1),
                            0,
                        )));
                        Task::batch(vec![open_task, cursor_task])
                    }
                    md_editor_core::types::SearchResultGroup::Filename => {
                        if result.path.ends_with(".pdf") {
                            if self.pdf_paths_match(self.pdf.active_path.as_deref(), &result.path) {
                                self.set_active_panel(ActivePanel::Pdf);
                                self.pdf.showing_pdf = true;
                                Task::none()
                            } else {
                                self.open_pdf(&result.path)
                            }
                        } else {
                            self.open_file(&result.path)
                        }
                    }
                    md_editor_core::types::SearchResultGroup::PdfContent => {
                        if self.pdf_paths_match(self.pdf.active_path.as_deref(), &result.path) {
                            self.set_active_panel(ActivePanel::Pdf);
                            self.pdf.showing_pdf = true;
                            if let Some(index) = result
                                .annotation_id
                                .as_deref()
                                .and_then(|id| id.parse::<usize>().ok())
                            {
                                self.navigate_pdf_search_to_index(index)
                            } else {
                                let page = result.page_index.unwrap_or(0);
                                self.navigate_pdf_page(page)
                            }
                        } else {
                            let page = result.page_index.unwrap_or(0);
                            self.pdf.initial_target_page = Some(page);
                            self.open_pdf(&result.path)
                        }
                    }
                    md_editor_core::types::SearchResultGroup::Annotation
                    | md_editor_core::types::SearchResultGroup::QuickNote => {
                        if self.pdf_paths_match(self.pdf.active_path.as_deref(), &result.path) {
                            self.set_active_panel(ActivePanel::Pdf);
                            self.pdf.showing_pdf = true;
                            let page = result.page_index.unwrap_or(0);
                            self.pdf.focused_annotation_id = result.annotation_id.clone();
                            self.navigate_pdf_page(page)
                        } else {
                            let page = result.page_index.unwrap_or(0);
                            self.pdf.initial_target_page = Some(page);
                            self.pdf.initial_target_annotation = result.annotation_id.clone();
                            self.open_pdf(&result.path)
                        }
                    }
                }
            }
            Message::Search(SearchMessage::PdfTextIndexFinished(result)) => {
                if let Err(err) = result {
                    self.search.global.error = Some(err);
                }
                Task::none()
            }
            Message::Overlay(OverlayMessage::ToastHide) => {
                self.overlays.toast = None;
                Task::none()
            }
            Message::System(SystemMessage::KeyboardShortcut(s)) => {
                match s {
                    Shortcut::Escape => {
                        // Close overlays in priority order
                        if self.pdf.selection.is_some() {
                            self.pdf.selection = None;
                        } else if self.pdf.focused_annotation_id.is_some() {
                            self.pdf.focused_annotation_id = None;
                        } else if self.pdf.link_preview.is_some() {
                            self.pdf.link_preview = None;
                            self.overlays.active_modal = None;
                        } else if self.overlays.active_modal.is_some() {
                            self.overlays.close_modal();
                        } else if self.tracker.visible {
                            self.tracker.visible = false;
                        } else if self.search.editor.visible || self.pdf.view.search.visible {
                            self.search.editor.visible = false;
                            self.pdf.view.search.visible = false;
                            return self.restore_scroll_positions();
                        } else if self.search.visible {
                            self.search.visible = false;
                            return self.restore_scroll_positions();
                        } else if self.overlays.command_palette_visible {
                            self.overlays.command_palette_visible = false;
                        } else if self.overlays.citation_palette_visible {
                            self.overlays.citation_palette_visible = false;
                        } else if self.shell.toc_visible {
                            self.shell.toc_visible = false;
                        }
                        Task::none()
                    }
                    Shortcut::ToggleSidebar => {
                        self.toggle_sidebar_visible();
                        Task::none()
                    }
                    Shortcut::NavBack => Task::done(Message::Pdf(PdfMessage::NavBack)),
                    Shortcut::NavForward => Task::done(Message::Pdf(PdfMessage::NavForward)),
                    Shortcut::Save => Task::done(Message::Editor(EditorMessage::Save(false))),
                    Shortcut::OpenVault => {
                        Task::done(Message::Workspace(WorkspaceMessage::OpenVaultDialog))
                    }
                    Shortcut::NewFile => {
                        Task::done(Message::Workspace(WorkspaceMessage::CreateFileDialog))
                    }
                    Shortcut::Search => {
                        if self.shell.split_view_active && self.workspace.active_path.is_some() {
                            if self.shell.active_panel == ActivePanel::Pdf
                                && self.pdf.active_path.is_some()
                            {
                                self.pdf.view.search.visible = !self.pdf.view.search.visible;
                                self.search.editor.visible = false;
                                self.search.visible = false;
                                if self.pdf.view.search.visible {
                                    if !self.pdf.view.search.query.trim().is_empty() {
                                        return Task::batch(vec![
                                            self.search_pdf(),
                                            focus_pdf_search_input(),
                                            self.restore_scroll_positions(),
                                        ]);
                                    }
                                    return Task::batch(vec![
                                        focus_pdf_search_input(),
                                        self.restore_scroll_positions(),
                                    ]);
                                }
                            } else {
                                self.search.editor.visible = !self.search.editor.visible;
                                self.pdf.view.search.visible = false;
                                self.search.visible = false;
                                if self.search.editor.visible {
                                    return Task::batch(vec![
                                        focus_file_search_input(),
                                        self.restore_scroll_positions(),
                                    ]);
                                }
                            }
                        } else if self.pdf.active_path.is_some() && self.pdf.showing_pdf {
                            self.pdf.view.search.visible = !self.pdf.view.search.visible;
                            self.search.editor.visible = false;
                            self.search.visible = false;
                            if self.pdf.view.search.visible {
                                if !self.pdf.view.search.query.trim().is_empty() {
                                    return Task::batch(vec![
                                        self.search_pdf(),
                                        focus_pdf_search_input(),
                                        self.restore_scroll_positions(),
                                    ]);
                                }
                                return Task::batch(vec![
                                    focus_pdf_search_input(),
                                    self.restore_scroll_positions(),
                                ]);
                            }
                        } else if self.workspace.active_path.is_some() {
                            self.search.editor.visible = !self.search.editor.visible;
                            self.pdf.view.search.visible = false;
                            self.search.visible = false;
                            if self.search.editor.visible {
                                return Task::batch(vec![
                                    focus_file_search_input(),
                                    self.restore_scroll_positions(),
                                ]);
                            }
                        } else {
                            self.search.visible = true;
                            return focus_global_search_input();
                        }
                        Task::none()
                    }
                    Shortcut::CommandPalette => {
                        self.overlays.command_palette_visible = true;
                        self.overlays.command_palette_query.clear();
                        self.overlays.citation_palette_visible = false;
                        focus_command_palette_input()
                    }
                    Shortcut::CitationPalette => {
                        self.overlays.citation_palette_visible =
                            !self.overlays.citation_palette_visible;
                        self.overlays.citation_palette_query.clear();
                        if self.overlays.citation_palette_visible {
                            self.overlays.command_palette_visible = false;
                            self.search.visible = false;
                            return focus_citation_palette_input();
                        }
                        Task::none()
                    }
                    Shortcut::ExcerptModeToggle => Task::done(Message::ExcerptModeToggle),
                    Shortcut::ExcerptInsertBatch => Task::done(Message::ExcerptQueueInsertBatch),
                    Shortcut::Submit => {
                        if self.overlays.citation_palette_visible {
                            Task::done(Message::CitationPaletteSubmitFirst)
                        } else {
                            Task::done(Message::NameModalSubmitCurrent)
                        }
                    }
                    Shortcut::ToggleBacklinks => {
                        self.workspace.backlinks_visible = !self.workspace.backlinks_visible;
                        self.persist_shell_state();
                        Task::none()
                    }
                    Shortcut::TableOfContents => {
                        if self.workspace.active_path.is_some() || self.pdf.active_path.is_some() {
                            self.shell.toc_visible = !self.shell.toc_visible;
                            self.persist_shell_state();
                        }
                        Task::none()
                    }
                    Shortcut::StudyTracker => {
                        self.tracker.toggle_visibility();
                        self.persist_shell_state();
                        Task::none()
                    }
                    Shortcut::SplitView => Task::done(Message::SplitViewToggle),
                    Shortcut::SwitchPane => {
                        if self.shell.split_view_active
                            && self.workspace.active_path.is_some()
                            && self.pdf.active_path.is_some()
                        {
                            let next_panel = match self.shell.active_panel {
                                ActivePanel::Markdown => ActivePanel::Pdf,
                                ActivePanel::Pdf => ActivePanel::Markdown,
                            };
                            self.set_active_panel(next_panel);
                        }
                        Task::none()
                    }
                    Shortcut::ThemeDark => {
                        app_theme::set_active_theme(app_theme::AppTheme::Dark);
                        self.overlays.command_palette_visible = false;
                        self.persist_shell_state();
                        Task::none()
                    }
                    Shortcut::ThemeLight => {
                        app_theme::set_active_theme(app_theme::AppTheme::Light);
                        self.overlays.command_palette_visible = false;
                        self.persist_shell_state();
                        Task::none()
                    }
                    Shortcut::ThemeHighContrast => {
                        app_theme::set_active_theme(app_theme::AppTheme::HighContrast);
                        self.overlays.command_palette_visible = false;
                        self.persist_shell_state();
                        Task::none()
                    }
                    Shortcut::FocusMode => {
                        self.shell.sidebar_visible = false;
                        self.workspace.backlinks_visible = false;
                        self.shell.toc_visible = false;
                        self.tracker.visible = false;
                        self.shell.pdf_annotations_visible = false;
                        self.persist_shell_state();
                        Task::none()
                    }
                    Shortcut::ZoomIn => {
                        if self.pdf.active_path.is_some() && self.pdf.showing_pdf {
                            let new_zoom = (self.pdf.view.zoom + 0.1).min(4.0);
                            Task::done(Message::Pdf(PdfMessage::ZoomChanged(new_zoom)))
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::ZoomOut => {
                        if self.pdf.active_path.is_some() && self.pdf.showing_pdf {
                            let new_zoom = (self.pdf.view.zoom - 0.1).max(0.5);
                            Task::done(Message::Pdf(PdfMessage::ZoomChanged(new_zoom)))
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::ZoomFit => {
                        if self.pdf.active_path.is_some() && self.pdf.showing_pdf {
                            Task::done(Message::Pdf(PdfMessage::FitToWidth))
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::GoToPage => {
                        if self.pdf.active_path.is_some()
                            && self.pdf.showing_pdf
                            && self.pdf.total_pages > 0
                        {
                            self.overlays.active_modal = Some(views::modals::ModalType::GoToPage {
                                total: self.pdf.total_pages,
                                error: None,
                            });
                            self.overlays.modal_input.clear();
                            Task::none()
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::PdfSearch => {
                        if self.pdf.showing_pdf {
                            Task::done(Message::Pdf(PdfMessage::SearchToggle))
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::PdfHighlight => {
                        if self.pdf.showing_pdf {
                            if self.pdf.selection.is_some() {
                                let color = md_editor_core::domain::pdf::PdfAnnotationColor::Yellow;
                                Task::done(Message::Pdf(PdfMessage::CreateHighlight(color)))
                            } else {
                                self.overlays.toast =
                                    Some("Select PDF text before highlighting".to_string());
                                Task::none()
                            }
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::InsertPdfQuote => {
                        Task::done(Message::Pdf(PdfMessage::InsertQuoteLink))
                    }
                    Shortcut::InsertPdfHighlight => {
                        if let Some(annotation_id) = self.pdf.focused_annotation_id.clone() {
                            Task::done(Message::Pdf(PdfMessage::InsertAnnotationLink(
                                annotation_id,
                            )))
                        } else {
                            self.overlays.toast =
                                Some("Select a PDF highlight before inserting it".to_string());
                            Task::none()
                        }
                    }
                    Shortcut::PdfFirstPage => {
                        if self.pdf.showing_pdf {
                            Task::done(Message::Pdf(PdfMessage::FirstPage))
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::PdfLastPage => {
                        if self.pdf.showing_pdf {
                            Task::done(Message::Pdf(PdfMessage::LastPage))
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::PdfZoomInput => {
                        if self.pdf.showing_pdf {
                            self.overlays.active_modal = Some(views::modals::ModalType::GoToPage {
                                total: self.pdf.total_pages,
                                error: None,
                            });
                            self.overlays.modal_input.clear();
                            Task::none()
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::FollowCitation => self.follow_citation(),
                    Shortcut::ShowUsages => self.show_usages(),
                    _ => Task::none(),
                }
            }
            Message::Shell(ShellMessage::SplitViewToggle) => {
                if self.workspace.active_path.is_some() && self.pdf.active_path.is_some() {
                    self.shell.split_view_active = !self.shell.split_view_active;
                    self.persist_shell_state();
                    if self.pdf.fit_to_width {
                        return Task::done(Message::Pdf(PdfMessage::FitToWidth));
                    } else if self.pdf.fit_to_page {
                        return Task::done(Message::Pdf(PdfMessage::FitToPage));
                    }
                } else if self.workspace.active_path.is_some() {
                    if let Ok(Some(last_pdf)) =
                        md_editor_core::config::get_sys_config(&self.state, "last_pdf")
                    {
                        self.shell.split_view_active = true;
                        self.persist_shell_state();
                        return self.open_pdf(&last_pdf);
                    }
                    self.overlays.toast = Some("Open a PDF once to use split view".to_string());
                } else {
                    self.overlays.toast =
                        Some("Open a markdown file and a PDF to use split view".to_string());
                }
                Task::none()
            }
            Message::Shell(ShellMessage::SplitViewDragStart) => {
                self.shell.is_resizing_split = true;
                // Also start PDF split resize if showing PDF
                if self.pdf.showing_pdf && self.pdf.active_path.is_some() {
                    let has_split = !self.shell.sidebar_visible
                        && !self.tracker.visible
                        && !self.shell.toc_visible;
                    if has_split || self.shell.split_view_active {
                        self.shell.pdf_split_ratio = 0.3;
                    }
                }
                Task::none()
            }
            Message::Shell(ShellMessage::SplitViewDragging(x_pos)) => {
                if !self.shell.is_resizing_split {
                    return Task::none();
                }
                // If PDF-only mode (no split view), resize page list
                if self.pdf.showing_pdf
                    && self.pdf.active_path.is_some()
                    && !self.shell.split_view_active
                {
                    let content_width = (self.shell.window_width - 250.0).max(480.0); // sidebar width
                    let x_min = 300.0;
                    let x_max = content_width - 300.0;
                    let total_width = x_max - x_min;
                    if total_width > 1.0 {
                        self.shell.pdf_split_ratio =
                            ((x_pos - x_min) / total_width).clamp(0.15, 0.75);
                    }
                    return Task::none();
                }
                let side_width = if self.shell.sidebar_visible {
                    250.0
                } else {
                    0.0
                } + if self.tracker.visible { 300.0 } else { 0.0 }
                    + if self.shell.toc_visible { 250.0 } else { 0.0 };
                let content_width = (self.shell.window_width - side_width).max(480.0);
                let x_min = side_width + 240.0;
                let x_max = side_width + content_width - 240.0;
                let total_width = x_max - x_min;
                if total_width > 1.0 {
                    self.shell.split_ratio = ((x_pos - x_min) / total_width).clamp(0.25, 0.75);
                }
                Task::none()
            }
            Message::Shell(ShellMessage::SplitViewDragEnd) => {
                self.shell.is_resizing_split = false;
                if self.pdf.fit_to_width && self.pdf.active_path.is_some() {
                    self.persist_shell_state();
                    return Task::done(Message::Pdf(PdfMessage::FitToWidth));
                } else if self.pdf.fit_to_page && self.pdf.active_path.is_some() {
                    self.persist_shell_state();
                    return Task::done(Message::Pdf(PdfMessage::FitToPage));
                }
                self.persist_shell_state();
                Task::none()
            }
            Message::Shell(ShellMessage::WindowResized(width, height)) => {
                self.shell.window_width = width;
                self.shell.window_height = height;
                if self.pdf.fit_to_width && self.pdf.active_path.is_some() {
                    return Task::done(Message::Pdf(PdfMessage::FitToWidth));
                } else if self.pdf.fit_to_page && self.pdf.active_path.is_some() {
                    return Task::done(Message::Pdf(PdfMessage::FitToPage));
                }
                Task::none()
            }
            Message::Shell(ShellMessage::ToggleToc) => {
                if self.workspace.active_path.is_some() || self.pdf.active_path.is_some() {
                    self.shell.toc_visible = !self.shell.toc_visible;
                    self.persist_shell_state();
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn update_pdf_message(&mut self, message: PdfMessage) -> Task<Message> {
        crate::features::pdf::update::update_pdf(self, message)
    }

    fn update_tracker(&mut self, message: TrackerMessage) -> Task<Message> {
        let effect = self.tracker.update(message, std::time::Instant::now());
        match effect {
            TrackerEffect::None => {}
            TrackerEffect::PersistShellAndReload => {
                self.persist_shell_state();
                if self.tracker.visible {
                    self.tracker.kv = md_editor_core::tracker::get_kv(&self.state)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|item| (item.key, item.value))
                        .collect();
                    let config_json =
                        md_editor_core::config::get_sys_config(&self.state, "tracker_config")
                            .ok()
                            .flatten()
                            .filter(|json| views::tracker::parse_config(json).is_ok())
                            .unwrap_or_else(views::tracker::default_config_json);
                    self.tracker.replace_config(config_json);
                }
            }
            TrackerEffect::ShowToast(message) => {
                self.overlays.toast = Some(message.to_string());
            }
            TrackerEffect::SaveElapsed(elapsed) => {
                let session = md_editor_core::tracker::StudySession {
                    id: 0,
                    date: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
                    hours: (elapsed.as_secs_f32() / 3600.0).max(0.01),
                    activity_type: "Study".to_string(),
                    phase: "Focus".to_string(),
                    notes: None,
                };
                if md_editor_core::tracker::save_session(&self.state, session).is_ok() {
                    self.tracker.sessions =
                        md_editor_core::tracker::get_sessions(&self.state).unwrap_or_default();
                    self.overlays.toast = Some("Study session saved".to_string());
                }
            }
            TrackerEffect::PersistKv { key, value } => {
                if md_editor_core::tracker::set_kv(&self.state, &key, &value).is_ok() {
                    self.tracker.kv.insert(key, value);
                }
            }
            TrackerEffect::SaveConfig => {
                match views::tracker::parse_config(&self.tracker.config_json) {
                    Ok(_) => {
                        if md_editor_core::config::set_sys_config(
                            &self.state,
                            "tracker_config",
                            &self.tracker.config_json,
                        )
                        .is_ok()
                        {
                            self.overlays.toast = Some("Tracker configuration saved".to_string());
                        }
                    }
                    Err(error) => {
                        self.overlays.toast = Some(format!("Invalid tracker JSON: {error}"));
                    }
                }
            }
            TrackerEffect::AddManualSession { date, hours, notes } => {
                let session = md_editor_core::tracker::StudySession {
                    id: 0,
                    date,
                    hours,
                    activity_type: "Manual".to_string(),
                    phase: "Manual".to_string(),
                    notes,
                };
                match md_editor_core::tracker::save_session(&self.state, session) {
                    Ok(()) => {
                        self.tracker.sessions =
                            md_editor_core::tracker::get_sessions(&self.state).unwrap_or_default();
                        self.tracker.manual_hours.clear();
                        self.tracker.manual_notes.clear();
                        self.overlays.toast = Some("Manual study session added".to_string());
                    }
                    Err(error) => self.overlays.toast = Some(error),
                }
            }
            TrackerEffect::InvalidManualHours => {
                self.overlays.toast = Some("Enter a positive hour value".to_string());
            }
            TrackerEffect::DeleteSession(id) => {
                match md_editor_core::tracker::delete_session(&self.state, id) {
                    Ok(()) => {
                        self.tracker.sessions =
                            md_editor_core::tracker::get_sessions(&self.state).unwrap_or_default();
                        self.overlays.toast = Some("Session deleted".to_string());
                    }
                    Err(error) => self.overlays.toast = Some(error),
                }
            }
        }
        Task::none()
    }

    pub(crate) fn follow_citation(&mut self) -> Task<Message> {
        let cursor_line = self.editor.buffer.cursor_line;
        let cursor_col = self.editor.buffer.cursor_col;
        if cursor_line < self.editor.highlighted_lines.len() {
            let line = &self.editor.highlighted_lines[cursor_line];
            let mut current_col = 0;
            let mut target_span = None;
            for span in &line.spans {
                let span_len = span.text.chars().count();
                if cursor_col >= current_col && cursor_col < current_col + span_len {
                    target_span = Some(span);
                    break;
                }
                current_col += span_len;
            }
            if target_span.is_none() && cursor_col == current_col && !line.spans.is_empty() {
                target_span = line.spans.last();
            }
            if let Some(span) = target_span {
                if span.is_link {
                    if let Some(target) = span.link_target.as_deref() {
                        return Task::done(Message::Workspace(WorkspaceMessage::FileClicked(
                            target.to_string(),
                        )));
                    }
                }
            }
        }
        Task::none()
    }

    pub(crate) fn show_usages(&mut self) -> Task<Message> {
        let path = if self.pdf.showing_pdf && self.pdf.active_path.is_some() {
            self.pdf.active_path.clone()
        } else if self.shell.split_view_active
            && self.shell.active_panel == ActivePanel::Pdf
            && self.pdf.active_path.is_some()
        {
            self.pdf.active_path.clone()
        } else {
            self.workspace.active_path.clone()
        };

        if let Some(p) = &path {
            self.workspace.backlinks =
                md_editor_core::vault::get_mixed_backlinks(&self.state, p).unwrap_or_default();
            self.workspace.backlinks_visible = true;
            self.persist_shell_state();
        }
        Task::none()
    }

    pub(crate) fn open_file(&mut self, path: &str) -> Task<Message> {
        self.open_file_extended(path, true)
    }

    pub(crate) fn open_file_extended(&mut self, path: &str, reset_scroll: bool) -> Task<Message> {
        let is_different = self.workspace.active_path.as_deref() != Some(path);
        if is_different {
            if self.pdf.showing_pdf && self.pdf.active_path.is_some() {
                self.push_pdf_navigation_history();
            } else if self.workspace.active_path.is_some() {
                self.push_markdown_navigation_history();
            }
        }
        if let Ok(bytes) = md_editor_core::vault::open_file(&self.state, path) {
            if let Ok(content) = String::from_utf8(bytes) {
                self.editor.buffer = DocBuffer::from_text(&content);
                self.workspace.active_path = Some(path.to_string());
                let _ = reindex_markdown_file_with_parser_targets(&self.state, path, &content);
                let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
                self.editor.active_image_path = None;
                self.editor.active_image = None;
                self.pdf.showing_pdf = false;
                self.set_active_panel(ActivePanel::Markdown);
                self.editor.toc_entries = Vec::new();
                let highlight_task = self.refresh_highlighting_for_current_buffer(true);
                self.workspace.backlinks =
                    md_editor_core::vault::get_mixed_backlinks(&self.state, path)
                        .unwrap_or_default();
                if is_different && reset_scroll {
                    self.editor.scroll_y = 0.0;
                    let scroll_task = operation::scroll_to(
                        iced::advanced::widget::Id::new(EDITOR_SCROLLABLE_ID),
                        AbsoluteOffset { x: 0.0, y: 0.0 },
                    );
                    return Task::batch(vec![highlight_task, scroll_task]);
                }
                return highlight_task;
            }
        }
        Task::none()
    }

    pub(crate) fn open_pdf(&mut self, path: &str) -> Task<Message> {
        let is_different = self.pdf.active_path.as_deref() != Some(path);
        if is_different {
            if self.pdf.showing_pdf && self.pdf.active_path.is_some() {
                self.push_pdf_navigation_history();
            } else if self.workspace.active_path.is_some() {
                self.push_markdown_navigation_history();
            }
        }
        let Some(abs_path) = self.resolve_active_path(path) else {
            self.overlays.toast = Some("Open a vault before opening a PDF".to_string());
            return Task::none();
        };
        let path_str = abs_path.to_string_lossy().to_string();
        let generation = self.pdf.begin_document_load(path.to_string());
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_pdf", path);
        self.editor.active_image_path = None;
        self.editor.active_image = None;
        self.set_active_panel(ActivePanel::Pdf);
        self.search.pdf_error = None;
        self.search.pdf_active_id = 0;
        self.workspace.backlinks =
            md_editor_core::vault::get_mixed_backlinks(&self.state, path).unwrap_or_default();

        let path_for_hash = path.to_string();
        let abs_path_for_hash = abs_path.clone();
        let hash_task = Task::perform(
            async move {
                match md_editor_core::infrastructure::pdfium::document::compute_provisional_id(
                    &abs_path_for_hash,
                ) {
                    Ok((hash, len, mtime)) => Some((path_for_hash, hash, len, mtime)),
                    Err(_) => None,
                }
            },
            |res| Message::Pdf(PdfMessage::DocumentIdComputed(res)),
        );

        let _state = self.state.clone();
        let _state_toc = self.state.clone();
        let _state_sizes = self.state.clone();
        let path_clone = path_str.clone();
        let path_str_toc = path_str.clone();
        let path_for_sizes = path.to_string();
        let path_str_sizes = path_str.clone();

        Task::batch(vec![
            hash_task,
            Task::perform(
                async move {
                    let renderer = _state.pdf_renderer()?;
                    renderer.page_count(&path_clone).ok()
                },
                move |res| Message::Pdf(PdfMessage::Loaded(generation, res.unwrap_or(0))),
            ),
            Task::perform(
                async move {
                    let renderer = _state_sizes.pdf_renderer()?;
                    renderer.page_sizes(&path_str_sizes).ok()
                },
                move |res| {
                    Message::Pdf(PdfMessage::PageSizesLoaded(
                        generation,
                        path_for_sizes.clone(),
                        res.unwrap_or_default(),
                    ))
                },
            ),
            Task::perform(
                async move {
                    let renderer = _state_toc.pdf_renderer()?;
                    renderer.get_toc(&path_str_toc).ok()
                },
                move |res| Message::Pdf(PdfMessage::TocLoaded(generation, res.unwrap_or_default())),
            ),
        ])
    }

    pub(crate) fn open_image(&mut self, path: &str) -> Task<Message> {
        let Some(abs_path) = self.resolve_active_path(path) else {
            self.overlays.toast = Some("Open a vault before opening an image".to_string());
            return Task::none();
        };

        match image::open(&abs_path) {
            Ok(img) => {
                let width = img.width();
                let height = img.height();
                let handle = iced::widget::image::Handle::from_rgba(
                    width,
                    height,
                    img.into_rgba8().into_raw(),
                );
                self.editor.active_image_path = Some(path.to_string());
                let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
                self.editor.active_image = Some((handle, width as f32, height as f32));
                self.workspace.clear_active_markdown();
                self.pdf.active_path = None;
                self.pdf.showing_pdf = false;
                self.set_active_panel(ActivePanel::Markdown);
                self.editor.toc_entries.clear();
                self.pdf.toc_entries_flat = None;
            }
            Err(err) => {
                self.overlays.toast = Some(format!("Could not open image: {err}"));
            }
        }
        Task::none()
    }

    pub(crate) fn rebuild_pdf_search_page_index(&mut self) {
        self.pdf.view.search.page_index.clear();
        for (idx, result) in self.pdf.view.search.matches.iter().enumerate() {
            self.pdf
                .view
                .search
                .page_index
                .entry(result.page_index)
                .or_default()
                .push(idx);
        }
    }

    pub(crate) fn navigate_file_search(&mut self, forward: bool) -> Task<Message> {
        let matches = self.current_document_matches();
        if matches.is_empty() {
            self.search.editor.active_index = None;
            self.search.editor.wrap_status = None;
            return Task::none();
        }

        let mut wrap_status = None;
        let next_index = match self.search.editor.active_index {
            Some(index) if forward => {
                if index + 1 >= matches.len() {
                    wrap_status = Some(SearchWrapStatus::WrappedForward);
                    0
                } else {
                    index + 1
                }
            }
            Some(0) if !forward => {
                wrap_status = Some(SearchWrapStatus::WrappedBackward);
                matches.len() - 1
            }
            Some(index) => index.saturating_sub(1),
            None if forward => 0,
            None => matches.len() - 1,
        };
        self.search.editor.active_index = Some(next_index);
        self.search.editor.wrap_status = wrap_status;
        let item = matches[next_index];
        self.editor.buffer.execute(EditorCommand::SetSelection {
            anchor_line: item.line,
            anchor_col: item.start_col,
            focus_line: item.line,
            focus_col: item.end_col,
        });
        self.center_editor_line(item.line)
    }

    pub(crate) fn navigate_pdf_search(&mut self, forward: bool) -> Task<Message> {
        if self.pdf.view.search.matches.is_empty() {
            self.pdf.view.search.active_index = None;
            return Task::none();
        }

        let next_index = match self.pdf.view.search.active_index {
            Some(index) if forward => (index + 1) % self.pdf.view.search.matches.len(),
            Some(0) if !forward => self.pdf.view.search.matches.len() - 1,
            Some(index) => index.saturating_sub(1),
            None if forward => 0,
            None => self.pdf.view.search.matches.len() - 1,
        };
        self.navigate_pdf_search_to_index(next_index)
    }

    pub(crate) fn navigate_pdf_search_to_index(&mut self, index: usize) -> Task<Message> {
        let Some(result) = self.pdf.view.search.matches.get(index).cloned() else {
            self.pdf.view.search.active_index = None;
            return Task::none();
        };

        self.push_pdf_navigation_history();
        self.pdf.view.search.active_index = Some(index);
        let target_page = result
            .page_index
            .min(self.pdf.total_pages.saturating_sub(1));
        self.pdf.current_page = target_page;
        self.pdf.programmatic_scroll = true;
        self.pdf.toc_target_page = None;

        let scroll_y = self.pdf_search_match_scroll_y(&result);
        if let Some(path) = &self.pdf.active_path {
            if let Some(abs_path) = self.resolve_active_path(path) {
                let path_str = abs_path.to_string_lossy().to_string();
                if let Some(renderer) = self.state.pdf_renderer() {
                    renderer.set_visible_range(
                        target_page.saturating_sub(1).into(),
                        (target_page + 1)
                            .min(self.pdf.total_pages.saturating_sub(1))
                            .into(),
                        &path_str,
                    );
                }
            }
        }

        let mut tasks = vec![self.render_pdf_page_direct(target_page)];
        tasks.push(self.render_pdf_page_range(
            target_page.saturating_sub(2),
            (target_page + 2).min(self.pdf.total_pages.saturating_sub(1)),
        ));
        tasks.push(operation::scroll_to(
            iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
            AbsoluteOffset {
                x: 0.0,
                y: scroll_y,
            },
        ));
        Task::batch(tasks)
    }

    pub(crate) fn push_pdf_navigation_history(&mut self) {
        if self.pdf.showing_pdf && self.pdf.total_pages > 0 {
            if let Some(path) = &self.pdf.active_path {
                let target = NavigationTarget::Pdf {
                    path: path.clone(),
                    page: self.pdf.current_page,
                    scroll_offset: self.pdf.scroll_y,
                    zoom: self.pdf.view.zoom,
                };
                self.workspace.navigation_history.push(target);
            }
        }
    }

    pub(crate) fn push_markdown_navigation_history(&mut self) {
        if let Some(ref path) = self.workspace.active_path {
            let target = NavigationTarget::Markdown {
                path: path.clone(),
                line: self.editor.buffer.cursor_line,
                column: self.editor.buffer.cursor_col,
            };
            self.workspace.navigation_history.push(target);
        }
    }

    pub(crate) fn navigate_to_target(&mut self, target: NavigationTarget) -> Task<Message> {
        match target {
            NavigationTarget::Markdown { path, line, column } => {
                let mut tasks = Vec::new();
                let is_different_file = self.workspace.active_path.as_deref() != Some(&path);
                if is_different_file {
                    tasks.push(self.open_file_extended(&path, false));
                } else {
                    self.pdf.showing_pdf = false;
                    self.set_active_panel(ActivePanel::Markdown);
                }

                tasks.push(Task::done(Message::Editor(EditorMessage::Command(
                    crate::editor::buffer::EditorCommand::SetCursor { line, col: column },
                ))));

                tasks.push(self.center_editor_line(line));
                Task::batch(tasks)
            }
            NavigationTarget::Pdf {
                path,
                page,
                scroll_offset,
                zoom,
            } => {
                let mut tasks = Vec::new();
                let is_different_pdf = self.pdf.active_path.as_deref() != Some(&path);
                if is_different_pdf {
                    tasks.push(self.open_pdf(&path));
                } else {
                    self.pdf.showing_pdf = true;
                    self.set_active_panel(ActivePanel::Pdf);
                }
                self.pdf.view.zoom = zoom;
                self.pdf.current_page = page.min(self.pdf.total_pages.saturating_sub(1));
                self.pdf.pending_pages.clear();
                self.pdf.stale_pages.clear();
                self.pdf.pending_links.clear();
                self.pdf.render_generation = self.pdf.render_generation.wrapping_add(1);
                self.pdf.toc_target_page = Some(self.pdf.current_page);
                self.pdf.programmatic_scroll = true;

                let start = self.pdf.current_page.saturating_sub(2);
                let end = (self.pdf.current_page + 2).min(self.pdf.total_pages.saturating_sub(1));
                self.update_pdf_page_cache();

                tasks.push(self.render_pdf_page_range(start, end));
                tasks.push(operation::scroll_to(
                    iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                    AbsoluteOffset {
                        x: 0.0,
                        y: scroll_offset,
                    },
                ));
                Task::batch(tasks)
            }
        }
    }

    pub(crate) fn navigate_pdf_page(&mut self, page: u16) -> Task<Message> {
        let target_page = page.min(self.pdf.total_pages.saturating_sub(1));
        self.pdf.current_page = target_page;
        self.pdf.pending_pages.clear();
        self.pdf.stale_pages.clear();
        self.pdf.pending_links.clear();
        self.pdf.render_generation = self.pdf.render_generation.wrapping_add(1);
        self.pdf.toc_target_page = Some(target_page);

        if let Some(path) = &self.pdf.active_path {
            if let Some(abs_path) = self.resolve_active_path(path) {
                let path_str = abs_path.to_string_lossy().to_string();
                if let Some(renderer) = self.state.pdf_renderer() {
                    renderer.set_visible_range(target_page.into(), target_page.into(), &path_str);
                }
            }
        }

        let target_dimensions_ready = self
            .pdf
            .dimensions
            .get(target_page as usize)
            .and_then(|d| *d)
            .is_some();
        let target_image_ready = self
            .pdf
            .pages
            .get(target_page as usize)
            .is_some_and(|page| page.is_some());

        let mut tasks = Vec::new();
        if target_image_ready && target_dimensions_ready {
            tasks.push(self.load_pdf_page_links(target_page));
        } else {
            tasks.push(self.render_pdf_page_direct(target_page));
        }

        self.pdf.programmatic_scroll = true;
        let scroll_y = self.pdf_page_offset(target_page);
        let current_scroll_y = self.pdf.scroll_y;
        if (current_scroll_y - scroll_y).abs() < 1.0 && target_image_ready {
            self.pdf.programmatic_scroll = false;
            self.pdf.toc_target_page = None;
            let start = target_page.saturating_sub(2);
            let end = (target_page + 2).min(self.pdf.total_pages.saturating_sub(1));
            self.update_pdf_page_cache();
            tasks.push(self.render_pdf_page_range(start, end));
        } else {
            tasks.push(operation::scroll_to(
                iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                AbsoluteOffset {
                    x: 0.0,
                    y: scroll_y,
                },
            ));
        }
        Task::batch(tasks)
    }

    pub(crate) fn restore_scroll_positions(&self) -> Task<Message> {
        let mut tasks = Vec::new();
        // Restore editor scroll position after search bar toggle
        let editor_y = self.editor.scroll_y;
        tasks.push(operation::scroll_to(
            iced::advanced::widget::Id::new(EDITOR_SCROLLABLE_ID),
            AbsoluteOffset {
                x: 0.0,
                y: editor_y,
            },
        ));
        // Restore PDF scroll position after search bar toggle
        if self.pdf.active_path.is_some() {
            let pdf_y = self.pdf.scroll_y;
            tasks.push(operation::scroll_to(
                iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                AbsoluteOffset { x: 0.0, y: pdf_y },
            ));
        }
        Task::batch(tasks)
    }

    pub(crate) fn pdf_selection_quote_link_command(&self) -> Option<EditorCommand> {
        let sel = self.pdf.selection.as_ref()?;
        let page_text = self.pdf.page_text.get(&sel.page_index)?;
        let pdf_path = self.pdf.active_path.as_ref()?;
        let start = sel.anchor_idx.min(sel.focus_idx);
        let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
        let selected = text_by_char_range(&page_text.text, start, end);
        if selected.trim().is_empty() {
            return None;
        }

        let link = build_pdf_link(pdf_path, Some(sel.page_index + 1), None);
        Some(EditorCommand::InsertPdfQuoteLink {
            selected_text: selected,
            page_number: sel.page_index + 1,
            link,
        })
    }

    pub(crate) fn pdf_annotation_link_command(&self, annotation_id: &str) -> Option<EditorCommand> {
        let (_, ann) = self.find_pdf_annotation(annotation_id)?;
        let pdf_path = self.pdf.active_path.as_ref()?;
        let page_number = ann.page_index + 1;
        let link = build_pdf_link(pdf_path, Some(page_number), Some(&ann.id));
        Some(EditorCommand::InsertPdfAnnotationLink {
            selected_text: ann.selected_text,
            page_number,
            link,
        })
    }

    pub(crate) fn submit_first_citation_palette_item(&mut self) -> Task<Message> {
        if !self.overlays.citation_palette_visible {
            return Task::none();
        }
        let Some(item) = self.citation_palette_items().into_iter().next() else {
            self.overlays.toast = Some("No citation matches".to_string());
            return Task::none();
        };
        self.choose_citation_item(item)
    }

    pub(crate) fn choose_citation_item(
        &mut self,
        item: crate::messages::CitationItem,
    ) -> Task<Message> {
        if self.workspace.active_path.is_none() {
            self.overlays.toast =
                Some("Open a markdown file before inserting a citation".to_string());
            return Task::none();
        }
        self.overlays.close_citation_palette();
        if self.overlays.excerpt_mode_active {
            self.overlays.excerpts_queue.push(item);
            self.overlays.toast = Some("Citation queued to excerpts".to_string());
            return Task::none();
        }
        match item {
            crate::messages::CitationItem::Selection { .. } => {
                Task::done(Message::Pdf(PdfMessage::InsertQuoteLink))
            }
            crate::messages::CitationItem::Annotation { id, .. } => {
                Task::done(Message::Pdf(PdfMessage::InsertAnnotationLink(id)))
            }
            crate::messages::CitationItem::SearchHit {
                path,
                page_index,
                snippet,
            } => {
                let link = crate::features::pdf::navigation::build_pdf_link(
                    &path,
                    Some(page_index + 1),
                    None,
                );
                let command = crate::editor::buffer::EditorCommand::InsertPdfQuoteLink {
                    selected_text: snippet,
                    page_number: page_index + 1,
                    link,
                };
                self.set_active_panel(ActivePanel::Markdown);
                self.run_editor_command(command)
            }
        }
    }

    pub(crate) fn center_editor_line(&self, line: usize) -> Task<Message> {
        let y = self.estimated_editor_line_y(line);
        let viewport_height = self.estimated_editor_viewport_height();
        // Always center the matched line in the viewport
        let target_y = (y - viewport_height / 2.0 + 18.0).max(0.0);

        Task::perform(async move { target_y }, |target_y| {
            Message::Editor(EditorMessage::ScrollToTarget(target_y))
        })
    }

    pub(crate) fn ensure_editor_line_visible(&self, line: usize) -> Task<Message> {
        let y = self.estimated_editor_line_y(line);
        let viewport_height = self.estimated_editor_viewport_height();
        let current_scroll = self.editor.scroll_y;
        let margin = 40.0;

        if y < current_scroll + margin {
            let target_y = (y - margin).max(0.0);
            Task::perform(async move { target_y }, |target_y| {
                Message::Editor(EditorMessage::ScrollToTarget(target_y))
            })
        } else if y > current_scroll + viewport_height - margin - 24.0 {
            let target_y = (y - viewport_height + margin + 24.0).max(0.0);
            Task::perform(async move { target_y }, |target_y| {
                Message::Editor(EditorMessage::ScrollToTarget(target_y))
            })
        } else {
            Task::none()
        }
    }

    pub(crate) fn replace_all_in_current_document(
        &mut self,
    ) -> Result<(usize, Task<Message>), String> {
        if self.workspace.active_path.is_none() {
            return Err("Open a markdown file before replacing text".to_string());
        }
        if self.search.editor.query.is_empty() {
            return Err("Search query is empty".to_string());
        }

        let text = self.editor.buffer.text();
        let (_, count) = if self.search.editor.regex {
            let re = regex::RegexBuilder::new(&self.search.editor.query)
                .case_insensitive(!self.search.editor.match_case)
                .build()
                .map_err(|err| format!("Invalid regex: {err}"))?;
            let count = re.find_iter(&text).count();
            (
                re.replace_all(&text, self.search.editor.replace.as_str())
                    .to_string(),
                count,
            )
        } else if self.search.editor.match_case {
            let count = text.match_indices(&self.search.editor.query).count();
            (
                text.replace(&self.search.editor.query, &self.search.editor.replace),
                count,
            )
        } else {
            let re = regex::RegexBuilder::new(&regex::escape(&self.search.editor.query))
                .case_insensitive(true)
                .build()
                .map_err(|err| err.to_string())?;
            let count = re.find_iter(&text).count();
            (
                re.replace_all(&text, self.search.editor.replace.as_str())
                    .to_string(),
                count,
            )
        };

        if count > 0 {
            self.editor.buffer.execute(EditorCommand::ReplaceAll {
                query: self.search.editor.query.clone(),
                replacement: self.search.editor.replace.clone(),
                regex: self.search.editor.regex,
                match_case: self.search.editor.match_case,
            });
            let task = self.highlight_all();
            return Ok((count, task));
        }
        Ok((count, Task::none()))
    }

    pub(crate) fn replace_current_match(&mut self) -> Result<Task<Message>, String> {
        if self.workspace.active_path.is_none() {
            return Err("Open a markdown file before replacing text".to_string());
        }
        if self.search.editor.query.is_empty() {
            return Err("Search query is empty".to_string());
        }
        let Some(active_idx) = self.search.editor.active_index else {
            return Err("No active search match selected".to_string());
        };
        let matches = self.current_document_matches();
        let Some(m) = matches.get(active_idx) else {
            return Err("Active search match is invalid".to_string());
        };

        let replace_text = self.search.editor.replace.clone();

        self.editor.buffer.execute(EditorCommand::SetSelection {
            anchor_line: m.line,
            anchor_col: m.start_col,
            focus_line: m.line,
            focus_col: m.end_col,
        });

        let result = self
            .editor
            .buffer
            .execute(EditorCommand::InsertText(replace_text));

        let highlight_task = if result.projection_changed {
            self.highlight_all()
        } else {
            Task::none()
        };

        self.search.editor.wrap_status = None;

        let new_matches = self.current_document_matches();
        if new_matches.is_empty() {
            self.search.editor.active_index = None;
            return Ok(highlight_task);
        }

        let (cursor_line, cursor_col) = (
            self.editor.buffer.cursor_line,
            self.editor.buffer.cursor_col,
        );
        let mut next_idx = 0;
        for (i, nm) in new_matches.iter().enumerate() {
            if nm.line > cursor_line || (nm.line == cursor_line && nm.start_col >= cursor_col) {
                next_idx = i;
                break;
            }
        }

        self.search.editor.active_index = Some(next_idx);
        let next_match = new_matches[next_idx];

        self.editor.buffer.execute(EditorCommand::SetSelection {
            anchor_line: next_match.line,
            anchor_col: next_match.start_col,
            focus_line: next_match.line,
            focus_col: next_match.end_col,
        });

        let center_task = self.center_editor_line(next_match.line);

        Ok(Task::batch(vec![highlight_task, center_task]))
    }

    pub(crate) fn cancel_global_pdf_search(&mut self) {
        if let Some(renderer) = self.state.pdf_renderer() {
            let _ = renderer.cancel_search(self.search.pdf_active_id);
        }
        self.search.pdf_active_id = self.search.pdf_active_id.wrapping_add(1);
        self.pdf.view.search.searching = false;
        self.search.global.pdf_search_id = None;
        self.search.global.pending_pdf = false;
        self.search.global.pending_vault_pdf = false;
        self.search.global.pending_db = false;
        self.search.global.pdf_status = None;
        self.update_global_search_searching();
    }

    pub(crate) fn run_editor_command(&mut self, command: EditorCommand) -> Task<Message> {
        let keep_cursor_visible = command.should_keep_cursor_visible();
        self.run_editor_command_with_scroll(command, keep_cursor_visible)
    }

    pub(crate) fn handle_editor_block_action(
        &mut self,
        line_idx: usize,
        action: EditorBlockActionKind,
    ) -> Task<Message> {
        match action {
            EditorBlockActionKind::ConvertToH1 => {
                self.run_editor_command(EditorCommand::ConvertToH1 { line: line_idx })
            }
            EditorBlockActionKind::ConvertToH2 => {
                self.run_editor_command(EditorCommand::ConvertToH2 { line: line_idx })
            }
            EditorBlockActionKind::ConvertToH3 => {
                self.run_editor_command(EditorCommand::ConvertToH3 { line: line_idx })
            }
            EditorBlockActionKind::ConvertToParagraph => {
                self.run_editor_command(EditorCommand::ConvertToParagraph { line: line_idx })
            }
            EditorBlockActionKind::ToggleCheckbox => {
                self.run_editor_command(EditorCommand::ToggleCheckbox { line: line_idx })
            }
            EditorBlockActionKind::RemoveCheckbox => {
                self.run_editor_command(EditorCommand::RemoveCheckbox { line: line_idx })
            }
            EditorBlockActionKind::InsertRowAbove => {
                self.run_editor_command(EditorCommand::InsertRowAbove { line: line_idx })
            }
            EditorBlockActionKind::InsertRowBelow => {
                self.run_editor_command(EditorCommand::InsertRowBelow { line: line_idx })
            }
            EditorBlockActionKind::DeleteRow => {
                self.run_editor_command(EditorCommand::DeleteRow { line: line_idx })
            }
            EditorBlockActionKind::InsertColumnLeft => {
                self.run_editor_command(EditorCommand::InsertColumnLeft { line: line_idx })
            }
            EditorBlockActionKind::InsertColumnRight => {
                self.run_editor_command(EditorCommand::InsertColumnRight { line: line_idx })
            }
            EditorBlockActionKind::DeleteColumn => {
                self.run_editor_command(EditorCommand::DeleteColumn { line: line_idx })
            }
            EditorBlockActionKind::CopyCode => {
                let mut code_text = String::new();
                let mut line = line_idx + 1;
                while line < self.editor.buffer.line_count() {
                    let text = self.editor.buffer.line_text(line);
                    if text.trim_start().starts_with("```") {
                        break;
                    }
                    code_text.push_str(&text);
                    line += 1;
                }
                iced::clipboard::write(code_text)
            }
            EditorBlockActionKind::SetCodeLanguage(lang) => {
                self.run_editor_command(EditorCommand::SetCodeLanguage {
                    line: line_idx,
                    language: lang,
                })
            }
            EditorBlockActionKind::ConvertQuoteToParagraph => {
                self.run_editor_command(EditorCommand::ConvertQuoteToParagraph { line: line_idx })
            }
            EditorBlockActionKind::OpenPdfCitation => {
                let line_text = self.editor.buffer.line_text(line_idx);
                if let Some(start_idx) = line_text.find("pdf://") {
                    let rest = &line_text[start_idx..];
                    let end_idx = rest.find(')').unwrap_or(rest.len());
                    let link = rest[..end_idx].to_string();
                    Task::done(Message::Workspace(WorkspaceMessage::FileClicked(link)))
                } else {
                    Task::none()
                }
            }
        }
    }

    pub(crate) fn handle_editor_link_action(
        &mut self,
        line_idx: usize,
        start_col: usize,
        end_col: usize,
        link_target: String,
        action: crate::messages::EditorLinkActionKind,
    ) -> Task<Message> {
        use crate::messages::EditorLinkActionKind;
        match action {
            EditorLinkActionKind::OpenLink => Task::done(Message::Workspace(
                WorkspaceMessage::FileClicked(link_target),
            )),
            EditorLinkActionKind::CopyLinkTarget => {
                self.overlays.toast = Some("Copied".to_string());
                iced::clipboard::write::<Message>(link_target)
            }
            EditorLinkActionKind::CreateNote => {
                // Derive filename from link target stem.
                let stem = std::path::Path::new(&link_target)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| link_target.clone());
                let filename = if stem.to_lowercase().ends_with(".md")
                    || stem.to_lowercase().ends_with(".markdown")
                {
                    stem
                } else {
                    format!("{}.md", stem)
                };
                // Create the note adjacent to the current file.
                let new_path = if let Some(ref active) = self.workspace.active_path {
                    if let Some(parent) = std::path::Path::new(active).parent() {
                        let parent_s = parent.to_string_lossy();
                        if parent_s.is_empty() {
                            filename
                        } else {
                            format!("{}/{}", parent_s.trim_end_matches('/'), filename)
                        }
                    } else {
                        filename
                    }
                } else {
                    filename
                };
                match md_editor_core::vault::create_file(&self.state, &new_path) {
                    Ok(()) => {
                        self.workspace.vault_entries =
                            md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                        self.overlays.toast = Some(format!("Created {}", new_path));
                        Task::done(Message::Workspace(WorkspaceMessage::FileClicked(new_path)))
                    }
                    Err(e) => {
                        self.overlays.toast = Some(e);
                        Task::none()
                    }
                }
            }
            EditorLinkActionKind::RepairLink(suggested_path) => {
                // Replace the link target text in the buffer.
                // Build replacement: keep display text from original span if any.
                let replacement = suggested_path.clone();
                let task = self.run_editor_command(EditorCommand::ReplaceTextRange {
                    line: line_idx,
                    start_col,
                    end_col,
                    replacement,
                });
                self.overlays.toast = Some(format!("Link repaired → {}", suggested_path));
                task
            }
        }
    }

    pub(crate) fn run_editor_command_with_scroll(
        &mut self,
        command: EditorCommand,
        keep_cursor_visible: bool,
    ) -> Task<Message> {
        let result = self.editor.buffer.execute(command);
        if result.text_changed {
            self.editor.pending_save = Some(std::time::Instant::now());
        }
        let content_task = if result.projection_changed {
            self.highlight_all()
        } else {
            Task::none()
        };

        if keep_cursor_visible {
            Task::batch(vec![
                content_task,
                self.ensure_editor_line_visible(self.editor.buffer.cursor_line),
            ])
        } else {
            content_task
        }
    }
}

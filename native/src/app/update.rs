use iced::Task;
use iced::widget::operation::{self, AbsoluteOffset};

use crate::features::editor::EditorEffect;
use crate::features::pdf::view_model::PdfLayout;
use image::GenericImageView;

use crate::editor::buffer::{DocBuffer, EditorCommand};
use crate::features::pdf::annotations::{normalize_note_path, note_filename_from_path};
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
use crate::views::pdf_viewer::{PDF_PAGE_LIST_PADDING, PDF_PAGE_SPACING};
use std::collections::HashSet;

use super::model::*;
use crate::app::*;

impl MdEditor {
    pub(crate) fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            message @ Message::Shell(_) => self.coordinate(message),
            message @ Message::Workspace(_) => self.coordinate(message),
            message @ Message::Editor(_) => self.coordinate(message),
            message @ Message::Pdf(_) => self.coordinate(message),
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
            Message::Pdf(PdfMessage::LinkNoteFolderSelected(folder)) => {
                if matches!(
                    self.overlays.active_modal,
                    Some(views::modals::ModalType::LinkNote(_))
                ) {
                    let filename = note_filename_from_path(&self.overlays.modal_input);
                    self.overlays.modal_input = if folder.is_empty() {
                        filename
                    } else {
                        format!("{}/{}", folder.trim_end_matches('/'), filename)
                    };
                }
                Task::none()
            }
            Message::Pdf(PdfMessage::LinkNoteFileSelected(path)) => {
                if matches!(
                    self.overlays.active_modal,
                    Some(views::modals::ModalType::LinkNote(_))
                ) {
                    self.overlays.modal_input = normalize_note_path(&path);
                }
                Task::none()
            }
            Message::Pdf(PdfMessage::LinkNotePickerSearchChanged(query)) => {
                if matches!(
                    self.overlays.active_modal,
                    Some(views::modals::ModalType::LinkNote(_))
                ) {
                    self.overlays.link_note_picker_search = query;
                }
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
                    return Task::done(Message::PdfAddQuickNote(id, input));
                }
                if let Some(views::modals::ModalType::LinkNote(id)) =
                    self.overlays.active_modal.clone()
                {
                    self.overlays.close_modal();
                    return Task::done(Message::PdfLinkNote(id, input));
                }
                if let Some(views::modals::ModalType::AnnotationTags(id)) =
                    self.overlays.active_modal.clone()
                {
                    self.overlays.close_modal();
                    return Task::done(Message::PdfUpdateAnnotationTags(id, input));
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

            Message::Pdf(PdfMessage::Loaded(generation, pages)) => {
                if generation != self.pdf.render_generation {
                    return Task::none();
                }
                self.pdf.total_pages = pages;
                self.pdf.pages = vec![None; pages as usize];
                self.pdf.dimensions = vec![None; pages as usize];
                if self.pdf.view.page_sizes.len() != pages as usize {
                    self.pdf.view.page_sizes = vec![None; pages as usize];
                }
                self.pdf.view.layout = PdfLayout::rebuild(
                    &self.pdf.view.page_sizes,
                    self.pdf.view.zoom,
                    self.pdf_placeholder_display_size(),
                    PDF_PAGE_SPACING,
                    PDF_PAGE_LIST_PADDING,
                    self.pdf.rotation,
                );
                self.pdf.pending_pages.clear();
                self.pdf.stale_pages.clear();
                self.pdf.pending_links.clear();
                self.pdf.programmatic_scroll = false;
                self.pdf.toc_target_page = None;

                // Eagerly generate page-level TOC entries so the panel isn't
                // blank even if the bookmark extraction hasn't finished yet.
                let page_entries: Vec<views::toc::TocEntry> = (0..pages)
                    .map(|p| views::toc::TocEntry {
                        level: 1,
                        text: format!("Page {}", p + 1),
                        line: p as usize,
                    })
                    .collect();
                if self.pdf.toc_entries_flat.is_none() {
                    self.pdf.toc_entries_flat = Some(page_entries);
                }

                if pages == 0 {
                    self.overlays.toast = Some(
                        "PDF renderer is unavailable or the PDF could not be opened".to_string(),
                    );
                }
                if self.pdf.fit_to_width
                    && self
                        .pdf
                        .view
                        .page_sizes
                        .iter()
                        .take(pages as usize)
                        .any(Option::is_some)
                {
                    Task::done(Message::PdfFitToWidth)
                } else if self.pdf.fit_to_page
                    && self
                        .pdf
                        .view
                        .page_sizes
                        .iter()
                        .take(pages as usize)
                        .any(Option::is_some)
                {
                    Task::done(Message::PdfFitToPage)
                } else if self.pdf.fit_to_width || self.pdf.fit_to_page {
                    Task::none()
                } else {
                    self.render_all_pdf_pages()
                }
            }
            Message::Pdf(PdfMessage::ZoomChanged(zoom)) => {
                let current_page = self.pdf_page_at_scroll(self.pdf.scroll_y);
                let page_start_offset = self.pdf_page_offset(current_page);
                let relative_ratio = if self.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                    0.0
                } else {
                    let page_height_old = self.pdf_page_height(current_page);
                    if page_height_old > 0.0 {
                        ((self.pdf.scroll_y - page_start_offset).max(0.0)) / page_height_old
                    } else {
                        0.0
                    }
                };

                self.pdf.fit_to_width = false;
                self.pdf.fit_to_page = false;
                self.pdf.view.zoom = zoom.clamp(0.5, 4.0);
                self.pdf.stale_pages = self
                    .pdf
                    .pages
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, page)| page.as_ref().map(|_| idx as u16))
                    .collect();
                self.pdf.placeholder_page_size = self.first_pdf_page_size();
                self.pdf.pending_pages.clear();
                self.pdf.pending_links.clear();
                self.pdf.toc_target_page = Some(current_page);
                self.pdf.programmatic_scroll = true;
                self.pdf.render_generation = self.pdf.render_generation.wrapping_add(1);

                self.pdf.view.layout = PdfLayout::rebuild(
                    &self.pdf.view.page_sizes,
                    self.pdf.view.zoom,
                    self.pdf_placeholder_display_size(),
                    PDF_PAGE_SPACING,
                    PDF_PAGE_LIST_PADDING,
                    self.pdf.rotation,
                );
                self.update_pdf_page_cache();

                let new_scroll_y = if self.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                    self.pdf.scroll_y
                } else {
                    self.pdf_page_offset(current_page)
                        + relative_ratio * self.pdf_page_height(current_page)
                };
                self.pdf.scroll_y = new_scroll_y;

                Task::batch(vec![
                    self.render_visible_pdf_pages(),
                    operation::scroll_to(
                        iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                        AbsoluteOffset {
                            x: 0.0,
                            y: new_scroll_y,
                        },
                    ),
                ])
            }
            Message::Pdf(PdfMessage::WheelScrolledForZoom(delta)) => {
                if self.pdf.active_path.is_some()
                    && self.pdf.showing_pdf
                    && (self.shell.keyboard_modifiers.control()
                        || self.shell.keyboard_modifiers.command())
                    && delta.abs() > f32::EPSILON
                {
                    let next_zoom = (self.pdf.view.zoom + delta).clamp(0.5, 4.0);
                    if (next_zoom - self.pdf.view.zoom).abs() > f32::EPSILON {
                        Task::done(Message::PdfZoomChanged(next_zoom))
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
            Message::Pdf(PdfMessage::FitToWidth) => {
                let is_initial = self.pdf.initial_target_page.is_some();
                let current_page = if let Some(target_page) = self.pdf.initial_target_page.take() {
                    target_page.min(self.pdf.total_pages.saturating_sub(1))
                } else {
                    self.pdf_page_at_scroll(self.pdf.scroll_y)
                };
                let page_start_offset = self.pdf_page_offset(current_page);
                let relative_ratio = if is_initial {
                    0.0
                } else if self.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                    0.0
                } else {
                    let page_height_old = self.pdf_page_height(current_page);
                    if page_height_old > 0.0 {
                        ((self.pdf.scroll_y - page_start_offset).max(0.0)) / page_height_old
                    } else {
                        0.0
                    }
                };

                self.pdf.fit_to_width = true;
                self.pdf.fit_to_page = false;
                let available_width = self.pdf_available_width();
                let page_width = self
                    .pdf
                    .view
                    .page_sizes
                    .iter()
                    .flatten()
                    .next()
                    .map(|(w, _)| (*w).max(1.0))
                    .or_else(|| {
                        self.pdf
                            .dimensions
                            .iter()
                            .flatten()
                            .next()
                            .map(|(w, _)| (*w as f32 / self.pdf.view.zoom).max(1.0))
                    })
                    .unwrap_or(612.0);
                let next_zoom = ((available_width - 48.0).max(240.0) / page_width).clamp(0.5, 4.0);
                self.pdf.view.zoom = ((next_zoom * 100.0).round() / 100.0).clamp(0.5, 4.0);
                self.pdf.stale_pages = self
                    .pdf
                    .pages
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, page)| page.as_ref().map(|_| idx as u16))
                    .collect();
                self.pdf.placeholder_page_size = self.first_pdf_page_size();
                self.pdf.pending_pages.clear();
                self.pdf.pending_links.clear();
                self.pdf.toc_target_page = Some(current_page);
                self.pdf.programmatic_scroll = true;
                self.pdf.render_generation = self.pdf.render_generation.wrapping_add(1);

                self.pdf.view.layout = PdfLayout::rebuild(
                    &self.pdf.view.page_sizes,
                    self.pdf.view.zoom,
                    self.pdf_placeholder_display_size(),
                    PDF_PAGE_SPACING,
                    PDF_PAGE_LIST_PADDING,
                    self.pdf.rotation,
                );
                self.update_pdf_page_cache();

                let new_scroll_y = if is_initial {
                    self.pdf_page_offset(current_page)
                } else if self.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                    self.pdf.scroll_y
                } else {
                    self.pdf_page_offset(current_page)
                        + relative_ratio * self.pdf_page_height(current_page)
                };
                self.pdf.scroll_y = new_scroll_y;
                if is_initial {
                    self.pdf.current_page = current_page;
                }

                Task::batch(vec![
                    self.render_visible_pdf_pages(),
                    operation::scroll_to(
                        iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                        AbsoluteOffset {
                            x: 0.0,
                            y: new_scroll_y,
                        },
                    ),
                ])
            }
            Message::Pdf(PdfMessage::FitToPage) => {
                let is_initial = self.pdf.initial_target_page.is_some();
                let current_page = if let Some(target_page) = self.pdf.initial_target_page.take() {
                    target_page.min(self.pdf.total_pages.saturating_sub(1))
                } else {
                    self.pdf_page_at_scroll(self.pdf.scroll_y)
                };
                let page_start_offset = self.pdf_page_offset(current_page);
                let relative_ratio = if is_initial {
                    0.0
                } else if self.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                    0.0
                } else {
                    let page_height_old = self.pdf_page_height(current_page);
                    if page_height_old > 0.0 {
                        ((self.pdf.scroll_y - page_start_offset).max(0.0)) / page_height_old
                    } else {
                        0.0
                    }
                };

                self.pdf.fit_to_page = true;
                self.pdf.fit_to_width = false;
                let available_width = self.pdf_available_width();
                let viewport_height = if self.pdf.viewport_height > 0.0 {
                    self.pdf.viewport_height
                } else {
                    self.estimated_editor_viewport_height()
                };

                let (page_width, page_height) = self
                    .pdf
                    .view
                    .page_sizes
                    .iter()
                    .flatten()
                    .next()
                    .map(|(w, h)| ((*w).max(1.0), (*h).max(1.0)))
                    .or_else(|| {
                        self.pdf.dimensions.iter().flatten().next().map(|(w, h)| {
                            (
                                (*w as f32 / self.pdf.view.zoom).max(1.0),
                                (*h as f32 / self.pdf.view.zoom).max(1.0),
                            )
                        })
                    })
                    .unwrap_or((612.0, 792.0));

                let w_zoom = (available_width - 48.0).max(240.0) / page_width;
                let h_zoom = (viewport_height - 40.0).max(200.0) / page_height;
                let next_zoom = w_zoom.min(h_zoom).clamp(0.5, 4.0);
                self.pdf.view.zoom = ((next_zoom * 100.0).round() / 100.0).clamp(0.5, 4.0);
                self.pdf.stale_pages = self
                    .pdf
                    .pages
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, page)| page.as_ref().map(|_| idx as u16))
                    .collect();
                self.pdf.placeholder_page_size = self.first_pdf_page_size();
                self.pdf.pending_pages.clear();
                self.pdf.pending_links.clear();
                self.pdf.toc_target_page = Some(current_page);
                self.pdf.programmatic_scroll = true;
                self.pdf.render_generation = self.pdf.render_generation.wrapping_add(1);

                self.pdf.view.layout = PdfLayout::rebuild(
                    &self.pdf.view.page_sizes,
                    self.pdf.view.zoom,
                    self.pdf_placeholder_display_size(),
                    PDF_PAGE_SPACING,
                    PDF_PAGE_LIST_PADDING,
                    self.pdf.rotation,
                );
                self.update_pdf_page_cache();

                let new_scroll_y = if is_initial {
                    self.pdf_page_offset(current_page)
                } else if self.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                    self.pdf.scroll_y
                } else {
                    self.pdf_page_offset(current_page)
                        + relative_ratio * self.pdf_page_height(current_page)
                };
                self.pdf.scroll_y = new_scroll_y;
                if is_initial {
                    self.pdf.current_page = current_page;
                }

                Task::batch(vec![
                    self.render_visible_pdf_pages(),
                    operation::scroll_to(
                        iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                        AbsoluteOffset {
                            x: 0.0,
                            y: new_scroll_y,
                        },
                    ),
                ])
            }
            Message::Pdf(PdfMessage::RotateClockwise) => {
                if self.pdf.active_path.is_some() && self.pdf.showing_pdf {
                    self.pdf.rotation = (self.pdf.rotation + 90) % 360;
                    self.pdf.view.page_cache.clear();
                    self.pdf.pages.fill(None);
                    self.pdf.dimensions.fill(None);
                    self.pdf.stale_pages.clear();
                    self.pdf.pending_pages.clear();
                    self.pdf.pending_links.clear();
                    self.pdf.view.layout = PdfLayout::rebuild(
                        &self.pdf.view.page_sizes,
                        self.pdf.view.zoom,
                        self.pdf_placeholder_display_size(),
                        PDF_PAGE_SPACING,
                        PDF_PAGE_LIST_PADDING,
                        self.pdf.rotation,
                    );
                    self.pdf.render_generation = self.pdf.render_generation.wrapping_add(1);

                    if self.pdf.fit_to_width {
                        Task::done(Message::PdfFitToWidth)
                    } else if self.pdf.fit_to_page {
                        Task::done(Message::PdfFitToPage)
                    } else {
                        self.render_visible_pdf_pages()
                    }
                } else {
                    Task::none()
                }
            }
            Message::Pdf(PdfMessage::PageSizesLoaded(generation, path, sizes)) => {
                if generation != self.pdf.render_generation
                    || self.pdf.active_path.as_deref() != Some(path.as_str())
                {
                    return Task::none();
                }
                self.pdf.view.page_sizes = sizes.into_iter().map(Some).collect();
                if self.pdf.view.page_sizes.len() < self.pdf.total_pages as usize {
                    self.pdf
                        .view
                        .page_sizes
                        .resize(self.pdf.total_pages as usize, None);
                }
                if self.pdf.placeholder_page_size.is_none() {
                    self.pdf.placeholder_page_size = self.first_pdf_page_size();
                }
                self.pdf.view.layout = PdfLayout::rebuild(
                    &self.pdf.view.page_sizes,
                    self.pdf.view.zoom,
                    self.pdf_placeholder_display_size(),
                    PDF_PAGE_SPACING,
                    PDF_PAGE_LIST_PADDING,
                    self.pdf.rotation,
                );
                if self.pdf.fit_to_width && self.pdf.total_pages > 0 {
                    Task::done(Message::PdfFitToWidth)
                } else if self.pdf.fit_to_page && self.pdf.total_pages > 0 {
                    Task::done(Message::PdfFitToPage)
                } else if let Some(page) = self.pdf.initial_target_page.take() {
                    self.navigate_pdf_page(page)
                } else {
                    Task::none()
                }
            }
            Message::Pdf(PdfMessage::Rendered(generation, page, img)) => {
                self.pdf.pending_pages.remove(&page);
                if generation != self.pdf.render_generation {
                    return Task::none();
                }
                let img = match self.pdf.rotation {
                    90 => img.rotate90(),
                    180 => img.rotate180(),
                    270 => img.rotate270(),
                    _ => img,
                };
                let (width, height) = img.dimensions();
                let handle = iced::widget::image::Handle::from_rgba(
                    width,
                    height,
                    img.into_rgba8().into_raw(),
                );
                let logical_width = (width as f32 / PDF_RENDER_SUPERSAMPLE).round() as u32;
                let logical_height = (height as f32 / PDF_RENDER_SUPERSAMPLE).round() as u32;
                if (page as usize) < self.pdf.pages.len() {
                    self.pdf.pages[page as usize] = Some(handle.clone());
                    self.pdf.dimensions[page as usize] = Some((logical_width, logical_height));
                    self.pdf.stale_pages.remove(&page);

                    // Insert into the LRU cache for bounded memory.
                    let byte_size = width as usize * height as usize * 4; // RGBA
                    self.pdf.view.page_cache.insert(
                        page,
                        handle,
                        (logical_width, logical_height),
                        byte_size,
                    );
                    self.sync_pdf_pages_to_cache();
                }
                if self.pdf.placeholder_page_size.is_none() || page == 0 {
                    self.pdf.placeholder_page_size = Some((
                        logical_width as f32 / self.pdf.view.zoom,
                        logical_height as f32 / self.pdf.view.zoom,
                    ));
                }
                let mut tasks = vec![self.load_pdf_page_links(page)];
                if !self.pdf.page_text.contains_key(&page) && !self.pdf.pending_text.contains(&page)
                {
                    tasks.push(self.load_pdf_page_text(page));
                }
                if self.pdf.toc_target_page == Some(page) {
                    let scroll_y = self.pdf_page_offset(page);
                    let current_scroll_y = self.pdf.scroll_y;
                    if (current_scroll_y - scroll_y).abs() < 5.0 {
                        self.pdf.toc_target_page = None;
                        self.pdf.programmatic_scroll = false;
                        self.pdf.current_page = page.min(self.pdf.total_pages.saturating_sub(1));
                        let start = page.saturating_sub(2);
                        let end = (page + 2).min(self.pdf.total_pages.saturating_sub(1));
                        self.update_pdf_page_cache();
                        tasks.push(self.render_pdf_page_range(start, end));
                    } else {
                        self.pdf.programmatic_scroll = true;
                        tasks.push(operation::scroll_to(
                            iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                            AbsoluteOffset {
                                x: 0.0,
                                y: scroll_y,
                            },
                        ));
                    }
                }
                Task::batch(tasks)
            }
            Message::Pdf(PdfMessage::RenderFailed(generation, page)) => {
                self.pdf.pending_pages.remove(&page);
                if generation != self.pdf.render_generation {
                    return Task::none();
                }
                if self.pdf.toc_target_page == Some(page) {
                    self.pdf.toc_target_page = None;
                    self.pdf.programmatic_scroll = false;
                }
                self.overlays.toast = Some(format!("Could not render PDF page {}", page + 1));
                Task::none()
            }
            Message::Pdf(PdfMessage::RenderSkipped(generation, page)) => {
                self.pdf.pending_pages.remove(&page);
                if generation != self.pdf.render_generation {
                    return Task::none();
                }
                if self.pdf.toc_target_page == Some(page) {
                    self.pdf.toc_target_page = None;
                    self.pdf.programmatic_scroll = false;
                }
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
            Message::Pdf(PdfMessage::TocClicked(index)) => {
                if self.pdf.active_path.is_some() {
                    if self.pdf.showing_pdf && self.pdf.active_path.is_some() {
                        self.push_pdf_navigation_history();
                    } else if self.workspace.active_path.is_some() {
                        self.push_markdown_navigation_history();
                    }
                    let target_page = index
                        .min(self.pdf.total_pages.saturating_sub(1) as usize)
                        .max(0) as u16;
                    self.set_active_panel(ActivePanel::Pdf);
                    self.navigate_pdf_page(target_page)
                } else {
                    Task::none()
                }
            }
            Message::Pdf(PdfMessage::Scrolled { y, viewport_height }) => {
                if (self.shell.keyboard_modifiers.control()
                    || self.shell.keyboard_modifiers.command())
                    && !self.pdf.programmatic_scroll
                {
                    return operation::scroll_to(
                        iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                        AbsoluteOffset {
                            x: 0.0,
                            y: self.pdf.scroll_y,
                        },
                    );
                }
                self.pdf.viewport_height = viewport_height;
                self.set_active_panel(ActivePanel::Pdf);
                self.pdf.scroll_y = y;
                let new_page = self.pdf_page_at_scroll(y + viewport_height * 0.33);

                let target_page_ready = if let Some(target_page) = self.pdf.toc_target_page {
                    self.pdf
                        .pages
                        .get(target_page as usize)
                        .is_some_and(|page| page.is_some())
                } else {
                    false
                };

                if self.pdf.programmatic_scroll {
                    if let Some(target_page) = self.pdf.toc_target_page {
                        let target_y = self.pdf_page_offset(target_page);
                        let max_scroll_y = (self.pdf_total_height() - viewport_height).max(0.0);
                        let expected_y = target_y.min(max_scroll_y);
                        if ((y - expected_y).abs() < 5.0 || new_page == target_page)
                            && target_page_ready
                        {
                            self.pdf.programmatic_scroll = false;
                        }
                    } else {
                        self.pdf.programmatic_scroll = false;
                    }
                } else {
                    self.pdf.toc_target_page = None;
                }

                if let Some(target_page) = self.pdf.toc_target_page {
                    let target_y = self.pdf_page_offset(target_page);
                    let max_scroll_y = (self.pdf_total_height() - viewport_height).max(0.0);
                    let expected_y = target_y.min(max_scroll_y);
                    if ((y - expected_y).abs() < 5.0 || new_page == target_page)
                        && target_page_ready
                    {
                        // Arrived! Clear programmatic scroll flags and render.
                        self.pdf.toc_target_page = None;
                        self.pdf.programmatic_scroll = false;
                        self.pdf.current_page =
                            target_page.min(self.pdf.total_pages.saturating_sub(1));
                        let start = self.pdf.current_page.saturating_sub(2);
                        let end =
                            (self.pdf.current_page + 2).min(self.pdf.total_pages.saturating_sub(1));
                        self.update_pdf_page_cache();
                        return self.render_pdf_page_range(start, end);
                    } else {
                        // Still scrolling programmatically to target. Skip rendering intermediate pages.
                        self.update_pdf_page_cache();
                        return Task::none();
                    }
                }

                if new_page != self.pdf.current_page && new_page < self.pdf.total_pages {
                    if new_page.abs_diff(self.pdf.current_page) > 8 {
                        self.pdf.pending_pages.clear();
                        self.pdf.pending_links.clear();
                    }
                    self.pdf.current_page = new_page;
                    let task = self.render_pdf_pages_for_viewport(y, viewport_height);
                    self.update_pdf_page_cache();
                    task
                } else {
                    let task = self.render_pdf_pages_for_viewport(y, viewport_height);
                    self.update_pdf_page_cache();
                    task
                }
            }
            Message::Pdf(PdfMessage::LeftClicked(page_idx, x, y, modifiers)) => {
                self.set_active_panel(ActivePanel::Pdf);
                if let Some(link) = self.pdf_link_at(page_idx, x, y) {
                    if let Some(dest_page) = link.dest_page {
                        self.push_pdf_navigation_history();
                        self.pdf.current_page =
                            dest_page.min(u32::from(self.pdf.total_pages.saturating_sub(1))) as u16;
                        self.navigate_pdf_page(self.pdf.current_page)
                    } else if let Some(uri) = link.uri {
                        if modifiers.control() || modifiers.command() {
                            #[cfg(target_os = "windows")]
                            let _ = std::process::Command::new("cmd")
                                .args(["/C", "start", "", &uri])
                                .spawn();
                            #[cfg(target_os = "macos")]
                            let _ = std::process::Command::new("open").arg(&uri).spawn();
                            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                            let _ = std::process::Command::new("xdg-open").arg(&uri).spawn();
                            self.overlays.toast = Some(format!("Opening: {}", uri));
                        } else {
                            self.overlays.toast =
                                Some(format!("External link (Ctrl+click to open): {}", uri));
                        }
                        Task::none()
                    } else {
                        Task::none()
                    }
                } else if let Some(ann) = self.annotation_at(page_idx, x, y) {
                    self.pdf.focused_annotation_id = Some(ann.id.clone());
                    if let Some(ref path) = ann.linked_note_path {
                        if !path.is_empty() {
                            Task::done(Message::PdfOpenLinkedNote(path.clone()))
                        } else {
                            Task::none()
                        }
                    } else {
                        Task::none()
                    }
                } else {
                    self.pdf.focused_annotation_id = None;
                    Task::none()
                }
            }
            Message::Pdf(PdfMessage::RightClicked {
                page_index,
                x,
                y,
                absolute_pos,
            }) => {
                self.set_active_panel(ActivePanel::Pdf);
                let mut items = Vec::new();

                // 1. Text selection context
                if self.pdf_selection_contains_point(page_index, x, y) {
                    items.push(views::modals::PdfContextMenuItem::Copy);
                    items.push(views::modals::PdfContextMenuItem::CopyAsQuote);
                    items.push(views::modals::PdfContextMenuItem::CopyWithSourceLink);
                    items.push(views::modals::PdfContextMenuItem::HighlightYellow);
                    items.push(views::modals::PdfContextMenuItem::HighlightGreen);
                    items.push(views::modals::PdfContextMenuItem::HighlightBlue);
                    items.push(views::modals::PdfContextMenuItem::HighlightPink);
                    items.push(views::modals::PdfContextMenuItem::HighlightOrange);
                    items.push(views::modals::PdfContextMenuItem::UnderlineBlue);
                    items.push(views::modals::PdfContextMenuItem::StrikeRed);
                    items.push(views::modals::PdfContextMenuItem::SearchSelectedText);
                    if self.workspace.active_path.is_some() {
                        items.push(views::modals::PdfContextMenuItem::InsertQuoteLink);
                    }
                }

                // 2. Annotation context
                let mut target_ann = None;
                if x < 0.0 || y < 0.0 {
                    if let Some(ref ann_id) = self.pdf.focused_annotation_id {
                        for page_anns in self.pdf.annotations.values() {
                            if let Some(ann) = page_anns.iter().find(|a| a.id == *ann_id) {
                                target_ann = Some(ann.clone());
                                break;
                            }
                        }
                    }
                } else {
                    target_ann = self.annotation_at(page_index, x, y);
                }

                if let Some(ann) = target_ann {
                    items.extend(views::modals::pdf_annotation_context_menu_items(
                        &ann,
                        self.workspace.active_path.is_some(),
                    ));
                }

                // 3. Link context and preview task
                let mut preview_task = Task::none();
                if x >= 0.0 && y >= 0.0 {
                    if let Some(link) = self.pdf_link_at(page_index, x, y) {
                        items.push(views::modals::PdfContextMenuItem::OpenLink(link.clone()));
                        if let Some(ref uri) = link.uri {
                            items.push(views::modals::PdfContextMenuItem::CopyLink(uri.clone()));
                        }

                        if let Some(dest_page) = link.dest_page {
                            let dest_y = link.dest_y;
                            if let Some(ref path) = self.pdf.active_path {
                                if let Some(abs_path) = self.resolve_active_path(path) {
                                    let abs_path = abs_path.to_string_lossy().to_string();
                                    let _state = self.state.clone();
                                    preview_task = Task::perform(
                                        async move {
                                            let renderer = _state.pdf_renderer()?;
                                            renderer
                                                .render_link_preview(
                                                    &abs_path,
                                                    dest_page.into(),
                                                    dest_y,
                                                )
                                                .ok()
                                        },
                                        |res| {
                                            Message::PdfLinkPreviewResult(
                                                res.ok_or_else(|| "Failed to preview".into()),
                                            )
                                        },
                                    );
                                }
                            }
                        }
                    }
                }

                if !items.is_empty() {
                    self.overlays.active_modal = Some(views::modals::ModalType::PdfContextMenu(
                        views::modals::PdfContextMenuState {
                            absolute_pos,
                            items,
                        },
                    ));
                }

                preview_task
            }
            Message::Pdf(PdfMessage::ContextMenuAction(action)) => match action {
                views::modals::PdfContextMenuItem::Copy => {
                    if let Some(sel) = &self.pdf.selection {
                        if let Some(page_text) = self.pdf.page_text.get(&sel.page_index) {
                            let start = sel.anchor_idx.min(sel.focus_idx);
                            let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
                            let selected = text_by_char_range(&page_text.text, start, end);
                            if !selected.is_empty() {
                                self.overlays.active_modal = None;
                                return iced::clipboard::write(selected);
                            }
                        }
                    }
                    Task::none()
                }
                views::modals::PdfContextMenuItem::CopyAsQuote => {
                    if let Some(sel) = &self.pdf.selection {
                        if let Some(page_text) = self.pdf.page_text.get(&sel.page_index) {
                            let start = sel.anchor_idx.min(sel.focus_idx);
                            let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
                            let selected = text_by_char_range(&page_text.text, start, end);
                            if !selected.is_empty() {
                                let quote = selected
                                    .lines()
                                    .map(|l| format!("> {}", l))
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                self.overlays.active_modal = None;
                                return iced::clipboard::write(quote);
                            }
                        }
                    }
                    Task::none()
                }
                views::modals::PdfContextMenuItem::CopyWithSourceLink => {
                    if let Some(command) = self.pdf_selection_quote_link_command() {
                        let EditorCommand::InsertPdfQuoteLink {
                            selected_text,
                            page_number: _,
                            link,
                        } = command
                        else {
                            return Task::none();
                        };
                        let markdown = format!("{selected_text}\n[label]({link})");
                        self.overlays.active_modal = None;
                        return iced::clipboard::write(markdown);
                    }
                    Task::none()
                }
                views::modals::PdfContextMenuItem::HighlightYellow
                | views::modals::PdfContextMenuItem::HighlightGreen
                | views::modals::PdfContextMenuItem::HighlightBlue
                | views::modals::PdfContextMenuItem::HighlightPink
                | views::modals::PdfContextMenuItem::HighlightOrange => {
                    let color = match action {
                        views::modals::PdfContextMenuItem::HighlightYellow => {
                            md_editor_core::domain::pdf::PdfAnnotationColor::Yellow
                        }
                        views::modals::PdfContextMenuItem::HighlightGreen => {
                            md_editor_core::domain::pdf::PdfAnnotationColor::Green
                        }
                        views::modals::PdfContextMenuItem::HighlightBlue => {
                            md_editor_core::domain::pdf::PdfAnnotationColor::Blue
                        }
                        views::modals::PdfContextMenuItem::HighlightPink => {
                            md_editor_core::domain::pdf::PdfAnnotationColor::Pink
                        }
                        _ => md_editor_core::domain::pdf::PdfAnnotationColor::Orange,
                    };
                    self.overlays.active_modal = None;
                    Task::done(Message::PdfCreateHighlight(color))
                }
                views::modals::PdfContextMenuItem::UnderlineBlue => {
                    self.overlays.active_modal = None;
                    Task::done(Message::PdfCreateAnnotation(
                        md_editor_core::domain::pdf::PdfAnnotationKind::Underline,
                        md_editor_core::domain::pdf::PdfAnnotationColor::Blue,
                    ))
                }
                views::modals::PdfContextMenuItem::StrikeRed => {
                    self.overlays.active_modal = None;
                    Task::done(Message::PdfCreateAnnotation(
                        md_editor_core::domain::pdf::PdfAnnotationKind::Strike,
                        md_editor_core::domain::pdf::PdfAnnotationColor::Red,
                    ))
                }
                views::modals::PdfContextMenuItem::SearchSelectedText => {
                    if let Some(sel) = &self.pdf.selection {
                        if let Some(page_text) = self.pdf.page_text.get(&sel.page_index) {
                            let start = sel.anchor_idx.min(sel.focus_idx);
                            let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
                            let selected = text_by_char_range(&page_text.text, start, end);
                            if !selected.trim().is_empty() {
                                self.pdf.view.search.query = selected.trim().to_string();
                                self.pdf.selection = None;
                                self.overlays.active_modal = None;
                                self.pdf.view.search.visible = true;
                                self.search.visible = false;
                                return Task::batch(vec![
                                    self.search_pdf(),
                                    focus_pdf_search_input(),
                                    self.restore_scroll_positions(),
                                ]);
                            }
                        }
                    }
                    Task::none()
                }
                views::modals::PdfContextMenuItem::InsertQuoteLink => {
                    self.overlays.active_modal = None;
                    Task::done(Message::PdfInsertQuoteLink)
                }
                views::modals::PdfContextMenuItem::InsertAnnotationLink { id, page: _ } => {
                    self.overlays.active_modal = None;
                    Task::done(Message::PdfInsertAnnotationLink(id))
                }
                views::modals::PdfContextMenuItem::EditNote { id, page } => {
                    self.overlays.active_modal = None;
                    Task::done(Message::PdfEditAnnotationNote(id, page))
                }
                views::modals::PdfContextMenuItem::LinkToNote { id, page: _ } => {
                    self.overlays.active_modal = None;
                    Task::done(Message::PdfLinkNote(id, String::new()))
                }
                views::modals::PdfContextMenuItem::OpenLinkedNote(path) => {
                    self.overlays.active_modal = None;
                    Task::done(Message::PdfOpenLinkedNote(path))
                }
                views::modals::PdfContextMenuItem::DeleteHighlight(id) => {
                    self.overlays.active_modal = None;
                    Task::done(Message::PdfDeleteHighlight(id))
                }
                views::modals::PdfContextMenuItem::OpenLink(link) => {
                    self.overlays.active_modal = None;
                    if let Some(dest_page) = link.dest_page {
                        self.push_pdf_navigation_history();
                        self.pdf.current_page =
                            dest_page.min(u32::from(self.pdf.total_pages.saturating_sub(1))) as u16;
                        self.navigate_pdf_page(self.pdf.current_page)
                    } else if let Some(uri) = link.uri {
                        #[cfg(target_os = "windows")]
                        let _ = std::process::Command::new("cmd")
                            .args(["/C", "start", "", &uri])
                            .spawn();
                        #[cfg(target_os = "macos")]
                        let _ = std::process::Command::new("open").arg(&uri).spawn();
                        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                        let _ = std::process::Command::new("xdg-open").arg(&uri).spawn();
                        self.overlays.toast = Some(format!("Opening: {}", uri));
                        Task::none()
                    } else {
                        Task::none()
                    }
                }
                views::modals::PdfContextMenuItem::CopyLink(uri) => {
                    self.overlays.active_modal = None;
                    iced::clipboard::write(uri)
                }
            },
            Message::Pdf(PdfMessage::LinkPreviewResult(Ok(res))) => {
                if let Ok(img) = image::load_from_memory(&res.image_data) {
                    let (width, height) = img.dimensions();
                    self.pdf.link_preview = Some(iced::widget::image::Handle::from_rgba(
                        width,
                        height,
                        img.into_rgba8().into_raw(),
                    ));
                }
                Task::none()
            }
            Message::Pdf(PdfMessage::LinkPreviewResult(Err(e))) => {
                self.overlays.toast = Some(format!("Preview Error: {}", e));
                Task::none()
            }
            Message::Pdf(PdfMessage::CloseLinkPreview) => {
                self.pdf.link_preview = None;
                self.overlays.active_modal = None;
                Task::none()
            }
            Message::Pdf(PdfMessage::TocLoaded(generation, entries)) => {
                if generation != self.pdf.render_generation {
                    return Task::none();
                }
                pub(crate) fn flatten_pdf_toc(
                    entries: &[md_editor_core::application::pdf_service::TocEntry],
                    level: u8,
                    out: &mut Vec<views::toc::TocEntry>,
                ) {
                    for entry in entries {
                        if let Some(page_index) = entry.page_index {
                            out.push(views::toc::TocEntry {
                                level,
                                text: entry.title.clone(),
                                line: page_index as usize,
                            });
                        }
                        flatten_pdf_toc(&entry.children, (level + 1).min(6), out);
                    }
                }

                // Check if the PDF has any bookmarks at all
                let has_bookmarks = !entries.is_empty();
                let mut mapped = Vec::new();
                flatten_pdf_toc(&entries, 1, &mut mapped);

                // If the PDF has no bookmarks, fill with page entries (the eager
                // fallback in PdfLoaded already did this, but we may have had a
                // zero-page race earlier).
                if !has_bookmarks {
                    let current = self.pdf.toc_entries_flat.get_or_insert_with(Vec::new);
                    if current.is_empty() {
                        for p in 0..self.pdf.total_pages {
                            current.push(views::toc::TocEntry {
                                level: 1,
                                text: format!("Page {}", p + 1),
                                line: p as usize,
                            });
                        }
                    }
                } else if !mapped.is_empty() {
                    // PDF has bookmarks — replace page entries with real TOC.
                    self.pdf.toc_entries_flat = Some(mapped);
                }
                // else: PDF has bookmark structure but no valid page refs; keep
                // the eager page entries as fallback.
                Task::none()
            }
            Message::Pdf(PdfMessage::PageLinksLoaded(generation, page, links)) => {
                self.pdf.pending_links.remove(&page);
                if generation != self.pdf.render_generation {
                    return Task::none();
                }
                self.pdf.page_links.insert(page, links);
                Task::none()
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

            Message::Pdf(PdfMessage::SearchMatchesFound(search_id, matches)) => {
                if search_id == self.search.pdf_active_id {
                    if self.search.visible && self.search.global.pdf_search_id == Some(search_id) {
                        if let Some(pdf_path) = &self.pdf.active_path {
                            let query_lower = self.search.editor.query.to_lowercase();
                            let query_trimmed = self.search.editor.query.trim();

                            let is_linked =
                                |p1: &str, p2: &str| self.state.vault_paths_are_linked(p1, p2);

                            let match_index_base = self.pdf.view.search.matches.len();
                            for (match_offset, m) in matches.iter().enumerate() {
                                let mut score = 4.0;
                                score *= 1.5;
                                if m.context.to_lowercase().contains(&query_lower) {
                                    if m.context.trim().to_lowercase()
                                        == query_trimmed.to_lowercase()
                                    {
                                        score *= 2.0;
                                    }
                                }
                                if let Some(ref active) = self.workspace.active_path {
                                    if is_linked(pdf_path, active) {
                                        score *= 1.3;
                                    }
                                }

                                self.search.global.results.push(
                                    md_editor_core::types::UnifiedSearchResult {
                                        group: md_editor_core::types::SearchResultGroup::PdfContent,
                                        path: pdf_path.clone(),
                                        line: (m.page_index + 1) as usize,
                                        context: format!(
                                            "PDF text ({} areas): {}",
                                            m.rects.len(),
                                            md_editor_core::vault::search_result_preview(
                                                &m.context,
                                                query_trimmed,
                                                None,
                                            )
                                        ),
                                        score,
                                        page_index: Some(m.page_index),
                                        annotation_id: Some(
                                            (match_index_base + match_offset).to_string(),
                                        ),
                                    },
                                );
                            }

                            self.search.global.results.sort_by(|a, b| {
                                b.score
                                    .partial_cmp(&a.score)
                                    .unwrap_or(std::cmp::Ordering::Equal)
                                    .then_with(|| a.group.cmp(&b.group))
                                    .then_with(|| a.path.cmp(&b.path))
                                    .then_with(|| a.line.cmp(&b.line))
                            });
                        }
                    }

                    self.pdf.view.search.matches.extend(matches);
                    self.rebuild_pdf_search_page_index();
                    if self.pdf.view.search.active_index.is_none()
                        && !self.pdf.view.search.matches.is_empty()
                        && !self.search.visible
                    {
                        self.pdf.view.search.active_index = Some(0);
                        self.navigate_pdf_search_to_index(0)
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
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
            Message::Pdf(PdfMessage::SearchFinished(search_id, result)) => {
                if search_id == self.search.pdf_active_id {
                    self.pdf.view.search.searching = false;
                    if self.search.global.pdf_search_id == Some(search_id) {
                        self.search.global.pending_pdf = false;
                        self.search.global.pdf_search_id = None;
                        self.update_global_search_searching();
                    }
                    match result {
                        Ok(()) => Task::none(),
                        Err(err) => {
                            self.search.pdf_error = Some(err);
                            self.pdf.view.search.matches.clear();
                            self.pdf.view.search.page_index.clear();
                            Task::none()
                        }
                    }
                } else {
                    Task::none()
                }
            }
            Message::Pdf(PdfMessage::SearchResultClicked(page)) => {
                self.search.visible = false;
                self.pdf.view.search.visible = true;
                self.set_active_panel(ActivePanel::Pdf);
                self.pdf.view.search.active_index = self
                    .pdf
                    .view
                    .search
                    .matches
                    .iter()
                    .position(|result| result.page_index == page);
                if let Some(index) = self.pdf.view.search.active_index {
                    self.navigate_pdf_search_to_index(index)
                } else {
                    self.pdf.current_page = page.min(self.pdf.total_pages.saturating_sub(1));
                    self.navigate_pdf_page(self.pdf.current_page)
                }
            }
            Message::Pdf(PdfMessage::ScrollBy(delta)) => {
                if self.pdf.active_path.is_none()
                    || (!self.pdf.showing_pdf
                        && !(self.shell.split_view_active && self.workspace.active_path.is_some()))
                    || (self.shell.split_view_active
                        && self.workspace.active_path.is_some()
                        && self.shell.active_panel != ActivePanel::Pdf)
                    || self.search.visible
                    || self.search.editor.visible
                    || self.pdf.view.search.visible
                    || self.overlays.active_modal.is_some()
                    || self.overlays.command_palette_visible
                {
                    return Task::none();
                }
                let max_y = self.pdf_total_height().max(0.0);
                let y = (self.pdf.scroll_y + delta).clamp(0.0, max_y);
                operation::scroll_to(
                    iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                    AbsoluteOffset { x: 0.0, y },
                )
            }
            Message::Pdf(PdfMessage::FirstPage) => {
                if self.pdf.showing_pdf && self.pdf.total_pages > 0 {
                    self.navigate_pdf_page(0)
                } else {
                    Task::none()
                }
            }
            Message::Pdf(PdfMessage::LastPage) => {
                if self.pdf.showing_pdf && self.pdf.total_pages > 0 {
                    self.navigate_pdf_page(self.pdf.total_pages.saturating_sub(1))
                } else {
                    Task::none()
                }
            }
            Message::Pdf(PdfMessage::NavBack) => {
                let current_target = if self.pdf.showing_pdf && self.pdf.active_path.is_some() {
                    Some(NavigationTarget::Pdf {
                        path: self.pdf.active_path.clone().unwrap(),
                        page: self.pdf.current_page,
                        scroll_offset: self.pdf.scroll_y,
                        zoom: self.pdf.view.zoom,
                    })
                } else {
                    self.workspace
                        .active_path
                        .as_ref()
                        .map(|path| NavigationTarget::Markdown {
                            path: path.clone(),
                            line: self.editor.buffer.cursor_line,
                            column: self.editor.buffer.cursor_col,
                        })
                };

                if let Some(target) = current_target {
                    if !self.workspace.navigation_history.entries.is_empty() {
                        if self.workspace.navigation_history.current_index
                            == self.workspace.navigation_history.entries.len() - 1
                            && self.workspace.navigation_history.entries
                                [self.workspace.navigation_history.current_index]
                                .target
                                != target
                        {
                            self.workspace.navigation_history.push(target);
                        }
                    }
                }

                if let Some(target) = self.workspace.navigation_history.go_back() {
                    self.navigate_to_target(target)
                } else {
                    Task::none()
                }
            }
            Message::Pdf(PdfMessage::NavForward) => {
                if let Some(target) = self.workspace.navigation_history.go_forward() {
                    self.navigate_to_target(target)
                } else {
                    Task::none()
                }
            }
            Message::Pdf(PdfMessage::SearchToggle) => {
                if self.pdf.showing_pdf {
                    if self.pdf.view.search.visible {
                        self.pdf.view.search.visible = false;
                        self.pdf.view.search.matches.clear();
                        self.pdf.view.search.page_index.clear();
                    } else {
                        self.pdf.view.search.visible = true;
                        self.search.visible = false;
                    }
                    Task::none()
                } else {
                    Task::none()
                }
            }
            Message::Pdf(PdfMessage::GoToPage) => {
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
            Message::Pdf(PdfMessage::DocumentIdComputed(Some((path, hash, len, mtime)))) => {
                let _ = self.state.save_pdf_document(&hash, &path, len, mtime);
                self.pdf.document_id = Some(hash.clone());

                let annotations = self
                    .state
                    .get_pdf_annotations(&hash, None)
                    .unwrap_or_default();
                self.pdf.annotations.clear();
                for ann in annotations {
                    self.pdf
                        .annotations
                        .entry(ann.page_index)
                        .or_default()
                        .push(ann);
                }

                let mut target_page = None;
                if let Some(ref target_id) = self.pdf.initial_target_annotation {
                    for (page_idx, page_anns) in &self.pdf.annotations {
                        if page_anns.iter().any(|a| &a.id == target_id) {
                            target_page = Some(*page_idx);
                            self.pdf.focused_annotation_id = Some(target_id.clone());
                            break;
                        }
                    }
                }

                let scroll_task = if self.pdf.total_pages > 0 {
                    if let Some(page) = target_page {
                        self.pdf.initial_target_page = None;
                        self.pdf.initial_target_annotation = None;
                        self.navigate_pdf_page(page)
                    } else if let Some(page) = self.pdf.initial_target_page {
                        self.pdf.initial_target_page = None;
                        self.navigate_pdf_page(page)
                    } else {
                        Task::none()
                    }
                } else {
                    if let Some(page) = target_page {
                        self.pdf.initial_target_page = Some(page);
                        self.pdf.initial_target_annotation = None;
                    }
                    Task::none()
                };

                scroll_task
            }
            Message::Pdf(PdfMessage::DocumentIdComputed(None)) => Task::none(),
            Message::Pdf(PdfMessage::PageTextLoaded(generation, page, res)) => {
                self.pdf.pending_text.remove(&page);
                if generation == self.pdf.render_generation {
                    if let Ok(page_text) = res {
                        if let Some(ref path) = self.pdf.active_path {
                            let _ = self.state.save_pdf_page_text(path, page, &page_text.text);
                        }
                        self.pdf.page_text.insert(page, page_text);
                        self.pdf.text_lru.push_back(page);
                        if self.pdf.text_lru.len() > PDF_TEXT_PAGE_CACHE_LIMIT {
                            if let Some(oldest) = self.pdf.text_lru.pop_front() {
                                self.pdf.page_text.remove(&oldest);
                            }
                        }
                    }
                }
                Task::none()
            }
            Message::Pdf(PdfMessage::SelectionChanged(page, anchor, focus)) => {
                self.set_active_panel(ActivePanel::Pdf);
                self.pdf.selection = Some(views::interactive_pdf::PdfSelection {
                    page_index: page,
                    anchor_idx: anchor,
                    focus_idx: focus,
                });
                Task::none()
            }
            Message::Pdf(PdfMessage::SelectionCleared) => {
                self.pdf.selection = None;
                Task::none()
            }
            Message::Pdf(PdfMessage::SelectionFinished(page, anchor, focus)) => {
                self.set_active_panel(ActivePanel::Pdf);
                self.pdf.selection = Some(views::interactive_pdf::PdfSelection {
                    page_index: page,
                    anchor_idx: anchor,
                    focus_idx: focus,
                });
                Task::none()
            }
            Message::Pdf(PdfMessage::CopySelection) => {
                if !self.pdf_copy_shortcut_is_active() {
                    return Task::none();
                }
                if let Some(sel) = &self.pdf.selection {
                    if let Some(page_text) = self.pdf.page_text.get(&sel.page_index) {
                        let start = sel.anchor_idx.min(sel.focus_idx);
                        let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
                        let selected = text_by_char_range(&page_text.text, start, end);
                        if !selected.is_empty() {
                            return iced::clipboard::write(selected);
                        }
                    }
                }
                Task::none()
            }
            Message::Pdf(PdfMessage::InsertQuoteLink) => {
                if self.workspace.active_path.is_none() {
                    self.overlays.toast =
                        Some("Open a markdown file before inserting a quote link".to_string());
                    return Task::none();
                }
                if self.overlays.excerpt_mode_active {
                    if let Some(sel) = &self.pdf.selection {
                        if let Some(page_text) = self.pdf.page_text.get(&sel.page_index) {
                            let start = sel.anchor_idx.min(sel.focus_idx);
                            let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
                            let selected = text_by_char_range(&page_text.text, start, end);
                            if !selected.trim().is_empty() {
                                self.overlays.excerpts_queue.push(
                                    crate::messages::CitationItem::Selection {
                                        text: selected,
                                        page_index: sel.page_index,
                                    },
                                );
                                self.overlays.toast = Some("Quote queued to excerpts".to_string());
                            }
                        }
                    }
                    return Task::none();
                }
                let Some(command) = self.pdf_selection_quote_link_command() else {
                    self.overlays.toast =
                        Some("Select PDF text before inserting a quote link".to_string());
                    return Task::none();
                };
                self.set_active_panel(ActivePanel::Markdown);
                self.run_editor_command(command)
            }
            Message::Pdf(PdfMessage::InsertAnnotationLink(annotation_id)) => {
                if self.workspace.active_path.is_none() {
                    self.overlays.toast =
                        Some("Open a markdown file before inserting a highlight".to_string());
                    return Task::none();
                }
                if self.overlays.excerpt_mode_active {
                    if let Some((_, ann)) = self.find_pdf_annotation(&annotation_id) {
                        self.overlays.excerpts_queue.push(
                            crate::messages::CitationItem::Annotation {
                                id: ann.id.clone(),
                                text: ann.selected_text.clone(),
                                page_index: ann.page_index,
                            },
                        );
                        self.overlays.toast = Some("Annotation queued to excerpts".to_string());
                    }
                    return Task::none();
                }
                let Some(command) = self.pdf_annotation_link_command(&annotation_id) else {
                    self.overlays.toast =
                        Some("Select a PDF highlight before inserting it".to_string());
                    return Task::none();
                };
                self.set_active_panel(ActivePanel::Markdown);
                self.run_editor_command(command)
            }
            Message::Pdf(PdfMessage::CreateHighlight(color)) => {
                Task::done(Message::PdfCreateAnnotation(
                    md_editor_core::domain::pdf::PdfAnnotationKind::Highlight,
                    color,
                ))
            }
            Message::Pdf(PdfMessage::CreateAnnotation(kind, color)) => {
                if let (Some(sel), Some(doc_id)) = (&self.pdf.selection, &self.pdf.document_id) {
                    if let Some(page_text) = self.pdf.page_text.get(&sel.page_index) {
                        let start = sel.anchor_idx.min(sel.focus_idx);
                        let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);

                        let mut selected_chars = Vec::new();
                        for c in &page_text.chars {
                            if c.text_index >= start && c.text_index < end {
                                selected_chars.push(c.clone());
                            }
                        }

                        let selected_text = text_by_char_range(&page_text.text, start, end);

                        let rects = md_editor_core::domain::pdf::merge_char_rects(&selected_chars);

                        let id = uuid::Uuid::new_v4().to_string();
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64;

                        let ann = md_editor_core::domain::pdf::PdfAnnotation {
                            id: id.clone(),
                            document_id: doc_id.clone(),
                            page_index: sel.page_index,
                            kind,
                            color,
                            selected_text,
                            ranges: vec![md_editor_core::domain::pdf::PdfTextRange {
                                start_text_index: start,
                                end_text_index: end,
                            }],
                            rects,
                            note: None,
                            linked_note_path: None,
                            markdown_anchor: None,
                            tags: Vec::new(),
                            status: md_editor_core::domain::pdf::PdfAnnotationStatus::Unresolved,
                            created_at: now,
                            updated_at: now,
                        };

                        if let Err(e) = self.state.save_pdf_annotation(&ann) {
                            self.overlays.toast = Some(format!("Failed to save annotation: {}", e));
                        } else {
                            self.pdf
                                .annotations
                                .entry(sel.page_index)
                                .or_default()
                                .push(ann);
                            self.pdf.selection = None;
                            if let Some(path) = &self.pdf.active_path {
                                self.workspace.backlinks =
                                    md_editor_core::vault::get_mixed_backlinks(&self.state, path)
                                        .unwrap_or_default();
                            }
                        }
                    }
                }
                Task::none()
            }
            Message::Pdf(PdfMessage::DeleteHighlight(id)) => {
                if let Err(e) = self.state.delete_pdf_annotation(&id) {
                    self.overlays.toast = Some(format!("Failed to delete highlight: {}", e));
                } else {
                    for page_anns in self.pdf.annotations.values_mut() {
                        page_anns.retain(|a| a.id != id);
                    }
                    if self.pdf.focused_annotation_id.as_ref() == Some(&id) {
                        self.pdf.focused_annotation_id = None;
                    }
                    if let Some(views::modals::ModalType::QuickNote(ref mid)) =
                        self.overlays.active_modal
                    {
                        if mid == &id {
                            self.overlays.active_modal = None;
                            self.overlays.modal_input.clear();
                        }
                    }
                    if let Some(path) = &self.pdf.active_path {
                        self.workspace.backlinks =
                            md_editor_core::vault::get_mixed_backlinks(&self.state, path)
                                .unwrap_or_default();
                    }
                }
                Task::none()
            }
            Message::Pdf(PdfMessage::AddQuickNote(id, note_content)) => {
                let mut found_ann = None;
                for page_anns in self.pdf.annotations.values_mut() {
                    if let Some(ann) = page_anns.iter_mut().find(|a| a.id == id) {
                        let trimmed_note = note_content.trim().to_string();
                        ann.note = if trimmed_note.is_empty() {
                            None
                        } else {
                            Some(trimmed_note)
                        };
                        ann.updated_at = std::time::SystemTime::now()
                            .duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64;
                        found_ann = Some(ann.clone());
                        break;
                    }
                }
                let mut task = Task::none();
                if let Some(ann) = found_ann {
                    if let Err(e) = self.state.save_pdf_annotation(&ann) {
                        self.overlays.toast = Some(format!("Failed to save note: {}", e));
                    } else {
                        if let Some(path) = &self.pdf.active_path {
                            self.workspace.backlinks =
                                md_editor_core::vault::get_mixed_backlinks(&self.state, path)
                                    .unwrap_or_default();

                            if let Some(note_path) = ann.linked_note_path.as_deref() {
                                if let Ok(bytes) =
                                    md_editor_core::vault::open_file(&self.state, note_path)
                                {
                                    if let Ok(existing_content) = String::from_utf8(bytes) {
                                        let updated_content =
                                            crate::features::pdf::annotations::sync_annotation_note_in_markdown(
                                                &existing_content,
                                                path,
                                                &ann,
                                            );
                                        if updated_content != existing_content {
                                            if let Err(e) = save_markdown_file_with_parser_targets(
                                                &self.state,
                                                note_path,
                                                &updated_content,
                                            ) {
                                                self.overlays.toast = Some(format!(
                                                    "Failed to sync linked note: {}",
                                                    e
                                                ));
                                            } else if self.workspace.active_path.as_deref()
                                                == Some(note_path)
                                            {
                                                self.editor.buffer =
                                                    DocBuffer::from_text(&updated_content);
                                                let _ = reindex_markdown_file_with_parser_targets(
                                                    &self.state,
                                                    note_path,
                                                    &updated_content,
                                                );
                                                task = self.highlight_all();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                task
            }
            Message::Pdf(PdfMessage::LinkNote(annotation_id, mut note_path)) => {
                let mut annotation = None;
                for page_anns in self.pdf.annotations.values() {
                    if let Some(ann) = page_anns.iter().find(|a| a.id == annotation_id) {
                        annotation = Some(ann.clone());
                        break;
                    }
                }
                if let Some(mut ann) = annotation {
                    if note_path.is_empty() {
                        self.overlays.modal_input = self.default_pdf_note_path(&ann);
                        self.overlays.link_note_picker_search.clear();
                        self.overlays.active_modal =
                            Some(views::modals::ModalType::LinkNote(annotation_id));
                        return Task::none();
                    }

                    note_path = normalize_note_path(&note_path);
                    if let Some(pdf_path) = &self.pdf.active_path {
                        let content = self.linked_pdf_note_file_content(&note_path, pdf_path, &ann);

                        if let Err(e) = save_markdown_file_with_parser_targets(
                            &self.state,
                            &note_path,
                            &content,
                        ) {
                            self.overlays.toast =
                                Some(format!("Failed to create linked note: {}", e));
                            return Task::none();
                        }
                    } else {
                        return Task::none();
                    }

                    ann.linked_note_path = Some(note_path.clone());
                    ann.updated_at = std::time::SystemTime::now()
                        .duration_since(std::time::SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;

                    if let Err(e) = self.state.save_pdf_annotation(&ann) {
                        self.overlays.toast = Some(format!("Failed to link note: {}", e));
                    } else {
                        for page_anns in self.pdf.annotations.values_mut() {
                            if let Some(a) = page_anns.iter_mut().find(|a| a.id == annotation_id) {
                                a.linked_note_path = Some(note_path.clone());
                                a.updated_at = ann.updated_at;
                                break;
                            }
                        }
                        self.workspace.vault_entries =
                            md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                        if let Some(pdf_path) = &self.pdf.active_path {
                            let _ = md_editor_core::config::set_sys_config(
                                &self.state,
                                &pdf_companion_note_key(pdf_path),
                                &note_path,
                            );
                        }
                        self.overlays.toast = Some(format!("Linked note: {}", note_path));
                        return Task::done(Message::PdfOpenLinkedNote(note_path));
                    }
                }
                Task::none()
            }
            Message::Pdf(PdfMessage::OpenLinkedNote(note_path)) => {
                self.shell.split_view_active = true;
                let open_task = self.open_file_extended(&note_path, false);
                if self.pdf.fit_to_width {
                    Task::batch(vec![open_task, Task::done(Message::PdfFitToWidth)])
                } else {
                    Task::batch(vec![open_task, self.restore_scroll_positions()])
                }
            }
            Message::Pdf(PdfMessage::AnnotationFocused {
                document_path,
                annotation_id,
                page,
            }) => {
                let resolved_pdf_path = resolve_relative_link_path(
                    self.workspace.vault_root.as_deref(),
                    self.workspace.active_path.as_deref(),
                    &document_path,
                );

                self.shell.split_view_active = true;
                self.pdf.showing_pdf = true;

                if self.pdf.active_path.as_deref() == Some(&resolved_pdf_path) {
                    self.pdf.focused_annotation_id = Some(annotation_id);
                    self.navigate_pdf_page(page.saturating_sub(1))
                } else {
                    self.pdf.initial_target_page = Some(page.saturating_sub(1));
                    self.pdf.initial_target_annotation = Some(annotation_id);
                    self.open_pdf(&resolved_pdf_path)
                }
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
                    Shortcut::NavBack => Task::done(Message::PdfNavBack),
                    Shortcut::NavForward => Task::done(Message::PdfNavForward),
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
                            Task::done(Message::PdfZoomChanged(new_zoom))
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::ZoomOut => {
                        if self.pdf.active_path.is_some() && self.pdf.showing_pdf {
                            let new_zoom = (self.pdf.view.zoom - 0.1).max(0.5);
                            Task::done(Message::PdfZoomChanged(new_zoom))
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::ZoomFit => {
                        if self.pdf.active_path.is_some() && self.pdf.showing_pdf {
                            Task::done(Message::PdfFitToWidth)
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
                            Task::done(Message::PdfSearchToggle)
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::PdfHighlight => {
                        if self.pdf.showing_pdf {
                            if self.pdf.selection.is_some() {
                                let color = md_editor_core::domain::pdf::PdfAnnotationColor::Yellow;
                                Task::done(Message::PdfCreateHighlight(color))
                            } else {
                                self.overlays.toast =
                                    Some("Select PDF text before highlighting".to_string());
                                Task::none()
                            }
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::InsertPdfQuote => Task::done(Message::PdfInsertQuoteLink),
                    Shortcut::InsertPdfHighlight => {
                        if let Some(annotation_id) = self.pdf.focused_annotation_id.clone() {
                            Task::done(Message::PdfInsertAnnotationLink(annotation_id))
                        } else {
                            self.overlays.toast =
                                Some("Select a PDF highlight before inserting it".to_string());
                            Task::none()
                        }
                    }
                    Shortcut::PdfFirstPage => {
                        if self.pdf.showing_pdf {
                            Task::done(Message::PdfFirstPage)
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::PdfLastPage => {
                        if self.pdf.showing_pdf {
                            Task::done(Message::PdfLastPage)
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
                        return Task::done(Message::PdfFitToWidth);
                    } else if self.pdf.fit_to_page {
                        return Task::done(Message::PdfFitToPage);
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
                    return Task::done(Message::PdfFitToWidth);
                } else if self.pdf.fit_to_page && self.pdf.active_path.is_some() {
                    self.persist_shell_state();
                    return Task::done(Message::PdfFitToPage);
                }
                self.persist_shell_state();
                Task::none()
            }
            Message::Shell(ShellMessage::WindowResized(width, height)) => {
                self.shell.window_width = width;
                self.shell.window_height = height;
                if self.pdf.fit_to_width && self.pdf.active_path.is_some() {
                    return Task::done(Message::PdfFitToWidth);
                } else if self.pdf.fit_to_page && self.pdf.active_path.is_some() {
                    return Task::done(Message::PdfFitToPage);
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
            Message::Pdf(PdfMessage::ToggleAnnotationsSidebar) => {
                if self.pdf.active_path.is_some() {
                    self.shell.pdf_annotations_visible = !self.shell.pdf_annotations_visible;
                    self.persist_shell_state();
                }
                Task::none()
            }
            Message::Pdf(PdfMessage::FilterAnnotationsByColor(color)) => {
                self.pdf.annotations_filter_color = color;
                Task::none()
            }
            Message::Pdf(PdfMessage::FilterAnnotationsByPage(page)) => {
                self.pdf.annotations_filter_page = page;
                Task::none()
            }
            Message::Pdf(PdfMessage::FilterAnnotationsByTag(tag)) => {
                self.pdf.annotations_filter_tag = tag;
                Task::none()
            }
            Message::Pdf(PdfMessage::FilterAnnotationsByLinked(linked)) => {
                self.pdf.annotations_filter_linked = linked;
                Task::none()
            }
            Message::Pdf(PdfMessage::FilterAnnotationsByUnresolved(unresolved)) => {
                self.pdf.annotations_filter_unresolved = unresolved;
                Task::none()
            }
            Message::Pdf(PdfMessage::ToggleAnnotationStatus(id)) => {
                let mut found_ann = None;
                for page_anns in self.pdf.annotations.values_mut() {
                    if let Some(ann) = page_anns.iter_mut().find(|a| a.id == id) {
                        ann.status = match ann.status {
                            md_editor_core::domain::pdf::PdfAnnotationStatus::Unresolved => {
                                md_editor_core::domain::pdf::PdfAnnotationStatus::Resolved
                            }
                            md_editor_core::domain::pdf::PdfAnnotationStatus::Resolved => {
                                md_editor_core::domain::pdf::PdfAnnotationStatus::Unresolved
                            }
                        };
                        ann.updated_at = std::time::SystemTime::now()
                            .duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64;
                        found_ann = Some(ann.clone());
                        break;
                    }
                }
                if let Some(ann) = found_ann {
                    if let Err(e) = self.state.save_pdf_annotation(&ann) {
                        self.overlays.toast =
                            Some(format!("Failed to toggle annotation status: {}", e));
                    }
                }
                Task::none()
            }
            Message::Pdf(PdfMessage::EditAnnotationTags(id)) => {
                self.pdf.focused_annotation_id = Some(id.clone());
                let mut tags_str = String::new();
                for page_anns in self.pdf.annotations.values() {
                    if let Some(ann) = page_anns.iter().find(|a| a.id == id) {
                        tags_str = ann.tags.join(", ");
                        break;
                    }
                }
                self.overlays.active_modal = Some(views::modals::ModalType::AnnotationTags(id));
                self.overlays.modal_input = tags_str;
                Task::none()
            }
            Message::Pdf(PdfMessage::UpdateAnnotationTags(id, input)) => {
                let tags = input
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<String>>();
                let mut found_ann = None;
                for page_anns in self.pdf.annotations.values_mut() {
                    if let Some(ann) = page_anns.iter_mut().find(|a| a.id == id) {
                        ann.tags = tags.clone();
                        ann.updated_at = std::time::SystemTime::now()
                            .duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64;
                        found_ann = Some(ann.clone());
                        break;
                    }
                }
                if let Some(ann) = found_ann {
                    if let Err(e) = self.state.save_pdf_annotation(&ann) {
                        self.overlays.toast =
                            Some(format!("Failed to save annotation tags: {}", e));
                    }
                }
                Task::none()
            }
            Message::Pdf(PdfMessage::NavigateToAnnotation { id, page }) => {
                self.pdf.focused_annotation_id = Some(id);
                self.navigate_pdf_page(page)
            }
            Message::Pdf(PdfMessage::EditAnnotationNote(id, _page)) => {
                self.pdf.focused_annotation_id = Some(id.clone());
                let mut note = String::new();
                for page_anns in self.pdf.annotations.values() {
                    if let Some(ann) = page_anns.iter().find(|a| a.id == id) {
                        note = ann.note.clone().unwrap_or_default();
                        break;
                    }
                }
                self.overlays.active_modal = Some(views::modals::ModalType::QuickNote(id));
                self.overlays.modal_input = note;
                Task::none()
            }
            Message::Pdf(PdfMessage::ExportAnnotations) => {
                let Some(ref pdf_path) = self.pdf.active_path else {
                    return Task::none();
                };
                let path_str = pdf_path.clone();
                let pdf_filename = std::path::Path::new(&path_str)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("document.pdf")
                    .to_string();
                let default_name = format!(
                    "{}-annotations.md",
                    std::path::Path::new(&pdf_filename)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("document")
                );

                let mut annotations_list = Vec::new();
                for page_anns in self.pdf.annotations.values() {
                    annotations_list.extend(page_anns.clone());
                }

                Task::perform(
                    async move {
                        let file = rfd::AsyncFileDialog::new()
                            .set_title("Export Annotations")
                            .set_file_name(&default_name)
                            .add_filter("Markdown", &["md", "markdown"])
                            .save_file()
                            .await;

                        if let Some(f) = file {
                            let dest_path = f.path().to_path_buf();
                            let content =
                                crate::features::pdf::annotations::export_annotations_to_markdown(
                                    &pdf_filename,
                                    &path_str,
                                    &annotations_list,
                                );
                            match std::fs::write(&dest_path, content) {
                                Ok(()) => Ok(dest_path.to_string_lossy().to_string()),
                                Err(e) => Err(format!("Failed to write file: {}", e)),
                            }
                        } else {
                            Err("Export cancelled".to_string())
                        }
                    },
                    Message::PdfAnnotationsExported,
                )
            }
            Message::Pdf(PdfMessage::AnnotationsExported(res)) => {
                match res {
                    Ok(path) => {
                        self.overlays.toast = Some(format!("Exported to {}", path));
                    }
                    Err(err) => {
                        if err != "Export cancelled" {
                            self.overlays.toast = Some(err);
                        }
                    }
                }
                Task::none()
            }
            _ => Task::none(),
        }
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
            Message::PdfDocumentIdComputed,
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
                move |res| Message::PdfLoaded(generation, res.unwrap_or(0)),
            ),
            Task::perform(
                async move {
                    let renderer = _state_sizes.pdf_renderer()?;
                    renderer.page_sizes(&path_str_sizes).ok()
                },
                move |res| {
                    Message::PdfPageSizesLoaded(
                        generation,
                        path_for_sizes.clone(),
                        res.unwrap_or_default(),
                    )
                },
            ),
            Task::perform(
                async move {
                    let renderer = _state_toc.pdf_renderer()?;
                    renderer.get_toc(&path_str_toc).ok()
                },
                move |res| Message::PdfTocLoaded(generation, res.unwrap_or_default()),
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
                let (width, height) = img.dimensions();
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
                Task::done(Message::PdfInsertQuoteLink)
            }
            crate::messages::CitationItem::Annotation { id, .. } => {
                Task::done(Message::PdfInsertAnnotationLink(id))
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

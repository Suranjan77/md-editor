use iced::widget::operation::{self, AbsoluteOffset};
use iced::widget::{Space, column, container, mouse_area, row, scrollable, stack, text};
use iced::{Alignment, Element, Length, Subscription, Task, Theme};

use image::GenericImageView;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::editor::buffer::{DocBuffer, EditorCommand};
use crate::editor::highlight;
use crate::messages::{Message, Shortcut};
use crate::pdf_notes::{
    append_linked_pdf_note_section, new_linked_pdf_note_content, normalize_note_path,
    note_filename_from_path, slug_fragment,
};
use crate::theme as app_theme;
use crate::views;
use crate::views::pdf_viewer::{PDF_PAGE_LIST_PADDING, PDF_PAGE_SPACING};

const PDF_SCROLLABLE_ID: &str = "pdf_scrollable";
const EDITOR_SCROLLABLE_ID: &str = "editor_scrollable";
const PDF_RENDER_SUPERSAMPLE: f32 = 2.0;
const LARGE_DOC_LINE_THRESHOLD: usize = 1_000;
const HUGE_DOC_LINE_THRESHOLD: usize = 5_000;
const HIGHLIGHT_DEBOUNCE: Duration = Duration::from_millis(80);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivePanel {
    Markdown,
    Pdf,
}

pub(crate) fn is_supported_image_path(path: &str) -> bool {
    path.ends_with(".png")
        || path.ends_with(".jpg")
        || path.ends_with(".jpeg")
        || path.ends_with(".gif")
        || path.ends_with(".bmp")
        || path.ends_with(".webp")
}

#[allow(dead_code)]
fn pdf_slot_offset(page: u16, slot_height: f32) -> f32 {
    PDF_PAGE_LIST_PADDING + f32::from(page) * (slot_height + PDF_PAGE_SPACING)
}

#[allow(dead_code)]
fn pdf_slot_total_height(total_pages: u16, slot_height: f32) -> f32 {
    PDF_PAGE_LIST_PADDING + f32::from(total_pages) * (slot_height + PDF_PAGE_SPACING)
}

#[allow(dead_code)]
fn pdf_slot_page_at_scroll(scroll_y: f32, total_pages: u16, slot_height: f32) -> u16 {
    if total_pages == 0 {
        return 0;
    }

    let slot_stride = slot_height + PDF_PAGE_SPACING;
    if slot_stride <= 0.0 {
        return 0;
    }

    let page = ((scroll_y - PDF_PAGE_LIST_PADDING).max(0.0) / slot_stride).floor() as u16;
    page.min(total_pages.saturating_sub(1))
}

fn text_by_char_range(text: &str, start: usize, end: usize) -> String {
    if start >= end {
        return String::new();
    }

    text.chars().skip(start).take(end - start).collect()
}

pub struct MdEditor {
    state: Arc<md_editor_core::state::AppState>,
    // Vault navigation: root, file tree, sidebar selection/expansion, backlinks
    vault: crate::vault_state::VaultState,
    active_path: Option<String>,

    // Editor pane: buffer, highlighting, TOC, scroll/viewport, resource caches
    editor: crate::editor_state::EditorPane,

    // PDF viewer (document pages, geometry, annotations, render bookkeeping)
    pdf: crate::pdf_pane::PdfPane,

    // Image viewer + active-viewer mode flag (shell-level routing)
    active_image_path: Option<String>,
    active_image: Option<(iced::widget::image::Handle, f32, f32)>,
    showing_pdf: bool,

    // Study tracker
    tracker: crate::tracker_state::TrackerState,

    // Shell UI chrome: modals, command palette, toast, split view, window size
    ui: crate::ui_state::UiState,

    // Search (query/replace, vault + PDF results, in-document match cache)
    search: crate::search_state::SearchState,
    active_panel: ActivePanel,
}

impl MdEditor {
    pub fn new() -> (Self, Task<Message>) {
        let state = Arc::new(md_editor_core::state::AppState::new());
        let last_vault = md_editor_core::config::get_sys_config(&state, "last_vault")
            .ok()
            .flatten();
        let last_file = md_editor_core::config::get_sys_config(&state, "last_file")
            .ok()
            .flatten();
        let tracker = crate::tracker_state::TrackerState::new(&state);

        let mut app = Self {
            state: state.clone(),
            vault: crate::vault_state::VaultState::new(),
            active_path: None,
            editor: crate::editor_state::EditorPane::new(),
            pdf: crate::pdf_pane::PdfPane::new(),
            active_image_path: None,
            active_image: None,
            showing_pdf: false,
            tracker,
            ui: crate::ui_state::UiState::new(),
            search: crate::search_state::SearchState::new(),
            active_panel: ActivePanel::Markdown,
        };

        let mut task = Task::none();
        if let Some(path) = last_vault {
            let index_task = app.open_vault(&path);
            if let Some(file_path) = last_file {
                let lower = file_path.to_lowercase();
                if lower.ends_with(".md") || lower.ends_with(".markdown") {
                    task = app.open_file(&file_path);
                } else if lower.ends_with(".pdf") {
                    app.pdf.active_path = Some(file_path.clone());
                    app.showing_pdf = true;
                    task = app.open_pdf(&file_path);
                } else if is_supported_image_path(&lower) {
                    task = app.open_image(&file_path);
                }
            }
            task = Task::batch(vec![index_task, task]);
        }

        (app, task)
    }

    pub fn title(&self) -> String {
        format!(
            "{}Md-editor — {}",
            if self.editor.buffer.dirty { "● " } else { "" },
            self.active_path
                .as_deref()
                .or(self.pdf.active_path.as_deref())
                .or(self.active_image_path.as_deref())
                .unwrap_or("New File")
        )
    }

    pub fn theme(&self) -> Theme {
        app_theme::md_editor_theme()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let keyboard = iced::keyboard::listen().map(|event| {
            match event {
                iced::keyboard::Event::KeyPressed { key, modifiers, .. } => {
                    // Escape key — close overlays
                    if key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) {
                        return Message::KeyboardShortcut(Shortcut::Escape);
                    }
                    if key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter) {
                        return Message::NameModalSubmitCurrent;
                    }
                    if modifiers.command() || modifiers.control() {
                        match key {
                            iced::keyboard::Key::Character(c) if c == "s" => {
                                return Message::KeyboardShortcut(Shortcut::Save);
                            }
                            iced::keyboard::Key::Character(c) if c == "o" => {
                                return Message::KeyboardShortcut(Shortcut::OpenVault);
                            }
                            iced::keyboard::Key::Character(c) if c == "n" => {
                                return Message::KeyboardShortcut(Shortcut::NewFile);
                            }
                            iced::keyboard::Key::Character(c) if c == "f" => {
                                return Message::KeyboardShortcut(Shortcut::Search);
                            }
                            iced::keyboard::Key::Character(c) if c == "c" => {
                                return Message::PdfCopySelection;
                            }
                            iced::keyboard::Key::Character(c) if c == "p" => {
                                return Message::KeyboardShortcut(Shortcut::CommandPalette);
                            }
                            iced::keyboard::Key::Character(c) if c == "b" => {
                                return Message::KeyboardShortcut(Shortcut::ToggleSidebar);
                            }
                            iced::keyboard::Key::Character(c) if c == "t" => {
                                return Message::KeyboardShortcut(Shortcut::TableOfContents);
                            }
                            _ => {}
                        }
                    }
                    match key {
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowDown) => {
                            return Message::PdfScrollBy(64.0);
                        }
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowUp) => {
                            return Message::PdfScrollBy(-64.0);
                        }
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::PageDown) => {
                            return Message::PdfScrollBy(520.0);
                        }
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::PageUp) => {
                            return Message::PdfScrollBy(-520.0);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            Message::Tick
        });

        let toast = if self.ui.toast.is_some() {
            iced::time::every(std::time::Duration::from_secs(3)).map(|_| Message::ToastHide)
        } else {
            Subscription::none()
        };

        let highlight_debounce = if self.editor.pending_highlight_generation.is_some() {
            iced::time::every(HIGHLIGHT_DEBOUNCE).map(|_| Message::HighlightDebounceElapsed)
        } else {
            Subscription::none()
        };

        let mouse_drag = if self.ui.is_resizing_split {
            iced::event::listen_with(|event, _status, _window_id| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::SplitViewDragging(position.x))
                }
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )) => Some(Message::SplitViewDragEnd),
                _ => None,
            })
        } else {
            Subscription::none()
        };

        let window_events = iced::event::listen_with(|event, _status, _window_id| {
            if let iced::Event::Window(iced::window::Event::Resized(size)) = event {
                Some(Message::WindowResized(
                    size.width as f32,
                    size.height as f32,
                ))
            } else {
                None
            }
        });

        Subscription::batch(vec![
            keyboard,
            toast,
            highlight_debounce,
            mouse_drag,
            window_events,
        ])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.update_inner(message);
        // Refresh the memoized search-match cache once per message so the
        // subsequent view() can read it without rescanning the buffer.
        self.search.ensure_matches(
            &self.editor.buffer,
            self.active_path.as_deref(),
            self.editor.buffer_revision,
        );
        task
    }

    fn update_inner(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenVaultDialog => Task::perform(
                async {
                    let folder = rfd::AsyncFileDialog::new()
                        .set_title("Open Vault Folder")
                        .pick_folder()
                        .await;
                    folder.map(|f| f.path().to_string_lossy().to_string())
                },
                Message::VaultOpened,
            ),
            Message::VaultOpened(Some(path)) => self.open_vault(&path),
            Message::VaultIndexed(entries) => {
                self.vault.entries = entries;
                // Backlinks for the active file depend on the freshly built
                // index; refresh them now that indexing has completed.
                if let Some(path) = self.active_path.clone().or_else(|| self.pdf.active_path.clone())
                {
                    self.vault.backlinks =
                        md_editor_core::vault::get_mixed_backlinks(&self.state, &path)
                            .unwrap_or_default();
                }
                Task::none()
            }
            Message::SidebarToggle => {
                self.vault.sidebar_visible = !self.vault.sidebar_visible;
                Task::none()
            }
            Message::SidebarFileClicked(path) => {
                let path = path.trim().to_string();
                if path.starts_with("pdf://") {
                    let url_str = &path["pdf://".len()..];
                    let (pdf_path, query) = if let Some(idx) = url_str.find('?') {
                        (&url_str[..idx], Some(&url_str[idx + 1..]))
                    } else {
                        (url_str, None)
                    };

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
                        pdf_path,
                    );

                    self.ui.split_view_active = true;
                    self.showing_pdf = true;

                    if self.pdf.active_path.as_deref() == Some(&resolved_pdf_path) {
                        self.pdf.focused_annotation_id = annotation_id;
                        if let Some(p) = page {
                            let p_0 = p.saturating_sub(1);
                            self.navigate_pdf_page(p_0)
                        } else {
                            if let Some(ref ann_id) = self.pdf.focused_annotation_id {
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
                            }
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
                                    let scroll_task = self.scroll_editor_to_line(line_idx);
                                    let cmd_task =
                                        self.run_editor_command(EditorCommand::SetCursor {
                                            line: line_idx,
                                            col: 0,
                                        });
                                    Task::batch(vec![cmd_task, scroll_task])
                                } else {
                                    self.ui.toast = Some(format!(
                                        "Heading or widget not found: #{}",
                                        anchor_part
                                    ));
                                    Task::none()
                                }
                            } else {
                                let mut resolved_file = resolve_relative_link_path(
                                    self.vault.root.as_deref(),
                                    self.active_path.as_deref(),
                                    file_part,
                                );
                                if std::path::Path::new(&resolved_file).extension().is_none() {
                                    resolved_file.push_str(".md");
                                }
                                self.vault.selected_path = Some(resolved_file.clone());
                                let open_task = self.open_file_extended(&resolved_file, false);

                                let target_slug = slugify(anchor_part);
                                if let Some(line_idx) = find_heading_or_widget_line(
                                    &self.editor.buffer.text(),
                                    &self.editor.highlighted_lines,
                                    &target_slug,
                                ) {
                                    let scroll_task = self.scroll_editor_to_line(line_idx);
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
                        } else {
                            let mut resolved_path = resolve_relative_link_path(
                                self.vault.root.as_deref(),
                                self.active_path.as_deref(),
                                &path,
                            );
                            if std::path::Path::new(&resolved_path).extension().is_none() {
                                resolved_path.push_str(".md");
                            }
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
                            } else {
                                Task::none()
                            }
                        }
                    }
                }
            }
            Message::SidebarFolderToggled(path) => {
                if self.vault.expanded_folders.contains(&path) {
                    self.vault.expanded_folders.remove(&path);
                } else {
                    self.vault.expanded_folders.insert(path);
                }
                Task::none()
            }
            Message::CreateFileDialog => {
                self.ui.active_modal = Some(views::modals::ModalType::CreateFile);
                self.ui.modal_input.clear();
                self.ui.link_note_picker_search.clear();
                Task::none()
            }
            Message::CreateFolderDialog => {
                self.ui.active_modal = Some(views::modals::ModalType::CreateFolder);
                self.ui.modal_input.clear();
                self.ui.link_note_picker_search.clear();
                Task::none()
            }
            Message::DeleteFileDialog(path) => {
                self.ui.active_modal = Some(views::modals::ModalType::Delete(path));
                Task::none()
            }
            Message::NameModalInputChanged(input) => {
                self.ui.modal_input = input;
                Task::none()
            }
            Message::PdfLinkNoteFolderSelected(folder) => {
                if matches!(
                    self.ui.active_modal,
                    Some(views::modals::ModalType::LinkNote(_))
                ) {
                    let filename = note_filename_from_path(&self.ui.modal_input);
                    self.ui.modal_input = if folder.is_empty() {
                        filename
                    } else {
                        format!("{}/{}", folder.trim_end_matches('/'), filename)
                    };
                }
                Task::none()
            }
            Message::PdfLinkNoteFileSelected(path) => {
                if matches!(
                    self.ui.active_modal,
                    Some(views::modals::ModalType::LinkNote(_))
                ) {
                    self.ui.modal_input = normalize_note_path(&path);
                }
                Task::none()
            }
            Message::PdfLinkNotePickerSearchChanged(query) => {
                if matches!(
                    self.ui.active_modal,
                    Some(views::modals::ModalType::LinkNote(_))
                ) {
                    self.ui.link_note_picker_search = query;
                }
                Task::none()
            }
            Message::NameModalCancel => {
                self.ui.active_modal = None;
                self.ui.modal_input.clear();
                self.ui.link_note_picker_search.clear();
                Task::none()
            }
            Message::NameModalSubmitCurrent => {
                if matches!(
                    self.ui.active_modal,
                    Some(views::modals::ModalType::CreateFile)
                        | Some(views::modals::ModalType::CreateFolder)
                        | Some(views::modals::ModalType::QuickNote(_))
                        | Some(views::modals::ModalType::LinkNote(_))
                ) {
                    Task::done(Message::NameModalSubmit(self.ui.modal_input.clone()))
                } else {
                    Task::none()
                }
            }
            Message::NameModalSubmit(input) => {
                if let Some(views::modals::ModalType::QuickNote(id)) = self.ui.active_modal.clone() {
                    self.ui.active_modal = None;
                    self.ui.modal_input.clear();
                    self.ui.link_note_picker_search.clear();
                    return Task::done(Message::PdfAddQuickNote(id, input));
                }
                if let Some(views::modals::ModalType::LinkNote(id)) = self.ui.active_modal.clone() {
                    self.ui.active_modal = None;
                    self.ui.modal_input.clear();
                    self.ui.link_note_picker_search.clear();
                    return Task::done(Message::PdfLinkNote(id, input));
                }

                let name = input.trim();
                if name.is_empty() {
                    self.ui.toast = Some("Name cannot be empty".to_string());
                    return Task::none();
                }

                let target_path = self.new_entry_path(name);
                let result = match self.ui.active_modal.as_ref() {
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
                        self.vault.entries =
                            md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                        self.ui.active_modal = None;
                        self.ui.modal_input.clear();
                        self.ui.link_note_picker_search.clear();
                        self.ui.toast = Some("Created".to_string());
                    }
                    Err(err) => self.ui.toast = Some(err),
                }
                Task::none()
            }
            Message::DeleteFile(path) => {
                match md_editor_core::vault::delete_entry(&self.state, &path) {
                    Ok(()) => {
                        self.vault.entries =
                            md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                        if self.active_path.as_deref() == Some(path.as_str()) {
                            self.active_path = None;
                            self.editor.buffer = DocBuffer::new();
                            self.editor.highlighted_lines.clear();
                        }
                        if self.pdf.active_path.as_deref() == Some(path.as_str()) {
                            self.pdf.active_path = None;
                            self.pdf.pages.clear();
                            self.pdf.dimensions.clear();
                        }
                        self.ui.active_modal = None;
                        self.ui.link_note_picker_search.clear();
                        self.ui.toast = Some("Deleted".to_string());
                    }
                    Err(err) => self.ui.toast = Some(err),
                }
                Task::none()
            }

            Message::EditorCommand(command) => self.run_editor_command(command),
            Message::EditorCommandNoScroll(command) => {
                self.run_editor_command_with_scroll(command, false)
            }
            Message::MathRendered(tex, res) => {
                if let Ok(tuple) = res {
                    self.editor.math_cache.insert(tex, tuple);
                }
                Task::none()
            }
            Message::EditorSave => {
                if let Some(path) = &self.active_path {
                    let content = self.editor.buffer.text();
                    let _ = md_editor_core::vault::save_file(&self.state, path, &content);
                    self.editor.buffer.dirty = false;
                    self.ui.toast = Some("File saved".to_string());
                }
                Task::none()
            }
            Message::EditorCheckboxToggle(line_idx) => {
                self.run_editor_command(EditorCommand::ToggleCheckbox { line: line_idx })
            }
            Message::EditorCursorMove(line, col) => {
                self.run_editor_command(EditorCommand::SetCursor { line, col })
            }
            Message::EditorScrolled {
                y,
                viewport_width,
                viewport_height,
            } => {
                self.active_panel = ActivePanel::Markdown;
                self.editor.scroll_y = y;
                self.editor.viewport_width = viewport_width;
                self.editor.viewport_height = viewport_height;
                Task::none()
            }
            Message::ScrollEditorToTarget(target_y) => operation::scroll_to(
                iced::advanced::widget::Id::new(EDITOR_SCROLLABLE_ID),
                AbsoluteOffset {
                    x: 0.0,
                    y: target_y,
                },
            ),
            Message::HighlightDebounceElapsed => {
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
            Message::HighlightReady(generation, lines) => {
                if generation != self.editor.highlight_generation {
                    return Task::none();
                }
                self.editor.highlighted_lines = lines;
                self.load_images();
                self.load_math()
            }

            Message::PdfLoaded(generation, pages) => {
                if generation != self.pdf.render_generation {
                    return Task::none();
                }
                self.pdf.total_pages = pages;
                self.pdf.pages = vec![None; pages as usize];
                self.pdf.dimensions = vec![None; pages as usize];
                if self.pdf.page_sizes.len() != pages as usize {
                    self.pdf.page_sizes = vec![None; pages as usize];
                }
                self.pdf.pending_pages.clear();
                self.pdf.pending_links.clear();
                self.pdf.programmatic_scroll = false;
                self.pdf.toc_target_page = None;
                if pages == 0 {
                    self.ui.toast = Some(
                        "PDF renderer is unavailable or the PDF could not be opened".to_string(),
                    );
                }
                if self.pdf.fit_to_width
                    && self
                        .pdf.page_sizes
                        .iter()
                        .take(pages as usize)
                        .any(Option::is_some)
                {
                    Task::done(Message::PdfFitToWidth)
                } else if self.pdf.fit_to_width {
                    Task::none()
                } else {
                    self.render_all_pdf_pages()
                }
            }
            Message::PdfZoomChanged(zoom) => {
                let current_page = self.pdf.page_at_scroll(self.pdf.scroll_y);
                let page_start_offset = self.pdf.page_offset(current_page);
                let relative_ratio = if self.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                    0.0
                } else {
                    let page_height_old = self.pdf.page_height(current_page);
                    if page_height_old > 0.0 {
                        ((self.pdf.scroll_y - page_start_offset).max(0.0)) / page_height_old
                    } else {
                        0.0
                    }
                };

                // Page bitmaps are rendered at a quantized zoom bucket, so a
                // zoom change only needs a re-render when it crosses a bucket
                // boundary. Within a bucket the cached bitmaps are reused and
                // iced rescales them to the new layout box — making zoom feel
                // instant instead of re-rasterizing every page.
                let bucket_changed = md_editor_core::pdf::pdf_render_bucket(self.pdf.zoom)
                    != md_editor_core::pdf::pdf_render_bucket(zoom);

                self.pdf.fit_to_width = false;
                self.pdf.zoom = zoom;
                if bucket_changed {
                    self.pdf.pages = vec![None; self.pdf.total_pages as usize];
                    self.pdf.dimensions = vec![None; self.pdf.total_pages as usize];
                    self.pdf.placeholder_page_size = self.pdf.first_page_size();
                    self.pdf.pending_pages.clear();
                    self.pdf.pending_links.clear();
                    self.pdf.render_generation = self.pdf.render_generation.wrapping_add(1);
                }
                self.pdf.toc_target_page = Some(current_page);
                self.pdf.programmatic_scroll = true;

                let new_scroll_y = if self.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                    self.pdf.scroll_y
                } else {
                    self.pdf.page_offset(current_page)
                        + relative_ratio * self.pdf.page_height(current_page)
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
            Message::PdfFitToWidth => {
                let is_initial = self.pdf.initial_target_page.is_some();
                let current_page = if let Some(target_page) = self.pdf.initial_target_page.take() {
                    target_page.min(self.pdf.total_pages.saturating_sub(1))
                } else {
                    self.pdf.page_at_scroll(self.pdf.scroll_y)
                };
                let page_start_offset = self.pdf.page_offset(current_page);
                let relative_ratio = if is_initial {
                    0.0
                } else if self.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                    0.0
                } else {
                    let page_height_old = self.pdf.page_height(current_page);
                    if page_height_old > 0.0 {
                        ((self.pdf.scroll_y - page_start_offset).max(0.0)) / page_height_old
                    } else {
                        0.0
                    }
                };

                self.pdf.fit_to_width = true;
                let available_width = self.pdf_available_width();
                let page_width = self
                    .pdf.page_sizes
                    .iter()
                    .flatten()
                    .next()
                    .map(|(w, _)| (*w).max(1.0))
                    .or_else(|| {
                        self.pdf.dimensions
                            .iter()
                            .flatten()
                            .next()
                            .map(|(w, _)| (*w as f32 / self.pdf.zoom).max(1.0))
                    })
                    .unwrap_or(612.0);
                let next_zoom = ((available_width - 48.0).max(240.0) / page_width).clamp(0.5, 4.0);
                let next_zoom = (next_zoom * 100.0).round() / 100.0;
                // Only re-render when fit-to-width crosses a zoom bucket;
                // otherwise reuse cached bitmaps (see PdfZoomChanged).
                let bucket_changed = md_editor_core::pdf::pdf_render_bucket(self.pdf.zoom)
                    != md_editor_core::pdf::pdf_render_bucket(next_zoom);
                self.pdf.zoom = next_zoom;
                if bucket_changed {
                    self.pdf.pages = vec![None; self.pdf.total_pages as usize];
                    self.pdf.dimensions = vec![None; self.pdf.total_pages as usize];
                    self.pdf.placeholder_page_size = self.pdf.first_page_size();
                    self.pdf.pending_pages.clear();
                    self.pdf.pending_links.clear();
                    self.pdf.render_generation = self.pdf.render_generation.wrapping_add(1);
                }
                self.pdf.toc_target_page = Some(current_page);
                self.pdf.programmatic_scroll = true;

                let new_scroll_y = if is_initial {
                    self.pdf.page_offset(current_page)
                } else if self.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                    self.pdf.scroll_y
                } else {
                    self.pdf.page_offset(current_page)
                        + relative_ratio * self.pdf.page_height(current_page)
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
            Message::PdfPageSizesLoaded(generation, path, sizes) => {
                if generation != self.pdf.render_generation
                    && self.pdf.active_path.as_deref() != Some(path.as_str())
                {
                    return Task::none();
                }
                self.pdf.page_sizes = sizes.into_iter().map(Some).collect();
                if self.pdf.page_sizes.len() < self.pdf.total_pages as usize {
                    self.pdf.page_sizes
                        .resize(self.pdf.total_pages as usize, None);
                }
                if self.pdf.placeholder_page_size.is_none() {
                    self.pdf.placeholder_page_size = self.pdf.first_page_size();
                }
                if self.pdf.fit_to_width && self.pdf.total_pages > 0 {
                    Task::done(Message::PdfFitToWidth)
                } else {
                    Task::none()
                }
            }
            Message::PdfRendered(generation, page, img) => {
                self.pdf.pending_pages.remove(&page);
                if generation != self.pdf.render_generation {
                    return Task::none();
                }
                let (width, height) = img.dimensions();
                let handle = iced::widget::image::Handle::from_rgba(
                    width,
                    height,
                    img.into_rgba8().into_raw(),
                );
                let logical_width = (width as f32 / PDF_RENDER_SUPERSAMPLE).round() as u32;
                let logical_height = (height as f32 / PDF_RENDER_SUPERSAMPLE).round() as u32;
                if (page as usize) < self.pdf.pages.len() {
                    self.pdf.pages[page as usize] = Some(handle);
                    self.pdf.dimensions[page as usize] = Some((logical_width, logical_height));
                }
                if self.pdf.placeholder_page_size.is_none() || page == 0 {
                    self.pdf.placeholder_page_size = Some((
                        logical_width as f32 / self.pdf.zoom,
                        logical_height as f32 / self.pdf.zoom,
                    ));
                }
                let mut tasks = vec![self.load_pdf_page_links(page)];
                if !self.pdf.page_text.contains_key(&page) && !self.pdf.pending_text.contains(&page)
                {
                    tasks.push(self.load_pdf_page_text(page));
                }
                if self.pdf.toc_target_page == Some(page) {
                    self.pdf.programmatic_scroll = true;
                    let scroll_y = self.pdf.page_offset(page);
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
            Message::PdfRenderFailed(generation, page) => {
                self.pdf.pending_pages.remove(&page);
                if generation != self.pdf.render_generation {
                    return Task::none();
                }
                if self.pdf.toc_target_page == Some(page) {
                    self.pdf.toc_target_page = None;
                    self.pdf.programmatic_scroll = false;
                }
                self.ui.toast = Some(format!("Could not render PDF page {}", page + 1));
                Task::none()
            }
            Message::PdfRenderSkipped(generation, page) => {
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
            Message::TocClicked(index) => {
                if self.pdf.active_path.is_some() {
                    let target_page = (index as u16).min(self.pdf.total_pages.saturating_sub(1));
                    self.navigate_pdf_page(target_page)
                } else {
                    Task::done(Message::EditorCursorMove(index, 0))
                }
            }
            Message::PdfScrolled { y, viewport_height } => {
                self.active_panel = ActivePanel::Pdf;
                self.pdf.scroll_y = y;
                let new_page = self.pdf.page_at_scroll(y + viewport_height * 0.33);
                if self.pdf.programmatic_scroll {
                    self.pdf.programmatic_scroll = false;
                    let target_page = self.pdf.toc_target_page.take().unwrap_or(new_page);
                    self.pdf.current_page = target_page.min(self.pdf.total_pages.saturating_sub(1));
                    let start = self.pdf.current_page.saturating_sub(2);
                    let end =
                        (self.pdf.current_page + 2).min(self.pdf.total_pages.saturating_sub(1));
                    return self.render_pdf_page_range(start, end);
                }
                if new_page != self.pdf.current_page && new_page < self.pdf.total_pages {
                    if new_page.abs_diff(self.pdf.current_page) > 8 {
                        self.pdf.pending_pages.clear();
                        self.pdf.pending_links.clear();
                    }
                    self.pdf.current_page = new_page;
                    self.render_pdf_pages_for_viewport(y, viewport_height)
                } else {
                    self.render_pdf_pages_for_viewport(y, viewport_height)
                }
            }
            Message::PdfLeftClicked(page_idx, x, y, modifiers) => {
                self.active_panel = ActivePanel::Pdf;
                if let Some(link) = self.pdf.link_at(page_idx, x, y) {
                    if let Some(dest_page) = link.dest_page {
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
                            self.ui.toast = Some(format!("Opening: {}", uri));
                        } else {
                            self.ui.toast =
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
            Message::PdfRightClicked(page_idx, x, y) => {
                self.active_panel = ActivePanel::Pdf;
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
                    target_ann = self.annotation_at(page_idx, x, y);
                }

                if let Some(ann) = target_ann {
                    self.ui.active_modal = Some(views::modals::ModalType::QuickNote(ann.id));
                    self.ui.modal_input = ann.note.unwrap_or_default();
                    Task::none()
                } else if let Some(link) = self
                    .pdf
                    .link_at(page_idx, x, y)
                    .filter(|link| link.dest_page.is_some())
                {
                    let Some(dest_page) = link.dest_page else {
                        return Task::none();
                    };
                    let dest_y = link.dest_y;
                    let Some(path) = self.pdf.active_path.clone() else {
                        return Task::none();
                    };
                    let Some(abs_path) = self.resolve_active_path(&path) else {
                        return Task::none();
                    };
                    let abs_path = abs_path.to_string_lossy().to_string();
                    let _state = self.state.clone();

                    Task::perform(
                        async move {
                            let renderer = _state.pdf_renderer.as_ref()?;
                            renderer
                                .render_link_preview(&abs_path, dest_page, dest_y)
                                .ok()
                        },
                        |res| {
                            Message::PdfLinkPreviewResult(
                                res.ok_or_else(|| "Failed to preview".into()),
                            )
                        },
                    )
                } else {
                    Task::none()
                }
            }
            Message::PdfLinkPreviewResult(Ok(res)) => {
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
            Message::PdfLinkPreviewResult(Err(e)) => {
                self.ui.toast = Some(format!("Preview Error: {}", e));
                Task::none()
            }
            Message::ClosePdfLinkPreview => {
                self.pdf.link_preview = None;
                Task::none()
            }
            Message::PdfTocLoaded(generation, entries) => {
                if generation != self.pdf.render_generation {
                    return Task::none();
                }
                fn flatten_pdf_toc(
                    entries: &[md_editor_core::pdf::TocEntry],
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

                let mut mapped = Vec::new();
                flatten_pdf_toc(&entries, 1, &mut mapped);
                self.editor.toc_entries = mapped;
                Task::none()
            }
            Message::PdfPageLinksLoaded(generation, page, links) => {
                self.pdf.pending_links.remove(&page);
                if generation != self.pdf.render_generation {
                    return Task::none();
                }
                self.pdf.page_links.insert(page, links);
                Task::none()
            }

            m @ Message::TrackerToggle => self.tracker.update(m, &self.state),
            Message::CommandPaletteOpen => {
                self.ui.command_palette_visible = true;
                self.ui.command_palette_query.clear();
                Task::none()
            }
            Message::CommandPaletteQueryChanged(query) => {
                self.ui.command_palette_query = query;
                Task::none()
            }
            Message::CommandPaletteCommandClicked(shortcut) => {
                self.ui.command_palette_visible = false;
                self.ui.command_palette_query.clear();
                Task::done(Message::KeyboardShortcut(shortcut))
            }
            m @ (Message::TrackerStart
                | Message::TrackerStop
                | Message::TrackerTabSelected(_)
                | Message::TrackerProjectStatusChanged(..)
                | Message::TrackerGateToggled(..)
                | Message::TrackerReadingToggled(..)
                | Message::TrackerConfigEdited(_)
                | Message::TrackerConfigSave
                | Message::TrackerManualDateChanged(_)
                | Message::TrackerManualHoursChanged(_)
                | Message::TrackerManualNotesChanged(_)
                | Message::TrackerManualAdd
                | Message::TrackerSessionDelete(_)) => self.tracker.update(m, &self.state),

            Message::GlobalSearchOpen => {
                self.search.visible = true;
                if self.pdf.active_path.is_some() && !self.search.query.trim().is_empty() {
                    Task::batch(vec![self.search_pdf(), focus_global_search_input()])
                } else {
                    focus_global_search_input()
                }
            }
            Message::SearchClose => {
                self.search.visible = false;
                self.search.file_visible = false;
                self.restore_scroll_positions()
            }
            Message::SearchQueryChanged(q) => {
                self.search.query = q.clone();
                self.search.match_index = None;
                self.search.pdf_error = None;
                if q.len() > 2 && !self.search.regex {
                    if let Ok(res) = md_editor_core::vault::search_vault(&self.state, &q) {
                        self.search.results = res;
                    }
                } else {
                    self.search.results.clear();
                }
                if (self.search.visible || self.pdf_search_is_active())
                    && self.pdf.active_path.is_some()
                    && q.len() > 1
                {
                    self.search_pdf()
                } else {
                    self.search.pdf_results.clear();
                    self.search.pdf_indices_by_page.clear();
                    Task::none()
                }
            }
            Message::SearchReplaceChanged(replace) => {
                self.search.replace = replace;
                Task::none()
            }
            Message::SearchRegexToggled(value) => {
                self.search.regex = value;
                self.search.match_index = None;
                if (self.search.visible || self.pdf_search_is_active())
                    && self.pdf.active_path.is_some()
                    && self.search.query.len() > 1
                {
                    self.search_pdf()
                } else {
                    Task::none()
                }
            }
            Message::SearchMatchCaseToggled(value) => {
                self.search.match_case = value;
                self.search.match_index = None;
                if (self.search.visible || self.pdf_search_is_active())
                    && self.pdf.active_path.is_some()
                    && self.search.query.len() > 1
                {
                    self.search_pdf()
                } else {
                    Task::none()
                }
            }
            Message::SearchPrevious => {
                if self.pdf_search_is_active() {
                    self.navigate_pdf_search(false)
                } else if self.editor_search_is_active() {
                    self.navigate_file_search(false)
                } else {
                    Task::none()
                }
            }
            Message::SearchNext => {
                if self.pdf_search_is_active() {
                    self.navigate_pdf_search(true)
                } else if self.editor_search_is_active() {
                    self.navigate_file_search(true)
                } else {
                    Task::none()
                }
            }
            Message::SearchReplaceAll => match self.replace_all_in_current_document() {
                Ok((count, task)) => {
                    self.ui.toast = Some(format!("Replaced {} matches", count));
                    task
                }
                Err(err) => {
                    self.ui.toast = Some(err);
                    Task::none()
                }
            },
            Message::PdfSearchResult(Ok(results)) => {
                self.search.pdf_error = None;
                self.search.pdf_results = results;
                self.search.rebuild_pdf_page_index();
                if self
                    .search
                    .match_index
                    .is_some_and(|index| index >= self.search.pdf_results.len())
                {
                    self.search.match_index = None;
                }
                if self.pdf_search_is_active() && !self.search.pdf_results.is_empty() {
                    if let Some(index) = self.search.match_index {
                        self.navigate_pdf_search_to_index(index)
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
            Message::PdfSearchResult(Err(err)) => {
                self.search.pdf_results.clear();
                self.search.pdf_indices_by_page.clear();
                self.search.pdf_error = Some(err);
                Task::none()
            }
            Message::PdfSearchResultClicked(page) => {
                self.search.visible = false;
                self.search.file_visible = true;
                self.active_panel = ActivePanel::Pdf;
                self.search.match_index = self
                    .search
                    .pdf_results
                    .iter()
                    .position(|result| result.page_index == page);
                if let Some(index) = self.search.match_index {
                    self.navigate_pdf_search_to_index(index)
                } else {
                    self.pdf.current_page = page.min(self.pdf.total_pages.saturating_sub(1));
                    self.navigate_pdf_page(self.pdf.current_page)
                }
            }
            Message::PdfScrollBy(delta) => {
                if self.pdf.active_path.is_none()
                    || (!self.showing_pdf
                        && !(self.ui.split_view_active && self.active_path.is_some()))
                    || (self.ui.split_view_active
                        && self.active_path.is_some()
                        && self.active_panel != ActivePanel::Pdf)
                    || self.search.visible
                    || self.search.file_visible
                    || self.ui.active_modal.is_some()
                    || self.ui.command_palette_visible
                {
                    return Task::none();
                }
                let max_y = self.pdf.total_height().max(0.0);
                let y = (self.pdf.scroll_y + delta).clamp(0.0, max_y);
                operation::scroll_to(
                    iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                    AbsoluteOffset { x: 0.0, y },
                )
            }
            Message::PdfDocumentIdComputed(Some((path, hash, len, mtime))) => {
                let _ = self.state.save_pdf_document(&hash, &path, len, mtime);
                self.pdf.document_id = Some(hash.clone());

                let annotations = self
                    .state
                    .get_pdf_annotations(&hash, None)
                    .unwrap_or_default();
                self.pdf.annotations.clear();
                for ann in annotations {
                    self.pdf.annotations
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

                let scroll_task = if let Some(page) = target_page {
                    self.pdf.initial_target_page = None;
                    self.pdf.initial_target_annotation = None;
                    self.navigate_pdf_page(page)
                } else if let Some(page) = self.pdf.initial_target_page {
                    self.pdf.initial_target_page = None;
                    self.navigate_pdf_page(page)
                } else {
                    Task::none()
                };

                scroll_task
            }
            Message::PdfDocumentIdComputed(None) => Task::none(),
            Message::PdfPageTextLoaded(generation, page, res) => {
                self.pdf.pending_text.remove(&page);
                if generation == self.pdf.render_generation {
                    if let Ok(page_text) = res {
                        self.pdf.page_text.insert(page, page_text);
                        self.pdf.text_lru.push_back(page);
                        if self.pdf.text_lru.len() > 12 {
                            if let Some(oldest) = self.pdf.text_lru.pop_front() {
                                self.pdf.page_text.remove(&oldest);
                            }
                        }
                    }
                }
                Task::none()
            }
            Message::PdfSelectionChanged(page, anchor, focus) => {
                self.active_panel = ActivePanel::Pdf;
                self.pdf.selection = Some(views::interactive_pdf::PdfSelection {
                    page_index: page,
                    anchor_idx: anchor,
                    focus_idx: focus,
                });
                Task::none()
            }
            Message::PdfSelectionCleared => {
                self.pdf.selection = None;
                Task::none()
            }
            Message::PdfSelectionFinished(page, anchor, focus) => {
                self.active_panel = ActivePanel::Pdf;
                self.pdf.selection = Some(views::interactive_pdf::PdfSelection {
                    page_index: page,
                    anchor_idx: anchor,
                    focus_idx: focus,
                });
                Task::none()
            }
            Message::PdfCopySelection => {
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
            Message::PdfCreateHighlight(color) => {
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

                        let rects = md_editor_core::pdf::merge_char_rects(&selected_chars);

                        let id = uuid::Uuid::new_v4().to_string();
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64;

                        let ann = md_editor_core::pdf::PdfAnnotation {
                            id: id.clone(),
                            document_id: doc_id.clone(),
                            page_index: sel.page_index,
                            kind: md_editor_core::pdf::PdfAnnotationKind::Highlight,
                            color,
                            selected_text,
                            ranges: vec![md_editor_core::pdf::PdfTextRange {
                                start_text_index: start,
                                end_text_index: end,
                            }],
                            rects,
                            note: None,
                            linked_note_path: None,
                            markdown_anchor: None,
                            created_at: now,
                            updated_at: now,
                        };

                        if let Err(e) = self.state.save_pdf_annotation(&ann) {
                            self.ui.toast = Some(format!("Failed to save highlight: {}", e));
                        } else {
                            self.pdf.annotations
                                .entry(sel.page_index)
                                .or_default()
                                .push(ann);
                            self.pdf.selection = None;
                            if let Some(ref path) = self.pdf.active_path {
                                self.vault.backlinks =
                                    md_editor_core::vault::get_mixed_backlinks(&self.state, path)
                                        .unwrap_or_default();
                            }
                        }
                    }
                }
                Task::none()
            }
            Message::PdfDeleteHighlight(id) => {
                if let Err(e) = self.state.delete_pdf_annotation(&id) {
                    self.ui.toast = Some(format!("Failed to delete highlight: {}", e));
                } else {
                    for page_anns in self.pdf.annotations.values_mut() {
                        page_anns.retain(|a| a.id != id);
                    }
                    if self.pdf.focused_annotation_id.as_ref() == Some(&id) {
                        self.pdf.focused_annotation_id = None;
                    }
                    if let Some(views::modals::ModalType::QuickNote(ref mid)) = self.ui.active_modal {
                        if mid == &id {
                            self.ui.active_modal = None;
                            self.ui.modal_input.clear();
                        }
                    }
                    if let Some(ref path) = self.pdf.active_path {
                        self.vault.backlinks =
                            md_editor_core::vault::get_mixed_backlinks(&self.state, path)
                                .unwrap_or_default();
                    }
                }
                Task::none()
            }
            Message::PdfAddQuickNote(id, note_content) => {
                let mut found_ann = None;
                for page_anns in self.pdf.annotations.values_mut() {
                    if let Some(ann) = page_anns.iter_mut().find(|a| a.id == id) {
                        ann.note = Some(note_content.clone());
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
                        self.ui.toast = Some(format!("Failed to save note: {}", e));
                    } else {
                        if let Some(ref path) = self.pdf.active_path {
                            self.vault.backlinks =
                                md_editor_core::vault::get_mixed_backlinks(&self.state, path)
                                    .unwrap_or_default();
                        }
                    }
                }
                Task::none()
            }
            Message::PdfLinkNote(annotation_id, mut note_path) => {
                let mut annotation = None;
                for page_anns in self.pdf.annotations.values() {
                    if let Some(ann) = page_anns.iter().find(|a| a.id == annotation_id) {
                        annotation = Some(ann.clone());
                        break;
                    }
                }
                if let Some(mut ann) = annotation {
                    if note_path.is_empty() {
                        self.ui.modal_input = self.default_pdf_note_path(&ann);
                        self.ui.link_note_picker_search.clear();
                        self.ui.active_modal = Some(views::modals::ModalType::LinkNote(annotation_id));
                        return Task::none();
                    }

                    note_path = normalize_note_path(&note_path);
                    if let Some(ref pdf_path) = self.pdf.active_path {
                        let content = self.linked_pdf_note_file_content(&note_path, pdf_path, &ann);

                        if let Err(e) =
                            md_editor_core::vault::save_file(&self.state, &note_path, &content)
                        {
                            self.ui.toast = Some(format!("Failed to create linked note: {}", e));
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
                        self.ui.toast = Some(format!("Failed to link note: {}", e));
                    } else {
                        for page_anns in self.pdf.annotations.values_mut() {
                            if let Some(a) = page_anns.iter_mut().find(|a| a.id == annotation_id) {
                                a.linked_note_path = Some(note_path.clone());
                                a.updated_at = ann.updated_at;
                                break;
                            }
                        }
                        self.vault.entries =
                            md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                        self.ui.toast = Some(format!("Linked note: {}", note_path));
                        return Task::done(Message::PdfOpenLinkedNote(note_path));
                    }
                }
                Task::none()
            }
            Message::PdfOpenLinkedNote(note_path) => {
                self.ui.split_view_active = true;
                let open_task = self.open_file_extended(&note_path, false);
                if self.pdf.fit_to_width {
                    Task::batch(vec![open_task, Task::done(Message::PdfFitToWidth)])
                } else {
                    Task::batch(vec![open_task, self.restore_scroll_positions()])
                }
            }
            Message::PdfAnnotationFocused {
                document_path,
                annotation_id,
                page,
            } => {
                let resolved_pdf_path = resolve_relative_link_path(
                    self.vault.root.as_deref(),
                    self.active_path.as_deref(),
                    &document_path,
                );

                self.ui.split_view_active = true;
                self.showing_pdf = true;

                if self.pdf.active_path.as_deref() == Some(&resolved_pdf_path) {
                    self.pdf.focused_annotation_id = Some(annotation_id);
                    self.navigate_pdf_page(page.saturating_sub(1))
                } else {
                    self.pdf.initial_target_page = Some(page.saturating_sub(1));
                    self.pdf.initial_target_annotation = Some(annotation_id);
                    self.open_pdf(&resolved_pdf_path)
                }
            }
            Message::SearchResultClicked(path) => {
                self.search.visible = false;
                self.open_file(&path)
            }

            Message::ShowToast(text) => {
                self.ui.toast = Some(text);
                Task::none()
            }
            Message::ToastHide => {
                self.ui.toast = None;
                Task::none()
            }
            Message::KeyboardShortcut(s) => {
                match s {
                    Shortcut::Escape => {
                        // Close overlays in priority order
                        if self.pdf.selection.is_some() {
                            self.pdf.selection = None;
                        } else if self.pdf.focused_annotation_id.is_some() {
                            self.pdf.focused_annotation_id = None;
                        } else if self.pdf.link_preview.is_some() {
                            self.pdf.link_preview = None;
                        } else if self.ui.active_modal.is_some() {
                            self.ui.active_modal = None;
                            self.ui.modal_input.clear();
                            self.ui.link_note_picker_search.clear();
                        } else if self.tracker.visible {
                            self.tracker.hide();
                        } else if self.search.file_visible {
                            self.search.file_visible = false;
                            return self.restore_scroll_positions();
                        } else if self.search.visible {
                            self.search.visible = false;
                            return self.restore_scroll_positions();
                        } else if self.ui.command_palette_visible {
                            self.ui.command_palette_visible = false;
                        } else if self.editor.toc_visible {
                            self.editor.toc_visible = false;
                        }
                        Task::none()
                    }
                    Shortcut::ToggleSidebar => {
                        self.vault.sidebar_visible = !self.vault.sidebar_visible;
                        Task::none()
                    }
                    Shortcut::Save => Task::done(Message::EditorSave),
                    Shortcut::OpenVault => Task::done(Message::OpenVaultDialog),
                    Shortcut::NewFile => Task::done(Message::CreateFileDialog),
                    Shortcut::Search => {
                        if self.ui.split_view_active && self.active_path.is_some() {
                            self.search.file_visible = true;
                            self.search.visible = false;
                            if self.active_panel == ActivePanel::Pdf
                                && self.pdf.active_path.is_some()
                            {
                                if !self.search.query.trim().is_empty() {
                                    return Task::batch(vec![
                                        self.search_pdf(),
                                        focus_pdf_search_input(),
                                        self.restore_scroll_positions(),
                                    ]);
                                }
                                Task::batch(vec![
                                    focus_pdf_search_input(),
                                    self.restore_scroll_positions(),
                                ])
                            } else {
                                self.search.pdf_results.clear();
                                self.search.pdf_indices_by_page.clear();
                                Task::batch(vec![
                                    focus_file_search_input(),
                                    self.restore_scroll_positions(),
                                ])
                            }
                        } else if self.pdf.active_path.is_some() && self.showing_pdf {
                            self.search.file_visible = true;
                            self.search.visible = false;
                            if !self.search.query.trim().is_empty() {
                                return Task::batch(vec![
                                    self.search_pdf(),
                                    focus_pdf_search_input(),
                                    self.restore_scroll_positions(),
                                ]);
                            }
                            Task::batch(vec![
                                focus_pdf_search_input(),
                                self.restore_scroll_positions(),
                            ])
                        } else if self.active_path.is_some() {
                            self.search.file_visible = true;
                            self.search.visible = false;
                            Task::batch(vec![
                                focus_file_search_input(),
                                self.restore_scroll_positions(),
                            ])
                        } else {
                            self.search.visible = true;
                            focus_global_search_input()
                        }
                    }
                    Shortcut::CommandPalette => {
                        self.ui.command_palette_visible = true;
                        self.ui.command_palette_query.clear();
                        Task::none()
                    }
                    Shortcut::ToggleBacklinks => {
                        self.vault.backlinks_visible = !self.vault.backlinks_visible;
                        Task::none()
                    }
                    Shortcut::TableOfContents => {
                        if self.pdf.active_path.is_some()
                            && (self.showing_pdf
                                || (self.ui.split_view_active && self.active_path.is_some()))
                        {
                            self.editor.toc_visible = !self.editor.toc_visible;
                        }
                        Task::none()
                    }
                    Shortcut::StudyTracker => {
                        self.tracker.toggle_visible();
                        Task::none()
                    }
                    Shortcut::SplitView => Task::done(Message::SplitViewToggle),
                    Shortcut::FocusMode => {
                        self.vault.sidebar_visible = false;
                        self.vault.backlinks_visible = false;
                        self.editor.toc_visible = false;
                        self.tracker.hide();
                        Task::none()
                    }
                }
            }
            Message::SplitViewToggle => {
                if self.active_path.is_some() && self.pdf.active_path.is_some() {
                    self.ui.split_view_active = !self.ui.split_view_active;
                    if self.pdf.fit_to_width {
                        return Task::done(Message::PdfFitToWidth);
                    }
                } else {
                    self.ui.toast =
                        Some("Open a markdown file and a PDF to use split view".to_string());
                }
                Task::none()
            }
            Message::SplitViewDragStart => {
                self.ui.is_resizing_split = true;
                Task::none()
            }
            Message::SplitViewDragging(x_pos) => {
                if !self.ui.is_resizing_split {
                    return Task::none();
                }
                let side_width = if self.vault.sidebar_visible { 250.0 } else { 0.0 }
                    + if self.tracker.visible { 300.0 } else { 0.0 }
                    + if self.editor.toc_visible { 250.0 } else { 0.0 };
                let content_width = (self.ui.window_width - side_width).max(480.0);
                let x_min = side_width + 240.0;
                let x_max = side_width + content_width - 240.0;
                let total_width = x_max - x_min;
                if total_width > 1.0 {
                    self.ui.split_ratio = ((x_pos - x_min) / total_width).clamp(0.25, 0.75);
                }
                Task::none()
            }
            Message::SplitViewDragEnd => {
                self.ui.is_resizing_split = false;
                if self.pdf.fit_to_width && self.pdf.active_path.is_some() {
                    return Task::done(Message::PdfFitToWidth);
                }
                Task::none()
            }
            Message::WindowResized(width, height) => {
                self.ui.window_width = width;
                self.ui.window_height = height;
                if self.pdf.fit_to_width && self.pdf.active_path.is_some() {
                    return Task::done(Message::PdfFitToWidth);
                }
                Task::none()
            }
            Message::ToggleTOC => {
                if self.pdf.active_path.is_some()
                    && (self.showing_pdf || (self.ui.split_view_active && self.active_path.is_some()))
                {
                    self.editor.toc_visible = !self.editor.toc_visible;
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    pub fn view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        if self.vault.root.is_none() {
            return views::welcome::view();
        }

        let toolbar = views::toolbar::view(
            self.active_path.as_deref(),
            self.pdf.active_path
                .as_deref()
                .or(self.active_image_path.as_deref()),
            None,
            self.vault.sidebar_visible,
            self.vault.backlinks_visible,
            self.tracker.visible,
            self.editor.toc_visible,
            self.pdf.active_path.is_some()
                && (self.showing_pdf || (self.ui.split_view_active && self.active_path.is_some())),
            self.ui.split_view_active,
            self.active_path.is_some(),
        );

        let sidebar = views::sidebar::view(
            &self.vault.entries,
            self.vault.selected_path.as_deref(),
            self.active_path
                .as_deref()
                .or(self.pdf.active_path.as_deref())
                .or(self.active_image_path.as_deref()),
            &self.vault.expanded_folders,
            !self.vault.sidebar_visible,
        );

        let editor_search_active = self.editor_search_is_active();
        let pdf_search_active = self.pdf_search_is_active();

        let active_search_match = if editor_search_active {
            self.search.active_match_position()
        } else {
            None
        };
        let editor_search_query = if editor_search_active {
            self.search.query.as_str()
        } else {
            ""
        };
        let editor_scroll = scrollable(
            container(
                crate::editor::renderer::Editor::new(
                    &self.editor.buffer,
                    &self.editor.highlighted_lines,
                    &self.editor.image_cache,
                    &self.editor.math_cache,
                    Message::EditorCommand,
                    Message::EditorCommandNoScroll,
                    Message::SidebarFileClicked,
                    Message::EditorCheckboxToggle,
                )
                .search(
                    editor_search_query,
                    self.search.regex,
                    self.search.match_case,
                    active_search_match,
                ),
            )
            .padding(20)
            .width(Length::Fill),
        )
        .id(iced::advanced::widget::Id::new(EDITOR_SCROLLABLE_ID))
        .on_scroll(|vp| Message::EditorScrolled {
            y: vp.absolute_offset().y,
            viewport_width: vp.bounds().width,
            viewport_height: vp.bounds().height,
        })
        .height(Length::Fill);

        let editor_view: Element<Message, Theme, iced::Renderer> = {
            let file_bar: Element<'_, Message, Theme, iced::Renderer> = if editor_search_active {
                views::search::file_bar(
                    &self.search.query,
                    &self.search.replace,
                    self.search.regex,
                    self.search.match_case,
                    self.search.match_count(),
                    self.search.match_index,
                )
                .into()
            } else {
                container(Space::new())
                    .height(Length::Fixed(0.0))
                    .width(Length::Fill)
                    .into()
            };
            column![file_bar, editor_scroll].height(Length::Fill).into()
        };

        let pdf_view: Element<Message, Theme, iced::Renderer> =
            if let Some(_) = &self.pdf.active_path {
                let focused_ann = self.pdf.focused_annotation_id.as_ref().and_then(|ann_id| {
                    self.pdf.annotations
                        .values()
                        .flatten()
                        .find(|a| &a.id == ann_id)
                });
                let pdf_toolbar = views::pdf_viewer::toolbar(
                    self.pdf.current_page,
                    self.pdf.total_pages,
                    self.pdf.zoom,
                    self.editor.toc_visible,
                    self.pdf.selection.is_some(),
                    focused_ann,
                );
                let pdf_pages = scrollable(views::pdf_viewer::view_continuous(
                    &self.pdf.pages,
                    self.pdf.zoom,
                    &self.pdf.dimensions,
                    &self.pdf.page_sizes,
                    self.pdf.placeholder_page_size,
                    if pdf_search_active || self.search.visible || self.search.file_visible {
                        &self.search.pdf_results
                    } else {
                        &[]
                    },
                    &self.search.pdf_indices_by_page,
                    self.search.match_index,
                    &self.pdf.page_text,
                    &self.pdf.annotations,
                    self.pdf.selection,
                    self.pdf.focused_annotation_id.as_deref(),
                ))
                .id(iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID))
                .on_scroll(|vp| Message::PdfScrolled {
                    y: vp.absolute_offset().y,
                    viewport_height: vp.bounds().height,
                })
                .height(Length::Fill);

                let search_bar: Element<'_, Message, Theme, iced::Renderer> = if pdf_search_active {
                    views::pdf_viewer::search_bar(
                        &self.search.query,
                        self.search.regex,
                        self.search.match_case,
                        self.search.pdf_results.len(),
                        self.search.match_index,
                    )
                    .into()
                } else {
                    container(Space::new())
                        .height(Length::Fixed(0.0))
                        .width(Length::Fill)
                        .into()
                };

                column![search_bar, pdf_pages, pdf_toolbar]
                    .height(Length::Fill)
                    .into()
            } else {
                container(Space::new()).width(Length::Fixed(0.0)).into()
            };

        let pdf_toc_available = self.pdf.active_path.is_some()
            && (self.showing_pdf || (self.ui.split_view_active && self.active_path.is_some()));
        let toc_view: Element<Message, Theme, iced::Renderer> =
            if self.editor.toc_visible && pdf_toc_available {
                views::toc::view(&self.editor.toc_entries)
            } else {
                container(Space::new()).width(Length::Fixed(0.0)).into()
            };

        let image_view: Element<Message, Theme, iced::Renderer> =
            if let Some((handle, width, height)) = &self.active_image {
                let label = self.active_image_path.as_deref().unwrap_or("Image");
                container(
                    column![
                        text(label).size(13).color(app_theme::TEXT_MUTED),
                        iced::widget::image(handle.clone())
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .content_fit(iced::ContentFit::Contain),
                        text(format!("{:.0} x {:.0}", width, height))
                            .size(11)
                            .color(app_theme::TEXT_MUTED),
                    ]
                    .spacing(12)
                    .align_x(Alignment::Center)
                    .padding(24),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(app_theme::BG_PRIMARY)),
                    ..Default::default()
                })
                .into()
            } else {
                container(Space::new()).width(Length::Fixed(0.0)).into()
            };

        let main_content: Element<Message, Theme, iced::Renderer> = if self.ui.split_view_active
            && self.active_path.is_some()
            && self.pdf.active_path.is_some()
        {
            let left_portion = (self.ui.split_ratio * 1000.0) as u16;
            let right_portion = ((1.0 - self.ui.split_ratio) * 1000.0) as u16;

            let divider = mouse_area(
                container(text("⋮").size(14).color(app_theme::TEXT_MUTED))
                    .width(Length::Fixed(10.0))
                    .height(Length::Fill)
                    .center_x(Length::Fixed(10.0))
                    .center_y(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(iced::Background::Color(app_theme::BG_TERTIARY)),
                        ..Default::default()
                    }),
            )
            .on_press(Message::SplitViewDragStart)
            .on_release(Message::SplitViewDragEnd)
            .interaction(iced::mouse::Interaction::ResizingHorizontally);

            row![
                container(editor_view).width(Length::FillPortion(left_portion)),
                divider,
                container(pdf_view)
                    .width(Length::FillPortion(right_portion))
                    .style(|_| container::Style {
                        border: iced::Border {
                            color: app_theme::BORDER,
                            width: 1.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    })
            ]
            .into()
        } else if self.showing_pdf && self.pdf.active_path.is_some() {
            pdf_view
        } else if self.active_image.is_some() {
            image_view
        } else {
            editor_view.into()
        };

        let content = column![toolbar, main_content].height(Length::Fill);

        let backlinks_view: Element<Message, Theme, iced::Renderer> =
            views::backlinks::view(&self.vault.backlinks, self.vault.backlinks_visible);

        let layout = row![sidebar, content, backlinks_view, toc_view].height(Length::Fill);

        let mut layers = vec![
            container(layout)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(app_theme::BG_PRIMARY)),
                    ..Default::default()
                })
                .into(),
        ];

        if self.search.visible {
            layers.push(
                container(views::search::view(
                    &self.search.query,
                    &self.search.replace,
                    self.search.regex,
                    self.search.match_case,
                    self.search.match_count(),
                    &self.search.results,
                    &self.search.pdf_results,
                    self.search.pdf_error.as_deref(),
                    true,
                ))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgba(
                        0.0, 0.0, 0.0, 0.6,
                    ))),
                    ..Default::default()
                })
                .into(),
            );
        }

        if self.ui.command_palette_visible {
            layers.push(
                container(views::command_palette::view(
                    &self.ui.command_palette_query,
                    &self.ui.commands,
                ))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgba(
                        0.0, 0.0, 0.0, 0.58,
                    ))),
                    ..Default::default()
                })
                .into(),
            );
        }

        if let Some(modal_type) = &self.ui.active_modal {
            layers.push(views::modals::view(
                modal_type,
                &self.ui.modal_input,
                &self.ui.link_note_picker_search,
                &self.vault.entries,
            ));
        }

        if self.tracker.visible {
            layers.push(
                container(self.tracker.view())
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(28)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgba(
                        0.0, 0.0, 0.0, 0.55,
                    ))),
                    ..Default::default()
                })
                .into(),
            );
        }

        if let Some(preview_handle) = &self.pdf.link_preview {
            let img = iced::widget::image(preview_handle.clone())
                .width(Length::Fill)
                .content_fit(iced::ContentFit::Contain);

            let modal = container(
                iced::widget::column![
                    iced::widget::row![
                        Space::new().width(Length::Fill),
                        iced::widget::button("✕")
                            .on_press(Message::ClosePdfLinkPreview)
                            .padding(8)
                    ],
                    container(img)
                        .width(Length::Fixed(800.0))
                        .height(Length::Fixed(600.0))
                        .style(|_| container::Style {
                            background: Some(iced::Background::Color(iced::Color::WHITE)),
                            border: iced::Border {
                                radius: 8.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        })
                ]
                .align_x(Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgba(
                    0.0, 0.0, 0.0, 0.8,
                ))),
                ..Default::default()
            });
            layers.push(modal.into());
        }

        if let Some(msg) = &self.ui.toast {
            layers.push(
                container(views::toast::view(msg))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::Center)
                    .align_y(iced::alignment::Vertical::Bottom)
                    .padding(40)
                    .into(),
            );
        }

        stack(layers).into()
    }

    fn open_vault(&mut self, path: &str) -> Task<Message> {
        self.vault.root = Some(path.to_string());
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_vault", path);
        // Publish the root immediately so file opens resolve correctly, and
        // show the tree right away from a cheap directory listing.
        if let Ok(mut vault_root) = self.state.vault_root.lock() {
            vault_root.replace(std::path::PathBuf::from(path));
        }
        self.vault.entries = md_editor_core::vault::list_vault(&self.state).unwrap_or_default();

        // Build the full-text and backlink indexes off the UI thread; a large
        // vault must not freeze startup. `set_vault_root` does its disk I/O
        // lock-free, so the UI stays responsive while it runs.
        let state = self.state.clone();
        let path = path.to_string();
        Task::perform(
            async move { md_editor_core::vault::set_vault_root(&state, &path).unwrap_or_default() },
            Message::VaultIndexed,
        )
    }

    fn new_entry_path(&self, name: &str) -> String {
        let parent = self.vault.selected_path.as_deref().and_then(|path| {
            if self
                .vault
                .entries
                .iter()
                .any(|entry| entry.path == path && entry.is_dir)
            {
                Some(path.to_string())
            } else {
                std::path::Path::new(path).parent().and_then(|p| {
                    let parent = p.to_string_lossy().replace('\\', "/");
                    if parent.is_empty() {
                        None
                    } else {
                        Some(parent)
                    }
                })
            }
        });

        parent
            .map(|dir| format!("{}/{}", dir.trim_end_matches('/'), name))
            .unwrap_or_else(|| name.to_string())
    }

    fn open_file(&mut self, path: &str) -> Task<Message> {
        self.open_file_extended(path, true)
    }

    fn open_file_extended(&mut self, path: &str, reset_scroll: bool) -> Task<Message> {
        let is_different = self.active_path.as_deref() != Some(path);
        if let Ok(bytes) = md_editor_core::vault::open_file(&self.state, path) {
            if let Ok(content) = String::from_utf8(bytes) {
                self.editor.buffer = DocBuffer::from_text(&content);
                self.editor.buffer_revision = self.editor.buffer_revision.wrapping_add(1);
                self.active_path = Some(path.to_string());
                let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
                self.active_image_path = None;
                self.active_image = None;
                self.showing_pdf = false;
                self.active_panel = ActivePanel::Markdown;
                self.editor.toc_entries = views::toc::get_toc(&content);
                let highlight_task = self.refresh_highlighting_for_current_buffer(true);
                self.vault.backlinks = md_editor_core::vault::get_mixed_backlinks(&self.state, path)
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

    fn open_pdf(&mut self, path: &str) -> Task<Message> {
        let Some(abs_path) = self.resolve_active_path(path) else {
            self.ui.toast = Some("Open a vault before opening a PDF".to_string());
            return Task::none();
        };
        let path_str = abs_path.to_string_lossy().to_string();
        self.pdf.active_path = Some(path.to_string());
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
        self.active_image_path = None;
        self.active_image = None;
        self.showing_pdf = true;
        self.active_panel = ActivePanel::Pdf;
        self.pdf.current_page = 0;
        self.pdf.fit_to_width = true;
        self.pdf.pages = Vec::new();
        self.pdf.dimensions = Vec::new();
        self.pdf.page_sizes = Vec::new();
        self.pdf.placeholder_page_size = None;
        self.pdf.pending_pages.clear();
        self.pdf.pending_links.clear();
        self.pdf.page_links.clear();
        self.search.pdf_results.clear();
        self.search.pdf_indices_by_page.clear();
        self.search.pdf_error = None;
        self.pdf.programmatic_scroll = false;
        self.pdf.toc_target_page = None;
        self.pdf.render_generation = self.pdf.render_generation.wrapping_add(1);
        let generation = self.pdf.render_generation;

        // Reset PDF study state
        self.pdf.document_id = None;
        self.pdf.page_text.clear();
        self.pdf.selection = None;
        self.pdf.annotations.clear();
        self.pdf.focused_annotation_id = None;
        self.pdf.pending_text.clear();
        self.pdf.text_lru.clear();
        self.vault.backlinks =
            md_editor_core::vault::get_mixed_backlinks(&self.state, path).unwrap_or_default();

        let path_for_hash = path.to_string();
        let abs_path_for_hash = abs_path.clone();
        let hash_task = Task::perform(
            async move {
                match md_editor_core::pdf::compute_provisional_id(&abs_path_for_hash) {
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
                    let renderer = _state.pdf_renderer.as_ref()?;
                    renderer.page_count(&path_clone).ok()
                },
                move |res| Message::PdfLoaded(generation, res.unwrap_or(0)),
            ),
            Task::perform(
                async move {
                    let renderer = _state_sizes.pdf_renderer.as_ref()?;
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
                    let renderer = _state_toc.pdf_renderer.as_ref()?;
                    renderer.get_toc(&path_str_toc).ok()
                },
                move |res| Message::PdfTocLoaded(generation, res.unwrap_or_default()),
            ),
        ])
    }

    fn open_image(&mut self, path: &str) -> Task<Message> {
        let Some(abs_path) = self.resolve_active_path(path) else {
            self.ui.toast = Some("Open a vault before opening an image".to_string());
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
                self.active_image_path = Some(path.to_string());
                let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
                self.active_image = Some((handle, width as f32, height as f32));
                self.active_path = None;
                self.pdf.active_path = None;
                self.showing_pdf = false;
                self.active_panel = ActivePanel::Markdown;
                self.editor.toc_entries.clear();
                self.vault.backlinks.clear();
            }
            Err(err) => {
                self.ui.toast = Some(format!("Could not open image: {err}"));
            }
        }
        Task::none()
    }

    fn render_pdf_page(&self, page: u16) -> Task<Message> {
        let Some(path) = &self.pdf.active_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        let path_str = abs_path.to_string_lossy().to_string();
        let zoom =
            md_editor_core::pdf::pdf_render_bucket(self.pdf.zoom) * PDF_RENDER_SUPERSAMPLE;
        let generation = self.pdf.render_generation;
        let _state = self.state.clone();

        Task::perform(
            async move {
                let renderer = _state.pdf_renderer.as_ref()?;
                let res = renderer.render_page(&path_str, page, zoom);
                Some((page, res))
            },
            move |res| {
                if let Some((p, Ok(img))) = res {
                    Message::PdfRendered(generation, p, img)
                } else if let Some((p, Err(err))) = res {
                    if err == "Skipped" {
                        Message::PdfRenderSkipped(generation, p)
                    } else {
                        Message::PdfRenderFailed(generation, p)
                    }
                } else {
                    Message::PdfRenderFailed(generation, page)
                }
            },
        )
    }

    fn render_pdf_page_direct(&mut self, page: u16) -> Task<Message> {
        let Some(path) = &self.pdf.active_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        let path_str = abs_path.to_string_lossy().to_string();
        let zoom =
            md_editor_core::pdf::pdf_render_bucket(self.pdf.zoom) * PDF_RENDER_SUPERSAMPLE;
        let generation = self.pdf.render_generation;
        let _state = self.state.clone();
        if self
            .pdf.pages
            .get(page as usize)
            .map_or(true, |p| p.is_none())
        {
            self.pdf.pending_pages.insert(page);
        }

        Task::perform(
            async move {
                let renderer = _state.pdf_renderer.as_ref()?;
                renderer
                    .render_page_priority(&path_str, page, zoom)
                    .map_err(|e| println!("PDF PRIORITY RENDER ERROR (Page {}): {}", page, e))
                    .ok()
                    .map(|img| (page, img))
            },
            move |res| {
                if let Some((p, img)) = res {
                    Message::PdfRendered(generation, p, img)
                } else {
                    Message::PdfRenderFailed(generation, page)
                }
            },
        )
    }

    fn load_pdf_page_links(&mut self, page: u16) -> Task<Message> {
        if self.pdf.page_links.contains_key(&page) || self.pdf.pending_links.contains(&page) {
            return Task::none();
        }
        let Some(path) = &self.pdf.active_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        self.pdf.pending_links.insert(page);
        let path_str = abs_path.to_string_lossy().to_string();
        let generation = self.pdf.render_generation;
        let _state = self.state.clone();

        Task::perform(
            async move {
                let renderer = _state.pdf_renderer.as_ref()?;
                renderer.get_page_links(&path_str, page).ok()
            },
            move |res| Message::PdfPageLinksLoaded(generation, page, res.unwrap_or_default()),
        )
    }

    fn load_pdf_page_text(&mut self, page: u16) -> Task<Message> {
        if self.pdf.page_text.contains_key(&page) || self.pdf.pending_text.contains(&page) {
            return Task::none();
        }
        let Some(path) = &self.pdf.active_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        self.pdf.pending_text.insert(page);
        let path_str = abs_path.to_string_lossy().to_string();
        let generation = self.pdf.render_generation;
        let _state = self.state.clone();

        Task::perform(
            async move {
                let renderer = _state
                    .pdf_renderer
                    .as_ref()
                    .ok_or_else(|| "No PDF renderer".to_string())?;
                renderer.get_page_text(&path_str, page)
            },
            move |res| Message::PdfPageTextLoaded(generation, page, res),
        )
    }

    fn render_all_pdf_pages(&mut self) -> Task<Message> {
        self.render_visible_pdf_pages()
    }

    fn render_visible_pdf_pages(&mut self) -> Task<Message> {
        if self.pdf.total_pages == 0 {
            return Task::none();
        }
        // Estimate visible range using viewport height and page height
        let page_h = self.pdf.estimated_page_height().max(100.0);
        let viewport_h = self.ui.window_height.max(400.0);
        let pages_in_view = (viewport_h / page_h).ceil() as u16;
        let first_visible = self.pdf.current_page;
        let last_visible =
            (first_visible + pages_in_view).min(self.pdf.total_pages.saturating_sub(1));

        if let Some(path) = &self.pdf.active_path {
            if let Some(abs_path) = self.resolve_active_path(path) {
                let path_str = abs_path.to_string_lossy().to_string();
                if let Some(renderer) = self.state.pdf_renderer.as_ref() {
                    renderer.set_visible_range(first_visible, last_visible, &path_str);
                }
            }
        }

        let start = self.pdf.current_page.saturating_sub(3);
        let end =
            (self.pdf.current_page + pages_in_view + 3).min(self.pdf.total_pages.saturating_sub(1));
        self.render_pdf_page_range(start, end)
    }

    fn render_pdf_pages_for_viewport(
        &mut self,
        scroll_y: f32,
        viewport_height: f32,
    ) -> Task<Message> {
        if self.pdf.total_pages == 0 {
            return Task::none();
        }

        let first_visible = self.pdf.page_at_scroll(scroll_y);
        let last_visible = self.pdf.page_at_scroll(scroll_y + viewport_height);

        if let Some(path) = &self.pdf.active_path {
            if let Some(abs_path) = self.resolve_active_path(path) {
                let path_str = abs_path.to_string_lossy().to_string();
                if let Some(renderer) = self.state.pdf_renderer.as_ref() {
                    renderer.set_visible_range(first_visible, last_visible, &path_str);
                }
            }
        }

        let first = self.pdf.page_at_scroll((scroll_y - self.pdf.estimated_page_height()).max(0.0));
        let last =
            self.pdf.page_at_scroll(scroll_y + viewport_height + self.pdf.estimated_page_height());
        self.render_pdf_page_range(
            first.saturating_sub(2),
            (last + 2).min(self.pdf.total_pages.saturating_sub(1)),
        )
    }

    fn render_pdf_page_range(&mut self, start: u16, end: u16) -> Task<Message> {
        let mut tasks = Vec::new();
        for page_idx in start..=end {
            if self
                .pdf.pages
                .get(page_idx as usize)
                .map_or(true, |p| p.is_none())
                && !self.pdf.pending_pages.contains(&page_idx)
            {
                self.pdf.pending_pages.insert(page_idx);
                tasks.push(self.render_pdf_page(page_idx));
            }
            if !self.pdf.page_text.contains_key(&page_idx)
                && !self.pdf.pending_text.contains(&page_idx)
            {
                tasks.push(self.load_pdf_page_text(page_idx));
            }
        }

        Task::batch(tasks)
    }

    fn pdf_available_width(&self) -> f32 {
        let sidebar_width = if self.vault.sidebar_visible { 260.0 } else { 0.0 };
        let toc_width = if self.editor.toc_visible { 260.0 } else { 0.0 };
        let backlinks_width = if self.vault.backlinks_visible { 260.0 } else { 0.0 };
        let chrome_width = sidebar_width + toc_width + backlinks_width;
        let content_width = (self.ui.window_width - chrome_width).max(320.0);

        if self.ui.split_view_active && self.active_path.is_some() && self.pdf.active_path.is_some() {
            (content_width * (1.0 - self.ui.split_ratio)).max(280.0)
        } else {
            content_width
        }
    }

    fn annotation_at(
        &self,
        page_idx: u16,
        x: f32,
        y: f32,
    ) -> Option<md_editor_core::pdf::PdfAnnotation> {
        let page_text = self.pdf.page_text.get(&page_idx)?;
        let px = x * page_text.page_width;
        let py = y * page_text.page_height;

        let page_anns = self.pdf.annotations.get(&page_idx)?;
        for ann in page_anns {
            for rect in &ann.rects {
                let view_y = page_text.page_height - rect.y - rect.height;
                if px >= rect.x
                    && px <= rect.x + rect.width
                    && py >= view_y
                    && py <= view_y + rect.height
                {
                    return Some(ann.clone());
                }
            }
        }
        None
    }

    fn resolve_active_path(&self, path: &str) -> Option<std::path::PathBuf> {
        let root = self.vault.root.as_deref()?;
        Some(md_editor_core::vault::resolve_vault_path(
            std::path::Path::new(root),
            path,
        ))
    }

    fn default_pdf_note_path(&self, ann: &md_editor_core::pdf::PdfAnnotation) -> String {
        let pdf_filename = self
            .pdf.active_path
            .as_deref()
            .and_then(|pdf_path| std::path::Path::new(pdf_path).file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("document");
        let clean_pdf_name = slug_fragment(pdf_filename);
        format!(
            "pdf-notes/{}-p{}-{}.md",
            clean_pdf_name,
            ann.page_index + 1,
            &ann.id[..8.min(ann.id.len())]
        )
    }

    fn linked_pdf_note_file_content(
        &self,
        note_path: &str,
        pdf_path: &str,
        ann: &md_editor_core::pdf::PdfAnnotation,
    ) -> String {
        match md_editor_core::vault::open_file(&self.state, note_path)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
        {
            Some(existing) => append_linked_pdf_note_section(&existing, pdf_path, ann),
            None => new_linked_pdf_note_content(note_path, pdf_path, ann),
        }
    }

    fn highlight_all(&mut self) -> Task<Message> {
        self.refresh_highlighting_for_current_buffer(false)
    }

    fn refresh_highlighting_for_current_buffer(&mut self, opened_file: bool) -> Task<Message> {
        let text = self.editor.buffer.text();
        let line_count = self.editor.buffer.line_count();
        self.editor.highlight_generation = self.editor.highlight_generation.wrapping_add(1);
        let generation = self.editor.highlight_generation;
        self.editor.pending_highlight_generation = None;
        self.editor.pending_highlight_requested_at = None;
        self.editor.pending_highlight_text = None;

        if opened_file && line_count > HUGE_DOC_LINE_THRESHOLD {
            self.editor.highlighted_lines = plain_highlight_placeholders(&text);
            return Self::highlight_task(generation, text);
        }

        if !opened_file && line_count > LARGE_DOC_LINE_THRESHOLD {
            self.editor.pending_highlight_generation = Some(generation);
            self.editor.pending_highlight_requested_at = Some(Instant::now());
            self.editor.pending_highlight_text = Some(text);
            return Task::none();
        }

        self.editor.highlighted_lines = highlight::highlight_markdown(&text);
        self.load_images();
        self.load_math()
    }

    fn highlight_task(generation: u64, text: String) -> Task<Message> {
        Task::perform(
            async move { highlight::highlight_markdown(&text) },
            move |lines| Message::HighlightReady(generation, lines),
        )
    }


    fn navigate_file_search(&mut self, forward: bool) -> Task<Message> {
        self.search.ensure_matches(
            &self.editor.buffer,
            self.active_path.as_deref(),
            self.editor.buffer_revision,
        );
        let matches = self.search.matches().to_vec();
        if matches.is_empty() {
            self.search.match_index = None;
            return Task::none();
        }

        let next_index = match self.search.match_index {
            Some(index) if forward => (index + 1) % matches.len(),
            Some(0) if !forward => matches.len() - 1,
            Some(index) => index.saturating_sub(1),
            None if forward => 0,
            None => matches.len() - 1,
        };
        self.search.match_index = Some(next_index);
        let item = matches[next_index];
        self.editor.buffer.execute(EditorCommand::SetSelection {
            anchor_line: item.line,
            anchor_col: item.start_col,
            focus_line: item.line,
            focus_col: item.end_col,
        });
        self.scroll_editor_to_line(item.line)
    }

    fn navigate_pdf_search(&mut self, forward: bool) -> Task<Message> {
        if self.search.pdf_results.is_empty() {
            self.search.match_index = None;
            return Task::none();
        }

        let next_index = match self.search.match_index {
            Some(index) if forward => (index + 1) % self.search.pdf_results.len(),
            Some(0) if !forward => self.search.pdf_results.len() - 1,
            Some(index) => index.saturating_sub(1),
            None if forward => 0,
            None => self.search.pdf_results.len() - 1,
        };
        self.navigate_pdf_search_to_index(next_index)
    }

    fn navigate_pdf_search_to_index(&mut self, index: usize) -> Task<Message> {
        let Some(result) = self.search.pdf_results.get(index).cloned() else {
            self.search.match_index = None;
            return Task::none();
        };

        self.search.match_index = Some(index);
        let target_page = result
            .page_index
            .min(self.pdf.total_pages.saturating_sub(1));
        self.pdf.current_page = target_page;
        self.pdf.programmatic_scroll = true;
        self.pdf.toc_target_page = None;

        let scroll_y = self.pdf.search_match_scroll_y(&result);
        if let Some(path) = &self.pdf.active_path {
            if let Some(abs_path) = self.resolve_active_path(path) {
                let path_str = abs_path.to_string_lossy().to_string();
                if let Some(renderer) = self.state.pdf_renderer.as_ref() {
                    renderer.set_visible_range(
                        target_page.saturating_sub(1),
                        (target_page + 1).min(self.pdf.total_pages.saturating_sub(1)),
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

    fn navigate_pdf_page(&mut self, page: u16) -> Task<Message> {
        let target_page = page.min(self.pdf.total_pages.saturating_sub(1));
        self.pdf.current_page = target_page;
        self.pdf.pending_pages.clear();
        self.pdf.pending_links.clear();
        self.pdf.render_generation = self.pdf.render_generation.wrapping_add(1);
        self.pdf.toc_target_page = Some(target_page);

        let target_dimensions_ready = self
            .pdf.dimensions
            .get(target_page as usize)
            .and_then(|d| *d)
            .is_some();
        let target_image_ready = self
            .pdf.pages
            .get(target_page as usize)
            .is_some_and(|page| page.is_some());

        let mut tasks = Vec::new();
        if target_image_ready && target_dimensions_ready {
            tasks.push(self.load_pdf_page_links(target_page));
        } else {
            tasks.push(self.render_pdf_page_direct(target_page));
        }

        self.pdf.programmatic_scroll = true;
        let scroll_y = self.pdf.page_offset(target_page);
        tasks.push(operation::scroll_to(
            iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
            AbsoluteOffset {
                x: 0.0,
                y: scroll_y,
            },
        ));
        Task::batch(tasks)
    }

    fn estimated_editor_viewport_width(&self) -> f32 {
        let sidebar_width = if self.vault.sidebar_visible { 260.0 } else { 0.0 };
        let toc_width = if self.editor.toc_visible { 260.0 } else { 0.0 };
        let backlinks_width = if self.vault.backlinks_visible { 260.0 } else { 0.0 };
        let chrome_width = sidebar_width + toc_width + backlinks_width;
        let content_width = (self.ui.window_width - chrome_width).max(320.0);

        if self.ui.split_view_active && self.active_path.is_some() && self.pdf.active_path.is_some() {
            (content_width * self.ui.split_ratio).max(280.0)
        } else {
            content_width
        }
    }

    fn estimated_editor_viewport_height(&self) -> f32 {
        let mut height = self.ui.window_height - 48.0; // toolbar ~48px
        if self.search.file_visible && self.active_path.is_some() {
            height -= 40.0; // search bar ~40px
        }
        height.max(200.0)
    }

    fn estimated_editor_line_y(&self, target_line: usize) -> f32 {
        crate::editor::renderer::line_visual_y::<iced::Renderer>(
            &self.editor.highlighted_lines,
            &self.editor.image_cache,
            &self.editor.math_cache,
            self.estimated_editor_viewport_width().max(240.0),
            self.editor.buffer.cursor_line,
            self.editor.buffer.cursor_col,
            target_line,
            true,
        ) + 20.0
    }

    fn restore_scroll_positions(&self) -> Task<Message> {
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

    fn pdf_search_is_active(&self) -> bool {
        self.search.file_visible
            && self.pdf.active_path.is_some()
            && (self.showing_pdf
                || (self.ui.split_view_active
                    && self.active_path.is_some()
                    && self.active_panel == ActivePanel::Pdf))
    }

    fn editor_search_is_active(&self) -> bool {
        self.search.file_visible
            && self.active_path.is_some()
            && !self.pdf_search_is_active()
            && (!self.ui.split_view_active || self.active_panel == ActivePanel::Markdown)
    }

    fn pdf_copy_shortcut_is_active(&self) -> bool {
        self.pdf.selection.is_some()
            && self.pdf.active_path.is_some()
            && (self.showing_pdf
                || (self.ui.split_view_active
                    && self.active_path.is_some()
                    && self.active_panel == ActivePanel::Pdf))
    }

    fn scroll_editor_to_line(&self, line: usize) -> Task<Message> {
        let y = self.estimated_editor_line_y(line);
        let viewport_height = self.estimated_editor_viewport_height();
        // Always center the matched line in the viewport
        let target_y = (y - viewport_height / 2.0 + 18.0).max(0.0);

        Task::perform(async move { target_y }, Message::ScrollEditorToTarget)
    }

    fn replace_all_in_current_document(&mut self) -> Result<(usize, Task<Message>), String> {
        if self.active_path.is_none() {
            return Err("Open a markdown file before replacing text".to_string());
        }
        if self.search.query.is_empty() {
            return Err("Search query is empty".to_string());
        }

        let text = self.editor.buffer.text();
        let (new_text, count) = if self.search.regex {
            let re = regex::RegexBuilder::new(&self.search.query)
                .case_insensitive(!self.search.match_case)
                .build()
                .map_err(|err| format!("Invalid regex: {err}"))?;
            let count = re.find_iter(&text).count();
            (
                re.replace_all(&text, self.search.replace.as_str())
                    .to_string(),
                count,
            )
        } else if self.search.match_case {
            let count = text.match_indices(&self.search.query).count();
            (
                text.replace(&self.search.query, &self.search.replace),
                count,
            )
        } else {
            let re = regex::RegexBuilder::new(&regex::escape(&self.search.query))
                .case_insensitive(true)
                .build()
                .map_err(|err| err.to_string())?;
            let count = re.find_iter(&text).count();
            (
                re.replace_all(&text, self.search.replace.as_str())
                    .to_string(),
                count,
            )
        };

        if count > 0 {
            self.editor.buffer.set_text(&new_text);
            self.editor.toc_entries = views::toc::get_toc(&self.editor.buffer.text());
            let task = self.highlight_all();
            return Ok((count, task));
        }
        Ok((count, Task::none()))
    }

    fn search_pdf(&self) -> Task<Message> {
        let Some(path) = &self.pdf.active_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        let query = self.search.query.clone();
        let regex = self.search.regex;
        let match_case = self.search.match_case;
        let _state = self.state.clone();
        let path_str = abs_path.to_string_lossy().to_string();
        Task::perform(
            async move {
                let Some(renderer) = _state.pdf_renderer.as_ref() else {
                    return Err(
                        "PDF search is unavailable because PDFium is not loaded".to_string()
                    );
                };
                renderer.search_text(&path_str, &query, regex, match_case)
            },
            Message::PdfSearchResult,
        )
    }

    fn run_editor_command(&mut self, command: EditorCommand) -> Task<Message> {
        let keep_cursor_visible = editor_command_keeps_cursor_visible(&command);
        self.run_editor_command_with_scroll(command, keep_cursor_visible)
    }

    fn run_editor_command_with_scroll(
        &mut self,
        command: EditorCommand,
        keep_cursor_visible: bool,
    ) -> Task<Message> {
        let result = self.editor.buffer.execute(command);
        if result.text_changed {
            self.editor.buffer_revision = self.editor.buffer_revision.wrapping_add(1);
        }
        let content_task = if result.projection_changed {
            if result.text_changed {
                self.editor.toc_entries = views::toc::get_toc(&self.editor.buffer.text());
            }
            self.highlight_all()
        } else if result.text_changed {
            self.editor.toc_entries = views::toc::get_toc(&self.editor.buffer.text());
            Task::none()
        } else {
            Task::none()
        };

        if keep_cursor_visible {
            Task::batch(vec![
                content_task,
                self.scroll_editor_to_line(self.editor.buffer.cursor_line),
            ])
        } else {
            content_task
        }
    }

    fn load_images(&mut self) {
        let Some(active_path) = &self.active_path else {
            return;
        };
        let Some(vault_root) = &self.vault.root else {
            return;
        };
        let Some(base_path) = std::path::Path::new(vault_root)
            .join(active_path)
            .parent()
            .map(|path| path.to_path_buf())
        else {
            return;
        };

        for line in &self.editor.highlighted_lines {
            for span in &line.spans {
                if span.is_image {
                    if let Some(path) = &span.image_path {
                        if !self.editor.image_cache.contains_key(path) {
                            let img_path = base_path.join(path);
                            if let Ok(img) = image::open(&img_path) {
                                let (width, height) = img.dimensions();
                                let handle = iced::widget::image::Handle::from_rgba(
                                    width,
                                    height,
                                    img.into_rgba8().into_raw(),
                                );
                                self.editor.image_cache
                                    .insert(path.clone(), (handle, width as f32, height as f32));
                            }
                        }
                    }
                }
            }
        }
    }

    fn load_math(&self) -> Task<Message> {
        let mut tasks = Vec::new();
        for line in &self.editor.highlighted_lines {
            for span in &line.spans {
                if span.is_math {
                    let tex = span
                        .visible_text(false)
                        .trim_matches('$')
                        .trim()
                        .to_string();
                    if !tex.is_empty() && !self.editor.math_cache.contains_key(&tex) {
                        let tex_clone = tex.clone();
                        tasks.push(Task::perform(
                            async move { (tex_clone.clone(), Self::render_latex_task(&tex_clone)) },
                            |(t, r)| Message::MathRendered(t, r),
                        ));
                    }
                }
            }
        }
        Task::batch(tasks)
    }

    fn render_latex_task(tex: &str) -> Result<(iced::widget::image::Handle, f32, f32), String> {
        use ratex_layout::{LayoutOptions, layout, to_display_list};
        use ratex_parser::parser::parse;
        use ratex_render::{RenderOptions, render_to_png};
        use ratex_types::color::Color as RatexColor;
        use ratex_types::math_style::MathStyle;

        let options = RenderOptions {
            font_size: 24.0,
            padding: 4.0,
            background_color: RatexColor {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.0,
            },
            font_dir: String::new(),
            device_pixel_ratio: 2.0,
        };

        let layout_opts = LayoutOptions::default()
            .with_style(MathStyle::Display)
            .with_color(RatexColor {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            });

        let ast = parse(tex).map_err(|e| format!("Parse error: {}", e))?;
        let lbox = layout(&ast, &layout_opts);
        let display_list = to_display_list(&lbox);
        let bytes =
            render_to_png(&display_list, &options).map_err(|e| format!("Render error: {:?}", e))?;

        let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
        let (w, h) = img.dimensions();
        Ok((
            iced::widget::image::Handle::from_bytes(bytes),
            w as f32 / 2.0,
            h as f32 / 2.0,
        ))
    }
}

fn editor_command_keeps_cursor_visible(command: &EditorCommand) -> bool {
    matches!(
        command,
        EditorCommand::InsertText(_)
            | EditorCommand::DeleteSelection
            | EditorCommand::DeleteBackward
            | EditorCommand::DeleteForward
            | EditorCommand::MoveCursor { .. }
            | EditorCommand::SetCursor { .. }
            | EditorCommand::SetSelection { .. }
            | EditorCommand::SelectAll
            | EditorCommand::ToggleCheckbox { .. }
            | EditorCommand::FormatBold
            | EditorCommand::FormatItalic
            | EditorCommand::FormatInlineCode
            | EditorCommand::InsertLink
            | EditorCommand::Undo
            | EditorCommand::Redo
    )
}

fn plain_highlight_placeholders(text: &str) -> Vec<highlight::StyledLine> {
    text.split('\n')
        .enumerate()
        .map(|(idx, line)| {
            let mut styled = highlight::StyledLine::new();
            styled.block_id = idx;
            styled.spans.push(highlight::StyledSpan::plain(line));
            styled
        })
        .collect()
}

fn focus_file_search_input() -> Task<Message> {
    operation::focus(iced::advanced::widget::Id::new(
        views::search::FILE_SEARCH_INPUT_ID,
    ))
}

fn focus_global_search_input() -> Task<Message> {
    operation::focus(iced::advanced::widget::Id::new(
        views::search::GLOBAL_SEARCH_INPUT_ID,
    ))
}

fn focus_pdf_search_input() -> Task<Message> {
    operation::focus(iced::advanced::widget::Id::new(
        views::pdf_viewer::PDF_SEARCH_INPUT_ID,
    ))
}

fn normalize_path(path: &std::path::Path) -> String {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::Normal(c) => {
                components.push(c);
            }
            std::path::Component::CurDir => {}
            _ => {
                components.push(component.as_os_str());
            }
        }
    }
    let normalized: std::path::PathBuf = components.into_iter().collect();
    normalized.to_string_lossy().to_string()
}

fn resolve_relative_link_path(
    vault_root: Option<&str>,
    active_path: Option<&str>,
    link_path: &str,
) -> String {
    if link_path.starts_with('.') {
        if let Some(active_file) = active_path {
            let active_path_buf = std::path::Path::new(active_file);
            if let Some(parent) = active_path_buf.parent() {
                let resolved = parent.join(link_path);
                return normalize_path(&resolved);
            }
        }
    }
    // If it doesn't start with '.', check if there is an existing file relative to the active path's parent.
    if let (Some(vault), Some(active_file)) = (vault_root, active_path) {
        let active_path_buf = std::path::Path::new(active_file);
        if let Some(parent) = active_path_buf.parent() {
            let relative_candidate = parent.join(link_path);
            let abs_relative = std::path::Path::new(vault).join(&relative_candidate);
            if abs_relative.exists()
                || abs_relative.with_extension("md").exists()
                || abs_relative.with_extension("markdown").exists()
            {
                return normalize_path(&relative_candidate);
            }
        }
    }
    link_path.to_string()
}

fn slugify(s: &str) -> String {
    let mut result = String::new();
    let mut last_was_hyphen = false;
    for c in s.to_lowercase().chars() {
        if c.is_alphanumeric() || c == '_' {
            result.push(c);
            last_was_hyphen = false;
        } else if c.is_whitespace() || c == '-' {
            if !last_was_hyphen {
                result.push('-');
                last_was_hyphen = true;
            }
        }
    }
    result.trim_matches('-').to_string()
}

fn find_heading_line(text: &str, target_slug: &str) -> Option<usize> {
    for (line_idx, line_content) in text.split('\n').enumerate() {
        let trimmed = line_content.trim_start();
        if trimmed.starts_with('#') {
            let mut level = 0;
            for c in trimmed.chars() {
                if c == '#' {
                    level += 1;
                } else {
                    break;
                }
            }
            if level > 0 && level <= 6 {
                let heading_text = trimmed[level..].trim();
                if slugify(heading_text) == target_slug {
                    return Some(line_idx);
                }
            }
        }
    }
    None
}

fn find_heading_or_widget_line(
    text: &str,
    highlighted_lines: &[crate::editor::highlight::StyledLine],
    target_slug: &str,
) -> Option<usize> {
    // If target_slug is "listing-N", we also want to look for "code-N", and vice-versa
    let alternative_slug = if let Some(num_str) = target_slug.strip_prefix("listing-") {
        Some(format!("code-{}", num_str))
    } else if let Some(num_str) = target_slug.strip_prefix("code-") {
        Some(format!("listing-{}", num_str))
    } else {
        None
    };

    for (line_idx, line) in highlighted_lines.iter().enumerate() {
        for span in &line.spans {
            if let Some(ref span_id) = span.id {
                if span_id.eq_ignore_ascii_case(target_slug) {
                    return Some(line_idx);
                }
                if let Some(ref alt) = alternative_slug {
                    if span_id.eq_ignore_ascii_case(alt) {
                        return Some(line_idx);
                    }
                }
            }
        }
    }

    if let Some(line_idx) = find_heading_line(text, target_slug) {
        return Some(line_idx);
    }
    let target_slug_underscored = target_slug.replace('-', "_");

    let re_slug_str = format!(
        r#"(?i)id\s*=\s*["']{}["']|name\s*=\s*["']{}["']|\\label\s*\{{\s*{}\s*\}}|\{{\s*#\s*{}\s*\}}"#,
        regex::escape(target_slug),
        regex::escape(target_slug),
        regex::escape(target_slug),
        regex::escape(target_slug)
    );
    let re_slug = regex::Regex::new(&re_slug_str).ok()?;

    let re_under_str = format!(
        r#"(?i)id\s*=\s*["']{}["']|name\s*=\s*["']{}["']|\\label\s*\{{\s*{}\s*\}}|\{{\s*#\s*{}\s*\}}"#,
        regex::escape(&target_slug_underscored),
        regex::escape(&target_slug_underscored),
        regex::escape(&target_slug_underscored),
        regex::escape(&target_slug_underscored)
    );
    let re_under = regex::Regex::new(&re_under_str).ok()?;

    for (line_idx, line_content) in text.split('\n').enumerate() {
        if re_slug.is_match(line_content) || re_under.is_match(line_content) {
            return Some(line_idx);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify_and_find_heading_line() {
        assert_eq!(slugify("Equation 1"), "equation-1");
        assert_eq!(slugify("Header: Equation 1"), "header-equation-1");
        assert_eq!(slugify("**Bold Heading**"), "bold-heading");

        let text = "# Equation 1\nSome text\n## Header: Equation 1\nMore text\n# **Bold Heading**";
        assert_eq!(find_heading_line(text, "equation-1"), Some(0));
        assert_eq!(find_heading_line(text, "header-equation-1"), Some(2));
        assert_eq!(find_heading_line(text, "bold-heading"), Some(4));
        assert_eq!(find_heading_line(text, "not-existent"), None);
    }

    #[test]
    fn test_resolve_relative_link_path() {
        assert_eq!(
            resolve_relative_link_path(None, Some("notes/math.md"), "../science/chemistry"),
            "science/chemistry"
        );
        assert_eq!(
            resolve_relative_link_path(None, Some("notes/math.md"), "./geometry"),
            "notes/geometry"
        );
        assert_eq!(
            resolve_relative_link_path(None, None, "../science/chemistry"),
            "../science/chemistry"
        );
        assert_eq!(
            resolve_relative_link_path(None, Some("math.md"), "./geometry"),
            "geometry"
        );
    }

    #[test]
    fn test_resolve_relative_link_path_with_vault() {
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let target_dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("test_vault_{}", unique_id));
        let sub_dir = target_dir.join("subdir");
        std::fs::create_dir_all(&sub_dir).unwrap();

        let target_file = sub_dir.join("another_file.md");
        std::fs::write(&target_file, "content").unwrap();

        let vault_root = target_dir.to_str().unwrap();
        let active_path = "subdir/active.md";

        let resolved =
            resolve_relative_link_path(Some(vault_root), Some(active_path), "another_file");
        assert_eq!(resolved, "subdir/another_file");

        let _ = std::fs::remove_dir_all(&target_dir);
    }

    #[test]
    fn test_find_heading_or_widget_line() {
        let text = "Line 0\n$$E = mc^2$$ \\label{equation-1}\nLine 2\n<div id=\"figure-1\">\nLine 4\n$$E = h\\nu$$ { #equation-2 }";
        let highlighted = highlight::highlight_markdown(text);
        assert_eq!(
            find_heading_or_widget_line(text, &highlighted, "equation-1"),
            Some(1)
        );
        assert_eq!(
            find_heading_or_widget_line(text, &highlighted, "figure-1"),
            Some(3)
        );
        assert_eq!(
            find_heading_or_widget_line(text, &highlighted, "equation-2"),
            Some(5)
        );
        assert_eq!(
            find_heading_or_widget_line(text, &highlighted, "not-existent"),
            None
        );

        // Also test the dynamic numbering of figures and math equations
        let dynamic_text = "Here is an image:\n![Alt](image.png)\nAnd a math block:\n$$\nE = mc^2\n$$\nAnother image:\n![Alt2](pic.png)";
        let dyn_highlighted = highlight::highlight_markdown(dynamic_text);
        assert_eq!(
            find_heading_or_widget_line(dynamic_text, &dyn_highlighted, "figure-1"),
            Some(1)
        );
        assert_eq!(
            find_heading_or_widget_line(dynamic_text, &dyn_highlighted, "equation-1"),
            Some(3)
        );
        assert_eq!(
            find_heading_or_widget_line(dynamic_text, &dyn_highlighted, "figure-2"),
            Some(7)
        );
    }

    #[test]
    fn insert_text_keeps_cursor_visible_after_enter_at_eof() {
        assert!(editor_command_keeps_cursor_visible(
            &EditorCommand::InsertText("\n".to_string())
        ));
    }

    #[test]
    fn pdf_slot_offsets_use_fixed_placeholder_stride() {
        let slot_height = 792.0;
        let target_page = 250;

        let offset = pdf_slot_offset(target_page, slot_height);

        assert_eq!(
            offset,
            PDF_PAGE_LIST_PADDING + f32::from(target_page) * (slot_height + PDF_PAGE_SPACING)
        );
        assert_eq!(
            pdf_slot_page_at_scroll(offset, 500, slot_height),
            target_page
        );
    }

    #[test]
    fn pdf_slot_page_lookup_does_not_drift_to_later_pages() {
        let slot_height = 792.0;
        let target_page = 250;
        let offset = pdf_slot_offset(target_page, slot_height);

        assert_eq!(pdf_slot_page_at_scroll(offset, 500, slot_height), 250);
        assert_ne!(pdf_slot_page_at_scroll(offset, 500, slot_height), 400);
    }

    #[test]
    fn pdf_total_height_reserves_space_for_every_blank_page() {
        let total_pages = 500;
        let slot_height = 792.0;

        assert_eq!(
            pdf_slot_total_height(total_pages, slot_height),
            PDF_PAGE_LIST_PADDING + f32::from(total_pages) * (slot_height + PDF_PAGE_SPACING)
        );
    }

    #[test]
    fn pdf_search_scroll_targets_match_rect_not_just_page_top() {
        assert_eq!(
            crate::pdf_pane::search_match_scroll_y_from(1000.0, Some(250.0), 20.0, 792.0, 2.0, 5000.0),
            1948.0
        );
        assert_eq!(
            crate::pdf_pane::search_match_scroll_y_from(20.0, Some(780.0), 10.0, 792.0, 1.0, 5000.0),
            0.0
        );
    }

    #[test]
    fn pdf_placeholder_size_scales_with_zoom() {
        assert_eq!(
            crate::pdf_pane::placeholder_display_size_from(Some((612.0, 792.0)), None, None, 2.0),
            (1224.0, 1584.0)
        );
    }

    #[test]
    fn pdf_placeholder_prefers_first_page_size_over_rendered_dimensions() {
        assert_eq!(
            crate::pdf_pane::placeholder_display_size_from(
                Some((612.0, 792.0)),
                Some((300.0, 300.0)),
                Some((5000, 5000)),
                1.5,
            ),
            (918.0, 1188.0)
        );
    }

    #[test]
    fn default_pdf_note_path_uses_pdf_name_page_and_annotation_prefix() {
        let ann = md_editor_core::pdf::PdfAnnotation {
            id: "abcdef123456".to_string(),
            document_id: "doc".to_string(),
            page_index: 4,
            kind: md_editor_core::pdf::PdfAnnotationKind::Highlight,
            color: md_editor_core::pdf::PdfAnnotationColor::Yellow,
            selected_text: "Important field result".to_string(),
            ranges: vec![],
            rects: vec![],
            note: None,
            linked_note_path: None,
            markdown_anchor: None,
            created_at: 0,
            updated_at: 0,
        };

        let mut app = MdEditor::new().0;
        app.pdf.active_path = Some("papers/My PDF File.pdf".to_string());
        assert_eq!(
            app.default_pdf_note_path(&ann),
            "pdf-notes/my-pdf-file-p5-abcdef12.md"
        );
    }
}

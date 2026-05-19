use iced::widget::operation::{self, AbsoluteOffset};
use iced::widget::{
    Space, column, container, mouse_area, row, scrollable, stack, text, text_editor,
};
use iced::{Alignment, Element, Length, Subscription, Task, Theme};

use image::GenericImageView;
use std::collections::BTreeSet;
use std::sync::Arc;

use crate::editor::buffer::{DocBuffer, EditorCommand};
use crate::editor::highlight;
use crate::messages::{Message, Shortcut, TrackerTab};
use crate::search::DocumentMatch;
use crate::theme as app_theme;
use crate::views;
use std::collections::HashSet;

const PDF_SCROLLABLE_ID: &str = "pdf_scrollable";
const EDITOR_SCROLLABLE_ID: &str = "editor_scrollable";

pub(crate) fn is_supported_image_path(path: &str) -> bool {
    path.ends_with(".png")
        || path.ends_with(".jpg")
        || path.ends_with(".jpeg")
        || path.ends_with(".gif")
        || path.ends_with(".bmp")
        || path.ends_with(".webp")
}

pub struct MdEditor {
    state: Arc<md_editor_core::state::AppState>,
    vault_root: Option<String>,
    vault_entries: Vec<md_editor_core::types::FileEntry>,
    selected_path: Option<String>,
    active_path: Option<String>,
    expanded_folders: BTreeSet<String>,
    sidebar_visible: bool,
    backlinks_visible: bool,
    backlinks: Vec<String>,

    // Editor state
    buffer: DocBuffer,
    highlighted_lines: Vec<highlight::StyledLine>,

    // PDF state
    pdf_current_page: u16,
    pdf_total_pages: u16,
    pdf_zoom: f32,
    pdf_pages: Vec<Option<iced::widget::image::Handle>>,
    pdf_dimensions: Vec<Option<(u32, u32)>>,
    active_pdf_path: Option<String>,
    active_image_path: Option<String>,
    active_image: Option<(iced::widget::image::Handle, f32, f32)>,
    pdf_scroll_y: f32,
    pdf_page_links: std::collections::HashMap<u16, Vec<md_editor_core::pdf::LinkInfo>>,
    pdf_link_preview: Option<iced::widget::image::Handle>,
    showing_pdf: bool,
    pdf_fit_to_width: bool,

    // Study tracker
    tracker_visible: bool,
    tracker_running: bool,
    tracker_started_at: Option<std::time::Instant>,
    tracker_sessions: Vec<md_editor_core::tracker::StudySession>,
    tracker_kv: std::collections::HashMap<String, String>,
    tracker_tab: TrackerTab,
    tracker_config_json: String,
    tracker_config_content: text_editor::Content,
    tracker_manual_date: String,
    tracker_manual_hours: String,
    tracker_manual_notes: String,

    // Modal state
    active_modal: Option<views::modals::ModalType>,
    modal_input: String,

    // Command palette
    command_palette_visible: bool,
    command_palette_query: String,
    commands: Vec<views::command_palette::Command>,

    // Toast
    toast: Option<String>,

    // Search
    search_visible: bool,
    file_search_visible: bool,
    search_query: String,
    search_replace: String,
    search_regex: bool,
    search_match_case: bool,
    search_match_index: Option<usize>,
    search_results: Vec<md_editor_core::types::SearchResult>,
    pdf_search_results: Vec<md_editor_core::pdf::PdfSearchMatch>,
    pdf_search_error: Option<String>,

    // TOC
    toc_visible: bool,
    toc_entries: Vec<views::toc::TocEntry>,
    image_cache: std::collections::HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    math_cache: std::collections::HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    pdf_pending_pages: HashSet<u16>,
    pdf_render_generation: u64,
    split_view_active: bool,
    split_ratio: f32,
    is_resizing_split: bool,
    window_width: f32,
    editor_scroll_y: f32,
    editor_viewport_width: f32,
    editor_viewport_height: f32,
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
        let tracker_sessions = md_editor_core::tracker::get_sessions(&state).unwrap_or_default();
        let tracker_config_json = md_editor_core::config::get_sys_config(&state, "tracker_config")
            .ok()
            .flatten()
            .filter(|json| views::tracker::parse_config(json).is_ok())
            .unwrap_or_else(views::tracker::default_config_json);

        let mut app = Self {
            state: state.clone(),
            vault_root: None,
            vault_entries: Vec::new(),
            selected_path: None,
            active_path: None,
            expanded_folders: BTreeSet::new(),
            sidebar_visible: true,
            backlinks_visible: false,
            backlinks: Vec::new(),
            buffer: DocBuffer::new(),
            highlighted_lines: Vec::new(),
            pdf_current_page: 0,
            pdf_total_pages: 0,
            pdf_zoom: 1.5,
            pdf_pages: Vec::new(),
            pdf_dimensions: Vec::new(),
            active_pdf_path: None,
            active_image_path: None,
            active_image: None,
            pdf_scroll_y: 0.0,
            pdf_page_links: std::collections::HashMap::new(),
            pdf_link_preview: None,
            showing_pdf: false,
            pdf_fit_to_width: true,
            tracker_visible: false,
            tracker_running: false,
            tracker_started_at: None,
            tracker_sessions,
            tracker_kv: md_editor_core::tracker::get_kv(&state)
                .unwrap_or_default()
                .into_iter()
                .map(|item| (item.key, item.value))
                .collect(),
            tracker_tab: TrackerTab::Dashboard,
            tracker_config_json,
            tracker_config_content: text_editor::Content::with_text(""),
            tracker_manual_date: chrono::Local::now().format("%Y-%m-%d").to_string(),
            tracker_manual_hours: String::new(),
            tracker_manual_notes: String::new(),
            active_modal: None,
            modal_input: String::new(),
            command_palette_visible: false,
            command_palette_query: String::new(),
            commands: views::command_palette::get_commands(),
            toast: None,
            search_visible: false,
            file_search_visible: false,
            search_query: String::new(),
            search_replace: String::new(),
            search_regex: false,
            search_match_case: false,
            search_match_index: None,
            search_results: Vec::new(),
            pdf_search_results: Vec::new(),
            pdf_search_error: None,
            toc_visible: false,
            toc_entries: Vec::new(),
            image_cache: std::collections::HashMap::new(),
            math_cache: std::collections::HashMap::new(),
            pdf_pending_pages: HashSet::new(),
            pdf_render_generation: 0,
            split_view_active: false,
            split_ratio: 0.5,
            is_resizing_split: false,
            window_width: 1200.0,
            editor_scroll_y: 0.0,
            editor_viewport_width: 900.0,
            editor_viewport_height: 720.0,
        };

        app.tracker_config_content = text_editor::Content::with_text(&app.tracker_config_json);

        let mut task = Task::none();
        if let Some(path) = last_vault {
            app.open_vault(&path);
            if let Some(file_path) = last_file {
                let lower = file_path.to_lowercase();
                if lower.ends_with(".md") || lower.ends_with(".markdown") {
                    task = app.open_file(&file_path);
                } else if lower.ends_with(".pdf") {
                    app.active_pdf_path = Some(file_path.clone());
                    app.showing_pdf = true;
                    task = app.open_pdf(&file_path);
                } else if is_supported_image_path(&lower) {
                    task = app.open_image(&file_path);
                }
            }
        }

        (app, task)
    }

    pub fn title(&self) -> String {
        format!(
            "{}Md-editor — {}",
            if self.buffer.dirty { "● " } else { "" },
            self.active_path
                .as_deref()
                .or(self.active_pdf_path.as_deref())
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

        let toast = if self.toast.is_some() {
            iced::time::every(std::time::Duration::from_secs(3)).map(|_| Message::ToastHide)
        } else {
            Subscription::none()
        };

        let mouse_drag = if self.is_resizing_split {
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
                Some(Message::WindowResized(size.width))
            } else {
                None
            }
        });

        Subscription::batch(vec![keyboard, toast, mouse_drag, window_events])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
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
            Message::VaultOpened(Some(path)) => {
                self.open_vault(&path);
                Task::none()
            }
            Message::SidebarToggle => {
                self.sidebar_visible = !self.sidebar_visible;
                Task::none()
            }
            Message::SidebarFileClicked(path) => {
                self.selected_path = Some(path.clone());
                let lower = path.to_lowercase();
                if lower.ends_with(".md") || lower.ends_with(".markdown") {
                    self.showing_pdf = false;
                    self.open_file(&path)
                } else if lower.ends_with(".pdf") {
                    self.active_pdf_path = Some(path.clone());
                    self.showing_pdf = true;
                    self.open_pdf(&path)
                } else if is_supported_image_path(&lower) {
                    self.open_image(&path)
                } else {
                    Task::none()
                }
            }
            Message::SidebarFolderToggled(path) => {
                if self.expanded_folders.contains(&path) {
                    self.expanded_folders.remove(&path);
                } else {
                    self.expanded_folders.insert(path);
                }
                Task::none()
            }
            Message::CreateFileDialog => {
                self.active_modal = Some(views::modals::ModalType::CreateFile);
                self.modal_input.clear();
                Task::none()
            }
            Message::CreateFolderDialog => {
                self.active_modal = Some(views::modals::ModalType::CreateFolder);
                self.modal_input.clear();
                Task::none()
            }
            Message::DeleteFileDialog(path) => {
                self.active_modal = Some(views::modals::ModalType::Delete(path));
                Task::none()
            }
            Message::NameModalInputChanged(input) => {
                self.modal_input = input;
                Task::none()
            }
            Message::NameModalCancel => {
                self.active_modal = None;
                self.modal_input.clear();
                Task::none()
            }
            Message::NameModalSubmitCurrent => {
                if matches!(
                    self.active_modal,
                    Some(views::modals::ModalType::CreateFile)
                        | Some(views::modals::ModalType::CreateFolder)
                ) {
                    Task::done(Message::NameModalSubmit(self.modal_input.clone()))
                } else {
                    Task::none()
                }
            }
            Message::NameModalSubmit(input) => {
                let name = input.trim();
                if name.is_empty() {
                    self.toast = Some("Name cannot be empty".to_string());
                    return Task::none();
                }

                let target_path = self.new_entry_path(name);
                let result = match self.active_modal.as_ref() {
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
                        self.vault_entries =
                            md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                        self.active_modal = None;
                        self.modal_input.clear();
                        self.toast = Some("Created".to_string());
                    }
                    Err(err) => self.toast = Some(err),
                }
                Task::none()
            }
            Message::DeleteFile(path) => {
                match md_editor_core::vault::delete_entry(&self.state, &path) {
                    Ok(()) => {
                        self.vault_entries =
                            md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                        if self.active_path.as_deref() == Some(path.as_str()) {
                            self.active_path = None;
                            self.buffer = DocBuffer::new();
                            self.highlighted_lines.clear();
                        }
                        if self.active_pdf_path.as_deref() == Some(path.as_str()) {
                            self.active_pdf_path = None;
                            self.pdf_pages.clear();
                            self.pdf_dimensions.clear();
                        }
                        self.active_modal = None;
                        self.toast = Some("Deleted".to_string());
                    }
                    Err(err) => self.toast = Some(err),
                }
                Task::none()
            }

            Message::EditorCommand(command) => self.run_editor_command(command),
            Message::MathRendered(tex, res) => {
                if let Ok(tuple) = res {
                    self.math_cache.insert(tex, tuple);
                }
                Task::none()
            }
            Message::EditorSave => {
                if let Some(path) = &self.active_path {
                    let content = self.buffer.text();
                    let _ = md_editor_core::vault::save_file(&self.state, path, &content);
                    self.buffer.dirty = false;
                    self.toast = Some("File saved".to_string());
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
                self.editor_scroll_y = y;
                self.editor_viewport_width = viewport_width;
                self.editor_viewport_height = viewport_height;
                Task::none()
            }
            Message::ScrollEditorToTarget(target_y) => operation::scroll_to(
                iced::advanced::widget::Id::new(EDITOR_SCROLLABLE_ID),
                AbsoluteOffset {
                    x: 0.0,
                    y: target_y,
                },
            ),

            Message::PdfLoaded(generation, pages) => {
                if generation != self.pdf_render_generation {
                    return Task::none();
                }
                self.pdf_total_pages = pages;
                self.pdf_pages = vec![None; pages as usize];
                self.pdf_dimensions = vec![None; pages as usize];
                self.pdf_pending_pages.clear();
                if pages == 0 {
                    self.toast = Some(
                        "PDF renderer is unavailable or the PDF could not be opened".to_string(),
                    );
                }
                if self.pdf_fit_to_width {
                    Task::done(Message::PdfFitToWidth)
                } else {
                    self.render_all_pdf_pages()
                }
            }
            Message::PdfZoomChanged(zoom) => {
                self.pdf_fit_to_width = false;
                self.pdf_zoom = zoom;
                self.pdf_pages = vec![None; self.pdf_total_pages as usize];
                self.pdf_dimensions = vec![None; self.pdf_total_pages as usize];
                self.pdf_pending_pages.clear();
                self.pdf_render_generation = self.pdf_render_generation.wrapping_add(1);
                self.render_visible_pdf_pages()
            }
            Message::PdfFitToWidth => {
                self.pdf_fit_to_width = true;
                let available_width = self.pdf_available_width();
                let page_width = self
                    .pdf_dimensions
                    .iter()
                    .flatten()
                    .next()
                    .map(|(w, _)| (*w as f32 / self.pdf_zoom).max(1.0))
                    .unwrap_or(612.0);
                let next_zoom = ((available_width - 48.0).max(240.0) / page_width).clamp(0.5, 4.0);
                self.pdf_zoom = (next_zoom * 100.0).round() / 100.0;
                self.pdf_pages = vec![None; self.pdf_total_pages as usize];
                self.pdf_dimensions = vec![None; self.pdf_total_pages as usize];
                self.pdf_pending_pages.clear();
                self.pdf_render_generation = self.pdf_render_generation.wrapping_add(1);
                self.render_visible_pdf_pages()
            }
            Message::PdfRendered(generation, page, img) => {
                if generation != self.pdf_render_generation {
                    return Task::none();
                }
                let (width, height) = img.dimensions();
                let handle = iced::widget::image::Handle::from_rgba(
                    width,
                    height,
                    img.into_rgba8().into_raw(),
                );
                if (page as usize) < self.pdf_pages.len() {
                    self.pdf_pages[page as usize] = Some(handle);
                    self.pdf_dimensions[page as usize] = Some((width, height));
                }
                self.pdf_pending_pages.remove(&page);
                Task::none()
            }
            Message::PdfRenderFailed(generation, page) => {
                if generation != self.pdf_render_generation {
                    return Task::none();
                }
                self.pdf_pending_pages.remove(&page);
                self.toast = Some(format!("Could not render PDF page {}", page + 1));
                Task::none()
            }
            Message::TocClicked(index) => {
                if self.active_pdf_path.is_some() {
                    self.pdf_current_page = index as u16;
                    let target_page = self.pdf_current_page;
                    let scroll_y = self.pdf_page_offset(target_page);
                    let start = target_page.saturating_sub(4);
                    let end = (target_page + 10).min(self.pdf_total_pages.saturating_sub(1));
                    Task::batch(vec![
                        self.render_pdf_page_direct(target_page),
                        self.render_pdf_page_range(start, end),
                        operation::scroll_to(
                            iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                            AbsoluteOffset {
                                x: 0.0,
                                y: scroll_y,
                            },
                        ),
                    ])
                } else {
                    Task::done(Message::EditorCursorMove(index, 0))
                }
            }
            Message::PdfScrolled { y, viewport_height } => {
                self.pdf_scroll_y = y;
                let new_page = self.pdf_page_at_scroll(y + viewport_height * 0.33);
                if new_page != self.pdf_current_page && new_page < self.pdf_total_pages {
                    if new_page.abs_diff(self.pdf_current_page) > 8 {
                        self.pdf_pending_pages.clear();
                    }
                    self.pdf_current_page = new_page;
                    self.render_pdf_pages_for_viewport(y, viewport_height)
                } else {
                    self.render_pdf_pages_for_viewport(y, viewport_height)
                }
            }
            Message::PdfLeftClicked(page_idx, x, y) => {
                if let Some(link) = self.pdf_link_at(page_idx, x, y) {
                    if let Some(dest_page) = link.dest_page {
                        self.pdf_current_page =
                            dest_page.min(u32::from(self.pdf_total_pages.saturating_sub(1))) as u16;
                        let scroll_y = self.pdf_page_offset(self.pdf_current_page);
                        Task::batch(vec![
                            self.render_visible_pdf_pages(),
                            operation::scroll_to(
                                iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                                AbsoluteOffset {
                                    x: 0.0,
                                    y: scroll_y,
                                },
                            ),
                        ])
                    } else if let Some(uri) = link.uri {
                        self.toast = Some(format!("External link: {}", uri));
                        Task::none()
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
            Message::PdfRightClicked(page_idx, x, y) => {
                if let Some(link) = self
                    .pdf_link_at(page_idx, x, y)
                    .filter(|link| link.dest_page.is_some())
                {
                    let Some(dest_page) = link.dest_page else {
                        return Task::none();
                    };
                    let dest_y = link.dest_y;
                    let Some(path) = self.active_pdf_path.clone() else {
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
                    self.pdf_link_preview = Some(iced::widget::image::Handle::from_rgba(
                        width,
                        height,
                        img.into_rgba8().into_raw(),
                    ));
                }
                Task::none()
            }
            Message::PdfLinkPreviewResult(Err(e)) => {
                self.toast = Some(format!("Preview Error: {}", e));
                Task::none()
            }
            Message::ClosePdfLinkPreview => {
                self.pdf_link_preview = None;
                Task::none()
            }
            Message::PdfTocLoaded(generation, entries) => {
                if generation != self.pdf_render_generation {
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
                self.toc_entries = mapped;
                Task::none()
            }
            Message::PdfPageLinksLoaded(page, links) => {
                self.pdf_page_links.insert(page, links);
                Task::none()
            }

            Message::TrackerToggle => {
                self.tracker_visible = !self.tracker_visible;
                if self.tracker_visible {
                    self.tracker_kv = md_editor_core::tracker::get_kv(&self.state)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|item| (item.key, item.value))
                        .collect();
                    self.tracker_config_json =
                        md_editor_core::config::get_sys_config(&self.state, "tracker_config")
                            .ok()
                            .flatten()
                            .filter(|json| views::tracker::parse_config(json).is_ok())
                            .unwrap_or_else(views::tracker::default_config_json);
                    self.tracker_config_content =
                        text_editor::Content::with_text(&self.tracker_config_json);
                }
                Task::none()
            }
            Message::CommandPaletteOpen => {
                self.command_palette_visible = true;
                self.command_palette_query.clear();
                Task::none()
            }
            Message::CommandPaletteQueryChanged(query) => {
                self.command_palette_query = query;
                Task::none()
            }
            Message::CommandPaletteCommandClicked(shortcut) => {
                self.command_palette_visible = false;
                self.command_palette_query.clear();
                Task::done(Message::KeyboardShortcut(shortcut))
            }
            Message::TrackerStart => {
                self.tracker_running = true;
                self.tracker_started_at = Some(std::time::Instant::now());
                self.toast = Some("Study timer started".to_string());
                Task::none()
            }
            Message::TrackerStop => {
                if let Some(started_at) = self.tracker_started_at.take() {
                    let elapsed = started_at.elapsed();
                    let hours = (elapsed.as_secs_f32() / 3600.0).max(0.01);
                    let session = md_editor_core::tracker::StudySession {
                        id: 0,
                        date: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
                        hours,
                        activity_type: "Study".to_string(),
                        phase: "Focus".to_string(),
                        notes: None,
                    };
                    if md_editor_core::tracker::save_session(&self.state, session).is_ok() {
                        self.tracker_sessions =
                            md_editor_core::tracker::get_sessions(&self.state).unwrap_or_default();
                        self.toast = Some("Study session saved".to_string());
                    }
                }
                self.tracker_running = false;
                Task::none()
            }
            Message::TrackerTabSelected(tab) => {
                self.tracker_tab = tab;
                Task::none()
            }
            Message::TrackerProjectStatusChanged(id, status) => {
                let key = format!("proj_{}", id);
                if md_editor_core::tracker::set_kv(&self.state, &key, &status).is_ok() {
                    self.tracker_kv.insert(key, status);
                }
                Task::none()
            }
            Message::TrackerGateToggled(gate_id, item_idx) => {
                let key = format!("gate_{}_{}", gate_id, item_idx);
                let next = if self
                    .tracker_kv
                    .get(&key)
                    .map(|v| v == "true")
                    .unwrap_or(false)
                {
                    "false"
                } else {
                    "true"
                };
                if md_editor_core::tracker::set_kv(&self.state, &key, next).is_ok() {
                    self.tracker_kv.insert(key, next.to_string());
                }
                Task::none()
            }
            Message::TrackerReadingToggled(section, item_idx) => {
                let key = format!("read_{}_{}", section, item_idx);
                let next = if self
                    .tracker_kv
                    .get(&key)
                    .map(|v| v == "true")
                    .unwrap_or(false)
                {
                    "false"
                } else {
                    "true"
                };
                if md_editor_core::tracker::set_kv(&self.state, &key, next).is_ok() {
                    self.tracker_kv.insert(key, next.to_string());
                }
                Task::none()
            }
            Message::TrackerConfigEdited(action) => {
                self.tracker_config_content.perform(action);
                self.tracker_config_json = self.tracker_config_content.text();
                Task::none()
            }
            Message::TrackerConfigSave => {
                match views::tracker::parse_config(&self.tracker_config_json) {
                    Ok(_) => {
                        if md_editor_core::config::set_sys_config(
                            &self.state,
                            "tracker_config",
                            &self.tracker_config_json,
                        )
                        .is_ok()
                        {
                            self.toast = Some("Tracker configuration saved".to_string());
                        }
                    }
                    Err(err) => self.toast = Some(format!("Invalid tracker JSON: {}", err)),
                }
                Task::none()
            }
            Message::TrackerManualDateChanged(value) => {
                self.tracker_manual_date = value;
                Task::none()
            }
            Message::TrackerManualHoursChanged(value) => {
                self.tracker_manual_hours = value;
                Task::none()
            }
            Message::TrackerManualNotesChanged(value) => {
                self.tracker_manual_notes = value;
                Task::none()
            }
            Message::TrackerManualAdd => {
                match self.tracker_manual_hours.trim().parse::<f32>() {
                    Ok(hours) if hours > 0.0 => {
                        let session = md_editor_core::tracker::StudySession {
                            id: 0,
                            date: self.tracker_manual_date.trim().to_string(),
                            hours,
                            activity_type: "Manual".to_string(),
                            phase: "Manual".to_string(),
                            notes: (!self.tracker_manual_notes.trim().is_empty())
                                .then(|| self.tracker_manual_notes.trim().to_string()),
                        };
                        match md_editor_core::tracker::save_session(&self.state, session) {
                            Ok(()) => {
                                self.tracker_sessions =
                                    md_editor_core::tracker::get_sessions(&self.state)
                                        .unwrap_or_default();
                                self.tracker_manual_hours.clear();
                                self.tracker_manual_notes.clear();
                                self.toast = Some("Manual study session added".to_string());
                            }
                            Err(err) => self.toast = Some(err),
                        }
                    }
                    _ => self.toast = Some("Enter a positive hour value".to_string()),
                }
                Task::none()
            }
            Message::TrackerSessionDelete(id) => {
                match md_editor_core::tracker::delete_session(&self.state, id) {
                    Ok(()) => {
                        self.tracker_sessions =
                            md_editor_core::tracker::get_sessions(&self.state).unwrap_or_default();
                        self.toast = Some("Session deleted".to_string());
                    }
                    Err(err) => self.toast = Some(err),
                }
                Task::none()
            }

            Message::GlobalSearchOpen => {
                self.search_visible = true;
                if self.active_pdf_path.is_some() && !self.search_query.trim().is_empty() {
                    self.search_pdf()
                } else {
                    Task::none()
                }
            }
            Message::SearchClose => {
                self.search_visible = false;
                self.file_search_visible = false;
                Task::none()
            }
            Message::SearchQueryChanged(q) => {
                self.search_query = q.clone();
                self.search_match_index = None;
                self.pdf_search_error = None;
                if q.len() > 2 && !self.search_regex {
                    if let Ok(res) = md_editor_core::vault::search_vault(&self.state, &q) {
                        self.search_results = res;
                    }
                } else {
                    self.search_results.clear();
                }
                if self.active_pdf_path.is_some() && q.len() > 1 {
                    self.search_pdf()
                } else {
                    self.pdf_search_results.clear();
                    Task::none()
                }
            }
            Message::SearchReplaceChanged(replace) => {
                self.search_replace = replace;
                Task::none()
            }
            Message::SearchRegexToggled(value) => {
                self.search_regex = value;
                self.search_match_index = None;
                if self.active_pdf_path.is_some() && self.search_query.len() > 1 {
                    self.search_pdf()
                } else {
                    Task::none()
                }
            }
            Message::SearchMatchCaseToggled(value) => {
                self.search_match_case = value;
                self.search_match_index = None;
                if self.active_pdf_path.is_some() && self.search_query.len() > 1 {
                    self.search_pdf()
                } else {
                    Task::none()
                }
            }
            Message::SearchPrevious => {
                if self.file_search_visible && self.active_pdf_path.is_some() && self.showing_pdf {
                    self.navigate_pdf_search(false)
                } else {
                    self.navigate_file_search(false)
                }
            }
            Message::SearchNext => {
                if self.file_search_visible && self.active_pdf_path.is_some() && self.showing_pdf {
                    self.navigate_pdf_search(true)
                } else {
                    self.navigate_file_search(true)
                }
            }
            Message::SearchReplaceAll => {
                match self.replace_all_in_current_document() {
                    Ok(count) => self.toast = Some(format!("Replaced {} matches", count)),
                    Err(err) => self.toast = Some(err),
                }
                Task::none()
            }
            Message::PdfSearchResult(Ok(results)) => {
                self.pdf_search_error = None;
                self.pdf_search_results = results;
                if self
                    .search_match_index
                    .is_some_and(|index| index >= self.pdf_search_results.len())
                {
                    self.search_match_index = None;
                }
                if self.file_search_visible
                    && self.active_pdf_path.is_some()
                    && self.showing_pdf
                    && !self.pdf_search_results.is_empty()
                {
                    self.navigate_pdf_search_to_index(self.search_match_index.unwrap_or(0))
                } else {
                    Task::none()
                }
            }
            Message::PdfSearchResult(Err(err)) => {
                self.pdf_search_results.clear();
                self.pdf_search_error = Some(err);
                Task::none()
            }
            Message::PdfSearchResultClicked(page) => {
                self.search_visible = false;
                self.file_search_visible = true;
                self.search_match_index = self
                    .pdf_search_results
                    .iter()
                    .position(|result| result.page_index == page);
                if let Some(index) = self.search_match_index {
                    self.navigate_pdf_search_to_index(index)
                } else {
                    self.pdf_current_page = page.min(self.pdf_total_pages.saturating_sub(1));
                    self.navigate_pdf_page(self.pdf_current_page)
                }
            }
            Message::PdfScrollBy(delta) => {
                if self.active_pdf_path.is_none()
                    || !self.showing_pdf
                    || (self.split_view_active && self.active_path.is_some())
                    || self.search_visible
                    || self.file_search_visible
                    || self.active_modal.is_some()
                    || self.command_palette_visible
                {
                    return Task::none();
                }
                let max_y = self.pdf_total_height().max(0.0);
                let y = (self.pdf_scroll_y + delta).clamp(0.0, max_y);
                operation::scroll_to(
                    iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                    AbsoluteOffset { x: 0.0, y },
                )
            }
            Message::SearchResultClicked(path) => {
                self.search_visible = false;
                self.open_file(&path)
            }

            Message::ToastHide => {
                self.toast = None;
                Task::none()
            }
            Message::KeyboardShortcut(s) => {
                match s {
                    Shortcut::Escape => {
                        // Close overlays in priority order
                        if self.pdf_link_preview.is_some() {
                            self.pdf_link_preview = None;
                        } else if self.active_modal.is_some() {
                            self.active_modal = None;
                            self.modal_input.clear();
                        } else if self.tracker_visible {
                            self.tracker_visible = false;
                        } else if self.file_search_visible {
                            self.file_search_visible = false;
                        } else if self.search_visible {
                            self.search_visible = false;
                        } else if self.command_palette_visible {
                            self.command_palette_visible = false;
                        } else if self.toc_visible {
                            self.toc_visible = false;
                        }
                        Task::none()
                    }
                    Shortcut::ToggleSidebar => {
                        self.sidebar_visible = !self.sidebar_visible;
                        Task::none()
                    }
                    Shortcut::Save => Task::done(Message::EditorSave),
                    Shortcut::OpenVault => Task::done(Message::OpenVaultDialog),
                    Shortcut::NewFile => Task::done(Message::CreateFileDialog),
                    Shortcut::Search => {
                        if self.active_pdf_path.is_some() && self.showing_pdf {
                            self.file_search_visible = true;
                            self.search_visible = false;
                            if !self.search_query.trim().is_empty() {
                                return self.search_pdf();
                            }
                        } else if self.active_path.is_some() {
                            self.file_search_visible = true;
                        } else {
                            self.search_visible = true;
                        }
                        Task::none()
                    }
                    Shortcut::CommandPalette => {
                        self.command_palette_visible = true;
                        self.command_palette_query.clear();
                        Task::none()
                    }
                    Shortcut::ToggleBacklinks => {
                        self.backlinks_visible = !self.backlinks_visible;
                        Task::none()
                    }
                    Shortcut::TableOfContents => {
                        if self.active_pdf_path.is_some()
                            && (self.showing_pdf
                                || (self.split_view_active && self.active_path.is_some()))
                        {
                            self.toc_visible = !self.toc_visible;
                        }
                        Task::none()
                    }
                    Shortcut::StudyTracker => {
                        self.tracker_visible = !self.tracker_visible;
                        Task::none()
                    }
                    Shortcut::SplitView => Task::done(Message::SplitViewToggle),
                    Shortcut::FocusMode => {
                        self.sidebar_visible = false;
                        self.backlinks_visible = false;
                        self.toc_visible = false;
                        self.tracker_visible = false;
                        Task::none()
                    }
                }
            }
            Message::SplitViewToggle => {
                if self.active_path.is_some() && self.active_pdf_path.is_some() {
                    self.split_view_active = !self.split_view_active;
                    if self.pdf_fit_to_width {
                        return Task::done(Message::PdfFitToWidth);
                    }
                } else {
                    self.toast =
                        Some("Open a markdown file and a PDF to use split view".to_string());
                }
                Task::none()
            }
            Message::SplitViewDragStart => {
                self.is_resizing_split = true;
                Task::none()
            }
            Message::SplitViewDragging(x_pos) => {
                if !self.is_resizing_split {
                    return Task::none();
                }
                let side_width = if self.sidebar_visible { 250.0 } else { 0.0 }
                    + if self.tracker_visible { 300.0 } else { 0.0 }
                    + if self.toc_visible { 250.0 } else { 0.0 };
                let content_width = (self.window_width - side_width).max(480.0);
                let x_min = side_width + 240.0;
                let x_max = side_width + content_width - 240.0;
                let total_width = x_max - x_min;
                if total_width > 1.0 {
                    self.split_ratio = ((x_pos - x_min) / total_width).clamp(0.25, 0.75);
                }
                Task::none()
            }
            Message::SplitViewDragEnd => {
                self.is_resizing_split = false;
                if self.pdf_fit_to_width && self.active_pdf_path.is_some() {
                    return Task::done(Message::PdfFitToWidth);
                }
                Task::none()
            }
            Message::WindowResized(width) => {
                self.window_width = width;
                if self.pdf_fit_to_width && self.active_pdf_path.is_some() {
                    return Task::done(Message::PdfFitToWidth);
                }
                Task::none()
            }
            Message::ToggleTOC => {
                if self.active_pdf_path.is_some()
                    && (self.showing_pdf || (self.split_view_active && self.active_path.is_some()))
                {
                    self.toc_visible = !self.toc_visible;
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    pub fn view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        if self.vault_root.is_none() {
            return views::welcome::view();
        }

        let toolbar = views::toolbar::view(
            self.active_path.as_deref(),
            self.active_pdf_path
                .as_deref()
                .or(self.active_image_path.as_deref()),
            None,
            self.sidebar_visible,
            self.backlinks_visible,
            self.tracker_visible,
            self.toc_visible,
            self.active_pdf_path.is_some()
                && (self.showing_pdf || (self.split_view_active && self.active_path.is_some())),
            self.split_view_active,
            self.active_path.is_some() && self.active_pdf_path.is_some(),
        );

        let sidebar = views::sidebar::view(
            &self.vault_entries,
            self.selected_path.as_deref(),
            self.active_path
                .as_deref()
                .or(self.active_pdf_path.as_deref())
                .or(self.active_image_path.as_deref()),
            &self.expanded_folders,
            !self.sidebar_visible,
        );

        let active_search_match = if self.file_search_visible {
            self.active_search_match_position()
        } else {
            None
        };
        let editor_search_query = if self.file_search_visible {
            self.search_query.as_str()
        } else {
            ""
        };
        let editor_scroll = scrollable(
            container(
                crate::editor::renderer::Editor::new(
                    &self.buffer,
                    &self.highlighted_lines,
                    &self.image_cache,
                    &self.math_cache,
                    Message::EditorCommand,
                    Message::SidebarFileClicked,
                    Message::EditorCheckboxToggle,
                )
                .search(
                    editor_search_query,
                    self.search_regex,
                    self.search_match_case,
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

        let editor_view: Element<Message, Theme, iced::Renderer> =
            if self.file_search_visible && self.active_path.is_some() {
                column![
                    views::search::file_bar(
                        &self.search_query,
                        &self.search_replace,
                        self.search_regex,
                        self.search_match_case,
                        self.current_document_match_count(),
                        self.search_match_index,
                    ),
                    editor_scroll
                ]
                .height(Length::Fill)
                .into()
            } else {
                editor_scroll.into()
            };

        let pdf_view: Element<Message, Theme, iced::Renderer> =
            if let Some(_) = &self.active_pdf_path {
                let pdf_toolbar = views::pdf_viewer::toolbar(
                    self.pdf_current_page,
                    self.pdf_total_pages,
                    self.pdf_zoom,
                    self.toc_visible,
                );
                let pdf_pages = scrollable(views::pdf_viewer::view_continuous(
                    &self.pdf_pages,
                    self.pdf_zoom,
                    &self.pdf_dimensions,
                    if self.file_search_visible || self.search_visible {
                        &self.pdf_search_results
                    } else {
                        &[]
                    },
                    self.search_match_index,
                ))
                .id(iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID))
                .on_scroll(|vp| Message::PdfScrolled {
                    y: vp.absolute_offset().y,
                    viewport_height: vp.bounds().height,
                })
                .height(Length::Fill);

                if self.file_search_visible && self.showing_pdf {
                    column![
                        views::pdf_viewer::search_bar(
                            &self.search_query,
                            self.search_regex,
                            self.search_match_case,
                            self.pdf_search_results.len(),
                            self.search_match_index,
                        ),
                        pdf_pages,
                        pdf_toolbar
                    ]
                    .height(Length::Fill)
                    .into()
                } else {
                    column![pdf_pages, pdf_toolbar].height(Length::Fill).into()
                }
            } else {
                container(Space::new()).width(Length::Fixed(0.0)).into()
            };

        let pdf_toc_available = self.active_pdf_path.is_some()
            && (self.showing_pdf || (self.split_view_active && self.active_path.is_some()));
        let toc_view: Element<Message, Theme, iced::Renderer> =
            if self.toc_visible && pdf_toc_available {
                views::toc::view(&self.toc_entries)
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

        let main_content: Element<Message, Theme, iced::Renderer> = if self.split_view_active
            && self.active_path.is_some()
            && self.active_pdf_path.is_some()
        {
            let left_portion = (self.split_ratio * 1000.0) as u16;
            let right_portion = ((1.0 - self.split_ratio) * 1000.0) as u16;

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
        } else if self.showing_pdf && self.active_pdf_path.is_some() {
            pdf_view
        } else if self.active_image.is_some() {
            image_view
        } else {
            editor_view.into()
        };

        let content = column![toolbar, main_content].height(Length::Fill);

        let backlinks_view: Element<Message, Theme, iced::Renderer> =
            views::backlinks::view(&self.backlinks, self.backlinks_visible);

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

        if self.search_visible {
            layers.push(
                container(views::search::view(
                    &self.search_query,
                    &self.search_replace,
                    self.search_regex,
                    self.search_match_case,
                    self.current_document_match_count(),
                    &self.search_results,
                    &self.pdf_search_results,
                    self.pdf_search_error.as_deref(),
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

        if self.command_palette_visible {
            layers.push(
                container(views::command_palette::view(
                    &self.command_palette_query,
                    &self.commands,
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

        if let Some(modal_type) = &self.active_modal {
            layers.push(views::modals::view(modal_type, &self.modal_input));
        }

        if self.tracker_visible {
            layers.push(
                container(views::tracker::view(
                    true,
                    self.tracker_running,
                    &self.tracker_sessions,
                    &self.tracker_kv,
                    self.tracker_tab,
                    &self.tracker_config_content,
                    &self.tracker_manual_date,
                    &self.tracker_manual_hours,
                    &self.tracker_manual_notes,
                ))
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

        if let Some(preview_handle) = &self.pdf_link_preview {
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

        if let Some(msg) = &self.toast {
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

    fn open_vault(&mut self, path: &str) {
        self.vault_root = Some(path.to_string());
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_vault", path);
        if let Ok(mut vault_root) = self.state.vault_root.lock() {
            vault_root.replace(std::path::PathBuf::from(path));
        }
        self.vault_entries = md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
    }

    fn new_entry_path(&self, name: &str) -> String {
        let parent = self.selected_path.as_deref().and_then(|path| {
            if self
                .vault_entries
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
        if let Ok(bytes) = md_editor_core::vault::open_file(&self.state, path) {
            if let Ok(content) = String::from_utf8(bytes) {
                self.buffer = DocBuffer::from_text(&content);
                self.active_path = Some(path.to_string());
                let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
                self.active_image_path = None;
                self.active_image = None;
                self.showing_pdf = false;
                self.toc_entries = views::toc::get_toc(&content);
                self.highlighted_lines = highlight::highlight_markdown(&content);
                self.load_images();
                let math_task = self.load_math();
                self.backlinks =
                    md_editor_core::vault::get_backlinks(&self.state, path).unwrap_or_default();
                return math_task;
            }
        }
        Task::none()
    }

    fn open_pdf(&mut self, path: &str) -> Task<Message> {
        let Some(abs_path) = self.resolve_active_path(path) else {
            self.toast = Some("Open a vault before opening a PDF".to_string());
            return Task::none();
        };
        let path_str = abs_path.to_string_lossy().to_string();
        self.active_pdf_path = Some(path.to_string());
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
        self.active_image_path = None;
        self.active_image = None;
        self.showing_pdf = true;
        self.pdf_current_page = 0;
        self.pdf_fit_to_width = true;
        self.pdf_pages = Vec::new();
        self.pdf_dimensions = Vec::new();
        self.pdf_pending_pages.clear();
        self.pdf_render_generation = self.pdf_render_generation.wrapping_add(1);
        let generation = self.pdf_render_generation;

        let _state = self.state.clone();
        let _state_toc = self.state.clone();
        let path_clone = path_str.clone();
        let path_str_toc = path_str.clone();

        Task::batch(vec![
            Task::perform(
                async move {
                    let renderer = _state.pdf_renderer.as_ref()?;
                    renderer.page_count(&path_clone).ok()
                },
                move |res| Message::PdfLoaded(generation, res.unwrap_or(0)),
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
            self.toast = Some("Open a vault before opening an image".to_string());
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
                self.active_pdf_path = None;
                self.showing_pdf = false;
                self.toc_entries.clear();
                self.backlinks.clear();
            }
            Err(err) => {
                self.toast = Some(format!("Could not open image: {err}"));
            }
        }
        Task::none()
    }

    fn render_pdf_page(&self, page: u16) -> Task<Message> {
        let Some(path) = &self.active_pdf_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        let path_str = abs_path.to_string_lossy().to_string();
        let zoom = self.pdf_zoom;
        let generation = self.pdf_render_generation;
        let _state = self.state.clone();
        let _state_links = self.state.clone();

        let render_task = Task::perform(
            async move {
                let renderer = _state.pdf_renderer.as_ref()?;
                let res = renderer
                    .render_page(&path_str, page, zoom)
                    .map_err(|e| println!("PDF RENDER ERROR (Page {}): {}", page, e))
                    .ok();
                Some((page, res))
            },
            move |res| {
                if let Some((p, Some(img))) = res {
                    Message::PdfRendered(generation, p, img)
                } else if let Some((p, None)) = res {
                    Message::PdfRenderFailed(generation, p)
                } else {
                    Message::Tick
                }
            },
        );

        let path_str_links = abs_path.to_string_lossy().to_string();
        let links_task = Task::perform(
            async move {
                let renderer = _state_links.pdf_renderer.as_ref()?;
                renderer.get_page_links(&path_str_links, page).ok()
            },
            move |res| Message::PdfPageLinksLoaded(page, res.unwrap_or_default()),
        );

        Task::batch(vec![render_task, links_task])
    }

    fn render_pdf_page_direct(&mut self, page: u16) -> Task<Message> {
        if self
            .pdf_pages
            .get(page as usize)
            .map_or(true, |p| p.is_none())
        {
            self.pdf_pending_pages.insert(page);
        }
        let Some(path) = &self.active_pdf_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        let path_str = abs_path.to_string_lossy().to_string();
        let zoom = self.pdf_zoom;
        let generation = self.pdf_render_generation;
        let _state = self.state.clone();

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

    fn render_all_pdf_pages(&mut self) -> Task<Message> {
        self.render_visible_pdf_pages()
    }

    fn render_visible_pdf_pages(&mut self) -> Task<Message> {
        if self.pdf_total_pages == 0 {
            return Task::none();
        }
        let start = self.pdf_current_page.saturating_sub(3);
        let end = (self.pdf_current_page + 5).min(self.pdf_total_pages.saturating_sub(1));
        self.render_pdf_page_range(start, end)
    }

    fn render_pdf_pages_for_viewport(
        &mut self,
        scroll_y: f32,
        viewport_height: f32,
    ) -> Task<Message> {
        if self.pdf_total_pages == 0 {
            return Task::none();
        }

        let first = self.pdf_page_at_scroll((scroll_y - self.estimated_pdf_page_height()).max(0.0));
        let last =
            self.pdf_page_at_scroll(scroll_y + viewport_height + self.estimated_pdf_page_height());
        self.render_pdf_page_range(
            first.saturating_sub(2),
            (last + 2).min(self.pdf_total_pages.saturating_sub(1)),
        )
    }

    fn render_pdf_page_range(&mut self, start: u16, end: u16) -> Task<Message> {
        let mut tasks = Vec::new();
        for page_idx in start..=end {
            if self
                .pdf_pages
                .get(page_idx as usize)
                .map_or(true, |p| p.is_none())
                && !self.pdf_pending_pages.contains(&page_idx)
            {
                self.pdf_pending_pages.insert(page_idx);
                tasks.push(self.render_pdf_page(page_idx));
            }
        }

        Task::batch(tasks)
    }

    fn estimated_pdf_page_height(&self) -> f32 {
        let mut count = 0.0;
        let mut total = 0.0;
        for (_, h) in self.pdf_dimensions.iter().flatten() {
            total += *h as f32;
            count += 1.0;
        }
        if count > 0.0 {
            total / count
        } else {
            792.0 * self.pdf_zoom
        }
    }

    fn pdf_available_width(&self) -> f32 {
        let sidebar_width = if self.sidebar_visible { 260.0 } else { 0.0 };
        let toc_width = if self.toc_visible { 260.0 } else { 0.0 };
        let backlinks_width = if self.backlinks_visible { 260.0 } else { 0.0 };
        let chrome_width = sidebar_width + toc_width + backlinks_width;
        let content_width = (self.window_width - chrome_width).max(320.0);

        if self.split_view_active && self.active_path.is_some() && self.active_pdf_path.is_some() {
            (content_width * (1.0 - self.split_ratio)).max(280.0)
        } else {
            content_width
        }
    }

    fn pdf_page_height(&self, page: u16) -> f32 {
        self.pdf_dimensions
            .get(page as usize)
            .and_then(|d| d.map(|(_, h)| h as f32))
            .unwrap_or_else(|| self.estimated_pdf_page_height())
    }

    fn pdf_page_offset(&self, page: u16) -> f32 {
        let mut y = 20.0;
        for idx in 0..page.min(self.pdf_total_pages) {
            y += self.pdf_page_height(idx) + 20.0;
        }
        y
    }

    fn pdf_total_height(&self) -> f32 {
        let mut y = 20.0;
        for idx in 0..self.pdf_total_pages {
            y += self.pdf_page_height(idx) + 20.0;
        }
        y
    }

    fn pdf_page_at_scroll(&self, scroll_y: f32) -> u16 {
        let mut y = 20.0;
        for idx in 0..self.pdf_total_pages {
            let next = y + self.pdf_page_height(idx) + 20.0;
            if scroll_y < next {
                return idx;
            }
            y = next;
        }
        self.pdf_total_pages.saturating_sub(1)
    }

    fn pdf_link_at(&self, page_idx: u16, x: f32, y: f32) -> Option<md_editor_core::pdf::LinkInfo> {
        let links = self.pdf_page_links.get(&page_idx)?;
        let dim = self
            .pdf_dimensions
            .get(page_idx as usize)
            .and_then(|d| *d)?;
        let real_x = (x * dim.0 as f32) / self.pdf_zoom;
        let real_y = (y * dim.1 as f32) / self.pdf_zoom;

        links
            .iter()
            .find(|link| {
                let lx = link.bbox.x;
                let ly = link.bbox.y;
                let lw = link.bbox.width;
                let lh = link.bbox.height;
                real_x >= lx && real_x <= lx + lw && real_y >= ly && real_y <= ly + lh
            })
            .cloned()
    }

    fn resolve_active_path(&self, path: &str) -> Option<std::path::PathBuf> {
        let root = self.vault_root.as_deref()?;
        Some(md_editor_core::vault::resolve_vault_path(
            std::path::Path::new(root),
            path,
        ))
    }

    fn highlight_all(&mut self) {
        self.highlighted_lines = highlight::highlight_markdown(&self.buffer.text());
    }

    fn current_document_match_count(&self) -> usize {
        self.current_document_matches().len()
    }

    fn active_search_match_position(&self) -> Option<(usize, usize)> {
        let matches = self.current_document_matches();
        let index = self.search_match_index?;
        matches
            .get(index.min(matches.len().saturating_sub(1)))
            .map(|m| (m.line, m.start_col))
    }

    fn current_document_matches(&self) -> Vec<DocumentMatch> {
        if self.search_query.is_empty() || self.active_path.is_none() {
            return Vec::new();
        }

        (0..self.buffer.line_count())
            .flat_map(|line| {
                let text = self.buffer.line_text(line);
                crate::search::line_matches(
                    &text,
                    &self.search_query,
                    self.search_regex,
                    self.search_match_case,
                )
                .into_iter()
                .map(move |line_match| DocumentMatch {
                    line,
                    start_col: line_match.start_col,
                    end_col: line_match.end_col,
                })
            })
            .collect()
    }

    fn navigate_file_search(&mut self, forward: bool) -> Task<Message> {
        let matches = self.current_document_matches();
        if matches.is_empty() {
            self.search_match_index = None;
            return Task::none();
        }

        let next_index = match self.search_match_index {
            Some(index) if forward => (index + 1) % matches.len(),
            Some(0) if !forward => matches.len() - 1,
            Some(index) => index.saturating_sub(1),
            None if forward => 0,
            None => matches.len() - 1,
        };
        self.search_match_index = Some(next_index);
        let item = matches[next_index];
        self.buffer.execute(EditorCommand::SetSelection {
            anchor_line: item.line,
            anchor_col: item.start_col,
            focus_line: item.line,
            focus_col: item.end_col,
        });
        self.scroll_editor_to_line(item.line)
    }

    fn navigate_pdf_search(&mut self, forward: bool) -> Task<Message> {
        if self.pdf_search_results.is_empty() {
            self.search_match_index = None;
            return Task::none();
        }

        let next_index = match self.search_match_index {
            Some(index) if forward => (index + 1) % self.pdf_search_results.len(),
            Some(0) if !forward => self.pdf_search_results.len() - 1,
            Some(index) => index.saturating_sub(1),
            None if forward => 0,
            None => self.pdf_search_results.len() - 1,
        };
        self.navigate_pdf_search_to_index(next_index)
    }

    fn navigate_pdf_search_to_index(&mut self, index: usize) -> Task<Message> {
        let Some(result) = self.pdf_search_results.get(index) else {
            self.search_match_index = None;
            return Task::none();
        };

        self.search_match_index = Some(index);
        self.pdf_current_page = result
            .page_index
            .min(self.pdf_total_pages.saturating_sub(1));
        self.navigate_pdf_page(self.pdf_current_page)
    }

    fn navigate_pdf_page(&mut self, page: u16) -> Task<Message> {
        let scroll_y = self.pdf_page_offset(page);
        let start = page.saturating_sub(2);
        let end = (page + 4).min(self.pdf_total_pages.saturating_sub(1));
        Task::batch(vec![
            self.render_pdf_page_direct(page),
            self.render_pdf_page_range(start, end),
            operation::scroll_to(
                iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                AbsoluteOffset {
                    x: 0.0,
                    y: scroll_y,
                },
            ),
        ])
    }

    fn estimated_editor_line_y(&self, target_line: usize) -> f32 {
        crate::editor::renderer::line_visual_y::<iced::Renderer>(
            &self.highlighted_lines,
            &self.image_cache,
            &self.math_cache,
            self.editor_viewport_width.max(240.0),
            self.buffer.cursor_line,
            self.buffer.cursor_col,
            target_line,
            true,
        ) + 20.0
    }

    fn scroll_editor_to_line(&self, line: usize) -> Task<Message> {
        let y = self.estimated_editor_line_y(line);
        let top_padding = 36.0;
        let bottom_padding = 72.0;
        let visible_top = self.editor_scroll_y + top_padding;
        let visible_bottom = self.editor_scroll_y + self.editor_viewport_height - bottom_padding;
        let target_y = if y < visible_top {
            (y - top_padding).max(0.0)
        } else if y + 34.0 > visible_bottom {
            (y + 34.0 + bottom_padding - self.editor_viewport_height).max(0.0)
        } else {
            self.editor_scroll_y
        };

        Task::perform(async move { target_y }, Message::ScrollEditorToTarget)
    }

    fn replace_all_in_current_document(&mut self) -> Result<usize, String> {
        if self.active_path.is_none() {
            return Err("Open a markdown file before replacing text".to_string());
        }
        if self.search_query.is_empty() {
            return Err("Search query is empty".to_string());
        }

        let text = self.buffer.text();
        let (new_text, count) = if self.search_regex {
            let re = regex::RegexBuilder::new(&self.search_query)
                .case_insensitive(!self.search_match_case)
                .build()
                .map_err(|err| format!("Invalid regex: {err}"))?;
            let count = re.find_iter(&text).count();
            (
                re.replace_all(&text, self.search_replace.as_str())
                    .to_string(),
                count,
            )
        } else if self.search_match_case {
            let count = text.match_indices(&self.search_query).count();
            (
                text.replace(&self.search_query, &self.search_replace),
                count,
            )
        } else {
            let re = regex::RegexBuilder::new(&regex::escape(&self.search_query))
                .case_insensitive(true)
                .build()
                .map_err(|err| err.to_string())?;
            let count = re.find_iter(&text).count();
            (
                re.replace_all(&text, self.search_replace.as_str())
                    .to_string(),
                count,
            )
        };

        if count > 0 {
            self.buffer.set_text(&new_text);
            self.highlight_all();
            self.load_images();
            self.toc_entries = views::toc::get_toc(&self.buffer.text());
        }
        Ok(count)
    }

    fn search_pdf(&self) -> Task<Message> {
        let Some(path) = &self.active_pdf_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        let query = self.search_query.clone();
        let regex = self.search_regex;
        let match_case = self.search_match_case;
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
        let should_keep_cursor_visible = matches!(
            command,
            EditorCommand::MoveCursor { .. }
                | EditorCommand::SetCursor { .. }
                | EditorCommand::SetSelection { .. }
        );
        let result = self.buffer.execute(command);
        if result.projection_changed {
            self.highlight_all();
        }
        let content_task = if result.text_changed {
            self.load_images();
            self.toc_entries = views::toc::get_toc(&self.buffer.text());
            self.load_math()
        } else {
            Task::none()
        };

        if should_keep_cursor_visible {
            Task::batch(vec![
                content_task,
                self.scroll_editor_to_line(self.buffer.cursor_line),
            ])
        } else {
            content_task
        }
    }

    fn load_images(&mut self) {
        let Some(active_path) = &self.active_path else {
            return;
        };
        let Some(vault_root) = &self.vault_root else {
            return;
        };
        let Some(base_path) = std::path::Path::new(vault_root)
            .join(active_path)
            .parent()
            .map(|path| path.to_path_buf())
        else {
            return;
        };

        for line in &self.highlighted_lines {
            for span in &line.spans {
                if span.is_image {
                    if let Some(path) = &span.image_path {
                        if !self.image_cache.contains_key(path) {
                            let img_path = base_path.join(path);
                            if let Ok(img) = image::open(&img_path) {
                                let (width, height) = img.dimensions();
                                let handle = iced::widget::image::Handle::from_rgba(
                                    width,
                                    height,
                                    img.into_rgba8().into_raw(),
                                );
                                self.image_cache
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
        for line in &self.highlighted_lines {
            for span in &line.spans {
                if span.is_math {
                    let tex = span
                        .visible_text(false)
                        .trim_matches('$')
                        .trim()
                        .to_string();
                    if !tex.is_empty() && !self.math_cache.contains_key(&tex) {
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

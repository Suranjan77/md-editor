use iced::widget::operation::{self, AbsoluteOffset};
use iced::widget::{button, column, container, row, scrollable, stack, text, Space};
use iced::{Alignment, Element, Length, Subscription, Task, Theme};

use image::GenericImageView;
use std::collections::BTreeSet;
use std::sync::Arc;

use crate::editor::buffer::DocBuffer;
use crate::editor::highlight;
use crate::messages::EditorAction;
use crate::messages::{Message, Shortcut, TrackerTab};
use crate::theme as app_theme;
use crate::views;
use std::collections::HashSet;

const PDF_SCROLLABLE_ID: &str = "pdf_scrollable";

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
    pdf_scroll_y: f32,
    pdf_page_links: std::collections::HashMap<u16, Vec<md_editor_core::pdf::LinkInfo>>,
    pdf_link_preview: Option<iced::widget::image::Handle>,
    showing_pdf: bool,

    // Study tracker
    tracker_visible: bool,
    tracker_running: bool,
    tracker_started_at: Option<std::time::Instant>,
    tracker_sessions: Vec<md_editor_core::tracker::StudySession>,
    tracker_kv: std::collections::HashMap<String, String>,
    tracker_tab: TrackerTab,
    tracker_config_json: String,

    // Modal state
    #[allow(dead_code)]
    active_modal: Option<views::modals::ModalType>,
    #[allow(dead_code)]
    modal_input: String,

    // Command palette
    #[allow(dead_code)]
    command_palette_visible: bool,
    #[allow(dead_code)]
    command_palette_query: String,
    #[allow(dead_code)]
    commands: Vec<views::command_palette::Command>,

    // Toast
    toast: Option<String>,

    // Search
    search_visible: bool,
    search_query: String,
    search_results: Vec<md_editor_core::types::SearchResult>,

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
}

impl MdEditor {
    pub fn new() -> (Self, Task<Message>) {
        let state = Arc::new(md_editor_core::state::AppState::new());
        let last_vault = md_editor_core::config::get_sys_config(&state, "last_vault")
            .ok()
            .flatten();
        let tracker_sessions = md_editor_core::tracker::get_sessions(&state).unwrap_or_default();

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
            pdf_scroll_y: 0.0,
            pdf_page_links: std::collections::HashMap::new(),
            pdf_link_preview: None,
            showing_pdf: false,
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
            tracker_config_json: md_editor_core::config::get_sys_config(&state, "tracker_config")
                .ok()
                .flatten()
                .unwrap_or_else(views::tracker::default_config_json),
            active_modal: None,
            modal_input: String::new(),
            command_palette_visible: false,
            command_palette_query: String::new(),
            commands: views::command_palette::get_commands(),
            toast: None,
            search_visible: false,
            search_query: String::new(),
            search_results: Vec::new(),
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
        };

        if let Some(path) = last_vault {
            app.open_vault(&path);
        }

        (app, Task::none())
    }

    pub fn title(&self) -> String {
        format!(
            "{}Antigravity — {}",
            if self.buffer.dirty { "● " } else { "" },
            self.active_path.as_deref().unwrap_or("New File")
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
                    if modifiers.command() || modifiers.control() {
                        match key {
                            iced::keyboard::Key::Character(c) if c == "s" => {
                                return Message::KeyboardShortcut(Shortcut::Save)
                            }
                            iced::keyboard::Key::Character(c) if c == "o" => {
                                return Message::KeyboardShortcut(Shortcut::OpenVault)
                            }
                            iced::keyboard::Key::Character(c) if c == "n" => {
                                return Message::KeyboardShortcut(Shortcut::NewFile)
                            }
                            iced::keyboard::Key::Character(c) if c == "f" => {
                                return Message::KeyboardShortcut(Shortcut::Search)
                            }
                            iced::keyboard::Key::Character(c) if c == "p" => {
                                return Message::KeyboardShortcut(Shortcut::CommandPalette)
                            }
                            iced::keyboard::Key::Character(c) if c == "b" => {
                                return Message::KeyboardShortcut(Shortcut::ToggleSidebar)
                            }
                            iced::keyboard::Key::Character(c) if c == "t" => {
                                return Message::KeyboardShortcut(Shortcut::TableOfContents)
                            }
                            _ => {}
                        }
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
                if path.ends_with(".md") || path.ends_with(".markdown") {
                    self.showing_pdf = false;
                    self.open_file(&path)
                } else if path.ends_with(".pdf") {
                    self.active_pdf_path = Some(path.clone());
                    self.showing_pdf = true;
                    self.open_pdf(&path)
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
            Message::NameModalSubmit(input) => {
                let name = input.trim();
                if name.is_empty() {
                    self.toast = Some("Name cannot be empty".to_string());
                    return Task::none();
                }

                let target_path = self.new_entry_path(name);
                let result = match self.active_modal.as_ref() {
                    Some(views::modals::ModalType::CreateFile) => {
                        let path = if target_path.ends_with(".md") || target_path.ends_with(".markdown") {
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

            Message::FileLoaded(path, content) => {
                self.buffer = DocBuffer::from_text(&content);
                self.active_path = Some(path);
                self.highlight_all();
                self.load_images();
                let math_task = self.load_math();
                self.toc_entries = views::toc::get_toc(&self.buffer.text());
                math_task
            }
            Message::EditorContentChanged(c) => {
                self.buffer.insert_at_cursor(&c);
                self.highlight_all();
                self.load_images();
                let math_task = self.load_math();
                self.toc_entries = views::toc::get_toc(&self.buffer.text());
                math_task
            }
            Message::MathRendered(tex, res) => {
                if let Ok(tuple) = res {
                    self.math_cache.insert(tex, tuple);
                }
                Task::none()
            }
            Message::EditorAction(action) => {
                match action {
                    EditorAction::Backspace => self.buffer.backspace(),
                    EditorAction::MoveLeft => self.buffer.move_cursor_left(),
                    EditorAction::MoveRight => self.buffer.move_cursor_right(),
                    EditorAction::MoveUp => self.buffer.move_cursor_up(),
                    EditorAction::MoveDown => self.buffer.move_cursor_down(),
                    EditorAction::MoveHome => self.buffer.move_cursor_home(),
                    EditorAction::MoveEnd => self.buffer.move_cursor_end(),
                    EditorAction::Delete => self.buffer.delete(),
                    EditorAction::Undo => self.buffer.undo(),
                    EditorAction::Redo => self.buffer.redo(),
                }
                self.highlight_all();
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
                let text = self.buffer.text();
                let mut lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
                if let Some(line) = lines.get_mut(line_idx) {
                    let indent_len = line.len() - line.trim_start().len();
                    let marker = &line[indent_len..];
                    if marker.starts_with("- [ ]") {
                        line.replace_range(indent_len..indent_len + 5, "- [x]");
                    } else if marker.starts_with("- [x]") || marker.starts_with("- [X]") {
                        line.replace_range(indent_len..indent_len + 5, "- [ ]");
                    }
                }
                let new_content = lines.join("\n");
                self.buffer.set_text(&new_content);
                self.highlight_all();
                Task::none()
            }
            Message::EditorCursorMove(line, col) => {
                self.buffer.set_cursor(line, col);
                Task::none()
            }

            Message::PdfLoaded(pages) => {
                self.pdf_total_pages = pages;
                self.pdf_pages = vec![None; pages as usize];
                self.pdf_dimensions = vec![None; pages as usize];
                if pages == 0 {
                    self.toast = Some("PDF renderer is unavailable or the PDF could not be opened".to_string());
                }
                self.render_all_pdf_pages()
            }
            Message::PdfPageChanged(page) => {
                self.pdf_current_page = page;
                self.render_visible_pdf_pages()
            }
            Message::PdfZoomChanged(zoom) => {
                self.pdf_zoom = zoom;
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
                    let scroll_y = self.pdf_page_offset(index as u16);
                    Task::batch(vec![
                        self.render_pdf_page_range(
                            self.pdf_current_page.saturating_sub(4),
                            (self.pdf_current_page + 10)
                                .min(self.pdf_total_pages.saturating_sub(1)),
                        ),
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
                        self.pdf_render_generation = self.pdf_render_generation.wrapping_add(1);
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
                    let dest_page = link.dest_page.unwrap();
                    let dest_y = link.dest_y;
                    let path = self.active_pdf_path.clone().unwrap_or_default();
                    let abs_path = md_editor_core::vault::resolve_vault_path(
                        self.state.vault_root.lock().unwrap().as_ref().unwrap(),
                        &path,
                    )
                    .to_string_lossy()
                    .to_string();
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
            Message::PdfTocLoaded(entries) => {
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
                            .unwrap_or_else(views::tracker::default_config_json);
                }
                Task::none()
            }
            Message::BacklinksToggle => {
                self.backlinks_visible = !self.backlinks_visible;
                Task::none()
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
            Message::TrackerSave(hours, notes) => {
                let session = md_editor_core::tracker::StudySession {
                    id: 0,
                    date: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
                    hours,
                    activity_type: "Study".to_string(),
                    phase: "Manual".to_string(),
                    notes: if notes.trim().is_empty() {
                        None
                    } else {
                        Some(notes)
                    },
                };
                if md_editor_core::tracker::save_session(&self.state, session).is_ok() {
                    self.tracker_sessions =
                        md_editor_core::tracker::get_sessions(&self.state).unwrap_or_default();
                }
                Task::none()
            }
            Message::TrackerLoaded(sessions) => {
                self.tracker_sessions = sessions;
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
                let next = if self.tracker_kv.get(&key).map(|v| v == "true").unwrap_or(false) {
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
                let next = if self.tracker_kv.get(&key).map(|v| v == "true").unwrap_or(false) {
                    "false"
                } else {
                    "true"
                };
                if md_editor_core::tracker::set_kv(&self.state, &key, next).is_ok() {
                    self.tracker_kv.insert(key, next.to_string());
                }
                Task::none()
            }
            Message::TrackerConfigChanged(config) => {
                self.tracker_config_json = config;
                Task::none()
            }
            Message::TrackerConfigSave => {
                match serde_json::from_str::<serde_json::Value>(&self.tracker_config_json) {
                    Ok(value) if value.get("PHASES").is_some() && value.get("PROJECTS").is_some() => {
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
                    Ok(_) => self.toast = Some("Tracker JSON must include PHASES and PROJECTS".to_string()),
                    Err(err) => self.toast = Some(format!("Invalid tracker JSON: {}", err)),
                }
                Task::none()
            }

            Message::SearchOpen => {
                self.search_visible = true;
                Task::none()
            }
            Message::SearchClose => {
                self.search_visible = false;
                Task::none()
            }
            Message::SearchQueryChanged(q) => {
                self.search_query = q.clone();
                if q.len() > 2 {
                    if let Ok(res) = md_editor_core::vault::search_vault(&self.state, &q) {
                        self.search_results = res;
                    }
                }
                Task::none()
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
                    Shortcut::Search => {
                        self.search_visible = true;
                        Task::none()
                    }
                    Shortcut::CommandPalette => {
                        self.command_palette_visible = true;
                        Task::none()
                    }
                    Shortcut::TableOfContents => {
                        self.toc_visible = !self.toc_visible;
                        Task::none()
                    }
                    _ => Task::none(),
                }
            }
            Message::SplitViewToggle => {
                if self.active_path.is_some() && self.active_pdf_path.is_some() {
                    self.split_view_active = !self.split_view_active;
                } else {
                    self.toast = Some("Open a markdown file and a PDF to use split view".to_string());
                }
                Task::none()
            }
            Message::SplitViewDragStart => {
                self.is_resizing_split = true;
                Task::none()
            }
            Message::SplitViewDragging(x_pos) => {
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
                Task::none()
            }
            Message::WindowResized(width) => {
                self.window_width = width;
                Task::none()
            }
            Message::ToggleTOC => {
                self.toc_visible = !self.toc_visible;
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
            self.active_pdf_path.as_deref(),
            None,
            self.sidebar_visible,
            self.backlinks_visible,
            self.tracker_visible,
            self.toc_visible,
            self.split_view_active,
            self.active_path.is_some() && self.active_pdf_path.is_some(),
        );

        let sidebar = views::sidebar::view(
            &self.vault_entries,
            self.selected_path.as_deref(),
            self.active_path.as_deref(),
            &self.expanded_folders,
            !self.sidebar_visible,
        );

        let editor_view = scrollable(
            container(crate::editor::renderer::Editor::new(
                &self.buffer,
                &self.highlighted_lines,
                &self.image_cache,
                &self.math_cache,
                Message::EditorContentChanged,
                Message::EditorCursorMove,
                Message::EditorAction,
                Message::SidebarFileClicked,
                Message::EditorCheckboxToggle,
            ))
            .padding(20)
            .width(Length::Fill),
        )
        .height(Length::Fill);

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
                ))
                .id(iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID))
                .on_scroll(|vp| Message::PdfScrolled {
                    y: vp.absolute_offset().y,
                    viewport_height: vp.bounds().height,
                })
                .height(Length::Fill);

                column![pdf_toolbar, pdf_pages].height(Length::Fill).into()
            } else {
                container(Space::new()).width(Length::Fixed(0.0)).into()
            };

        let toc_view: Element<Message, Theme, iced::Renderer> = if self.toc_visible {
            views::toc::view(&self.toc_entries)
        } else {
            container(Space::new()).width(Length::Fixed(0.0)).into()
        };

        let main_content: Element<Message, Theme, iced::Renderer> =
            if self.split_view_active && self.active_path.is_some() && self.active_pdf_path.is_some() {
                let left_portion = (self.split_ratio * 1000.0) as u16;
                let right_portion = ((1.0 - self.split_ratio) * 1000.0) as u16;

                let divider = button(
                    container(text("⋮").size(14).color(app_theme::TEXT_MUTED))
                        .width(Length::Fixed(8.0))
                        .height(Length::Fill)
                        .center_x(Length::Fixed(8.0))
                        .center_y(Length::Fill)
                        .style(|_| container::Style {
                            background: Some(iced::Background::Color(app_theme::BG_TERTIARY)),
                            ..Default::default()
                        }),
                )
                .padding(0)
                .on_press(Message::SplitViewDragStart)
                .style(button::text);

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
            } else {
                editor_view.into()
            };

        let content = column![toolbar, main_content].height(Length::Fill);

        let layout = row![sidebar, content, toc_view].height(Length::Fill);

        let mut layers = vec![container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(app_theme::BG_PRIMARY)),
                ..Default::default()
            })
            .into()];

        if self.search_visible {
            layers.push(
                container(views::search::view(
                    &self.search_query,
                    &self.search_results,
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
                    &self.tracker_config_json,
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
        self.state
            .vault_root
            .lock()
            .unwrap()
            .replace(std::path::PathBuf::from(path));
        self.vault_entries = md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
    }

    fn new_entry_path(&self, name: &str) -> String {
        let parent = self
            .selected_path
            .as_deref()
            .and_then(|path| {
                if self
                    .vault_entries
                    .iter()
                    .any(|entry| entry.path == path && entry.is_dir)
                {
                    Some(path.to_string())
                } else {
                    std::path::Path::new(path)
                        .parent()
                        .and_then(|p| {
                            let parent = p.to_string_lossy().replace('\\', "/");
                            if parent.is_empty() { None } else { Some(parent) }
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
        let abs_path = md_editor_core::vault::resolve_vault_path(
            self.state.vault_root.lock().unwrap().as_ref().unwrap(),
            path,
        );
        let path_str = abs_path.to_string_lossy().to_string();
        self.active_pdf_path = Some(path.to_string());
        self.showing_pdf = true;
        self.pdf_current_page = 0;
        self.pdf_pages = Vec::new();
        self.pdf_dimensions = Vec::new();
        self.pdf_pending_pages.clear();
        self.pdf_render_generation = self.pdf_render_generation.wrapping_add(1);

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
                |res| Message::PdfLoaded(res.unwrap_or(0)),
            ),
            Task::perform(
                async move {
                    let renderer = _state_toc.pdf_renderer.as_ref()?;
                    renderer.get_toc(&path_str_toc).ok()
                },
                |res| Message::PdfTocLoaded(res.unwrap_or_default()),
            ),
            self.render_visible_pdf_pages(),
        ])
    }

    fn render_pdf_page(&self, page: u16) -> Task<Message> {
        let Some(path) = &self.active_pdf_path else {
            return Task::none();
        };
        let abs_path = md_editor_core::vault::resolve_vault_path(
            self.state.vault_root.lock().unwrap().as_ref().unwrap(),
            path,
        );
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

    fn highlight_all(&mut self) {
        self.highlighted_lines = highlight::highlight_markdown(&self.buffer.text());
    }

    fn load_images(&mut self) {
        let Some(active_path) = &self.active_path else {
            return;
        };
        let Some(vault_root) = &self.vault_root else {
            return;
        };
        let base_path = std::path::Path::new(vault_root)
            .join(active_path)
            .parent()
            .unwrap()
            .to_path_buf();

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
        use ratex_layout::{layout, to_display_list, LayoutOptions};
        use ratex_parser::parser::parse;
        use ratex_render::{render_to_png, RenderOptions};
        use ratex_types::color::Color as RatexColor;
        use ratex_types::math_style::MathStyle;

        let options = RenderOptions {
            font_size: 20.0,
            padding: 2.0,
            background_color: RatexColor {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.0,
            },
            font_dir: String::new(),
            device_pixel_ratio: 1.0,
        };

        let layout_opts = LayoutOptions::default()
            .with_style(MathStyle::Display)
            .with_color(RatexColor {
                r: 0.96,
                g: 0.97,
                b: 0.99,
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
            w as f32,
            h as f32,
        ))
    }
}

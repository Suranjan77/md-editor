use iced::widget::operation::{self, AbsoluteOffset};
use iced::widget::{Space, column, container, mouse_area, row, scrollable, stack, text};
use iced::{Alignment, Element, Length, Subscription, Task, Theme};

use crate::app_shell::{
    AppShellInputs, AppShellMode, AppShellPane, AppShellPersistence, AppShellState, AppShellStatus,
    AppShellStatusInputs, WorkflowSidebarTab,
};
use crate::pdf_layout::PdfLayout;
use crate::pdf_page_cache::PdfPageCache;
use image::GenericImageView;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::editor::buffer::{DocBuffer, EditorCommand};
use crate::editor::highlight;
use crate::features::overlays::OverlayState;
use crate::features::search::{PdfSearchState, SearchState};
use crate::features::tracker::TrackerState;
use crate::features::workspace::WorkspaceState;
use crate::messages::{EditorBlockActionKind, Message, SearchWrapStatus, Shortcut};
use crate::pdf_links::{build_pdf_link, parse_pdf_link};
use crate::pdf_notes::{
    build_linked_pdf_note_content, normalize_note_path, note_filename_from_path, slug_fragment,
};
use crate::search::DocumentMatch;
use crate::theme as app_theme;
use crate::views;
use crate::views::pdf_viewer::{PDF_PAGE_LIST_PADDING, PDF_PAGE_SPACING};
use std::collections::HashSet;

const PDF_SCROLLABLE_ID: &str = "pdf_scrollable";
const EDITOR_SCROLLABLE_ID: &str = "editor_scrollable";
const PDF_RENDER_SUPERSAMPLE: f32 = 2.0;
const PDF_RENDER_PRELOAD_PAGES: u16 = 3;
const PDF_RENDER_MAX_SCHEDULED_PAGES: u16 = 64;
const PDF_TEXT_PAGE_CACHE_LIMIT: usize = 50;
const GLOBAL_PDF_TEXT_SEARCH_MAX_DOCUMENTS: usize = 32;
const GLOBAL_PDF_TEXT_SEARCH_MAX_RESULTS: usize = 200;
const PDF_TEXT_INDEX_MAX_DOCUMENTS: usize = 16;
const PDF_TEXT_INDEX_MAX_PAGES_PER_DOCUMENT: u16 = 3;
const LARGE_DOC_LINE_THRESHOLD: usize = 1_000;
const HUGE_DOC_LINE_THRESHOLD: usize = 5_000;
const HIGHLIGHT_DEBOUNCE: Duration = Duration::from_millis(80);
const EDITOR_AUTOSAVE_DELAY: Duration = Duration::from_secs(2);
const APP_SHELL_PERSISTENCE_CONFIG_KEY: &str = "app_shell_persistence";

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

fn pdf_search_match_scroll_y_from(
    page_offset: f32,
    rect_y: Option<f32>,
    rect_height: f32,
    page_height: f32,
    zoom: f32,
    max_y: f32,
) -> f32 {
    let match_top = rect_y
        .map(|y| (page_height - y - rect_height).max(0.0) * zoom)
        .unwrap_or(0.0);
    (page_offset + match_top - 96.0).clamp(0.0, max_y.max(0.0))
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

fn pdf_placeholder_display_size_from(
    placeholder_page_size: Option<(f32, f32)>,
    first_page_size: Option<(f32, f32)>,
    first_dimensions: Option<(u32, u32)>,
    zoom: f32,
) -> (f32, f32) {
    placeholder_page_size
        .or(first_page_size)
        .or_else(|| first_dimensions.map(|(w, h)| (w as f32 / zoom, h as f32 / zoom)))
        .map(|(w, h)| (w * zoom, h * zoom))
        .unwrap_or((612.0 * zoom, 792.0 * zoom))
}

fn text_by_char_range(text: &str, start: usize, end: usize) -> String {
    if start >= end {
        return String::new();
    }

    text.chars().skip(start).take(end - start).collect()
}

#[cfg(test)]
use crate::pdf_navigation::NavigationHistory;
use crate::pdf_navigation::NavigationTarget;

#[derive(Debug, Clone)]
pub struct PdfViewState {
    pub zoom: f32,
    pub page_sizes: Vec<Option<(f32, f32)>>,
    pub page_cache: PdfPageCache,
    pub layout: PdfLayout,
    pub search: PdfSearchState,
}

pub struct MdEditor {
    state: Arc<md_editor_core::state::AppState>,
    workspace: WorkspaceState,
    sidebar_visible: bool,

    // Editor state
    buffer: DocBuffer,
    highlighted_lines: Vec<highlight::StyledLine>,
    highlight_generation: u64,
    pending_highlight_generation: Option<u64>,
    pending_highlight_requested_at: Option<Instant>,
    pending_highlight_text: Option<String>,

    // PDF state
    pdf_current_page: u16,
    pdf_total_pages: u16,
    pdf_state: PdfViewState,
    pdf_rotation: u16,
    pdf_pages: Vec<Option<iced::widget::image::Handle>>,
    pdf_dimensions: Vec<Option<(u32, u32)>>,
    pdf_placeholder_page_size: Option<(f32, f32)>,
    active_pdf_path: Option<String>,
    active_image_path: Option<String>,
    active_image: Option<(iced::widget::image::Handle, f32, f32)>,
    pdf_scroll_y: f32,
    pdf_viewport_height: f32,
    pdf_page_links: std::collections::HashMap<u16, Vec<md_editor_core::pdf::LinkInfo>>,
    pdf_link_preview: Option<iced::widget::image::Handle>,
    showing_pdf: bool,
    pdf_fit_to_width: bool,
    pdf_fit_to_page: bool,

    // PDF study fields
    pdf_document_id: Option<String>,
    pdf_page_text: std::collections::HashMap<u16, md_editor_core::pdf::PdfPageText>,
    pdf_selection: Option<views::interactive_pdf::PdfSelection>,
    pdf_annotations: std::collections::HashMap<u16, Vec<md_editor_core::pdf::PdfAnnotation>>,
    focused_annotation_id: Option<String>,
    pending_editor_save: Option<std::time::Instant>,
    pdf_initial_target_page: Option<u16>,
    pdf_initial_target_annotation: Option<String>,
    pdf_pending_text: HashSet<u16>,
    pdf_text_lru: std::collections::VecDeque<u16>,

    tracker: TrackerState,
    overlays: OverlayState,

    search: SearchState,

    // TOC
    toc_visible: bool,
    pdf_annotations_visible: bool,
    pdf_annotations_filter_color: Option<md_editor_core::pdf::PdfAnnotationColor>,
    pdf_annotations_filter_page: Option<u16>,
    pdf_annotations_filter_tag: Option<String>,
    pdf_annotations_filter_linked: Option<bool>,
    pdf_annotations_filter_unresolved: Option<bool>,
    image_cache: std::collections::HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    math_cache: std::collections::HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    image_errors: std::collections::HashMap<String, String>,
    math_errors: std::collections::HashMap<String, String>,
    pdf_pending_pages: HashSet<u16>,
    pdf_stale_pages: HashSet<u16>,
    pdf_pending_links: HashSet<u16>,
    pdf_render_generation: u64,
    pdf_programmatic_scroll: bool,
    pdf_toc_target_page: Option<u16>,
    /// TOC entries parsed from the active markdown buffer (headings).
    md_toc_entries: Vec<views::toc::TocEntry>,
    /// Flattened PDF TOC entries (bookmarks or page entries).
    pdf_toc_entries_flat: Option<Vec<views::toc::TocEntry>>,
    split_view_active: bool,
    split_ratio: f32,
    is_resizing_split: bool,
    pdf_split_ratio: f32,
    active_panel: ActivePanel,
    keyboard_modifiers: iced::keyboard::Modifiers,
    window_width: f32,
    window_height: f32,
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
            workspace: WorkspaceState::default(),
            sidebar_visible: true,
            buffer: DocBuffer::new(),
            highlighted_lines: Vec::new(),
            highlight_generation: 0,
            pending_highlight_generation: None,
            pending_highlight_requested_at: None,
            pending_highlight_text: None,
            pdf_current_page: 0,
            pdf_total_pages: 0,
            pdf_state: PdfViewState {
                zoom: 1.5,
                page_sizes: Vec::new(),
                page_cache: PdfPageCache::default(),
                layout: PdfLayout::default(),
                search: PdfSearchState::default(),
            },
            pdf_rotation: 0,
            pdf_pages: Vec::new(),
            pdf_dimensions: Vec::new(),
            pdf_placeholder_page_size: None,
            active_pdf_path: None,
            active_image_path: None,
            active_image: None,
            pdf_scroll_y: 0.0,
            pdf_viewport_height: 0.0,
            pdf_page_links: std::collections::HashMap::new(),
            pdf_link_preview: None,
            showing_pdf: false,
            pdf_fit_to_width: true,
            pdf_fit_to_page: false,
            pdf_document_id: None,
            pdf_page_text: std::collections::HashMap::new(),
            pdf_selection: None,
            pdf_annotations: std::collections::HashMap::new(),
            focused_annotation_id: None,
            pending_editor_save: None,
            pdf_initial_target_page: None,
            pdf_initial_target_annotation: None,
            pdf_pending_text: HashSet::new(),
            pdf_text_lru: std::collections::VecDeque::new(),
            tracker: TrackerState::new(
                tracker_sessions,
                md_editor_core::tracker::get_kv(&state)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|item| (item.key, item.value))
                    .collect(),
                tracker_config_json,
                chrono::Local::now().format("%Y-%m-%d").to_string(),
            ),
            overlays: OverlayState::default(),
            search: SearchState::default(),
            toc_visible: false,
            pdf_annotations_visible: false,
            pdf_annotations_filter_color: None,
            pdf_annotations_filter_page: None,
            pdf_annotations_filter_tag: None,
            pdf_annotations_filter_linked: None,
            pdf_annotations_filter_unresolved: None,
            image_cache: std::collections::HashMap::new(),
            math_cache: std::collections::HashMap::new(),
            image_errors: std::collections::HashMap::new(),
            math_errors: std::collections::HashMap::new(),
            pdf_pending_pages: HashSet::new(),
            pdf_stale_pages: HashSet::new(),
            pdf_pending_links: HashSet::new(),
            pdf_render_generation: 0,
            pdf_programmatic_scroll: false,
            pdf_toc_target_page: None,
            md_toc_entries: Vec::new(),
            pdf_toc_entries_flat: None,
            split_view_active: false,
            split_ratio: 0.5,
            is_resizing_split: false,
            pdf_split_ratio: 0.3,
            active_panel: ActivePanel::Markdown,
            keyboard_modifiers: iced::keyboard::Modifiers::default(),
            window_width: 1200.0,
            window_height: 800.0,
            editor_scroll_y: 0.0,
            editor_viewport_width: 900.0,
            editor_viewport_height: 720.0,
        };

        app.load_shell_persistence();

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
            self.workspace
                .active_path
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
                        return Message::KeyboardShortcut(Shortcut::Submit);
                    }
                    if modifiers.alt() && (modifiers.command() || modifiers.control()) {
                        match key {
                            iced::keyboard::Key::Character(c) if c == "b" => {
                                return Message::KeyboardShortcut(Shortcut::ToggleBacklinks);
                            }
                            iced::keyboard::Key::Character(c) if c == "s" => {
                                return Message::KeyboardShortcut(Shortcut::StudyTracker);
                            }
                            _ => {}
                        }
                    }
                    if modifiers.alt() {
                        match key {
                            iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowLeft) => {
                                return Message::PdfNavBack;
                            }
                            iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowRight) => {
                                return Message::PdfNavForward;
                            }
                            iced::keyboard::Key::Character(c) if c == "p" => {
                                return Message::KeyboardShortcut(Shortcut::SwitchPane);
                            }
                            iced::keyboard::Key::Character(c) if c == "g" => {
                                return Message::KeyboardShortcut(Shortcut::FollowCitation);
                            }
                            iced::keyboard::Key::Character(c) if c == "u" => {
                                return Message::KeyboardShortcut(Shortcut::ShowUsages);
                            }
                            iced::keyboard::Key::Character(c) if c == "c" => {
                                return Message::KeyboardShortcut(Shortcut::CitationPalette);
                            }
                            iced::keyboard::Key::Character(c) if c == "e" => {
                                return Message::KeyboardShortcut(Shortcut::ExcerptModeToggle);
                            }
                            iced::keyboard::Key::Character(c) if c == "i" => {
                                return Message::KeyboardShortcut(Shortcut::ExcerptInsertBatch);
                            }
                            _ => {}
                        }
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
                            iced::keyboard::Key::Character(c) if c == "=" || c == "+" => {
                                return Message::KeyboardShortcut(Shortcut::ZoomIn);
                            }
                            iced::keyboard::Key::Character(c) if c == "-" => {
                                return Message::KeyboardShortcut(Shortcut::ZoomOut);
                            }
                            iced::keyboard::Key::Character(c) if c == "0" => {
                                return Message::KeyboardShortcut(Shortcut::ZoomFit);
                            }
                            iced::keyboard::Key::Character(c) if c == "g" => {
                                return Message::KeyboardShortcut(Shortcut::GoToPage);
                            }
                            iced::keyboard::Key::Character(c) if c == "r" => {
                                return Message::KeyboardShortcut(Shortcut::PdfSearch);
                            }
                            iced::keyboard::Key::Character(c) if c == "h" => {
                                return Message::KeyboardShortcut(Shortcut::PdfHighlight);
                            }
                            iced::keyboard::Key::Character(c) if c == "z" => {
                                return Message::KeyboardShortcut(Shortcut::PdfZoomInput);
                            }
                            _ => {}
                        }
                    }
                    match key {
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Home) => {
                            return Message::KeyboardShortcut(Shortcut::PdfFirstPage);
                        }
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::End) => {
                            return Message::KeyboardShortcut(Shortcut::PdfLastPage);
                        }
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

        let pdf_ctrl_scroll = iced::event::listen_with(|event, _status, _window_id| match event {
            iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => {
                Some(Message::KeyboardModifiersChanged(modifiers))
            }
            iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
                let zoom_delta = match delta {
                    iced::mouse::ScrollDelta::Lines { y, .. } => y * 0.1,
                    iced::mouse::ScrollDelta::Pixels { y, .. } => y * 0.001,
                };
                Some(Message::PdfWheelScrolledForZoom(zoom_delta))
            }
            _ => None,
        });

        let toast = if self.overlays.toast.is_some() {
            iced::time::every(std::time::Duration::from_secs(3)).map(|_| Message::ToastHide)
        } else {
            Subscription::none()
        };

        let highlight_debounce = if self.pending_highlight_generation.is_some() {
            iced::time::every(HIGHLIGHT_DEBOUNCE).map(|_| Message::HighlightDebounceElapsed)
        } else {
            Subscription::none()
        };

        let editor_autosave_timer = if self.pending_editor_save.is_some() {
            iced::time::every(std::time::Duration::from_millis(500))
                .map(|_| Message::EditorAutosaveElapsed)
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
            pdf_ctrl_scroll,
            toast,
            highlight_debounce,
            mouse_drag,
            editor_autosave_timer,
            window_events,
        ])
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
                self.index_registered_pdf_text_task()
            }
            Message::SidebarToggle => {
                self.toggle_sidebar_visible();
                Task::none()
            }
            Message::SidebarFileClicked(path) => {
                if self.showing_pdf && self.active_pdf_path.is_some() {
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
                    for line in self.buffer.text().lines() {
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

                    self.split_view_active = true;
                    self.showing_pdf = true;
                    self.set_active_panel(ActivePanel::Pdf);

                    if self.pdf_paths_match(self.active_pdf_path.as_deref(), &resolved_pdf_path) {
                        if let Some(ref ann_id) = target.annotation_id {
                            if let Some((target_page, _)) = self.find_pdf_annotation(ann_id) {
                                self.focused_annotation_id = Some(ann_id.clone());
                                return self.navigate_pdf_page(target_page);
                            }

                            self.pdf_initial_target_annotation = Some(ann_id.clone());
                            self.focused_annotation_id = Some(ann_id.clone());
                        }

                        if let Some(p) = target.page {
                            self.navigate_pdf_page(p.saturating_sub(1))
                        } else {
                            Task::none()
                        }
                    } else {
                        self.pdf_initial_target_page = target.page.map(|p| p.saturating_sub(1));
                        self.pdf_initial_target_annotation = target.annotation_id;
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
                                    &self.buffer.text(),
                                    &self.highlighted_lines,
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
                                        &self.buffer.text(),
                                        &self.highlighted_lines,
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
                                        &self.buffer.text(),
                                        &self.highlighted_lines,
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
                                        self.editor_scroll_y = 0.0;
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
                                self.showing_pdf = false;
                                self.open_file(&resolved_path)
                            } else if lower.ends_with(".pdf") {
                                self.active_pdf_path = Some(resolved_path.clone());
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
                self.workspace.toggle_folder(path);
                Task::none()
            }
            Message::CreateFileDialog => {
                self.overlays.active_modal = Some(views::modals::ModalType::CreateFile);
                self.overlays.modal_input.clear();
                self.overlays.link_note_picker_search.clear();
                Task::none()
            }
            Message::CreateFolderDialog => {
                self.overlays.active_modal = Some(views::modals::ModalType::CreateFolder);
                self.overlays.modal_input.clear();
                self.overlays.link_note_picker_search.clear();
                Task::none()
            }
            Message::DeleteFileDialog(path) => {
                self.overlays.active_modal = Some(views::modals::ModalType::Delete(path));
                Task::none()
            }
            Message::NameModalInputChanged(input) => {
                self.overlays.modal_input = input;
                Task::none()
            }
            Message::PdfLinkNoteFolderSelected(folder) => {
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
            Message::PdfLinkNoteFileSelected(path) => {
                if matches!(
                    self.overlays.active_modal,
                    Some(views::modals::ModalType::LinkNote(_))
                ) {
                    self.overlays.modal_input = normalize_note_path(&path);
                }
                Task::none()
            }
            Message::PdfLinkNotePickerSearchChanged(query) => {
                if matches!(
                    self.overlays.active_modal,
                    Some(views::modals::ModalType::LinkNote(_))
                ) {
                    self.overlays.link_note_picker_search = query;
                }
                Task::none()
            }
            Message::NameModalCancel => {
                self.overlays.close_modal();
                Task::none()
            }
            Message::NameModalSubmitCurrent => {
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
            Message::NameModalSubmit(input) => {
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
            Message::DeleteFile(path) => {
                match md_editor_core::vault::delete_entry(&self.state, &path) {
                    Ok(()) => {
                        self.workspace.vault_entries =
                            md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                        if self.workspace.active_path.as_deref() == Some(path.as_str()) {
                            self.workspace.active_path = None;
                            self.buffer = DocBuffer::new();
                            self.highlighted_lines.clear();
                        }
                        if self.active_pdf_path.as_deref() == Some(path.as_str()) {
                            self.active_pdf_path = None;
                            self.pdf_pages.clear();
                            self.pdf_dimensions.clear();
                            self.pdf_state.page_cache.clear();
                            self.pdf_toc_entries_flat = None;
                        }
                        self.overlays.active_modal = None;
                        self.overlays.link_note_picker_search.clear();
                        self.overlays.toast = Some("Deleted".to_string());
                    }
                    Err(err) => self.overlays.toast = Some(err),
                }
                Task::none()
            }

            Message::EditorCommand(command) => self.run_editor_command(command),
            Message::EditorCommandNoScroll(command) => {
                self.run_editor_command_with_scroll(command, false)
            }
            Message::MathRendered(tex, res) => {
                match res {
                    Ok(tuple) => {
                        self.math_errors.remove(&tex);
                        self.math_cache.insert(tex, tuple);
                    }
                    Err(err) => {
                        self.math_errors.insert(tex.clone(), err.clone());
                        self.overlays.toast = Some(format!("Math render failed: {err}"));
                    }
                }
                Task::none()
            }
            Message::ImageLoadFailed(path, err) => {
                self.image_errors.insert(path.clone(), err.clone());
                self.overlays.toast = Some(format!("Image load failed: {path}: {err}"));
                Task::none()
            }
            Message::EditorSave(is_autosave) => {
                self.pending_editor_save = None;
                if let Some(path) = &self.workspace.active_path {
                    let content = self.buffer.text();
                    let _ = save_markdown_file_with_parser_targets(&self.state, path, &content);
                    self.buffer.dirty = false;
                    if !is_autosave {
                        self.overlays.toast = Some("File saved".to_string());
                    }
                }
                Task::none()
            }
            Message::EditorCheckboxToggle(line_idx) => {
                self.run_editor_command(EditorCommand::ToggleCheckbox { line: line_idx })
            }
            Message::EditorBlockContextMenu {
                line_idx,
                absolute_pos,
            } => {
                if let Some(items) = crate::editor::renderer::get_block_context_menu_items(
                    &self.highlighted_lines,
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
            Message::EditorBlockAction { line_idx, action } => {
                self.overlays.active_modal = None;
                self.handle_editor_block_action(line_idx, action)
            }
            Message::EditorContextMenu {
                line_idx,
                col,
                absolute_pos,
            } => {
                // Build the link context-menu if the cursor lands on a link span.
                if let Some(line) = self.highlighted_lines.get(line_idx) {
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
                            if let Some(ref target) = span.link_target {
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
                                            link_target: target.clone(),
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
            Message::EditorLinkAction {
                line_idx,
                start_col,
                end_col,
                link_target,
                action,
            } => {
                self.overlays.active_modal = None;
                self.handle_editor_link_action(line_idx, start_col, end_col, link_target, action)
            }
            Message::EditorCursorMove(line, col) => {
                self.run_editor_command(EditorCommand::SetCursor { line, col })
            }
            Message::EditorScrolled {
                y,
                viewport_width,
                viewport_height,
            } => {
                self.set_active_panel(ActivePanel::Markdown);
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
            Message::EditorAutosaveElapsed => {
                if let Some(requested) = self.pending_editor_save {
                    if requested.elapsed() >= EDITOR_AUTOSAVE_DELAY {
                        self.pending_editor_save = None;
                        return Task::done(Message::EditorSave(true));
                    }
                }
                Task::none()
            }
            Message::HighlightDebounceElapsed => {
                if self
                    .pending_highlight_requested_at
                    .is_some_and(|requested| requested.elapsed() < HIGHLIGHT_DEBOUNCE)
                {
                    return Task::none();
                }
                let Some(generation) = self.pending_highlight_generation else {
                    return Task::none();
                };
                let Some(text) = self.pending_highlight_text.take() else {
                    self.pending_highlight_generation = None;
                    self.pending_highlight_requested_at = None;
                    return Task::none();
                };
                self.pending_highlight_generation = None;
                self.pending_highlight_requested_at = None;
                Self::highlight_task(generation, text)
            }
            Message::HighlightReady(generation, lines) => {
                if generation != self.highlight_generation {
                    return Task::none();
                }
                self.highlighted_lines = lines;
                self.md_toc_entries = views::toc::get_toc(&self.highlighted_lines);
                Task::batch(vec![self.load_images(), self.load_math()])
            }

            Message::KeyboardModifiersChanged(modifiers) => {
                self.keyboard_modifiers = modifiers;
                Task::none()
            }

            Message::PdfLoaded(generation, pages) => {
                if generation != self.pdf_render_generation {
                    return Task::none();
                }
                self.pdf_total_pages = pages;
                self.pdf_pages = vec![None; pages as usize];
                self.pdf_dimensions = vec![None; pages as usize];
                if self.pdf_state.page_sizes.len() != pages as usize {
                    self.pdf_state.page_sizes = vec![None; pages as usize];
                }
                self.pdf_state.layout = PdfLayout::rebuild(
                    &self.pdf_state.page_sizes,
                    self.pdf_state.zoom,
                    self.pdf_placeholder_display_size(),
                    PDF_PAGE_SPACING,
                    PDF_PAGE_LIST_PADDING,
                    self.pdf_rotation,
                );
                self.pdf_pending_pages.clear();
                self.pdf_stale_pages.clear();
                self.pdf_pending_links.clear();
                self.pdf_programmatic_scroll = false;
                self.pdf_toc_target_page = None;

                // Eagerly generate page-level TOC entries so the panel isn't
                // blank even if the bookmark extraction hasn't finished yet.
                let page_entries: Vec<views::toc::TocEntry> = (0..pages)
                    .map(|p| views::toc::TocEntry {
                        level: 1,
                        text: format!("Page {}", p + 1),
                        line: p as usize,
                    })
                    .collect();
                if self.pdf_toc_entries_flat.is_none() {
                    self.pdf_toc_entries_flat = Some(page_entries);
                }

                if pages == 0 {
                    self.overlays.toast = Some(
                        "PDF renderer is unavailable or the PDF could not be opened".to_string(),
                    );
                }
                if self.pdf_fit_to_width
                    && self
                        .pdf_state
                        .page_sizes
                        .iter()
                        .take(pages as usize)
                        .any(Option::is_some)
                {
                    Task::done(Message::PdfFitToWidth)
                } else if self.pdf_fit_to_page
                    && self
                        .pdf_state
                        .page_sizes
                        .iter()
                        .take(pages as usize)
                        .any(Option::is_some)
                {
                    Task::done(Message::PdfFitToPage)
                } else if self.pdf_fit_to_width || self.pdf_fit_to_page {
                    Task::none()
                } else {
                    self.render_all_pdf_pages()
                }
            }
            Message::PdfZoomChanged(zoom) => {
                let current_page = self.pdf_page_at_scroll(self.pdf_scroll_y);
                let page_start_offset = self.pdf_page_offset(current_page);
                let relative_ratio = if self.pdf_scroll_y < PDF_PAGE_LIST_PADDING {
                    0.0
                } else {
                    let page_height_old = self.pdf_page_height(current_page);
                    if page_height_old > 0.0 {
                        ((self.pdf_scroll_y - page_start_offset).max(0.0)) / page_height_old
                    } else {
                        0.0
                    }
                };

                self.pdf_fit_to_width = false;
                self.pdf_fit_to_page = false;
                self.pdf_state.zoom = zoom.clamp(0.5, 4.0);
                self.pdf_stale_pages = self
                    .pdf_pages
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, page)| page.as_ref().map(|_| idx as u16))
                    .collect();
                self.pdf_placeholder_page_size = self.first_pdf_page_size();
                self.pdf_pending_pages.clear();
                self.pdf_pending_links.clear();
                self.pdf_toc_target_page = Some(current_page);
                self.pdf_programmatic_scroll = true;
                self.pdf_render_generation = self.pdf_render_generation.wrapping_add(1);

                self.pdf_state.layout = PdfLayout::rebuild(
                    &self.pdf_state.page_sizes,
                    self.pdf_state.zoom,
                    self.pdf_placeholder_display_size(),
                    PDF_PAGE_SPACING,
                    PDF_PAGE_LIST_PADDING,
                    self.pdf_rotation,
                );
                self.update_pdf_page_cache();

                let new_scroll_y = if self.pdf_scroll_y < PDF_PAGE_LIST_PADDING {
                    self.pdf_scroll_y
                } else {
                    self.pdf_page_offset(current_page)
                        + relative_ratio * self.pdf_page_height(current_page)
                };
                self.pdf_scroll_y = new_scroll_y;

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
            Message::PdfWheelScrolledForZoom(delta) => {
                if self.active_pdf_path.is_some()
                    && self.showing_pdf
                    && (self.keyboard_modifiers.control() || self.keyboard_modifiers.command())
                    && delta.abs() > f32::EPSILON
                {
                    let next_zoom = (self.pdf_state.zoom + delta).clamp(0.5, 4.0);
                    if (next_zoom - self.pdf_state.zoom).abs() > f32::EPSILON {
                        Task::done(Message::PdfZoomChanged(next_zoom))
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
            Message::PdfFitToWidth => {
                let is_initial = self.pdf_initial_target_page.is_some();
                let current_page = if let Some(target_page) = self.pdf_initial_target_page.take() {
                    target_page.min(self.pdf_total_pages.saturating_sub(1))
                } else {
                    self.pdf_page_at_scroll(self.pdf_scroll_y)
                };
                let page_start_offset = self.pdf_page_offset(current_page);
                let relative_ratio = if is_initial {
                    0.0
                } else if self.pdf_scroll_y < PDF_PAGE_LIST_PADDING {
                    0.0
                } else {
                    let page_height_old = self.pdf_page_height(current_page);
                    if page_height_old > 0.0 {
                        ((self.pdf_scroll_y - page_start_offset).max(0.0)) / page_height_old
                    } else {
                        0.0
                    }
                };

                self.pdf_fit_to_width = true;
                self.pdf_fit_to_page = false;
                let available_width = self.pdf_available_width();
                let page_width = self
                    .pdf_state
                    .page_sizes
                    .iter()
                    .flatten()
                    .next()
                    .map(|(w, _)| (*w).max(1.0))
                    .or_else(|| {
                        self.pdf_dimensions
                            .iter()
                            .flatten()
                            .next()
                            .map(|(w, _)| (*w as f32 / self.pdf_state.zoom).max(1.0))
                    })
                    .unwrap_or(612.0);
                let next_zoom = ((available_width - 48.0).max(240.0) / page_width).clamp(0.5, 4.0);
                self.pdf_state.zoom = ((next_zoom * 100.0).round() / 100.0).clamp(0.5, 4.0);
                self.pdf_stale_pages = self
                    .pdf_pages
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, page)| page.as_ref().map(|_| idx as u16))
                    .collect();
                self.pdf_placeholder_page_size = self.first_pdf_page_size();
                self.pdf_pending_pages.clear();
                self.pdf_pending_links.clear();
                self.pdf_toc_target_page = Some(current_page);
                self.pdf_programmatic_scroll = true;
                self.pdf_render_generation = self.pdf_render_generation.wrapping_add(1);

                self.pdf_state.layout = PdfLayout::rebuild(
                    &self.pdf_state.page_sizes,
                    self.pdf_state.zoom,
                    self.pdf_placeholder_display_size(),
                    PDF_PAGE_SPACING,
                    PDF_PAGE_LIST_PADDING,
                    self.pdf_rotation,
                );
                self.update_pdf_page_cache();

                let new_scroll_y = if is_initial {
                    self.pdf_page_offset(current_page)
                } else if self.pdf_scroll_y < PDF_PAGE_LIST_PADDING {
                    self.pdf_scroll_y
                } else {
                    self.pdf_page_offset(current_page)
                        + relative_ratio * self.pdf_page_height(current_page)
                };
                self.pdf_scroll_y = new_scroll_y;
                if is_initial {
                    self.pdf_current_page = current_page;
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
            Message::PdfFitToPage => {
                let is_initial = self.pdf_initial_target_page.is_some();
                let current_page = if let Some(target_page) = self.pdf_initial_target_page.take() {
                    target_page.min(self.pdf_total_pages.saturating_sub(1))
                } else {
                    self.pdf_page_at_scroll(self.pdf_scroll_y)
                };
                let page_start_offset = self.pdf_page_offset(current_page);
                let relative_ratio = if is_initial {
                    0.0
                } else if self.pdf_scroll_y < PDF_PAGE_LIST_PADDING {
                    0.0
                } else {
                    let page_height_old = self.pdf_page_height(current_page);
                    if page_height_old > 0.0 {
                        ((self.pdf_scroll_y - page_start_offset).max(0.0)) / page_height_old
                    } else {
                        0.0
                    }
                };

                self.pdf_fit_to_page = true;
                self.pdf_fit_to_width = false;
                let available_width = self.pdf_available_width();
                let viewport_height = if self.pdf_viewport_height > 0.0 {
                    self.pdf_viewport_height
                } else {
                    self.estimated_editor_viewport_height()
                };

                let (page_width, page_height) = self
                    .pdf_state
                    .page_sizes
                    .iter()
                    .flatten()
                    .next()
                    .map(|(w, h)| ((*w).max(1.0), (*h).max(1.0)))
                    .or_else(|| {
                        self.pdf_dimensions.iter().flatten().next().map(|(w, h)| {
                            (
                                (*w as f32 / self.pdf_state.zoom).max(1.0),
                                (*h as f32 / self.pdf_state.zoom).max(1.0),
                            )
                        })
                    })
                    .unwrap_or((612.0, 792.0));

                let w_zoom = (available_width - 48.0).max(240.0) / page_width;
                let h_zoom = (viewport_height - 40.0).max(200.0) / page_height;
                let next_zoom = w_zoom.min(h_zoom).clamp(0.5, 4.0);
                self.pdf_state.zoom = ((next_zoom * 100.0).round() / 100.0).clamp(0.5, 4.0);
                self.pdf_stale_pages = self
                    .pdf_pages
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, page)| page.as_ref().map(|_| idx as u16))
                    .collect();
                self.pdf_placeholder_page_size = self.first_pdf_page_size();
                self.pdf_pending_pages.clear();
                self.pdf_pending_links.clear();
                self.pdf_toc_target_page = Some(current_page);
                self.pdf_programmatic_scroll = true;
                self.pdf_render_generation = self.pdf_render_generation.wrapping_add(1);

                self.pdf_state.layout = PdfLayout::rebuild(
                    &self.pdf_state.page_sizes,
                    self.pdf_state.zoom,
                    self.pdf_placeholder_display_size(),
                    PDF_PAGE_SPACING,
                    PDF_PAGE_LIST_PADDING,
                    self.pdf_rotation,
                );
                self.update_pdf_page_cache();

                let new_scroll_y = if is_initial {
                    self.pdf_page_offset(current_page)
                } else if self.pdf_scroll_y < PDF_PAGE_LIST_PADDING {
                    self.pdf_scroll_y
                } else {
                    self.pdf_page_offset(current_page)
                        + relative_ratio * self.pdf_page_height(current_page)
                };
                self.pdf_scroll_y = new_scroll_y;
                if is_initial {
                    self.pdf_current_page = current_page;
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
            Message::PdfRotateClockwise => {
                if self.active_pdf_path.is_some() && self.showing_pdf {
                    self.pdf_rotation = (self.pdf_rotation + 90) % 360;
                    self.pdf_state.page_cache.clear();
                    self.pdf_pages.fill(None);
                    self.pdf_dimensions.fill(None);
                    self.pdf_stale_pages.clear();
                    self.pdf_pending_pages.clear();
                    self.pdf_pending_links.clear();
                    self.pdf_state.layout = PdfLayout::rebuild(
                        &self.pdf_state.page_sizes,
                        self.pdf_state.zoom,
                        self.pdf_placeholder_display_size(),
                        PDF_PAGE_SPACING,
                        PDF_PAGE_LIST_PADDING,
                        self.pdf_rotation,
                    );
                    self.pdf_render_generation = self.pdf_render_generation.wrapping_add(1);

                    if self.pdf_fit_to_width {
                        Task::done(Message::PdfFitToWidth)
                    } else if self.pdf_fit_to_page {
                        Task::done(Message::PdfFitToPage)
                    } else {
                        self.render_visible_pdf_pages()
                    }
                } else {
                    Task::none()
                }
            }
            Message::PdfPageSizesLoaded(generation, path, sizes) => {
                if generation != self.pdf_render_generation
                    || self.active_pdf_path.as_deref() != Some(path.as_str())
                {
                    return Task::none();
                }
                self.pdf_state.page_sizes = sizes.into_iter().map(Some).collect();
                if self.pdf_state.page_sizes.len() < self.pdf_total_pages as usize {
                    self.pdf_state
                        .page_sizes
                        .resize(self.pdf_total_pages as usize, None);
                }
                if self.pdf_placeholder_page_size.is_none() {
                    self.pdf_placeholder_page_size = self.first_pdf_page_size();
                }
                self.pdf_state.layout = PdfLayout::rebuild(
                    &self.pdf_state.page_sizes,
                    self.pdf_state.zoom,
                    self.pdf_placeholder_display_size(),
                    PDF_PAGE_SPACING,
                    PDF_PAGE_LIST_PADDING,
                    self.pdf_rotation,
                );
                if self.pdf_fit_to_width && self.pdf_total_pages > 0 {
                    Task::done(Message::PdfFitToWidth)
                } else if self.pdf_fit_to_page && self.pdf_total_pages > 0 {
                    Task::done(Message::PdfFitToPage)
                } else if let Some(page) = self.pdf_initial_target_page.take() {
                    self.navigate_pdf_page(page)
                } else {
                    Task::none()
                }
            }
            Message::PdfRendered(generation, page, img) => {
                self.pdf_pending_pages.remove(&page);
                if generation != self.pdf_render_generation {
                    return Task::none();
                }
                let img = match self.pdf_rotation {
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
                if (page as usize) < self.pdf_pages.len() {
                    self.pdf_pages[page as usize] = Some(handle.clone());
                    self.pdf_dimensions[page as usize] = Some((logical_width, logical_height));
                    self.pdf_stale_pages.remove(&page);

                    // Insert into the LRU cache for bounded memory.
                    let byte_size = width as usize * height as usize * 4; // RGBA
                    self.pdf_state.page_cache.insert(
                        page,
                        handle,
                        (logical_width, logical_height),
                        byte_size,
                    );
                    self.sync_pdf_pages_to_cache();
                }
                if self.pdf_placeholder_page_size.is_none() || page == 0 {
                    self.pdf_placeholder_page_size = Some((
                        logical_width as f32 / self.pdf_state.zoom,
                        logical_height as f32 / self.pdf_state.zoom,
                    ));
                }
                let mut tasks = vec![self.load_pdf_page_links(page)];
                if !self.pdf_page_text.contains_key(&page) && !self.pdf_pending_text.contains(&page)
                {
                    tasks.push(self.load_pdf_page_text(page));
                }
                if self.pdf_toc_target_page == Some(page) {
                    let scroll_y = self.pdf_page_offset(page);
                    let current_scroll_y = self.pdf_scroll_y;
                    if (current_scroll_y - scroll_y).abs() < 5.0 {
                        self.pdf_toc_target_page = None;
                        self.pdf_programmatic_scroll = false;
                        self.pdf_current_page = page.min(self.pdf_total_pages.saturating_sub(1));
                        let start = page.saturating_sub(2);
                        let end = (page + 2).min(self.pdf_total_pages.saturating_sub(1));
                        self.update_pdf_page_cache();
                        tasks.push(self.render_pdf_page_range(start, end));
                    } else {
                        self.pdf_programmatic_scroll = true;
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
            Message::PdfRenderFailed(generation, page) => {
                self.pdf_pending_pages.remove(&page);
                if generation != self.pdf_render_generation {
                    return Task::none();
                }
                if self.pdf_toc_target_page == Some(page) {
                    self.pdf_toc_target_page = None;
                    self.pdf_programmatic_scroll = false;
                }
                self.overlays.toast = Some(format!("Could not render PDF page {}", page + 1));
                Task::none()
            }
            Message::PdfRenderSkipped(generation, page) => {
                self.pdf_pending_pages.remove(&page);
                if generation != self.pdf_render_generation {
                    return Task::none();
                }
                if self.pdf_toc_target_page == Some(page) {
                    self.pdf_toc_target_page = None;
                    self.pdf_programmatic_scroll = false;
                }
                Task::none()
            }
            Message::TocClicked(index) => {
                if self.workspace.active_path.is_some() {
                    if self.showing_pdf && self.active_pdf_path.is_some() {
                        self.push_pdf_navigation_history();
                    } else if self.workspace.active_path.is_some() {
                        self.push_markdown_navigation_history();
                    }
                    self.set_active_panel(ActivePanel::Markdown);
                    Task::done(Message::EditorCursorMove(index, 0))
                } else {
                    Task::none()
                }
            }
            Message::PdfTocClicked(index) => {
                if self.active_pdf_path.is_some() {
                    if self.showing_pdf && self.active_pdf_path.is_some() {
                        self.push_pdf_navigation_history();
                    } else if self.workspace.active_path.is_some() {
                        self.push_markdown_navigation_history();
                    }
                    let target_page = index
                        .min(self.pdf_total_pages.saturating_sub(1) as usize)
                        .max(0) as u16;
                    self.set_active_panel(ActivePanel::Pdf);
                    self.navigate_pdf_page(target_page)
                } else {
                    Task::none()
                }
            }
            Message::PdfScrolled { y, viewport_height } => {
                if (self.keyboard_modifiers.control() || self.keyboard_modifiers.command())
                    && !self.pdf_programmatic_scroll
                {
                    return operation::scroll_to(
                        iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                        AbsoluteOffset {
                            x: 0.0,
                            y: self.pdf_scroll_y,
                        },
                    );
                }
                self.pdf_viewport_height = viewport_height;
                self.set_active_panel(ActivePanel::Pdf);
                self.pdf_scroll_y = y;
                let new_page = self.pdf_page_at_scroll(y + viewport_height * 0.33);

                let target_page_ready = if let Some(target_page) = self.pdf_toc_target_page {
                    self.pdf_pages
                        .get(target_page as usize)
                        .is_some_and(|page| page.is_some())
                } else {
                    false
                };

                if self.pdf_programmatic_scroll {
                    if let Some(target_page) = self.pdf_toc_target_page {
                        let target_y = self.pdf_page_offset(target_page);
                        let max_scroll_y = (self.pdf_total_height() - viewport_height).max(0.0);
                        let expected_y = target_y.min(max_scroll_y);
                        if ((y - expected_y).abs() < 5.0 || new_page == target_page)
                            && target_page_ready
                        {
                            self.pdf_programmatic_scroll = false;
                        }
                    } else {
                        self.pdf_programmatic_scroll = false;
                    }
                } else {
                    self.pdf_toc_target_page = None;
                }

                if let Some(target_page) = self.pdf_toc_target_page {
                    let target_y = self.pdf_page_offset(target_page);
                    let max_scroll_y = (self.pdf_total_height() - viewport_height).max(0.0);
                    let expected_y = target_y.min(max_scroll_y);
                    if ((y - expected_y).abs() < 5.0 || new_page == target_page)
                        && target_page_ready
                    {
                        // Arrived! Clear programmatic scroll flags and render.
                        self.pdf_toc_target_page = None;
                        self.pdf_programmatic_scroll = false;
                        self.pdf_current_page =
                            target_page.min(self.pdf_total_pages.saturating_sub(1));
                        let start = self.pdf_current_page.saturating_sub(2);
                        let end =
                            (self.pdf_current_page + 2).min(self.pdf_total_pages.saturating_sub(1));
                        self.update_pdf_page_cache();
                        return self.render_pdf_page_range(start, end);
                    } else {
                        // Still scrolling programmatically to target. Skip rendering intermediate pages.
                        self.update_pdf_page_cache();
                        return Task::none();
                    }
                }

                if new_page != self.pdf_current_page && new_page < self.pdf_total_pages {
                    if new_page.abs_diff(self.pdf_current_page) > 8 {
                        self.pdf_pending_pages.clear();
                        self.pdf_pending_links.clear();
                    }
                    self.pdf_current_page = new_page;
                    let task = self.render_pdf_pages_for_viewport(y, viewport_height);
                    self.update_pdf_page_cache();
                    task
                } else {
                    let task = self.render_pdf_pages_for_viewport(y, viewport_height);
                    self.update_pdf_page_cache();
                    task
                }
            }
            Message::PdfLeftClicked(page_idx, x, y, modifiers) => {
                self.set_active_panel(ActivePanel::Pdf);
                if let Some(link) = self.pdf_link_at(page_idx, x, y) {
                    if let Some(dest_page) = link.dest_page {
                        self.push_pdf_navigation_history();
                        self.pdf_current_page =
                            dest_page.min(u32::from(self.pdf_total_pages.saturating_sub(1))) as u16;
                        self.navigate_pdf_page(self.pdf_current_page)
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
                    self.focused_annotation_id = Some(ann.id.clone());
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
                    self.focused_annotation_id = None;
                    Task::none()
                }
            }
            Message::PdfRightClicked {
                page_index,
                x,
                y,
                absolute_pos,
            } => {
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
                    if let Some(ref ann_id) = self.focused_annotation_id {
                        for page_anns in self.pdf_annotations.values() {
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
                            if let Some(path) = self.active_pdf_path.clone() {
                                if let Some(abs_path) = self.resolve_active_path(&path) {
                                    let abs_path = abs_path.to_string_lossy().to_string();
                                    let _state = self.state.clone();
                                    preview_task = Task::perform(
                                        async move {
                                            let renderer = _state.pdf_renderer()?;
                                            renderer
                                                .render_link_preview(&abs_path, dest_page, dest_y)
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
            Message::PdfContextMenuAction(action) => match action {
                views::modals::PdfContextMenuItem::Copy => {
                    if let Some(sel) = &self.pdf_selection {
                        if let Some(page_text) = self.pdf_page_text.get(&sel.page_index) {
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
                    if let Some(sel) = &self.pdf_selection {
                        if let Some(page_text) = self.pdf_page_text.get(&sel.page_index) {
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
                            md_editor_core::pdf::PdfAnnotationColor::Yellow
                        }
                        views::modals::PdfContextMenuItem::HighlightGreen => {
                            md_editor_core::pdf::PdfAnnotationColor::Green
                        }
                        views::modals::PdfContextMenuItem::HighlightBlue => {
                            md_editor_core::pdf::PdfAnnotationColor::Blue
                        }
                        views::modals::PdfContextMenuItem::HighlightPink => {
                            md_editor_core::pdf::PdfAnnotationColor::Pink
                        }
                        _ => md_editor_core::pdf::PdfAnnotationColor::Orange,
                    };
                    self.overlays.active_modal = None;
                    Task::done(Message::PdfCreateHighlight(color))
                }
                views::modals::PdfContextMenuItem::UnderlineBlue => {
                    self.overlays.active_modal = None;
                    Task::done(Message::PdfCreateAnnotation(
                        md_editor_core::pdf::PdfAnnotationKind::Underline,
                        md_editor_core::pdf::PdfAnnotationColor::Blue,
                    ))
                }
                views::modals::PdfContextMenuItem::StrikeRed => {
                    self.overlays.active_modal = None;
                    Task::done(Message::PdfCreateAnnotation(
                        md_editor_core::pdf::PdfAnnotationKind::Strike,
                        md_editor_core::pdf::PdfAnnotationColor::Red,
                    ))
                }
                views::modals::PdfContextMenuItem::SearchSelectedText => {
                    if let Some(sel) = &self.pdf_selection {
                        if let Some(page_text) = self.pdf_page_text.get(&sel.page_index) {
                            let start = sel.anchor_idx.min(sel.focus_idx);
                            let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
                            let selected = text_by_char_range(&page_text.text, start, end);
                            if !selected.trim().is_empty() {
                                self.pdf_state.search.query = selected.trim().to_string();
                                self.pdf_selection = None;
                                self.overlays.active_modal = None;
                                self.pdf_state.search.visible = true;
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
                        self.pdf_current_page =
                            dest_page.min(u32::from(self.pdf_total_pages.saturating_sub(1))) as u16;
                        self.navigate_pdf_page(self.pdf_current_page)
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
                self.overlays.toast = Some(format!("Preview Error: {}", e));
                Task::none()
            }
            Message::ClosePdfLinkPreview => {
                self.pdf_link_preview = None;
                self.overlays.active_modal = None;
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

                // Check if the PDF has any bookmarks at all
                let has_bookmarks = !entries.is_empty();
                let mut mapped = Vec::new();
                flatten_pdf_toc(&entries, 1, &mut mapped);

                // If the PDF has no bookmarks, fill with page entries (the eager
                // fallback in PdfLoaded already did this, but we may have had a
                // zero-page race earlier).
                if !has_bookmarks {
                    let current = self.pdf_toc_entries_flat.get_or_insert_with(Vec::new);
                    if current.is_empty() {
                        for p in 0..self.pdf_total_pages {
                            current.push(views::toc::TocEntry {
                                level: 1,
                                text: format!("Page {}", p + 1),
                                line: p as usize,
                            });
                        }
                    }
                } else if !mapped.is_empty() {
                    // PDF has bookmarks — replace page entries with real TOC.
                    self.pdf_toc_entries_flat = Some(mapped);
                }
                // else: PDF has bookmark structure but no valid page refs; keep
                // the eager page entries as fallback.
                Task::none()
            }
            Message::PdfPageLinksLoaded(generation, page, links) => {
                self.pdf_pending_links.remove(&page);
                if generation != self.pdf_render_generation {
                    return Task::none();
                }
                self.pdf_page_links.insert(page, links);
                Task::none()
            }

            Message::TrackerToggle => {
                self.tracker.toggle_visibility();
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
                Task::none()
            }
            Message::CommandPaletteOpen => {
                self.overlays.command_palette_visible = true;
                self.overlays.command_palette_query.clear();
                focus_command_palette_input()
            }
            Message::CommandPaletteQueryChanged(query) => {
                self.overlays.command_palette_query = query;
                Task::none()
            }
            Message::CommandPaletteCommandClicked(shortcut) => {
                self.overlays.close_command_palette();
                Task::done(Message::KeyboardShortcut(shortcut))
            }
            Message::CitationPaletteToggle => {
                self.overlays.citation_palette_visible = !self.overlays.citation_palette_visible;
                self.overlays.citation_palette_query.clear();
                if self.overlays.citation_palette_visible {
                    self.overlays.command_palette_visible = false;
                    self.search.visible = false;
                    return focus_citation_palette_input();
                }
                Task::none()
            }
            Message::CitationPaletteQueryChanged(query) => {
                self.overlays.citation_palette_query = query;
                Task::none()
            }
            Message::CitationPaletteSubmitFirst => self.submit_first_citation_palette_item(),
            Message::CitationPaletteChoose(item) => self.choose_citation_item(item),
            Message::ExcerptModeToggle => {
                self.overlays.excerpt_mode_active = !self.overlays.excerpt_mode_active;
                let status = if self.overlays.excerpt_mode_active {
                    "enabled"
                } else {
                    "disabled"
                };
                self.overlays.toast = Some(format!("Excerpt mode {status}"));
                Task::none()
            }
            Message::ExcerptQueueAdd(item) => {
                self.overlays.excerpts_queue.push(item);
                self.overlays.toast = Some("Excerpt added to queue".to_string());
                Task::none()
            }
            Message::ExcerptQueueRemove(idx) => {
                if idx < self.overlays.excerpts_queue.len() {
                    self.overlays.excerpts_queue.remove(idx);
                    self.overlays.toast = Some("Excerpt removed from queue".to_string());
                }
                Task::none()
            }
            Message::ExcerptQueueClear => {
                self.overlays.excerpts_queue.clear();
                self.overlays.toast = Some("Excerpt queue cleared".to_string());
                Task::none()
            }
            Message::ExcerptQueueInsertBatch => {
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
                        self.active_pdf_path.as_deref(),
                    ));
                }

                self.overlays.excerpts_queue.clear();
                self.set_active_panel(ActivePanel::Markdown);
                self.run_editor_command(crate::editor::buffer::EditorCommand::InsertText(
                    batch_text,
                ))
            }
            Message::TrackerStart => {
                self.tracker.start(std::time::Instant::now());
                self.overlays.toast = Some("Study timer started".to_string());
                Task::none()
            }
            Message::TrackerStop => {
                if let Some(started_at) = self.tracker.stop() {
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
                        self.tracker.sessions =
                            md_editor_core::tracker::get_sessions(&self.state).unwrap_or_default();
                        self.overlays.toast = Some("Study session saved".to_string());
                    }
                }
                Task::none()
            }
            Message::TrackerTabSelected(tab) => {
                self.tracker.tab = tab;
                Task::none()
            }
            Message::TrackerProjectStatusChanged(id, status) => {
                let key = format!("proj_{}", id);
                if md_editor_core::tracker::set_kv(&self.state, &key, &status).is_ok() {
                    self.tracker.kv.insert(key, status);
                }
                Task::none()
            }
            Message::TrackerGateToggled(gate_id, item_idx) => {
                let key = format!("gate_{}_{}", gate_id, item_idx);
                let next = if self
                    .tracker
                    .kv
                    .get(&key)
                    .map(|v| v == "true")
                    .unwrap_or(false)
                {
                    "false"
                } else {
                    "true"
                };
                if md_editor_core::tracker::set_kv(&self.state, &key, next).is_ok() {
                    self.tracker.kv.insert(key, next.to_string());
                }
                Task::none()
            }
            Message::TrackerReadingToggled(section, item_idx) => {
                let key = format!("read_{}_{}", section, item_idx);
                let next = if self
                    .tracker
                    .kv
                    .get(&key)
                    .map(|v| v == "true")
                    .unwrap_or(false)
                {
                    "false"
                } else {
                    "true"
                };
                if md_editor_core::tracker::set_kv(&self.state, &key, next).is_ok() {
                    self.tracker.kv.insert(key, next.to_string());
                }
                Task::none()
            }
            Message::TrackerConfigEdited(action) => {
                self.tracker.edit_config(action);
                Task::none()
            }
            Message::TrackerConfigSave => {
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
                    Err(err) => {
                        self.overlays.toast = Some(format!("Invalid tracker JSON: {}", err))
                    }
                }
                Task::none()
            }
            Message::TrackerManualDateChanged(value) => {
                self.tracker.manual_date = value;
                Task::none()
            }
            Message::TrackerManualHoursChanged(value) => {
                self.tracker.manual_hours = value;
                Task::none()
            }
            Message::TrackerManualNotesChanged(value) => {
                self.tracker.manual_notes = value;
                Task::none()
            }
            Message::TrackerManualAdd => {
                match self.tracker.manual_hours.trim().parse::<f32>() {
                    Ok(hours) if hours > 0.0 => {
                        let session = md_editor_core::tracker::StudySession {
                            id: 0,
                            date: self.tracker.manual_date.trim().to_string(),
                            hours,
                            activity_type: "Manual".to_string(),
                            phase: "Manual".to_string(),
                            notes: (!self.tracker.manual_notes.trim().is_empty())
                                .then(|| self.tracker.manual_notes.trim().to_string()),
                        };
                        match md_editor_core::tracker::save_session(&self.state, session) {
                            Ok(()) => {
                                self.tracker.sessions =
                                    md_editor_core::tracker::get_sessions(&self.state)
                                        .unwrap_or_default();
                                self.tracker.manual_hours.clear();
                                self.tracker.manual_notes.clear();
                                self.overlays.toast =
                                    Some("Manual study session added".to_string());
                            }
                            Err(err) => self.overlays.toast = Some(err),
                        }
                    }
                    _ => self.overlays.toast = Some("Enter a positive hour value".to_string()),
                }
                Task::none()
            }
            Message::TrackerSessionDelete(id) => {
                match md_editor_core::tracker::delete_session(&self.state, id) {
                    Ok(()) => {
                        self.tracker.sessions =
                            md_editor_core::tracker::get_sessions(&self.state).unwrap_or_default();
                        self.overlays.toast = Some("Session deleted".to_string());
                    }
                    Err(err) => self.overlays.toast = Some(err),
                }
                Task::none()
            }

            Message::GlobalSearchOpen => {
                self.search.visible = true;
                if self.active_pdf_path.is_some() && !self.pdf_state.search.query.trim().is_empty()
                {
                    Task::batch(vec![self.search_pdf(), focus_global_search_input()])
                } else {
                    focus_global_search_input()
                }
            }
            Message::SearchClose => {
                self.search.visible = false;
                self.search.global.id = self.search.global.id.wrapping_add(1);
                self.search.editor.visible = false;
                self.pdf_state.search.visible = false;
                self.cancel_global_pdf_search();
                self.search.global.results.clear();
                self.search.global.error = None;
                self.restore_scroll_positions()
            }
            Message::SearchQueryChanged(q) => {
                if self.pdf_search_is_active() {
                    self.pdf_state.search.query = q.clone();
                    self.pdf_state.search.active_index = None;
                    self.search.pdf_error = None;
                    if q.len() > 1 {
                        self.search_pdf()
                    } else {
                        self.pdf_state.search.matches.clear();
                        self.pdf_state.search.page_index.clear();
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
                                Ok(matches) => Message::UnifiedSearchMatchesFound(id, matches),
                                Err(err) => Message::UnifiedSearchFinished(id, Err(err)),
                            },
                        );

                        self.search.global.results.clear();

                        let active_pdf_task = if self.active_pdf_path.is_some()
                            && include_pdf_content
                        {
                            self.pdf_state.search.query = q.clone();
                            self.pdf_state.search.active_index = None;
                            self.search.pdf_error = None;
                            let task = self.search_pdf();
                            if self.pdf_state.search.searching {
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
                        if self.active_pdf_path.is_some() {
                            self.pdf_state.search.query = q.clone();
                            self.pdf_state.search.matches.clear();
                            self.pdf_state.search.page_index.clear();
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
            Message::SearchReplaceChanged(replace) => {
                self.search.editor.replace = replace;
                Task::none()
            }
            Message::SearchRegexToggled(value) => {
                if self.pdf_search_is_active() {
                    self.pdf_state.search.regex = value;
                    self.pdf_state.search.active_index = None;
                    if self.pdf_state.search.query.len() > 1 {
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
            Message::SearchMatchCaseToggled(value) => {
                if self.pdf_search_is_active() {
                    self.pdf_state.search.match_case = value;
                    self.pdf_state.search.active_index = None;
                    if self.pdf_state.search.query.len() > 1 {
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
            Message::UnifiedSearchSourceToggled(source, enabled) => {
                if enabled {
                    if !self.search.global.sources.contains(&source) {
                        self.search.global.sources.push(source);
                    }
                } else {
                    self.search.global.sources.retain(|item| *item != source);
                }

                if self.search.visible {
                    Task::done(Message::SearchQueryChanged(
                        self.search.editor.query.clone(),
                    ))
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
                    self.overlays.toast = Some(format!("Replaced {} matches", count));
                    task
                }
                Err(err) => {
                    self.overlays.toast = Some(err);
                    Task::none()
                }
            },
            Message::SearchReplace => match self.replace_current_match() {
                Ok(task) => task,
                Err(err) => {
                    self.overlays.toast = Some(err);
                    Task::none()
                }
            },

            Message::PdfSearchMatchesFound(search_id, matches) => {
                if search_id == self.search.pdf_active_id {
                    if self.search.visible && self.search.global.pdf_search_id == Some(search_id) {
                        if let Some(ref pdf_path) = self.active_pdf_path {
                            let query_lower = self.search.editor.query.to_lowercase();
                            let query_trimmed = self.search.editor.query.trim();

                            let is_linked =
                                |p1: &str, p2: &str| self.state.vault_paths_are_linked(p1, p2);

                            let match_index_base = self.pdf_state.search.matches.len();
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

                    self.pdf_state.search.matches.extend(matches);
                    self.rebuild_pdf_search_page_index();
                    if self.pdf_state.search.active_index.is_none()
                        && !self.pdf_state.search.matches.is_empty()
                        && !self.search.visible
                    {
                        self.pdf_state.search.active_index = Some(0);
                        self.navigate_pdf_search_to_index(0)
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
            Message::UnifiedSearchMatchesFound(search_id, matches) => {
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
            Message::UnifiedPdfTextSearchMatchesFound(search_id, batch) => {
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
            Message::UnifiedSearchFinished(search_id, result) => {
                if search_id == self.search.global.id {
                    self.search.global.pending_db = false;
                    if let Err(err) = result {
                        self.search.global.error = Some(err);
                    }
                    self.update_global_search_searching();
                }
                Task::none()
            }
            Message::UnifiedSearchResultClicked(result) => {
                if self.showing_pdf && self.active_pdf_path.is_some() {
                    self.push_pdf_navigation_history();
                } else if self.workspace.active_path.is_some() {
                    self.push_markdown_navigation_history();
                }
                self.search.visible = false;

                match result.group {
                    md_editor_core::types::SearchResultGroup::MarkdownContent
                    | md_editor_core::types::SearchResultGroup::Heading => {
                        let open_task = self.open_file(&result.path);
                        let cursor_task =
                            Task::done(Message::EditorCursorMove(result.line.saturating_sub(1), 0));
                        Task::batch(vec![open_task, cursor_task])
                    }
                    md_editor_core::types::SearchResultGroup::Filename => {
                        if result.path.ends_with(".pdf") {
                            if self.pdf_paths_match(self.active_pdf_path.as_deref(), &result.path) {
                                self.set_active_panel(ActivePanel::Pdf);
                                self.showing_pdf = true;
                                Task::none()
                            } else {
                                self.open_pdf(&result.path)
                            }
                        } else {
                            self.open_file(&result.path)
                        }
                    }
                    md_editor_core::types::SearchResultGroup::PdfContent => {
                        if self.pdf_paths_match(self.active_pdf_path.as_deref(), &result.path) {
                            self.set_active_panel(ActivePanel::Pdf);
                            self.showing_pdf = true;
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
                            self.pdf_initial_target_page = Some(page);
                            self.open_pdf(&result.path)
                        }
                    }
                    md_editor_core::types::SearchResultGroup::Annotation
                    | md_editor_core::types::SearchResultGroup::QuickNote => {
                        if self.pdf_paths_match(self.active_pdf_path.as_deref(), &result.path) {
                            self.set_active_panel(ActivePanel::Pdf);
                            self.showing_pdf = true;
                            let page = result.page_index.unwrap_or(0);
                            self.focused_annotation_id = result.annotation_id.clone();
                            self.navigate_pdf_page(page)
                        } else {
                            let page = result.page_index.unwrap_or(0);
                            self.pdf_initial_target_page = Some(page);
                            self.pdf_initial_target_annotation = result.annotation_id.clone();
                            self.open_pdf(&result.path)
                        }
                    }
                }
            }
            Message::PdfTextIndexFinished(result) => {
                if let Err(err) = result {
                    self.search.global.error = Some(err);
                }
                Task::none()
            }
            Message::PdfSearchFinished(search_id, result) => {
                if search_id == self.search.pdf_active_id {
                    self.pdf_state.search.searching = false;
                    if self.search.global.pdf_search_id == Some(search_id) {
                        self.search.global.pending_pdf = false;
                        self.search.global.pdf_search_id = None;
                        self.update_global_search_searching();
                    }
                    match result {
                        Ok(()) => Task::none(),
                        Err(err) => {
                            self.search.pdf_error = Some(err);
                            self.pdf_state.search.matches.clear();
                            self.pdf_state.search.page_index.clear();
                            Task::none()
                        }
                    }
                } else {
                    Task::none()
                }
            }
            Message::PdfSearchResultClicked(page) => {
                self.search.visible = false;
                self.pdf_state.search.visible = true;
                self.set_active_panel(ActivePanel::Pdf);
                self.pdf_state.search.active_index = self
                    .pdf_state
                    .search
                    .matches
                    .iter()
                    .position(|result| result.page_index == page);
                if let Some(index) = self.pdf_state.search.active_index {
                    self.navigate_pdf_search_to_index(index)
                } else {
                    self.pdf_current_page = page.min(self.pdf_total_pages.saturating_sub(1));
                    self.navigate_pdf_page(self.pdf_current_page)
                }
            }
            Message::PdfScrollBy(delta) => {
                if self.active_pdf_path.is_none()
                    || (!self.showing_pdf
                        && !(self.split_view_active && self.workspace.active_path.is_some()))
                    || (self.split_view_active
                        && self.workspace.active_path.is_some()
                        && self.active_panel != ActivePanel::Pdf)
                    || self.search.visible
                    || self.search.editor.visible
                    || self.pdf_state.search.visible
                    || self.overlays.active_modal.is_some()
                    || self.overlays.command_palette_visible
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
            Message::PdfFirstPage => {
                if self.showing_pdf && self.pdf_total_pages > 0 {
                    self.navigate_pdf_page(0)
                } else {
                    Task::none()
                }
            }
            Message::PdfLastPage => {
                if self.showing_pdf && self.pdf_total_pages > 0 {
                    self.navigate_pdf_page(self.pdf_total_pages.saturating_sub(1))
                } else {
                    Task::none()
                }
            }
            Message::PdfNavBack => {
                let current_target = if self.showing_pdf && self.active_pdf_path.is_some() {
                    Some(NavigationTarget::Pdf {
                        path: self.active_pdf_path.clone().unwrap(),
                        page: self.pdf_current_page,
                        scroll_offset: self.pdf_scroll_y,
                        zoom: self.pdf_state.zoom,
                    })
                } else {
                    self.workspace
                        .active_path
                        .as_ref()
                        .map(|path| NavigationTarget::Markdown {
                            path: path.clone(),
                            line: self.buffer.cursor_line,
                            column: self.buffer.cursor_col,
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
            Message::PdfNavForward => {
                if let Some(target) = self.workspace.navigation_history.go_forward() {
                    self.navigate_to_target(target)
                } else {
                    Task::none()
                }
            }
            Message::PdfSearchToggle => {
                if self.showing_pdf {
                    if self.pdf_state.search.visible {
                        self.pdf_state.search.visible = false;
                        self.pdf_state.search.matches.clear();
                        self.pdf_state.search.page_index.clear();
                    } else {
                        self.pdf_state.search.visible = true;
                        self.search.visible = false;
                    }
                    Task::none()
                } else {
                    Task::none()
                }
            }
            Message::PdfGoToPage => {
                if self.active_pdf_path.is_some() && self.showing_pdf && self.pdf_total_pages > 0 {
                    self.overlays.active_modal = Some(views::modals::ModalType::GoToPage {
                        total: self.pdf_total_pages,
                        error: None,
                    });
                    self.overlays.modal_input.clear();
                    Task::none()
                } else {
                    Task::none()
                }
            }
            Message::PdfDocumentIdComputed(Some((path, hash, len, mtime))) => {
                let _ = self.state.save_pdf_document(&hash, &path, len, mtime);
                self.pdf_document_id = Some(hash.clone());

                let annotations = self
                    .state
                    .get_pdf_annotations(&hash, None)
                    .unwrap_or_default();
                self.pdf_annotations.clear();
                for ann in annotations {
                    self.pdf_annotations
                        .entry(ann.page_index)
                        .or_default()
                        .push(ann);
                }

                let mut target_page = None;
                if let Some(ref target_id) = self.pdf_initial_target_annotation {
                    for (page_idx, page_anns) in &self.pdf_annotations {
                        if page_anns.iter().any(|a| &a.id == target_id) {
                            target_page = Some(*page_idx);
                            self.focused_annotation_id = Some(target_id.clone());
                            break;
                        }
                    }
                }

                let scroll_task = if self.pdf_total_pages > 0 {
                    if let Some(page) = target_page {
                        self.pdf_initial_target_page = None;
                        self.pdf_initial_target_annotation = None;
                        self.navigate_pdf_page(page)
                    } else if let Some(page) = self.pdf_initial_target_page {
                        self.pdf_initial_target_page = None;
                        self.navigate_pdf_page(page)
                    } else {
                        Task::none()
                    }
                } else {
                    if let Some(page) = target_page {
                        self.pdf_initial_target_page = Some(page);
                        self.pdf_initial_target_annotation = None;
                    }
                    Task::none()
                };

                scroll_task
            }
            Message::PdfDocumentIdComputed(None) => Task::none(),
            Message::PdfPageTextLoaded(generation, page, res) => {
                self.pdf_pending_text.remove(&page);
                if generation == self.pdf_render_generation {
                    if let Ok(page_text) = res {
                        if let Some(path) = self.active_pdf_path.as_deref() {
                            let _ = self.state.save_pdf_page_text(path, page, &page_text.text);
                        }
                        self.pdf_page_text.insert(page, page_text);
                        self.pdf_text_lru.push_back(page);
                        if self.pdf_text_lru.len() > PDF_TEXT_PAGE_CACHE_LIMIT {
                            if let Some(oldest) = self.pdf_text_lru.pop_front() {
                                self.pdf_page_text.remove(&oldest);
                            }
                        }
                    }
                }
                Task::none()
            }
            Message::PdfSelectionChanged(page, anchor, focus) => {
                self.set_active_panel(ActivePanel::Pdf);
                self.pdf_selection = Some(views::interactive_pdf::PdfSelection {
                    page_index: page,
                    anchor_idx: anchor,
                    focus_idx: focus,
                });
                Task::none()
            }
            Message::PdfSelectionCleared => {
                self.pdf_selection = None;
                Task::none()
            }
            Message::PdfSelectionFinished(page, anchor, focus) => {
                self.set_active_panel(ActivePanel::Pdf);
                self.pdf_selection = Some(views::interactive_pdf::PdfSelection {
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
                if let Some(sel) = &self.pdf_selection {
                    if let Some(page_text) = self.pdf_page_text.get(&sel.page_index) {
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
            Message::PdfInsertQuoteLink => {
                if self.workspace.active_path.is_none() {
                    self.overlays.toast =
                        Some("Open a markdown file before inserting a quote link".to_string());
                    return Task::none();
                }
                if self.overlays.excerpt_mode_active {
                    if let Some(sel) = &self.pdf_selection {
                        if let Some(page_text) = self.pdf_page_text.get(&sel.page_index) {
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
            Message::PdfInsertAnnotationLink(annotation_id) => {
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
            Message::PdfCreateHighlight(color) => Task::done(Message::PdfCreateAnnotation(
                md_editor_core::pdf::PdfAnnotationKind::Highlight,
                color,
            )),
            Message::PdfCreateAnnotation(kind, color) => {
                if let (Some(sel), Some(doc_id)) = (&self.pdf_selection, &self.pdf_document_id) {
                    if let Some(page_text) = self.pdf_page_text.get(&sel.page_index) {
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
                            kind,
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
                            tags: Vec::new(),
                            status: md_editor_core::pdf::PdfAnnotationStatus::Unresolved,
                            created_at: now,
                            updated_at: now,
                        };

                        if let Err(e) = self.state.save_pdf_annotation(&ann) {
                            self.overlays.toast = Some(format!("Failed to save annotation: {}", e));
                        } else {
                            self.pdf_annotations
                                .entry(sel.page_index)
                                .or_default()
                                .push(ann);
                            self.pdf_selection = None;
                            if let Some(ref path) = self.active_pdf_path {
                                self.workspace.backlinks =
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
                    self.overlays.toast = Some(format!("Failed to delete highlight: {}", e));
                } else {
                    for page_anns in self.pdf_annotations.values_mut() {
                        page_anns.retain(|a| a.id != id);
                    }
                    if self.focused_annotation_id.as_ref() == Some(&id) {
                        self.focused_annotation_id = None;
                    }
                    if let Some(views::modals::ModalType::QuickNote(ref mid)) =
                        self.overlays.active_modal
                    {
                        if mid == &id {
                            self.overlays.active_modal = None;
                            self.overlays.modal_input.clear();
                        }
                    }
                    if let Some(ref path) = self.active_pdf_path {
                        self.workspace.backlinks =
                            md_editor_core::vault::get_mixed_backlinks(&self.state, path)
                                .unwrap_or_default();
                    }
                }
                Task::none()
            }
            Message::PdfAddQuickNote(id, note_content) => {
                let mut found_ann = None;
                for page_anns in self.pdf_annotations.values_mut() {
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
                        if let Some(ref path) = self.active_pdf_path {
                            self.workspace.backlinks =
                                md_editor_core::vault::get_mixed_backlinks(&self.state, path)
                                    .unwrap_or_default();

                            if let Some(ref note_path) = ann.linked_note_path {
                                if let Ok(bytes) =
                                    md_editor_core::vault::open_file(&self.state, note_path)
                                {
                                    if let Ok(existing_content) = String::from_utf8(bytes) {
                                        let updated_content =
                                            crate::pdf_notes::sync_annotation_note_in_markdown(
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
                                                self.buffer =
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
            Message::PdfLinkNote(annotation_id, mut note_path) => {
                let mut annotation = None;
                for page_anns in self.pdf_annotations.values() {
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
                    if let Some(ref pdf_path) = self.active_pdf_path {
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
                        for page_anns in self.pdf_annotations.values_mut() {
                            if let Some(a) = page_anns.iter_mut().find(|a| a.id == annotation_id) {
                                a.linked_note_path = Some(note_path.clone());
                                a.updated_at = ann.updated_at;
                                break;
                            }
                        }
                        self.workspace.vault_entries =
                            md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                        if let Some(ref pdf_path) = self.active_pdf_path {
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
            Message::PdfOpenLinkedNote(note_path) => {
                self.split_view_active = true;
                let open_task = self.open_file_extended(&note_path, false);
                if self.pdf_fit_to_width {
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
                    self.workspace.vault_root.as_deref(),
                    self.workspace.active_path.as_deref(),
                    &document_path,
                );

                self.split_view_active = true;
                self.showing_pdf = true;

                if self.active_pdf_path.as_deref() == Some(&resolved_pdf_path) {
                    self.focused_annotation_id = Some(annotation_id);
                    self.navigate_pdf_page(page.saturating_sub(1))
                } else {
                    self.pdf_initial_target_page = Some(page.saturating_sub(1));
                    self.pdf_initial_target_annotation = Some(annotation_id);
                    self.open_pdf(&resolved_pdf_path)
                }
            }
            Message::SearchResultClicked(path) => {
                if self.showing_pdf && self.active_pdf_path.is_some() {
                    self.push_pdf_navigation_history();
                } else if self.workspace.active_path.is_some() {
                    self.push_markdown_navigation_history();
                }
                self.search.visible = false;
                self.open_file(&path)
            }

            Message::ToastHide => {
                self.overlays.toast = None;
                Task::none()
            }
            Message::KeyboardShortcut(s) => {
                match s {
                    Shortcut::Escape => {
                        // Close overlays in priority order
                        if self.pdf_selection.is_some() {
                            self.pdf_selection = None;
                        } else if self.focused_annotation_id.is_some() {
                            self.focused_annotation_id = None;
                        } else if self.pdf_link_preview.is_some() {
                            self.pdf_link_preview = None;
                            self.overlays.active_modal = None;
                        } else if self.overlays.active_modal.is_some() {
                            self.overlays.close_modal();
                        } else if self.tracker.visible {
                            self.tracker.visible = false;
                        } else if self.search.editor.visible || self.pdf_state.search.visible {
                            self.search.editor.visible = false;
                            self.pdf_state.search.visible = false;
                            return self.restore_scroll_positions();
                        } else if self.search.visible {
                            self.search.visible = false;
                            return self.restore_scroll_positions();
                        } else if self.overlays.command_palette_visible {
                            self.overlays.command_palette_visible = false;
                        } else if self.overlays.citation_palette_visible {
                            self.overlays.citation_palette_visible = false;
                        } else if self.toc_visible {
                            self.toc_visible = false;
                        }
                        Task::none()
                    }
                    Shortcut::ToggleSidebar => {
                        self.toggle_sidebar_visible();
                        Task::none()
                    }
                    Shortcut::NavBack => Task::done(Message::PdfNavBack),
                    Shortcut::NavForward => Task::done(Message::PdfNavForward),
                    Shortcut::Save => Task::done(Message::EditorSave(false)),
                    Shortcut::OpenVault => Task::done(Message::OpenVaultDialog),
                    Shortcut::NewFile => Task::done(Message::CreateFileDialog),
                    Shortcut::Search => {
                        if self.split_view_active && self.workspace.active_path.is_some() {
                            if self.active_panel == ActivePanel::Pdf
                                && self.active_pdf_path.is_some()
                            {
                                self.pdf_state.search.visible = !self.pdf_state.search.visible;
                                self.search.editor.visible = false;
                                self.search.visible = false;
                                if self.pdf_state.search.visible {
                                    if !self.pdf_state.search.query.trim().is_empty() {
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
                                self.pdf_state.search.visible = false;
                                self.search.visible = false;
                                if self.search.editor.visible {
                                    return Task::batch(vec![
                                        focus_file_search_input(),
                                        self.restore_scroll_positions(),
                                    ]);
                                }
                            }
                        } else if self.active_pdf_path.is_some() && self.showing_pdf {
                            self.pdf_state.search.visible = !self.pdf_state.search.visible;
                            self.search.editor.visible = false;
                            self.search.visible = false;
                            if self.pdf_state.search.visible {
                                if !self.pdf_state.search.query.trim().is_empty() {
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
                            self.pdf_state.search.visible = false;
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
                        if self.workspace.active_path.is_some() || self.active_pdf_path.is_some() {
                            self.toc_visible = !self.toc_visible;
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
                        if self.split_view_active
                            && self.workspace.active_path.is_some()
                            && self.active_pdf_path.is_some()
                        {
                            let next_panel = match self.active_panel {
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
                        self.sidebar_visible = false;
                        self.workspace.backlinks_visible = false;
                        self.toc_visible = false;
                        self.tracker.visible = false;
                        self.pdf_annotations_visible = false;
                        self.persist_shell_state();
                        Task::none()
                    }
                    Shortcut::ZoomIn => {
                        if self.active_pdf_path.is_some() && self.showing_pdf {
                            let new_zoom = (self.pdf_state.zoom + 0.1).min(4.0);
                            Task::done(Message::PdfZoomChanged(new_zoom))
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::ZoomOut => {
                        if self.active_pdf_path.is_some() && self.showing_pdf {
                            let new_zoom = (self.pdf_state.zoom - 0.1).max(0.5);
                            Task::done(Message::PdfZoomChanged(new_zoom))
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::ZoomFit => {
                        if self.active_pdf_path.is_some() && self.showing_pdf {
                            Task::done(Message::PdfFitToWidth)
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::GoToPage => {
                        if self.active_pdf_path.is_some()
                            && self.showing_pdf
                            && self.pdf_total_pages > 0
                        {
                            self.overlays.active_modal = Some(views::modals::ModalType::GoToPage {
                                total: self.pdf_total_pages,
                                error: None,
                            });
                            self.overlays.modal_input.clear();
                            Task::none()
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::PdfSearch => {
                        if self.showing_pdf {
                            Task::done(Message::PdfSearchToggle)
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::PdfHighlight => {
                        if self.showing_pdf {
                            if self.pdf_selection.is_some() {
                                let color = md_editor_core::pdf::PdfAnnotationColor::Yellow;
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
                        if let Some(annotation_id) = self.focused_annotation_id.clone() {
                            Task::done(Message::PdfInsertAnnotationLink(annotation_id))
                        } else {
                            self.overlays.toast =
                                Some("Select a PDF highlight before inserting it".to_string());
                            Task::none()
                        }
                    }
                    Shortcut::PdfFirstPage => {
                        if self.showing_pdf {
                            Task::done(Message::PdfFirstPage)
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::PdfLastPage => {
                        if self.showing_pdf {
                            Task::done(Message::PdfLastPage)
                        } else {
                            Task::none()
                        }
                    }
                    Shortcut::PdfZoomInput => {
                        if self.showing_pdf {
                            self.overlays.active_modal = Some(views::modals::ModalType::GoToPage {
                                total: self.pdf_total_pages,
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
            Message::SplitViewToggle => {
                if self.workspace.active_path.is_some() && self.active_pdf_path.is_some() {
                    self.split_view_active = !self.split_view_active;
                    self.persist_shell_state();
                    if self.pdf_fit_to_width {
                        return Task::done(Message::PdfFitToWidth);
                    } else if self.pdf_fit_to_page {
                        return Task::done(Message::PdfFitToPage);
                    }
                } else if self.workspace.active_path.is_some() {
                    if let Ok(Some(last_pdf)) =
                        md_editor_core::config::get_sys_config(&self.state, "last_pdf")
                    {
                        self.split_view_active = true;
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
            Message::SplitViewDragStart => {
                self.is_resizing_split = true;
                // Also start PDF split resize if showing PDF
                if self.showing_pdf && self.active_pdf_path.is_some() {
                    let has_split =
                        !self.sidebar_visible && !self.tracker.visible && !self.toc_visible;
                    if has_split || self.split_view_active {
                        self.pdf_split_ratio = 0.3;
                    }
                }
                Task::none()
            }
            Message::SplitViewDragging(x_pos) => {
                if !self.is_resizing_split {
                    return Task::none();
                }
                // If PDF-only mode (no split view), resize page list
                if self.showing_pdf && self.active_pdf_path.is_some() && !self.split_view_active {
                    let content_width = (self.window_width - 250.0).max(480.0); // sidebar width
                    let x_min = 300.0;
                    let x_max = content_width - 300.0;
                    let total_width = x_max - x_min;
                    if total_width > 1.0 {
                        self.pdf_split_ratio = ((x_pos - x_min) / total_width).clamp(0.15, 0.75);
                    }
                    return Task::none();
                }
                let side_width = if self.sidebar_visible { 250.0 } else { 0.0 }
                    + if self.tracker.visible { 300.0 } else { 0.0 }
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
                    self.persist_shell_state();
                    return Task::done(Message::PdfFitToWidth);
                } else if self.pdf_fit_to_page && self.active_pdf_path.is_some() {
                    self.persist_shell_state();
                    return Task::done(Message::PdfFitToPage);
                }
                self.persist_shell_state();
                Task::none()
            }
            Message::WindowResized(width, height) => {
                self.window_width = width;
                self.window_height = height;
                if self.pdf_fit_to_width && self.active_pdf_path.is_some() {
                    return Task::done(Message::PdfFitToWidth);
                } else if self.pdf_fit_to_page && self.active_pdf_path.is_some() {
                    return Task::done(Message::PdfFitToPage);
                }
                Task::none()
            }
            Message::ToggleTOC => {
                if self.workspace.active_path.is_some() || self.active_pdf_path.is_some() {
                    self.toc_visible = !self.toc_visible;
                    self.persist_shell_state();
                }
                Task::none()
            }
            Message::PdfToggleAnnotationsSidebar => {
                if self.active_pdf_path.is_some() {
                    self.pdf_annotations_visible = !self.pdf_annotations_visible;
                    self.persist_shell_state();
                }
                Task::none()
            }
            Message::PdfFilterAnnotationsByColor(color) => {
                self.pdf_annotations_filter_color = color;
                Task::none()
            }
            Message::PdfFilterAnnotationsByPage(page) => {
                self.pdf_annotations_filter_page = page;
                Task::none()
            }
            Message::PdfFilterAnnotationsByTag(tag) => {
                self.pdf_annotations_filter_tag = tag;
                Task::none()
            }
            Message::PdfFilterAnnotationsByLinked(linked) => {
                self.pdf_annotations_filter_linked = linked;
                Task::none()
            }
            Message::PdfFilterAnnotationsByUnresolved(unresolved) => {
                self.pdf_annotations_filter_unresolved = unresolved;
                Task::none()
            }
            Message::PdfToggleAnnotationStatus(id) => {
                let mut found_ann = None;
                for page_anns in self.pdf_annotations.values_mut() {
                    if let Some(ann) = page_anns.iter_mut().find(|a| a.id == id) {
                        ann.status = match ann.status {
                            md_editor_core::pdf::PdfAnnotationStatus::Unresolved => {
                                md_editor_core::pdf::PdfAnnotationStatus::Resolved
                            }
                            md_editor_core::pdf::PdfAnnotationStatus::Resolved => {
                                md_editor_core::pdf::PdfAnnotationStatus::Unresolved
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
            Message::PdfEditAnnotationTags(id) => {
                self.focused_annotation_id = Some(id.clone());
                let mut tags_str = String::new();
                for page_anns in self.pdf_annotations.values() {
                    if let Some(ann) = page_anns.iter().find(|a| a.id == id) {
                        tags_str = ann.tags.join(", ");
                        break;
                    }
                }
                self.overlays.active_modal = Some(views::modals::ModalType::AnnotationTags(id));
                self.overlays.modal_input = tags_str;
                Task::none()
            }
            Message::PdfUpdateAnnotationTags(id, input) => {
                let tags = input
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<String>>();
                let mut found_ann = None;
                for page_anns in self.pdf_annotations.values_mut() {
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
            Message::PdfNavigateToAnnotation { id, page } => {
                self.focused_annotation_id = Some(id);
                self.navigate_pdf_page(page)
            }
            Message::PdfEditAnnotationNote(id, _page) => {
                self.focused_annotation_id = Some(id.clone());
                let mut note = String::new();
                for page_anns in self.pdf_annotations.values() {
                    if let Some(ann) = page_anns.iter().find(|a| a.id == id) {
                        note = ann.note.clone().unwrap_or_default();
                        break;
                    }
                }
                self.overlays.active_modal = Some(views::modals::ModalType::QuickNote(id));
                self.overlays.modal_input = note;
                Task::none()
            }
            Message::PdfExportAnnotations => {
                let Some(ref pdf_path) = self.active_pdf_path else {
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
                for page_anns in self.pdf_annotations.values() {
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
                            let content = crate::pdf_notes::export_annotations_to_markdown(
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
            Message::PdfAnnotationsExported(res) => {
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

    fn current_shell_persistence(&self) -> AppShellPersistence {
        let active_workflow_tab = if self.tracker.visible {
            WorkflowSidebarTab::Tracker
        } else if self.toc_visible {
            WorkflowSidebarTab::Outline
        } else if self.pdf_annotations_visible {
            WorkflowSidebarTab::Annotations
        } else if self.workspace.backlinks_visible {
            WorkflowSidebarTab::Backlinks
        } else {
            WorkflowSidebarTab::None
        };
        let last_focused_pane = match self.active_panel {
            ActivePanel::Markdown => AppShellPane::Markdown,
            ActivePanel::Pdf => AppShellPane::Pdf,
        };

        AppShellPersistence {
            reduce_motion: false,
            sidebar_width: 260.0,
            reference_width: self.pdf_split_ratio * self.window_width,
            workflow_width: 280.0,
            split_ratio: self.split_ratio,
            sidebar_collapsed: !self.sidebar_visible,
            reference_collapsed: !self.split_view_active,
            workflow_collapsed: !self.workspace.backlinks_visible
                && !self.toc_visible
                && !self.tracker.visible
                && !self.pdf_annotations_visible,
            active_workflow_tab,
            last_focused_pane,
            theme: app_theme::get_active_theme(),
        }
    }

    fn app_shell_state(&self) -> AppShellState {
        let persistence = self
            .current_shell_persistence()
            .clamp_for_window(self.window_width);

        AppShellState::derive(
            AppShellInputs {
                active_pane: AppShellPane::Markdown,
                vault_open: self.workspace.vault_root.is_some(),
                vault_has_entries: !self.workspace.vault_entries.is_empty(),
                markdown_open: self.workspace.active_path.is_some(),
                pdf_open: self.active_pdf_path.is_some(),
                image_open: self.active_image_path.is_some(),
                split_requested: self.split_view_active,
                search_visible: self.search.visible,
                command_palette_visible: self.overlays.command_palette_visible,
                citation_palette_visible: self.overlays.citation_palette_visible,
            },
            persistence,
        )
    }

    fn app_shell_status(&self, shell_state: AppShellState) -> AppShellStatus {
        AppShellStatus::derive(AppShellStatusInputs {
            background_status: None,
            toast: self.overlays.toast.clone(),
            document_open: self.workspace.active_path.is_some()
                || self.active_pdf_path.is_some()
                || self.active_image_path.is_some(),
            document_dirty: self.workspace.active_path.is_some() && self.buffer.dirty,
            global_search_searching: self.search.global.searching,
            global_search_status: self.search.global.pdf_status.clone(),
            global_search_visible: self.search.visible,
            active_pane: shell_state.active_pane,
            background_error: self
                .search
                .global
                .error
                .clone()
                .or_else(|| self.search.pdf_error.clone()),
        })
    }

    fn load_shell_persistence(&mut self) {
        let Ok(Some(value)) =
            md_editor_core::config::get_sys_config(&self.state, APP_SHELL_PERSISTENCE_CONFIG_KEY)
        else {
            return;
        };
        let Some(saved) = AppShellPersistence::deserialize(&value) else {
            return;
        };
        let saved = saved.clamp_for_window(self.window_width);

        self.sidebar_visible = !saved.sidebar_collapsed;
        self.workspace.backlinks_visible =
            matches!(saved.active_workflow_tab, WorkflowSidebarTab::Backlinks)
                && !saved.workflow_collapsed;
        self.toc_visible = matches!(saved.active_workflow_tab, WorkflowSidebarTab::Outline)
            && !saved.workflow_collapsed;
        self.tracker.visible = matches!(saved.active_workflow_tab, WorkflowSidebarTab::Tracker)
            && !saved.workflow_collapsed;
        self.pdf_annotations_visible =
            matches!(saved.active_workflow_tab, WorkflowSidebarTab::Annotations)
                && !saved.workflow_collapsed;
        self.split_ratio = saved.split_ratio;
        self.pdf_split_ratio =
            (saved.reference_width / self.window_width.max(1.0)).clamp(0.15, 0.75);
        self.active_panel = if matches!(saved.last_focused_pane, AppShellPane::Pdf) {
            ActivePanel::Pdf
        } else {
            ActivePanel::Markdown
        };
        app_theme::set_active_theme(saved.theme);
    }

    fn persist_shell_state(&self) {
        let _ = md_editor_core::config::set_sys_config(
            &self.state,
            APP_SHELL_PERSISTENCE_CONFIG_KEY,
            &self.current_shell_persistence().serialize(),
        );
    }

    fn toggle_sidebar_visible(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
        self.persist_shell_state();
    }

    fn set_active_panel(&mut self, active_panel: ActivePanel) {
        if self.active_panel != active_panel {
            self.active_panel = active_panel;
            self.persist_shell_state();
        } else {
            self.active_panel = active_panel;
        }
    }

    pub fn view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        let shell_state = self.app_shell_state();
        let _command_groups = shell_state.command_groups();
        let shell_status = self.app_shell_status(shell_state);

        if matches!(shell_state.mode, AppShellMode::NoVault) {
            return views::welcome::view(&[]);
        }

        let toolbar = views::toolbar::view(
            self.workspace.active_path.as_deref(),
            self.active_pdf_path
                .as_deref()
                .or(self.active_image_path.as_deref()),
            self.sidebar_visible,
            self.tracker.visible,
            self.toc_visible,
            self.workspace.active_path.is_some() || self.active_pdf_path.is_some(),
            self.split_view_active,
            self.workspace.active_path.is_some(),
            self.pdf_annotations_visible,
            self.active_pdf_path.is_some(),
        );

        let sidebar = views::sidebar::view(
            &self.workspace.vault_entries,
            self.workspace.selected_path.as_deref(),
            self.workspace
                .active_path
                .as_deref()
                .or(self.active_pdf_path.as_deref())
                .or(self.active_image_path.as_deref()),
            &self.workspace.expanded_folders,
            !self.sidebar_visible,
            shell_state.persistence.sidebar_width,
        );

        let editor_search_active = self.editor_search_is_active();
        let pdf_search_active = self.pdf_search_is_active();

        let active_search_match = if editor_search_active {
            self.active_search_match_position()
        } else {
            None
        };
        let editor_search_query = if editor_search_active {
            self.search.editor.query.as_str()
        } else {
            ""
        };
        let existing_files: HashSet<String> = self
            .workspace
            .vault_entries
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| e.path.clone())
            .collect();
        let editor_scroll = scrollable(
            container(
                crate::editor::renderer::Editor::new(
                    &self.buffer,
                    &self.highlighted_lines,
                    &self.image_cache,
                    &self.math_cache,
                    Message::EditorCommand,
                    Message::EditorCommandNoScroll,
                    Message::SidebarFileClicked,
                    Message::EditorCheckboxToggle,
                )
                .on_block_context_menu(|line_idx, absolute_pos| Message::EditorBlockContextMenu {
                    line_idx,
                    absolute_pos,
                })
                .on_context_menu(|line_idx, col, absolute_pos| Message::EditorContextMenu {
                    line_idx,
                    col,
                    absolute_pos,
                })
                .vault_context(
                    self.workspace.vault_root.as_deref(),
                    self.workspace.active_path.as_deref(),
                    &existing_files,
                )
                .search(
                    editor_search_query,
                    self.search.editor.regex,
                    self.search.editor.match_case,
                    active_search_match,
                )
                .modifiers(self.keyboard_modifiers),
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
                    &self.search.editor.query,
                    &self.search.editor.replace,
                    self.search.editor.regex,
                    self.search.editor.match_case,
                    self.current_document_match_count(),
                    self.search.editor.active_index,
                    self.search.editor.wrap_status,
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

        let pdf_view: Element<Message, Theme, iced::Renderer> = if self.active_pdf_path.is_some() {
            let focused_ann = self.focused_annotation_id.as_ref().and_then(|ann_id| {
                self.pdf_annotations
                    .values()
                    .flatten()
                    .find(|a| &a.id == ann_id)
            });
            let pdf_toolbar = views::pdf_viewer::toolbar_with_companion_note(
                self.pdf_current_page,
                self.pdf_total_pages,
                self.pdf_state.zoom,
                self.pdf_fit_to_width,
                self.pdf_fit_to_page,
                self.pdf_selection.is_some(),
                focused_ann,
                self.workspace.active_path.is_some(),
                None,
            );
            // B5: pdf_toolbar now at TOP of the pdf pane.
            // search_bar or zero-height space appears between toolbar and content.
            let pdf_search_bar: Element<_, _, iced::Renderer> = if pdf_search_active {
                views::pdf_viewer::search_bar(
                    &self.pdf_state.search.query,
                    self.pdf_state.search.regex,
                    self.pdf_state.search.match_case,
                    self.pdf_state.search.matches.len(),
                    self.pdf_state.search.active_index,
                    self.pdf_state.search.searching,
                )
            } else {
                container(Space::new())
                    .height(Length::Fixed(0.0))
                    .width(Length::Fill)
                    .into()
            };
            let left_panel: Element<_, _, iced::Renderer> = container(column![
                pdf_toolbar,
                pdf_search_bar,
                scrollable(views::pdf_viewer::view_continuous(
                    &self.pdf_pages,
                    self.pdf_state.zoom,
                    self.pdf_rotation,
                    &self.pdf_dimensions,
                    &self.pdf_state.page_sizes,
                    self.pdf_placeholder_page_size,
                    if pdf_search_active
                        || self.search.visible
                        || self.search.editor.visible
                        || self.pdf_state.search.visible
                    {
                        &self.pdf_state.search.matches
                    } else {
                        &[]
                    },
                    &self.pdf_state.search.page_index,
                    self.pdf_state.search.active_index,
                    &self.pdf_page_text,
                    &self.pdf_annotations,
                    &self.pdf_page_links,
                    self.pdf_selection,
                    self.focused_annotation_id.as_deref(),
                    self.pdf_scroll_y,
                    self.pdf_viewport_height,
                    0,
                ))
                .id(iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID))
                .on_scroll(|vp| Message::PdfScrolled {
                    y: vp.absolute_offset().y,
                    viewport_height: vp.bounds().height,
                })
                .height(Length::Fill),
            ])
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

            // pdf_toolbar is at TOP of pane (B5); no second column wrap needed.
            left_panel
        } else {
            container(Space::new()).width(Length::Fixed(0.0)).into()
        };

        let md_toc: &[views::toc::TocEntry] = if self.workspace.active_path.is_some() {
            &self.md_toc_entries
        } else {
            &[]
        };
        let pdf_toc: &[views::toc::TocEntry] =
            if self.active_pdf_path.is_some() && (self.showing_pdf || self.split_view_active) {
                self.pdf_toc_entries_flat.as_deref().unwrap_or(&[])
            } else {
                &[]
            };

        let active_md_line = if self.workspace.active_path.is_some() {
            self.md_toc_entries
                .iter()
                .rev()
                .find(|e| e.line <= self.buffer.cursor_line)
                .map(|e| e.line)
        } else {
            None
        };

        let active_pdf_page = if self.active_pdf_path.is_some() {
            let current_page = self.pdf_current_page as usize;
            self.pdf_toc_entries_flat
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .rev()
                .find(|e| e.line <= current_page)
                .map(|e| e.line)
        } else {
            None
        };

        let toc_view: Element<Message, Theme, iced::Renderer> = if self.toc_visible {
            views::toc::view(
                md_toc,
                pdf_toc,
                shell_state.persistence.workflow_width,
                active_md_line,
                active_pdf_page,
            )
        } else {
            container(Space::new()).width(Length::Fixed(0.0)).into()
        };

        let image_view: Element<Message, Theme, iced::Renderer> =
            if let Some((handle, width, height)) = &self.active_image {
                let label = self.active_image_path.as_deref().unwrap_or("Image");
                container(
                    column![
                        text(label).size(13).color(app_theme::text_muted()),
                        iced::widget::image(handle.clone())
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .content_fit(iced::ContentFit::Contain),
                        text(format!("{:.0} x {:.0}", width, height))
                            .size(11)
                            .color(app_theme::text_muted()),
                    ]
                    .spacing(12)
                    .align_x(Alignment::Center)
                    .padding(24),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(app_theme::bg_primary())),
                    ..Default::default()
                })
                .into()
            } else {
                container(Space::new()).width(Length::Fixed(0.0)).into()
            };

        let main_content: Element<Message, Theme, iced::Renderer> =
            if shell_state.uses_split_research_layout() {
                let left_portion = (self.split_ratio * 1000.0) as u16;
                let right_portion = ((1.0 - self.split_ratio) * 1000.0) as u16;

                let divider = mouse_area(
                    container(text("⋮").size(14).color(app_theme::text_muted()))
                        .width(Length::Fixed(10.0))
                        .height(Length::Fill)
                        .center_x(Length::Fixed(10.0))
                        .center_y(Length::Fill)
                        .style(|_| container::Style {
                            background: Some(iced::Background::Color(app_theme::bg_tertiary())),
                            ..Default::default()
                        }),
                )
                .on_press(Message::SplitViewDragStart)
                .on_release(Message::SplitViewDragEnd)
                .interaction(iced::mouse::Interaction::ResizingHorizontally);

                row![
                    container(pdf_view)
                        .width(Length::FillPortion(left_portion))
                        .style(|_| container::Style {
                            border: iced::Border {
                                color: app_theme::border(),
                                width: 1.0,
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    divider,
                    container(editor_view).width(Length::FillPortion(right_portion))
                ]
                .into()
            } else if shell_state.shows_pdf_document()
                && self.showing_pdf
                && self.active_pdf_path.is_some()
            {
                pdf_view
            } else if shell_state.shows_image_document() && self.active_image.is_some() {
                image_view
            } else {
                editor_view.into()
            };

        let content = column![toolbar, main_content].height(Length::Fill);

        let backlinks_view: Element<Message, Theme, iced::Renderer> = views::backlinks::view(
            &self.workspace.backlinks,
            self.workspace.backlinks_visible,
            shell_state.persistence.workflow_width,
            false,
        );

        let pdf_annotations_view: Element<Message, Theme, iced::Renderer> =
            if self.pdf_annotations_visible && self.active_pdf_path.is_some() {
                views::pdf_annotations::view(
                    &self.pdf_annotations,
                    self.pdf_annotations_filter_color,
                    self.pdf_annotations_filter_page,
                    self.pdf_annotations_filter_tag.as_deref(),
                    self.pdf_annotations_filter_unresolved,
                    self.pdf_annotations_filter_linked,
                    self.focused_annotation_id.as_deref(),
                    self.workspace.active_path.is_some(),
                    shell_state.persistence.workflow_width,
                )
            } else {
                container(Space::new()).width(Length::Fixed(0.0)).into()
            };

        let status_bar = views::status_bar::view(shell_status);

        let layout = column![
            row![
                sidebar,
                content,
                pdf_annotations_view,
                backlinks_view,
                toc_view
            ]
            .height(Length::Fill),
            status_bar
        ]
        .height(Length::Fill);

        let mut layers = vec![
            container(layout)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(app_theme::bg_primary())),
                    ..Default::default()
                })
                .into(),
        ];

        if self.search.visible {
            layers.push(
                container(views::search::view(
                    &self.search.editor.query,
                    &self.search.editor.replace,
                    self.search.editor.regex,
                    self.search.editor.match_case,
                    self.current_document_match_count(),
                    &self.search.global.results,
                    self.search.global.searching,
                    self.search.global.error.as_deref(),
                    true,
                    &self.search.global.sources,
                    self.search.global.pdf_status.as_deref(),
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

        if self.overlays.command_palette_visible {
            layers.push(
                container(views::command_palette::view(
                    &self.overlays.command_palette_query,
                    self.command_palette_commands(),
                    self.window_width,
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

        if self.overlays.citation_palette_visible {
            layers.push(
                container(views::citation_palette::view(
                    &self.overlays.citation_palette_query,
                    self.citation_palette_items(),
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

        if let Some(modal_type) = &self.overlays.active_modal {
            layers.push(views::modals::view(
                modal_type,
                &self.overlays.modal_input,
                &self.overlays.link_note_picker_search,
                &self.workspace.vault_entries,
            ));
        }

        if self.tracker.visible {
            layers.push(
                container(views::tracker::view(
                    true,
                    self.tracker.running,
                    &self.tracker.sessions,
                    &self.tracker.kv,
                    self.tracker.tab,
                    &self.tracker.config_content,
                    &self.tracker.manual_date,
                    &self.tracker.manual_hours,
                    &self.tracker.manual_notes,
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
                .height(Length::Fill)
                .content_fit(iced::ContentFit::Contain);

            let modal = container(
                iced::widget::column![
                    iced::widget::row![
                        Space::new().width(Length::Fill),
                        iced::widget::button("✕")
                            .on_press(Message::ClosePdfLinkPreview)
                            .padding(10)
                            .style(iced::widget::button::text)
                    ],
                    container(img)
                        .width(Length::Fixed(1160.0))
                        .height(Length::Fixed(820.0))
                        .padding(16)
                        .style(|_| container::Style {
                            background: Some(iced::Background::Color(iced::Color::WHITE)),
                            border: iced::Border {
                                radius: 6.0.into(),
                                ..Default::default()
                            },
                            shadow: iced::Shadow {
                                color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                                offset: iced::Vector::new(0.0, 8.0),
                                blur_radius: 24.0,
                            },
                            ..Default::default()
                        })
                ]
                .spacing(8)
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

        if let Some(msg) = &self.overlays.toast {
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
        self.workspace.vault_root = Some(path.to_string());
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_vault", path);
        self.workspace.vault_entries =
            md_editor_core::vault::set_vault_root(&self.state, path).unwrap_or_default();
        let _ = reindex_vault_with_parser_targets(&self.state, std::path::Path::new(path));

        if let Ok(broken) =
            crate::integrity::check_vault_integrity(&self.state, std::path::Path::new(path))
        {
            if !broken.is_empty() {
                eprintln!(
                    "Vault integrity check: found {} broken references",
                    broken.len()
                );
            }
        }
    }

    fn new_entry_path(&self, name: &str) -> String {
        let parent = self.workspace.selected_path.as_deref().and_then(|path| {
            if self
                .workspace
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

    fn follow_citation(&mut self) -> Task<Message> {
        let cursor_line = self.buffer.cursor_line;
        let cursor_col = self.buffer.cursor_col;
        if cursor_line < self.highlighted_lines.len() {
            let line = &self.highlighted_lines[cursor_line];
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
                    if let Some(ref target) = span.link_target {
                        return Task::done(Message::SidebarFileClicked(target.clone()));
                    }
                }
            }
        }
        Task::none()
    }

    fn show_usages(&mut self) -> Task<Message> {
        let path = if self.showing_pdf && self.active_pdf_path.is_some() {
            self.active_pdf_path.clone()
        } else if self.split_view_active
            && self.active_panel == ActivePanel::Pdf
            && self.active_pdf_path.is_some()
        {
            self.active_pdf_path.clone()
        } else {
            self.workspace.active_path.clone()
        };

        if let Some(ref p) = path {
            self.workspace.backlinks =
                md_editor_core::vault::get_mixed_backlinks(&self.state, p).unwrap_or_default();
            self.workspace.backlinks_visible = true;
            self.persist_shell_state();
        }
        Task::none()
    }

    fn open_file(&mut self, path: &str) -> Task<Message> {
        self.open_file_extended(path, true)
    }

    fn open_file_extended(&mut self, path: &str, reset_scroll: bool) -> Task<Message> {
        let is_different = self.workspace.active_path.as_deref() != Some(path);
        if is_different {
            if self.showing_pdf && self.active_pdf_path.is_some() {
                self.push_pdf_navigation_history();
            } else if self.workspace.active_path.is_some() {
                self.push_markdown_navigation_history();
            }
        }
        if let Ok(bytes) = md_editor_core::vault::open_file(&self.state, path) {
            if let Ok(content) = String::from_utf8(bytes) {
                self.buffer = DocBuffer::from_text(&content);
                self.workspace.active_path = Some(path.to_string());
                let _ = reindex_markdown_file_with_parser_targets(&self.state, path, &content);
                let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
                self.active_image_path = None;
                self.active_image = None;
                self.showing_pdf = false;
                self.set_active_panel(ActivePanel::Markdown);
                self.md_toc_entries = Vec::new();
                let highlight_task = self.refresh_highlighting_for_current_buffer(true);
                self.workspace.backlinks =
                    md_editor_core::vault::get_mixed_backlinks(&self.state, path)
                        .unwrap_or_default();
                if is_different && reset_scroll {
                    self.editor_scroll_y = 0.0;
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
        let is_different = self.active_pdf_path.as_deref() != Some(path);
        if is_different {
            if self.showing_pdf && self.active_pdf_path.is_some() {
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
        self.active_pdf_path = Some(path.to_string());
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_pdf", path);
        self.active_image_path = None;
        self.active_image = None;
        self.showing_pdf = true;
        self.set_active_panel(ActivePanel::Pdf);
        self.pdf_current_page = 0;
        self.pdf_total_pages = 0;
        self.pdf_rotation = 0;
        self.pdf_fit_to_width = true;
        self.pdf_fit_to_page = false;
        self.pdf_pages = Vec::new();
        self.pdf_dimensions = Vec::new();
        self.pdf_state.page_sizes = Vec::new();
        self.pdf_placeholder_page_size = None;
        self.pdf_state.page_cache = PdfPageCache::default();
        self.pdf_state.page_cache.set_visible_range(None);
        self.pdf_state.layout = PdfLayout::default();
        self.pdf_pending_pages.clear();
        self.pdf_stale_pages.clear();
        self.pdf_pending_links.clear();
        self.pdf_page_links.clear();
        self.pdf_state.search.matches.clear();
        self.pdf_state.search.page_index.clear();
        self.search.pdf_error = None;
        self.pdf_state.search.searching = false;
        self.search.pdf_active_id = 0;
        self.pdf_programmatic_scroll = false;
        self.pdf_toc_target_page = None;
        self.pdf_render_generation = self.pdf_render_generation.wrapping_add(1);
        let generation = self.pdf_render_generation;

        // Reset PDF study state
        self.pdf_document_id = None;
        self.pdf_page_text.clear();
        self.pdf_selection = None;
        self.pdf_annotations.clear();
        self.focused_annotation_id = None;
        self.pdf_pending_text.clear();
        self.pdf_text_lru.clear();
        self.workspace.backlinks =
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

    fn open_image(&mut self, path: &str) -> Task<Message> {
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
                self.active_image_path = Some(path.to_string());
                let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
                self.active_image = Some((handle, width as f32, height as f32));
                self.workspace.clear_active_markdown();
                self.active_pdf_path = None;
                self.showing_pdf = false;
                self.set_active_panel(ActivePanel::Markdown);
                self.md_toc_entries.clear();
                self.pdf_toc_entries_flat = None;
            }
            Err(err) => {
                self.overlays.toast = Some(format!("Could not open image: {err}"));
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
        let zoom = self.pdf_state.zoom * PDF_RENDER_SUPERSAMPLE;
        let generation = self.pdf_render_generation;
        let _state = self.state.clone();

        Task::perform(
            async move {
                let renderer = _state.pdf_renderer()?;
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
        let Some(path) = &self.active_pdf_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        let path_str = abs_path.to_string_lossy().to_string();
        let zoom = self.pdf_state.zoom * PDF_RENDER_SUPERSAMPLE;
        let generation = self.pdf_render_generation;
        let _state = self.state.clone();
        if self
            .pdf_pages
            .get(page as usize)
            .is_none_or(|p| p.is_none() || self.pdf_stale_pages.contains(&page))
        {
            self.pdf_pending_pages.insert(page);
        }

        Task::perform(
            async move {
                let renderer = _state.pdf_renderer()?;
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
        if self.pdf_page_links.contains_key(&page) || self.pdf_pending_links.contains(&page) {
            return Task::none();
        }
        let Some(path) = &self.active_pdf_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        self.pdf_pending_links.insert(page);
        let path_str = abs_path.to_string_lossy().to_string();
        let generation = self.pdf_render_generation;
        let _state = self.state.clone();

        Task::perform(
            async move {
                let renderer = _state.pdf_renderer()?;
                renderer.get_page_links(&path_str, page).ok()
            },
            move |res| Message::PdfPageLinksLoaded(generation, page, res.unwrap_or_default()),
        )
    }

    fn load_pdf_page_text(&mut self, page: u16) -> Task<Message> {
        if self.pdf_page_text.contains_key(&page) || self.pdf_pending_text.contains(&page) {
            return Task::none();
        }
        let Some(path) = &self.active_pdf_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        self.pdf_pending_text.insert(page);
        let path_str = abs_path.to_string_lossy().to_string();
        let generation = self.pdf_render_generation;
        let _state = self.state.clone();

        Task::perform(
            async move {
                let renderer = _state
                    .pdf_renderer()
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
        if self.pdf_total_pages == 0 {
            return Task::none();
        }
        // Estimate visible range using viewport height and page height
        let page_h = self.estimated_pdf_page_height().max(100.0);
        let viewport_h = self.window_height.max(400.0);
        let pages_in_view = (viewport_h / page_h).ceil() as u16;
        let first_visible = self.pdf_current_page;
        let last_visible =
            (first_visible + pages_in_view).min(self.pdf_total_pages.saturating_sub(1));

        if let Some(path) = &self.active_pdf_path {
            if let Some(abs_path) = self.resolve_active_path(path) {
                let path_str = abs_path.to_string_lossy().to_string();
                if let Some(renderer) = self.state.pdf_renderer() {
                    renderer.set_visible_range(first_visible, last_visible, &path_str);
                }
            }
        }

        let start = self
            .pdf_current_page
            .saturating_sub(PDF_RENDER_PRELOAD_PAGES);
        let end = (self.pdf_current_page + pages_in_view + PDF_RENDER_PRELOAD_PAGES)
            .min(self.pdf_total_pages.saturating_sub(1));
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

        let first_visible = self.pdf_page_at_scroll(scroll_y);
        let last_visible = self.pdf_page_at_scroll(scroll_y + viewport_height);

        if let Some(path) = &self.active_pdf_path {
            if let Some(abs_path) = self.resolve_active_path(path) {
                let path_str = abs_path.to_string_lossy().to_string();
                if let Some(renderer) = self.state.pdf_renderer() {
                    renderer.set_visible_range(first_visible, last_visible, &path_str);
                }
            }
        }

        let Some((start, end)) = self.pdf_render_range_for_viewport(scroll_y, viewport_height)
        else {
            return Task::none();
        };
        self.render_pdf_page_range(start, end)
    }

    fn render_pdf_page_range(&mut self, start: u16, end: u16) -> Task<Message> {
        let Some((start, end)) = self.bounded_pdf_page_range(start, end) else {
            return Task::none();
        };
        let mut tasks = Vec::new();
        for page_idx in start..=end {
            if self
                .pdf_pages
                .get(page_idx as usize)
                .is_none_or(|p| p.is_none() || self.pdf_stale_pages.contains(&page_idx))
                && !self.pdf_pending_pages.contains(&page_idx)
            {
                self.pdf_pending_pages.insert(page_idx);
                tasks.push(self.render_pdf_page(page_idx));
            }
            if !self.pdf_page_text.contains_key(&page_idx)
                && !self.pdf_pending_text.contains(&page_idx)
            {
                tasks.push(self.load_pdf_page_text(page_idx));
            }
        }

        Task::batch(tasks)
    }

    fn pdf_render_range_for_viewport(
        &self,
        scroll_y: f32,
        viewport_height: f32,
    ) -> Option<(u16, u16)> {
        if self.pdf_total_pages == 0 {
            return None;
        }

        let range = self.pdf_state.layout.visible_range(
            scroll_y,
            viewport_height,
            PDF_RENDER_PRELOAD_PAGES,
        );
        if !range.is_empty() {
            return Some((range.start, range.end.saturating_sub(1)));
        }

        let page_h = self.estimated_pdf_page_height().max(100.0);
        let pages_in_view = (viewport_height.max(0.0) / page_h).ceil() as u16;
        let first = self.pdf_page_at_scroll(scroll_y);
        let last = (first + pages_in_view).min(self.pdf_total_pages.saturating_sub(1));
        Some((
            first.saturating_sub(PDF_RENDER_PRELOAD_PAGES),
            last.saturating_add(PDF_RENDER_PRELOAD_PAGES)
                .min(self.pdf_total_pages.saturating_sub(1)),
        ))
    }

    fn bounded_pdf_page_range(&self, start: u16, end: u16) -> Option<(u16, u16)> {
        if self.pdf_total_pages == 0 || start > end || start >= self.pdf_total_pages {
            return None;
        }

        let doc_last = self.pdf_total_pages.saturating_sub(1);
        let end = end.min(doc_last);
        let capped_end = end.min(start.saturating_add(PDF_RENDER_MAX_SCHEDULED_PAGES - 1));
        Some((start, capped_end))
    }

    /// Keep the PdfPageCache informed of the currently visible page range
    /// so it can protect those pages during eviction. Also insert rendered
    /// pages into the cache as they arrive.
    fn update_pdf_page_cache(&mut self) {
        let first = self.pdf_page_at_scroll(self.pdf_scroll_y);
        let viewport_height = if self.pdf_viewport_height > 0.0 {
            self.pdf_viewport_height
        } else {
            self.estimated_editor_viewport_height()
        };
        let last = self.pdf_page_at_scroll(self.pdf_scroll_y + viewport_height);

        // Clamp to document range
        let first = first.min(self.pdf_total_pages.saturating_sub(1));
        let last = last.min(self.pdf_total_pages.saturating_sub(1));

        let range = if self.pdf_total_pages > 0 {
            Some((first, last.max(first)))
        } else {
            None
        };
        self.pdf_state.page_cache.set_visible_range(range);
        self.pdf_state.page_cache.touch_visible();
    }

    fn sync_pdf_pages_to_cache(&mut self) {
        for (idx, page) in self.pdf_pages.iter_mut().enumerate() {
            if page.is_some() && !self.pdf_state.page_cache.contains(idx as u16) {
                *page = None;
                self.pdf_stale_pages.remove(&(idx as u16));
            }
        }
    }

    fn estimated_pdf_page_height(&self) -> f32 {
        self.pdf_placeholder_display_size().1
    }

    fn first_pdf_page_size(&self) -> Option<(f32, f32)> {
        self.pdf_state
            .page_sizes
            .first()
            .and_then(|s| *s)
            .or_else(|| {
                self.pdf_dimensions.first().and_then(|d| {
                    d.map(|(w, h)| {
                        (
                            w as f32 / self.pdf_state.zoom,
                            h as f32 / self.pdf_state.zoom,
                        )
                    })
                })
            })
    }

    fn pdf_placeholder_display_size(&self) -> (f32, f32) {
        let size = pdf_placeholder_display_size_from(
            self.pdf_placeholder_page_size,
            self.pdf_state.page_sizes.first().and_then(|s| *s),
            self.pdf_dimensions.first().and_then(|d| *d),
            self.pdf_state.zoom,
        );
        if self.pdf_rotation == 90 || self.pdf_rotation == 270 {
            (size.1, size.0)
        } else {
            size
        }
    }

    fn pdf_page_display_size(&self, page: u16) -> (f32, f32) {
        let size = if let Some(Some((w, h))) = self.pdf_state.page_sizes.get(page as usize) {
            (*w * self.pdf_state.zoom, *h * self.pdf_state.zoom)
        } else {
            self.pdf_placeholder_display_size()
        };
        if self.pdf_rotation == 90 || self.pdf_rotation == 270 {
            (size.1, size.0)
        } else {
            size
        }
    }

    fn pdf_available_width(&self) -> f32 {
        let sidebar_width = if self.sidebar_visible { 260.0 } else { 0.0 };
        let toc_width = if self.toc_visible { 260.0 } else { 0.0 };
        let backlinks_width = if self.workspace.backlinks_visible {
            260.0
        } else {
            0.0
        };
        let chrome_width = sidebar_width + toc_width + backlinks_width;
        let content_width = (self.window_width - chrome_width).max(320.0);

        if self.split_view_active
            && self.workspace.active_path.is_some()
            && self.active_pdf_path.is_some()
        {
            (content_width * self.split_ratio).max(280.0)
        } else {
            content_width
        }
    }

    fn pdf_page_height(&self, page: u16) -> f32 {
        if (page as usize) < self.pdf_total_pages as usize {
            self.pdf_page_display_size(page).1
        } else {
            self.estimated_pdf_page_height()
        }
    }

    fn pdf_page_offset(&self, page: u16) -> f32 {
        self.pdf_state.layout.page_offset(page)
    }

    fn pdf_total_height(&self) -> f32 {
        self.pdf_state.layout.total_height()
    }

    fn pdf_page_at_scroll(&self, scroll_y: f32) -> u16 {
        self.pdf_state.layout.page_at_scroll(scroll_y)
    }

    fn pdf_search_match_scroll_y(&self, result: &md_editor_core::pdf::PdfSearchMatch) -> f32 {
        let rect = result.rects.first();
        let page_height = self
            .pdf_state
            .page_sizes
            .get(result.page_index as usize)
            .and_then(|size| *size)
            .map(|(_, h)| h)
            .unwrap_or_else(|| {
                self.pdf_page_height(result.page_index) / self.pdf_state.zoom.max(0.01)
            });
        pdf_search_match_scroll_y_from(
            self.pdf_page_offset(result.page_index),
            rect.map(|rect| rect.y),
            rect.map(|rect| rect.height).unwrap_or(0.0),
            page_height,
            self.pdf_state.zoom,
            self.pdf_total_height(),
        )
    }

    fn pdf_link_at(&self, page_idx: u16, x: f32, y: f32) -> Option<md_editor_core::pdf::LinkInfo> {
        let links = self.pdf_page_links.get(&page_idx)?;
        let dim = self
            .pdf_dimensions
            .get(page_idx as usize)
            .and_then(|d| *d)?;
        let real_x = (x * dim.0 as f32) / self.pdf_state.zoom;
        let real_y = (y * dim.1 as f32) / self.pdf_state.zoom;

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

    fn find_pdf_annotation(&self, id: &str) -> Option<(u16, md_editor_core::pdf::PdfAnnotation)> {
        self.pdf_annotations
            .iter()
            .find_map(|(page_idx, page_anns)| {
                page_anns
                    .iter()
                    .find(|ann| ann.id == id)
                    .cloned()
                    .map(|ann| (*page_idx, ann))
            })
    }

    fn pdf_paths_match(&self, active_path: Option<&str>, target_path: &str) -> bool {
        let Some(active_path) = active_path else {
            return false;
        };
        if active_path == target_path {
            return true;
        }

        let Some(vault_root) = self.workspace.vault_root.as_deref() else {
            return false;
        };
        let active_abs = md_editor_core::vault::resolve_vault_path(
            std::path::Path::new(vault_root),
            active_path,
        );
        let target_abs = md_editor_core::vault::resolve_vault_path(
            std::path::Path::new(vault_root),
            target_path,
        );
        normalize_path(&active_abs) == normalize_path(&target_abs)
    }

    fn pdf_selection_contains_point(&self, page_idx: u16, x: f32, y: f32) -> bool {
        let Some(sel) = &self.pdf_selection else {
            return false;
        };
        if sel.page_index != page_idx {
            return false;
        }
        let Some(page_text) = self.pdf_page_text.get(&sel.page_index) else {
            return false;
        };
        let start = sel.anchor_idx.min(sel.focus_idx);
        let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
        let selected_chars = page_text
            .chars
            .iter()
            .filter(|c| c.text_index >= start && c.text_index < end)
            .cloned()
            .collect::<Vec<_>>();
        let px = x * page_text.page_width;
        let py = y * page_text.page_height;

        md_editor_core::pdf::merge_char_rects(&selected_chars)
            .iter()
            .any(|rect| {
                let view_y = page_text.page_height - rect.y - rect.height;
                let pad = 4.0;
                px >= rect.x - pad
                    && px <= rect.x + rect.width + pad
                    && py >= view_y - pad
                    && py <= view_y + rect.height + pad
            })
    }

    fn annotation_at(
        &self,
        page_idx: u16,
        x: f32,
        y: f32,
    ) -> Option<md_editor_core::pdf::PdfAnnotation> {
        let page_text = self.pdf_page_text.get(&page_idx)?;
        let px = x * page_text.page_width;
        let py = y * page_text.page_height;

        let page_anns = self.pdf_annotations.get(&page_idx)?;
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
        let root = self.workspace.vault_root.as_deref()?;
        Some(md_editor_core::vault::resolve_vault_path(
            std::path::Path::new(root),
            path,
        ))
    }

    fn default_pdf_note_path(&self, ann: &md_editor_core::pdf::PdfAnnotation) -> String {
        let pdf_filename = self
            .active_pdf_path
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
            Some(existing) => {
                build_linked_pdf_note_content(Some(&existing), note_path, pdf_path, ann).content
            }
            None => build_linked_pdf_note_content(None, note_path, pdf_path, ann).content,
        }
    }

    fn highlight_all(&mut self) -> Task<Message> {
        self.refresh_highlighting_for_current_buffer(false)
    }

    fn refresh_highlighting_for_current_buffer(&mut self, opened_file: bool) -> Task<Message> {
        let text = self.buffer.text();
        let line_count = self.buffer.line_count();
        self.highlight_generation = self.highlight_generation.wrapping_add(1);
        let generation = self.highlight_generation;
        self.pending_highlight_generation = None;
        self.pending_highlight_requested_at = None;
        self.pending_highlight_text = None;

        if opened_file && line_count > HUGE_DOC_LINE_THRESHOLD {
            self.highlighted_lines = plain_highlight_placeholders(&text);
            self.md_toc_entries = views::toc::get_toc(&self.highlighted_lines);
            return Self::highlight_task(generation, text);
        }

        if !opened_file && line_count > LARGE_DOC_LINE_THRESHOLD {
            self.pending_highlight_generation = Some(generation);
            self.pending_highlight_requested_at = Some(Instant::now());
            self.pending_highlight_text = Some(text);
            return Task::none();
        }

        self.highlighted_lines = highlight::highlight_markdown(&text);
        self.md_toc_entries = views::toc::get_toc(&self.highlighted_lines);
        Task::batch(vec![self.load_images(), self.load_math()])
    }

    fn highlight_task(generation: u64, text: String) -> Task<Message> {
        Task::perform(
            async move { highlight::highlight_markdown(&text) },
            move |lines| Message::HighlightReady(generation, lines),
        )
    }

    fn current_document_match_count(&self) -> usize {
        self.current_document_matches().len()
    }

    fn active_search_match_position(&self) -> Option<(usize, usize)> {
        let matches = self.current_document_matches();
        let index = self.search.editor.active_index?;
        matches
            .get(index.min(matches.len().saturating_sub(1)))
            .map(|m| (m.line, m.start_col))
    }

    fn current_document_matches(&self) -> Vec<DocumentMatch> {
        if self.search.editor.query.is_empty() || self.workspace.active_path.is_none() {
            return Vec::new();
        }

        (0..self.buffer.line_count())
            .flat_map(|line| {
                let text = self.buffer.line_text(line);
                crate::search::line_matches(
                    &text,
                    &self.search.editor.query,
                    self.search.editor.regex,
                    self.search.editor.match_case,
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

    fn rebuild_pdf_search_page_index(&mut self) {
        self.pdf_state.search.page_index.clear();
        for (idx, result) in self.pdf_state.search.matches.iter().enumerate() {
            self.pdf_state
                .search
                .page_index
                .entry(result.page_index)
                .or_default()
                .push(idx);
        }
    }

    fn navigate_file_search(&mut self, forward: bool) -> Task<Message> {
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
        self.buffer.execute(EditorCommand::SetSelection {
            anchor_line: item.line,
            anchor_col: item.start_col,
            focus_line: item.line,
            focus_col: item.end_col,
        });
        self.center_editor_line(item.line)
    }

    fn navigate_pdf_search(&mut self, forward: bool) -> Task<Message> {
        if self.pdf_state.search.matches.is_empty() {
            self.pdf_state.search.active_index = None;
            return Task::none();
        }

        let next_index = match self.pdf_state.search.active_index {
            Some(index) if forward => (index + 1) % self.pdf_state.search.matches.len(),
            Some(0) if !forward => self.pdf_state.search.matches.len() - 1,
            Some(index) => index.saturating_sub(1),
            None if forward => 0,
            None => self.pdf_state.search.matches.len() - 1,
        };
        self.navigate_pdf_search_to_index(next_index)
    }

    fn navigate_pdf_search_to_index(&mut self, index: usize) -> Task<Message> {
        let Some(result) = self.pdf_state.search.matches.get(index).cloned() else {
            self.pdf_state.search.active_index = None;
            return Task::none();
        };

        self.push_pdf_navigation_history();
        self.pdf_state.search.active_index = Some(index);
        let target_page = result
            .page_index
            .min(self.pdf_total_pages.saturating_sub(1));
        self.pdf_current_page = target_page;
        self.pdf_programmatic_scroll = true;
        self.pdf_toc_target_page = None;

        let scroll_y = self.pdf_search_match_scroll_y(&result);
        if let Some(path) = &self.active_pdf_path {
            if let Some(abs_path) = self.resolve_active_path(path) {
                let path_str = abs_path.to_string_lossy().to_string();
                if let Some(renderer) = self.state.pdf_renderer() {
                    renderer.set_visible_range(
                        target_page.saturating_sub(1),
                        (target_page + 1).min(self.pdf_total_pages.saturating_sub(1)),
                        &path_str,
                    );
                }
            }
        }

        let mut tasks = vec![self.render_pdf_page_direct(target_page)];
        tasks.push(self.render_pdf_page_range(
            target_page.saturating_sub(2),
            (target_page + 2).min(self.pdf_total_pages.saturating_sub(1)),
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

    fn push_pdf_navigation_history(&mut self) {
        if self.showing_pdf && self.pdf_total_pages > 0 {
            if let Some(ref path) = self.active_pdf_path {
                let target = NavigationTarget::Pdf {
                    path: path.clone(),
                    page: self.pdf_current_page,
                    scroll_offset: self.pdf_scroll_y,
                    zoom: self.pdf_state.zoom,
                };
                self.workspace.navigation_history.push(target);
            }
        }
    }

    fn push_markdown_navigation_history(&mut self) {
        if let Some(ref path) = self.workspace.active_path {
            let target = NavigationTarget::Markdown {
                path: path.clone(),
                line: self.buffer.cursor_line,
                column: self.buffer.cursor_col,
            };
            self.workspace.navigation_history.push(target);
        }
    }

    fn navigate_to_target(&mut self, target: NavigationTarget) -> Task<Message> {
        match target {
            NavigationTarget::Markdown { path, line, column } => {
                let mut tasks = Vec::new();
                let is_different_file = self.workspace.active_path.as_deref() != Some(&path);
                if is_different_file {
                    tasks.push(self.open_file_extended(&path, false));
                } else {
                    self.showing_pdf = false;
                    self.set_active_panel(ActivePanel::Markdown);
                }

                tasks.push(Task::done(Message::EditorCommand(
                    crate::editor::buffer::EditorCommand::SetCursor { line, col: column },
                )));

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
                let is_different_pdf = self.active_pdf_path.as_deref() != Some(&path);
                if is_different_pdf {
                    tasks.push(self.open_pdf(&path));
                } else {
                    self.showing_pdf = true;
                    self.set_active_panel(ActivePanel::Pdf);
                }
                self.pdf_state.zoom = zoom;
                self.pdf_current_page = page.min(self.pdf_total_pages.saturating_sub(1));
                self.pdf_pending_pages.clear();
                self.pdf_stale_pages.clear();
                self.pdf_pending_links.clear();
                self.pdf_render_generation = self.pdf_render_generation.wrapping_add(1);
                self.pdf_toc_target_page = Some(self.pdf_current_page);
                self.pdf_programmatic_scroll = true;

                let start = self.pdf_current_page.saturating_sub(2);
                let end = (self.pdf_current_page + 2).min(self.pdf_total_pages.saturating_sub(1));
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

    fn navigate_pdf_page(&mut self, page: u16) -> Task<Message> {
        let target_page = page.min(self.pdf_total_pages.saturating_sub(1));
        self.pdf_current_page = target_page;
        self.pdf_pending_pages.clear();
        self.pdf_stale_pages.clear();
        self.pdf_pending_links.clear();
        self.pdf_render_generation = self.pdf_render_generation.wrapping_add(1);
        self.pdf_toc_target_page = Some(target_page);

        if let Some(path) = &self.active_pdf_path {
            if let Some(abs_path) = self.resolve_active_path(path) {
                let path_str = abs_path.to_string_lossy().to_string();
                if let Some(renderer) = self.state.pdf_renderer() {
                    renderer.set_visible_range(target_page, target_page, &path_str);
                }
            }
        }

        let target_dimensions_ready = self
            .pdf_dimensions
            .get(target_page as usize)
            .and_then(|d| *d)
            .is_some();
        let target_image_ready = self
            .pdf_pages
            .get(target_page as usize)
            .is_some_and(|page| page.is_some());

        let mut tasks = Vec::new();
        if target_image_ready && target_dimensions_ready {
            tasks.push(self.load_pdf_page_links(target_page));
        } else {
            tasks.push(self.render_pdf_page_direct(target_page));
        }

        self.pdf_programmatic_scroll = true;
        let scroll_y = self.pdf_page_offset(target_page);
        let current_scroll_y = self.pdf_scroll_y;
        if (current_scroll_y - scroll_y).abs() < 1.0 && target_image_ready {
            self.pdf_programmatic_scroll = false;
            self.pdf_toc_target_page = None;
            let start = target_page.saturating_sub(2);
            let end = (target_page + 2).min(self.pdf_total_pages.saturating_sub(1));
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

    fn estimated_editor_viewport_width(&self) -> f32 {
        if self.editor_viewport_width > 0.0 {
            return self.editor_viewport_width;
        }
        let sidebar_width = if self.sidebar_visible { 260.0 } else { 0.0 };
        let toc_width = if self.toc_visible { 260.0 } else { 0.0 };
        let backlinks_width = if self.workspace.backlinks_visible {
            260.0
        } else {
            0.0
        };
        let pdf_ann_width = if self.pdf_annotations_visible && self.active_pdf_path.is_some() {
            270.0
        } else {
            0.0
        };
        let chrome_width = sidebar_width + toc_width + backlinks_width + pdf_ann_width;
        let content_width = (self.window_width - chrome_width).max(320.0);

        if self.split_view_active
            && self.workspace.active_path.is_some()
            && self.active_pdf_path.is_some()
        {
            (content_width * (1.0 - self.split_ratio)).max(280.0)
        } else {
            content_width
        }
    }

    fn estimated_editor_viewport_height(&self) -> f32 {
        if self.editor_viewport_height > 0.0 {
            return self.editor_viewport_height;
        }
        let mut height = self.window_height - 48.0; // toolbar ~48px
        if self.search.editor.visible && self.workspace.active_path.is_some() {
            height -= 40.0; // search bar ~40px
        }
        height.max(200.0)
    }

    fn estimated_editor_line_y(&self, target_line: usize) -> f32 {
        crate::editor::renderer::line_visual_y::<iced::Renderer>(
            &self.highlighted_lines,
            &self.image_cache,
            &self.math_cache,
            self.estimated_editor_viewport_width().max(240.0),
            self.buffer.cursor_line,
            self.buffer.cursor_col,
            target_line,
            true,
        ) + 20.0
    }

    fn restore_scroll_positions(&self) -> Task<Message> {
        let mut tasks = Vec::new();
        // Restore editor scroll position after search bar toggle
        let editor_y = self.editor_scroll_y;
        tasks.push(operation::scroll_to(
            iced::advanced::widget::Id::new(EDITOR_SCROLLABLE_ID),
            AbsoluteOffset {
                x: 0.0,
                y: editor_y,
            },
        ));
        // Restore PDF scroll position after search bar toggle
        if self.active_pdf_path.is_some() {
            let pdf_y = self.pdf_scroll_y;
            tasks.push(operation::scroll_to(
                iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                AbsoluteOffset { x: 0.0, y: pdf_y },
            ));
        }
        Task::batch(tasks)
    }

    fn pdf_search_is_active(&self) -> bool {
        self.pdf_state.search.visible
            && self.active_pdf_path.is_some()
            && (self.showing_pdf
                || (self.split_view_active
                    && self.workspace.active_path.is_some()
                    && self.active_panel == ActivePanel::Pdf))
    }

    fn editor_search_is_active(&self) -> bool {
        self.search.editor.visible
            && self.workspace.active_path.is_some()
            && (!self.split_view_active || self.active_panel == ActivePanel::Markdown)
    }

    fn pdf_copy_shortcut_is_active(&self) -> bool {
        self.pdf_selection.is_some()
            && self.active_pdf_path.is_some()
            && (self.showing_pdf
                || (self.split_view_active
                    && self.workspace.active_path.is_some()
                    && self.active_panel == ActivePanel::Pdf))
    }

    fn pdf_selection_quote_link_command(&self) -> Option<EditorCommand> {
        let sel = self.pdf_selection.as_ref()?;
        let page_text = self.pdf_page_text.get(&sel.page_index)?;
        let pdf_path = self.active_pdf_path.as_ref()?;
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

    fn pdf_annotation_link_command(&self, annotation_id: &str) -> Option<EditorCommand> {
        let (_, ann) = self.find_pdf_annotation(annotation_id)?;
        let pdf_path = self.active_pdf_path.as_ref()?;
        let page_number = ann.page_index + 1;
        let link = build_pdf_link(pdf_path, Some(page_number), Some(&ann.id));
        Some(EditorCommand::InsertPdfAnnotationLink {
            selected_text: ann.selected_text,
            page_number,
            link,
        })
    }

    fn command_palette_commands(&self) -> Vec<views::command_palette::Command> {
        let ctx = crate::command_registry::CommandContext {
            markdown_open: self.workspace.active_path.is_some(),
            pdf_open: self.active_pdf_path.is_some(),
            image_open: self.active_image_path.is_some(),
            active_pane: match self.active_panel {
                ActivePanel::Markdown => crate::app_shell::AppShellPane::Markdown,
                ActivePanel::Pdf => crate::app_shell::AppShellPane::Pdf,
            },
            has_vault: self.workspace.vault_root.is_some(),
            pdf_has_selection: self.workspace.active_path.is_some()
                && self.pdf_selection_quote_link_command().is_some(),
            has_focused_annotation: self.workspace.active_path.is_some()
                && self
                    .focused_annotation_id
                    .as_deref()
                    .and_then(|id| self.pdf_annotation_link_command(id))
                    .is_some(),
        };

        crate::command_registry::get_command_registry()
            .into_iter()
            .filter(|meta| {
                if matches!(
                    meta.id,
                    Shortcut::InsertPdfQuote | Shortcut::InsertPdfHighlight
                ) {
                    meta.is_enabled(ctx).is_ok()
                } else {
                    true
                }
            })
            .map(|meta| {
                let disabled_reason = meta.is_enabled(ctx).err();
                views::command_palette::Command {
                    name: meta.name.to_string(),
                    shortcut: meta.id,
                    icon: meta.icon.to_string(),
                    group_name: match meta.group {
                        crate::app_shell::CommandGroup::File => "File",
                        crate::app_shell::CommandGroup::Edit => "Edit",
                        crate::app_shell::CommandGroup::Navigation => "Navigation",
                        crate::app_shell::CommandGroup::View => "View",
                        crate::app_shell::CommandGroup::Research => "Research",
                        crate::app_shell::CommandGroup::Annotation => "Annotation",
                        crate::app_shell::CommandGroup::Search => "Search",
                    },
                    shortcut_label: meta.default_shortcut.map(|s| s.to_string()),
                    disabled_reason,
                }
            })
            .collect()
    }

    fn citation_palette_items(&self) -> Vec<crate::messages::CitationItem> {
        let mut items = Vec::new();

        // 1. Current selection
        if let (Some(sel), Some(_path)) = (&self.pdf_selection, &self.active_pdf_path) {
            if let Some(page_text) = self.pdf_page_text.get(&sel.page_index) {
                let start = sel.anchor_idx.min(sel.focus_idx);
                let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
                let selected_text = text_by_char_range(&page_text.text, start, end);
                if !selected_text.trim().is_empty() {
                    items.push(crate::messages::CitationItem::Selection {
                        text: selected_text,
                        page_index: sel.page_index,
                    });
                }
            }
        }

        // If query is empty, show selection + active PDF annotations.
        let query_trimmed = self.overlays.citation_palette_query.trim();
        if query_trimmed.is_empty() {
            // Add all annotations from current PDF
            for page_anns in self.pdf_annotations.values() {
                for ann in page_anns {
                    items.push(crate::messages::CitationItem::Annotation {
                        id: ann.id.clone(),
                        text: ann.selected_text.clone(),
                        page_index: ann.page_index,
                    });
                }
            }
        } else {
            // Search active PDF annotations
            for page_anns in self.pdf_annotations.values() {
                for ann in page_anns {
                    let matches_text = ann
                        .selected_text
                        .to_lowercase()
                        .contains(&query_trimmed.to_lowercase());
                    let matches_note = ann
                        .note
                        .as_ref()
                        .map(|n| n.to_lowercase().contains(&query_trimmed.to_lowercase()))
                        .unwrap_or(false);
                    if matches_text || matches_note {
                        items.push(crate::messages::CitationItem::Annotation {
                            id: ann.id.clone(),
                            text: ann.selected_text.clone(),
                            page_index: ann.page_index,
                        });
                    }
                }
            }

            // Search database cached PDF FTS content
            if let Ok(hits) = self.state.search_cached_pdf_text(query_trimmed, 20) {
                for hit in hits {
                    items.push(crate::messages::CitationItem::SearchHit {
                        path: hit.vault_path,
                        page_index: hit.page_index,
                        snippet: md_editor_core::vault::search_result_preview(
                            &hit.content,
                            query_trimmed,
                            None,
                        ),
                    });
                }
            }
        }

        items
    }

    fn submit_first_citation_palette_item(&mut self) -> Task<Message> {
        if !self.overlays.citation_palette_visible {
            return Task::none();
        }
        let Some(item) = self.citation_palette_items().into_iter().next() else {
            self.overlays.toast = Some("No citation matches".to_string());
            return Task::none();
        };
        self.choose_citation_item(item)
    }

    fn choose_citation_item(&mut self, item: crate::messages::CitationItem) -> Task<Message> {
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
                let link = crate::pdf_links::build_pdf_link(&path, Some(page_index + 1), None);
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

    fn center_editor_line(&self, line: usize) -> Task<Message> {
        let y = self.estimated_editor_line_y(line);
        let viewport_height = self.estimated_editor_viewport_height();
        // Always center the matched line in the viewport
        let target_y = (y - viewport_height / 2.0 + 18.0).max(0.0);

        Task::perform(async move { target_y }, Message::ScrollEditorToTarget)
    }

    fn ensure_editor_line_visible(&self, line: usize) -> Task<Message> {
        let y = self.estimated_editor_line_y(line);
        let viewport_height = self.estimated_editor_viewport_height();
        let current_scroll = self.editor_scroll_y;
        let margin = 40.0;

        if y < current_scroll + margin {
            let target_y = (y - margin).max(0.0);
            Task::perform(async move { target_y }, Message::ScrollEditorToTarget)
        } else if y > current_scroll + viewport_height - margin - 24.0 {
            let target_y = (y - viewport_height + margin + 24.0).max(0.0);
            Task::perform(async move { target_y }, Message::ScrollEditorToTarget)
        } else {
            Task::none()
        }
    }

    fn replace_all_in_current_document(&mut self) -> Result<(usize, Task<Message>), String> {
        if self.workspace.active_path.is_none() {
            return Err("Open a markdown file before replacing text".to_string());
        }
        if self.search.editor.query.is_empty() {
            return Err("Search query is empty".to_string());
        }

        let text = self.buffer.text();
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
            self.buffer.execute(EditorCommand::ReplaceAll {
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

    fn replace_current_match(&mut self) -> Result<Task<Message>, String> {
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

        self.buffer.execute(EditorCommand::SetSelection {
            anchor_line: m.line,
            anchor_col: m.start_col,
            focus_line: m.line,
            focus_col: m.end_col,
        });

        let result = self.buffer.execute(EditorCommand::InsertText(replace_text));

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

        let (cursor_line, cursor_col) = (self.buffer.cursor_line, self.buffer.cursor_col);
        let mut next_idx = 0;
        for (i, nm) in new_matches.iter().enumerate() {
            if nm.line > cursor_line || (nm.line == cursor_line && nm.start_col >= cursor_col) {
                next_idx = i;
                break;
            }
        }

        self.search.editor.active_index = Some(next_idx);
        let next_match = new_matches[next_idx];

        self.buffer.execute(EditorCommand::SetSelection {
            anchor_line: next_match.line,
            anchor_col: next_match.start_col,
            focus_line: next_match.line,
            focus_col: next_match.end_col,
        });

        let center_task = self.center_editor_line(next_match.line);

        Ok(Task::batch(vec![highlight_task, center_task]))
    }

    fn build_global_search_query(&self, text: String) -> md_editor_core::types::UnifiedSearchQuery {
        let mut query = md_editor_core::types::UnifiedSearchQuery::all_sources(text)
            .with_active_paths(
                self.workspace.active_path.as_deref(),
                self.active_pdf_path.as_deref(),
            );
        query.sources = self.search.global.sources.clone();
        query
    }

    fn update_global_search_searching(&mut self) {
        self.search.global.update_searching();
    }

    fn cancel_global_pdf_search(&mut self) {
        if let Some(renderer) = self.state.pdf_renderer() {
            let _ = renderer.cancel_search(self.search.pdf_active_id);
        }
        self.search.pdf_active_id = self.search.pdf_active_id.wrapping_add(1);
        self.pdf_state.search.searching = false;
        self.search.global.pdf_search_id = None;
        self.search.global.pending_pdf = false;
        self.search.global.pending_vault_pdf = false;
        self.search.global.pending_db = false;
        self.search.global.pdf_status = None;
        self.update_global_search_searching();
    }

    fn search_registered_pdf_text_task(
        &self,
        search_id: u64,
        query: md_editor_core::types::UnifiedSearchQuery,
    ) -> Task<Message> {
        let state = self.state.clone();
        let active_pdf_path = self.active_pdf_path.clone();

        Task::perform(
            async move {
                let results = tokio::task::spawn_blocking(move || {
                    search_registered_pdf_text_results(&state, &query, active_pdf_path.as_deref())
                })
                .await
                .unwrap_or_default();
                (search_id, results)
            },
            |(id, results)| Message::UnifiedPdfTextSearchMatchesFound(id, results),
        )
    }

    fn index_registered_pdf_text_task(&self) -> Task<Message> {
        let state = self.state.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || index_registered_pdf_text_pages(&state))
                    .await
                    .unwrap_or_else(|err| Err(err.to_string()))
            },
            Message::PdfTextIndexFinished,
        )
    }

    fn search_pdf(&mut self) -> Task<Message> {
        let Some(path) = &self.active_pdf_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        let query = self.pdf_state.search.query.clone();
        if query.trim().is_empty() {
            self.pdf_state.search.matches.clear();
            self.pdf_state.search.page_index.clear();
            self.pdf_state.search.searching = false;
            return Task::none();
        }
        let regex = self.pdf_state.search.regex;
        let match_case = self.pdf_state.search.match_case;
        let path_str = abs_path.to_string_lossy().to_string();

        let Some(renderer) = self.state.pdf_renderer() else {
            return Task::none();
        };

        // Increment active search id and set searching = true
        self.pdf_state.search.searching = true;
        self.search.pdf_active_id = self.search.pdf_active_id.wrapping_add(1);
        let search_id = self.search.pdf_active_id;

        // Cancel previous search
        let _ = renderer.cancel_search(search_id.wrapping_sub(1));

        self.pdf_state.search.matches.clear();
        self.pdf_state.search.page_index.clear();

        match renderer.search_text_stream(path_str, query, regex, match_case, search_id) {
            Ok((res_rx, done_rx)) => {
                let (tokio_tx, tokio_rx) = tokio::sync::mpsc::channel(100);

                tokio::task::spawn_blocking(move || {
                    while let Ok(m) = res_rx.recv() {
                        if tokio_tx
                            .blocking_send(Message::PdfSearchMatchesFound(search_id, vec![m]))
                            .is_err()
                        {
                            return;
                        }
                    }
                    let res = done_rx.recv().unwrap_or(Ok(()));
                    let _ = tokio_tx.blocking_send(Message::PdfSearchFinished(search_id, res));
                });

                let stream = iced::futures::stream::unfold(tokio_rx, |mut rx| async move {
                    if let Some(msg) = rx.recv().await {
                        Some((msg, rx))
                    } else {
                        None
                    }
                });

                Task::stream(stream)
            }
            Err(err) => {
                self.search.pdf_error = Some(err);
                self.pdf_state.search.searching = false;
                Task::none()
            }
        }
    }

    fn run_editor_command(&mut self, command: EditorCommand) -> Task<Message> {
        let keep_cursor_visible = editor_command_keeps_cursor_visible(&command);
        self.run_editor_command_with_scroll(command, keep_cursor_visible)
    }

    fn handle_editor_block_action(
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
                while line < self.buffer.line_count() {
                    let text = self.buffer.line_text(line);
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
                let line_text = self.buffer.line_text(line_idx);
                if let Some(start_idx) = line_text.find("pdf://") {
                    let rest = &line_text[start_idx..];
                    let end_idx = rest.find(')').unwrap_or(rest.len());
                    let link = rest[..end_idx].to_string();
                    Task::done(Message::SidebarFileClicked(link))
                } else {
                    Task::none()
                }
            }
        }
    }

    fn handle_editor_link_action(
        &mut self,
        line_idx: usize,
        start_col: usize,
        end_col: usize,
        link_target: String,
        action: crate::messages::EditorLinkActionKind,
    ) -> Task<Message> {
        use crate::messages::EditorLinkActionKind;
        match action {
            EditorLinkActionKind::OpenLink => Task::done(Message::SidebarFileClicked(link_target)),
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
                        Task::done(Message::SidebarFileClicked(new_path))
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

    fn run_editor_command_with_scroll(
        &mut self,
        command: EditorCommand,
        keep_cursor_visible: bool,
    ) -> Task<Message> {
        let result = self.buffer.execute(command);
        if result.text_changed {
            self.pending_editor_save = Some(std::time::Instant::now());
        }
        let content_task = if result.projection_changed {
            self.highlight_all()
        } else {
            Task::none()
        };

        if keep_cursor_visible {
            Task::batch(vec![
                content_task,
                self.ensure_editor_line_visible(self.buffer.cursor_line),
            ])
        } else {
            content_task
        }
    }

    fn load_images(&mut self) -> Task<Message> {
        let mut failures = Vec::new();
        let Some(active_path) = &self.workspace.active_path else {
            return Task::none();
        };
        let Some(vault_root) = &self.workspace.vault_root else {
            return Task::none();
        };
        let Some(base_path) = std::path::Path::new(vault_root)
            .join(active_path)
            .parent()
            .map(|path| path.to_path_buf())
        else {
            return Task::none();
        };

        for line in &self.highlighted_lines {
            for span in &line.spans {
                if span.is_image {
                    if let Some(path) = &span.image_path {
                        if !self.image_cache.contains_key(path)
                            && !self.image_errors.contains_key(path)
                        {
                            let img_path = base_path.join(path);
                            match image::open(&img_path) {
                                Ok(img) => {
                                    self.image_errors.remove(path);
                                    let (width, height) = img.dimensions();
                                    let handle = iced::widget::image::Handle::from_rgba(
                                        width,
                                        height,
                                        img.into_rgba8().into_raw(),
                                    );
                                    self.image_cache.insert(
                                        path.clone(),
                                        (handle, width as f32, height as f32),
                                    );
                                }
                                Err(err) => failures.push(Task::done(Message::ImageLoadFailed(
                                    path.clone(),
                                    err.to_string(),
                                ))),
                            }
                        }
                    }
                }
            }
        }
        Task::batch(failures)
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
                    if !tex.is_empty()
                        && !self.math_cache.contains_key(&tex)
                        && !self.math_errors.contains_key(&tex)
                    {
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
            | EditorCommand::ToggleHeading
            | EditorCommand::ToggleBlockquote
            | EditorCommand::ToggleUnorderedList
            | EditorCommand::ToggleOrderedList
            | EditorCommand::InsertCodeBlock
            | EditorCommand::InsertMathBlock
            | EditorCommand::InsertTable
            | EditorCommand::InsertPdfQuoteLink { .. }
            | EditorCommand::InsertPdfAnnotationLink { .. }
            | EditorCommand::DuplicateLine
            | EditorCommand::MoveLineUp
            | EditorCommand::MoveLineDown
            | EditorCommand::ReplaceAll { .. }
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

fn pdf_companion_note_key(pdf_path: &str) -> String {
    format!("pdf_companion_note:{}", pdf_path.replace('\\', "/"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusTarget {
    FileSearch,
    GlobalSearch,
    PdfSearch,
    CommandPalette,
    CitationPalette,
}

impl FocusTarget {
    fn widget_id(self) -> &'static str {
        match self {
            Self::FileSearch => views::search::FILE_SEARCH_INPUT_ID,
            Self::GlobalSearch => views::search::GLOBAL_SEARCH_INPUT_ID,
            Self::PdfSearch => views::pdf_viewer::PDF_SEARCH_INPUT_ID,
            Self::CommandPalette => views::command_palette::COMMAND_PALETTE_INPUT_ID,
            Self::CitationPalette => views::citation_palette::CITATION_PALETTE_INPUT_ID,
        }
    }
}

fn focus_target(target: FocusTarget) -> Task<Message> {
    operation::focus(iced::advanced::widget::Id::new(target.widget_id()))
}

fn focus_file_search_input() -> Task<Message> {
    focus_target(FocusTarget::FileSearch)
}

fn focus_global_search_input() -> Task<Message> {
    focus_target(FocusTarget::GlobalSearch)
}

fn focus_command_palette_input() -> Task<Message> {
    focus_target(FocusTarget::CommandPalette)
}

fn focus_citation_palette_input() -> Task<Message> {
    focus_target(FocusTarget::CitationPalette)
}

fn search_registered_pdf_text_results(
    state: &Arc<md_editor_core::state::AppState>,
    query: &md_editor_core::types::UnifiedSearchQuery,
    active_pdf_path: Option<&str>,
) -> md_editor_core::types::UnifiedPdfTextSearchResultBatch {
    let Some(renderer) = state.pdf_renderer() else {
        return empty_pdf_text_batch();
    };
    let vault_root = match state.vault_root_path() {
        Ok(Some(path)) => path,
        _ => return empty_pdf_text_batch(),
    };
    let pdf_paths = match md_editor_core::vault::list_all_pdf_files(&vault_root) {
        Ok(files) => files
            .into_iter()
            .map(|p| md_editor_core::vault::path_to_relative_string(&p, &vault_root))
            .collect::<Vec<_>>(),
        Err(_) => return empty_pdf_text_batch(),
    };
    let total_candidates = pdf_paths
        .iter()
        .filter(|path| active_pdf_path != Some(path.as_str()))
        .count();
    let targets = registered_pdf_search_targets(
        pdf_paths,
        active_pdf_path,
        GLOBAL_PDF_TEXT_SEARCH_MAX_DOCUMENTS,
    );
    let document_cap_reached = total_candidates > targets.len();

    let mut results =
        md_editor_core::vault::search_cached_pdf_text(state, query.text.trim(), &targets)
            .unwrap_or_default();
    let cached_paths = results
        .iter()
        .map(|result| result.path.clone())
        .collect::<std::collections::HashSet<_>>();
    if results.len() >= GLOBAL_PDF_TEXT_SEARCH_MAX_RESULTS {
        results.truncate(GLOBAL_PDF_TEXT_SEARCH_MAX_RESULTS);
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.line.cmp(&b.line))
        });
        return md_editor_core::types::UnifiedPdfTextSearchResultBatch {
            results,
            searched_documents: cached_paths.len(),
            total_candidates,
            result_cap_reached: true,
            document_cap_reached,
        };
    }

    let mut searched_documents = 0;
    let mut result_cap_reached = false;
    for vault_path in targets {
        if cached_paths.contains(&vault_path) {
            searched_documents += 1;
            continue;
        }
        searched_documents += 1;
        let abs_path = md_editor_core::vault::resolve_vault_path(&vault_root, &vault_path);
        let abs_path = abs_path.to_string_lossy().to_string();
        let Ok(matches) = renderer.search_text(&abs_path, &query.text, false, false) else {
            continue;
        };

        for search_match in matches {
            let mut score = 4.0;
            if search_match
                .context
                .trim()
                .eq_ignore_ascii_case(query.text.trim())
            {
                score *= query.ranking.exact_phrase_boost;
            }
            results.push(md_editor_core::types::UnifiedSearchResult {
                group: md_editor_core::types::SearchResultGroup::PdfContent,
                path: vault_path.clone(),
                line: (search_match.page_index + 1) as usize,
                context: format!(
                    "PDF text ({} areas): {}",
                    search_match.rects.len(),
                    md_editor_core::vault::search_result_preview(
                        &search_match.context,
                        query.text.trim(),
                        None,
                    )
                ),
                score,
                page_index: Some(search_match.page_index),
                annotation_id: None,
            });
            if results.len() >= GLOBAL_PDF_TEXT_SEARCH_MAX_RESULTS {
                result_cap_reached = true;
                break;
            }
        }
        if results.len() >= GLOBAL_PDF_TEXT_SEARCH_MAX_RESULTS {
            result_cap_reached = true;
            break;
        }
    }
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.cmp(&b.line))
    });
    md_editor_core::types::UnifiedPdfTextSearchResultBatch {
        results,
        searched_documents,
        total_candidates,
        result_cap_reached,
        document_cap_reached,
    }
}

fn empty_pdf_text_batch() -> md_editor_core::types::UnifiedPdfTextSearchResultBatch {
    md_editor_core::types::UnifiedPdfTextSearchResultBatch {
        results: Vec::new(),
        searched_documents: 0,
        total_candidates: 0,
        result_cap_reached: false,
        document_cap_reached: false,
    }
}

fn format_pdf_search_status(
    batch: &md_editor_core::types::UnifiedPdfTextSearchResultBatch,
) -> String {
    let mut status = format!(
        "PDF text: searched {} of {} registered PDFs",
        batch.searched_documents, batch.total_candidates
    );
    if batch.result_cap_reached {
        status.push_str("; result cap reached");
    } else if batch.document_cap_reached {
        status.push_str("; document cap reached");
    }
    status
}

fn index_registered_pdf_text_pages(
    state: &Arc<md_editor_core::state::AppState>,
) -> Result<usize, String> {
    let Some(renderer) = state.pdf_renderer() else {
        return Ok(0);
    };
    let vault_root = state.vault_root_path()?;
    let Some(vault_root) = vault_root else {
        return Ok(0);
    };
    let pdf_paths = md_editor_core::vault::list_all_pdf_files(&vault_root)?
        .into_iter()
        .map(|p| md_editor_core::vault::path_to_relative_string(&p, &vault_root))
        .collect::<Vec<_>>();
    let targets = registered_pdf_index_targets(pdf_paths, PDF_TEXT_INDEX_MAX_DOCUMENTS);

    let mut indexed_pages = 0;
    for vault_path in targets {
        if state
            .validate_and_invalidate_pdf_cache(&vault_path)
            .unwrap_or(false)
        {
            continue;
        }

        let abs_path = md_editor_core::vault::resolve_vault_path(&vault_root, &vault_path);
        let abs_path = abs_path.to_string_lossy().to_string();

        if let Ok((hash, len, mtime)) =
            md_editor_core::pdf::compute_provisional_id(std::path::Path::new(&abs_path))
        {
            let _ = state.save_pdf_document(&hash, &vault_path, len, mtime);
        }

        let page_count = renderer.page_count(&abs_path).unwrap_or(0);
        let pages_to_index = page_count.min(PDF_TEXT_INDEX_MAX_PAGES_PER_DOCUMENT);
        for page_index in 0..pages_to_index {
            if let Ok(page_text) = renderer.get_page_text(&abs_path, page_index) {
                state.save_pdf_page_text(&vault_path, page_index, &page_text.text)?;
                indexed_pages += 1;
            }
        }
    }
    Ok(indexed_pages)
}

fn registered_pdf_index_targets(pdf_paths: Vec<String>, max_documents: usize) -> Vec<String> {
    pdf_paths.into_iter().take(max_documents).collect()
}

fn registered_pdf_search_targets(
    pdf_paths: Vec<String>,
    active_pdf_path: Option<&str>,
    max_documents: usize,
) -> Vec<String> {
    pdf_paths
        .into_iter()
        .filter(|path| active_pdf_path != Some(path.as_str()))
        .take(max_documents)
        .collect()
}

fn focus_pdf_search_input() -> Task<Message> {
    focus_target(FocusTarget::PdfSearch)
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
    normalized.to_string_lossy().to_string().replace('\\', "/")
}

pub(crate) fn resolve_relative_link_path(
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
    crate::editor::highlight::markdown_anchor_slug(s)
}

fn save_markdown_file_with_parser_targets(
    state: &md_editor_core::state::AppState,
    path: &str,
    content: &str,
) -> Result<(), String> {
    let markdown_link_targets = parser_index_targets(content);
    md_editor_core::vault::save_file_with_markdown_link_targets(
        state,
        path,
        content,
        &markdown_link_targets,
    )
}

fn reindex_markdown_file_with_parser_targets(
    state: &md_editor_core::state::AppState,
    path: &str,
    content: &str,
) -> Result<(), String> {
    let targets = parser_index_targets(content);
    state.update_file_index_targets(path, &targets)
}

fn reindex_vault_with_parser_targets(
    state: &md_editor_core::state::AppState,
    vault_root: &std::path::Path,
) -> Result<(), String> {
    let md_files = md_editor_core::vault::list_all_md_files(vault_root)?;
    let mut files = Vec::with_capacity(md_files.len());
    for abs_path in md_files {
        let content = std::fs::read_to_string(&abs_path)
            .map_err(|err| format!("Failed to read file {}: {err}", abs_path.display()))?;
        let targets = parser_index_targets(&content);
        files.push((abs_path, targets));
    }
    state.rebuild_file_index_with_targets(vault_root, files)
}

fn parser_index_targets(content: &str) -> Vec<String> {
    let highlighted = highlight::highlight_markdown(content);
    let metadata = highlight::extract_document_metadata(&highlighted);
    metadata
        .links
        .iter()
        .filter_map(indexable_markdown_link_target)
        .collect()
}

fn indexable_markdown_link_target(
    link: &crate::editor::highlight::MarkdownLinkEntry,
) -> Option<String> {
    if !matches!(
        link.kind,
        crate::editor::highlight::MarkdownLinkKind::Wiki
            | crate::editor::highlight::MarkdownLinkKind::Inline
            | crate::editor::highlight::MarkdownLinkKind::ResolvedReference
    ) {
        return None;
    }

    let target = link.target.trim();
    if target.is_empty() || target.starts_with('#') {
        return None;
    }
    if let Some(pdf_target) = parse_pdf_link(target) {
        return Some(pdf_target.path);
    }
    if has_uri_scheme(target) {
        return None;
    }
    Some(target.to_string())
}

pub(crate) fn has_uri_scheme(target: &str) -> bool {
    let Some(colon_idx) = target.find(':') else {
        return false;
    };
    let first_separator = target
        .find('/')
        .into_iter()
        .chain(target.find('\\'))
        .min()
        .unwrap_or(usize::MAX);
    colon_idx < first_separator
        && target[..colon_idx]
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
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

    let metadata = crate::editor::highlight::extract_document_metadata(highlighted_lines);
    for anchor in &metadata.anchors {
        if anchor.slug.eq_ignore_ascii_case(target_slug) {
            return Some(anchor.line);
        }
        if let Some(ref alt) = alternative_slug
            && anchor.slug.eq_ignore_ascii_case(alt)
        {
            return Some(anchor.line);
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

fn format_citation_item_as_markdown(
    item: &crate::messages::CitationItem,
    active_pdf_path: Option<&str>,
) -> String {
    match item {
        crate::messages::CitationItem::Selection { text, page_index } => {
            let pdf_path = active_pdf_path.unwrap_or("document.pdf");
            let link = crate::pdf_links::build_pdf_link(pdf_path, Some(page_index + 1), None);
            format!(
                "> {}\n> [Selection (Page {})]({})\n\n",
                text.trim().replace('\n', "\n> "),
                page_index + 1,
                link
            )
        }
        crate::messages::CitationItem::Annotation {
            id,
            text,
            page_index,
        } => {
            let pdf_path = active_pdf_path.unwrap_or("document.pdf");
            let link = crate::pdf_links::build_pdf_link(pdf_path, Some(page_index + 1), Some(id));
            format!(
                "> {}\n> [Highlight (Page {})]({})\n\n",
                text.trim().replace('\n', "\n> "),
                page_index + 1,
                link
            )
        }
        crate::messages::CitationItem::SearchHit {
            path,
            page_index,
            snippet,
        } => {
            let link = crate::pdf_links::build_pdf_link(path, Some(page_index + 1), None);
            format!(
                "> {}\n> [PDF Text (Page {})]({})\n\n",
                snippet.trim().replace('\n', "\n> "),
                page_index + 1,
                link
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use md_editor_core::pdf::{
        PdfAnnotation, PdfAnnotationColor, PdfAnnotationKind, PdfAnnotationStatus,
    };
    use md_editor_core::types::FileEntry;

    use crate::views::modals::ModalType;

    fn text_layout_bounds(
        ui: &mut iced_test::Simulator<'_, Message, Theme, iced::Renderer>,
        text: &str,
    ) -> iced::Rectangle {
        ui.find(text)
            .unwrap_or_else(|_| panic!("{text:?} should render"))
            .bounds()
    }

    fn rectangles_overlap(a: iced::Rectangle, b: iced::Rectangle) -> bool {
        let a_right = a.x + a.width;
        let b_right = b.x + b.width;
        let a_bottom = a.y + a.height;
        let b_bottom = b.y + b.height;

        a.x < b_right && b.x < a_right && a.y < b_bottom && b.y < a_bottom
    }

    fn assert_no_text_overlap(
        ui: &mut iced_test::Simulator<'_, Message, Theme, iced::Renderer>,
        first: &str,
        second: &str,
    ) {
        let first_bounds = text_layout_bounds(ui, first);
        let second_bounds = text_layout_bounds(ui, second);

        assert!(
            !rectangles_overlap(first_bounds, second_bounds),
            "{first:?} at {first_bounds:?} should not overlap {second:?} at {second_bounds:?}"
        );
    }

    fn pdf_text_batch(
        results: Vec<md_editor_core::types::UnifiedSearchResult>,
        searched_documents: usize,
        total_candidates: usize,
        result_cap_reached: bool,
        document_cap_reached: bool,
    ) -> md_editor_core::types::UnifiedPdfTextSearchResultBatch {
        md_editor_core::types::UnifiedPdfTextSearchResultBatch {
            results,
            searched_documents,
            total_candidates,
            result_cap_reached,
            document_cap_reached,
        }
    }

    fn app_without_vault() -> MdEditor {
        let mut app = MdEditor::new().0;
        app.workspace.vault_root = None;
        app.workspace.vault_entries.clear();
        app.workspace.selected_path = None;
        app.workspace.active_path = None;
        app.active_pdf_path = None;
        app.active_image_path = None;
        app.active_image = None;
        app.showing_pdf = false;
        app.split_view_active = false;
        app.workspace.navigation_history = NavigationHistory::default();
        app
    }

    fn app_with_vault() -> MdEditor {
        let mut app = app_without_vault();
        app.sidebar_visible = true;
        app.workspace.backlinks_visible = false;
        app.toc_visible = false;
        app.tracker.visible = false;
        app.pdf_annotations_visible = false;
        app.split_view_active = false;
        app.split_ratio = 0.5;
        app.pdf_split_ratio = 0.3;
        app.active_panel = ActivePanel::Markdown;
        app.workspace.vault_root = Some("/tmp/md-editor-ui-audit".to_string());
        app.workspace.vault_entries = vec![
            FileEntry {
                path: "notes".to_string(),
                name: "notes".to_string(),
                is_dir: true,
            },
            FileEntry {
                path: "notes/research.md".to_string(),
                name: "research.md".to_string(),
                is_dir: false,
            },
            FileEntry {
                path: "papers/paper.pdf".to_string(),
                name: "paper.pdf".to_string(),
                is_dir: false,
            },
        ];
        app
    }

    fn app_with_markdown_file() -> MdEditor {
        let mut app = app_with_vault();
        app.workspace.active_path = Some("notes/research.md".to_string());
        app.workspace.selected_path = app.workspace.active_path.clone();
        app.buffer = DocBuffer::from_text("# Research\n\nSee [[related]].\n");
        app.highlighted_lines = highlight::highlight_markdown(&app.buffer.text());
        app.md_toc_entries = vec![views::toc::TocEntry {
            level: 1,
            text: "Research".to_string(),
            line: 0,
        }];
        app
    }

    fn app_with_large_markdown_file() -> MdEditor {
        let mut app = app_with_markdown_file();
        let mut text = String::from("# Large Research\n\n");
        for line in 0..1_500 {
            text.push_str(&format!("- finding {line}\n"));
        }
        app.workspace.active_path = Some("notes/large.md".to_string());
        app.workspace.selected_path = app.workspace.active_path.clone();
        app.buffer = DocBuffer::from_text(&text);
        app.highlighted_lines = highlight::highlight_markdown(&app.buffer.text());
        app.md_toc_entries = views::toc::get_toc(&app.highlighted_lines);
        app
    }

    fn app_with_pdf_file() -> MdEditor {
        let mut app = app_with_vault();
        app.active_pdf_path = Some("papers/paper.pdf".to_string());
        app.workspace.selected_path = app.active_pdf_path.clone();
        app.showing_pdf = true;
        app.pdf_total_pages = 3;
        app.pdf_current_page = 0;
        app.pdf_pages = vec![None, None, None];
        app.pdf_dimensions = vec![None, None, None];
        app.pdf_state.page_sizes = vec![Some((612.0, 792.0)); 3];
        app.pdf_placeholder_page_size = Some((612.0, 792.0));
        app.pdf_state.layout = PdfLayout::rebuild(
            &app.pdf_state.page_sizes,
            app.pdf_state.zoom,
            app.pdf_placeholder_page_size.unwrap_or((612.0, 792.0)),
            PDF_PAGE_SPACING,
            PDF_PAGE_LIST_PADDING,
            app.pdf_rotation,
        );
        app.active_panel = ActivePanel::Pdf;
        app
    }

    fn app_with_split_research() -> MdEditor {
        let mut app = app_with_pdf_file();
        app.workspace.active_path = Some("notes/research.md".to_string());
        app.buffer = DocBuffer::from_text("# Research\n\n[p. 1](pdf://papers/paper.pdf?page=1)\n");
        app.highlighted_lines = highlight::highlight_markdown(&app.buffer.text());
        app.split_view_active = true;
        app.active_panel = ActivePanel::Markdown;
        app
    }

    fn app_with_global_search() -> MdEditor {
        let mut app = app_with_markdown_file();
        app.search.visible = true;
        app.search.editor.query = "missing".to_string();
        app.search.global.searching = false;
        app.search.global.results.clear();
        app
    }

    fn app_with_file_search() -> MdEditor {
        let mut app = app_with_markdown_file();
        app.search.editor.visible = true;
        app.search.editor.query = "finding".to_string();
        app.search.editor.matches.clear();
        app.search.editor.active_index = None;
        app
    }

    fn app_with_pdf_search() -> MdEditor {
        let mut app = app_with_pdf_file();
        app.pdf_state.search.visible = true;
        app.pdf_state.search.query = "finding".to_string();
        app
    }

    fn app_with_command_palette() -> MdEditor {
        let mut app = app_with_markdown_file();
        app.overlays.command_palette_visible = true;
        app.overlays.command_palette_query = "navigate".to_string();
        app
    }

    fn app_with_active_modal() -> MdEditor {
        let mut app = app_with_markdown_file();
        app.overlays.active_modal = Some(ModalType::CreateFile);
        app.overlays.modal_input = "new-note.md".to_string();
        app
    }

    fn app_with_annotation_heavy_pdf() -> MdEditor {
        let mut app = app_with_pdf_file();
        app.pdf_annotations_visible = true;
        let annotations = (0..12)
            .map(|index| PdfAnnotation {
                id: format!("ann-{index}"),
                document_id: "doc".to_string(),
                page_index: index % 3,
                kind: PdfAnnotationKind::Highlight,
                color: PdfAnnotationColor::Yellow,
                selected_text: format!("Important quote {index}"),
                ranges: Vec::new(),
                rects: Vec::new(),
                note: None,
                linked_note_path: None,
                markdown_anchor: None,
                tags: vec!["review".to_string()],
                status: PdfAnnotationStatus::Unresolved,
                created_at: index as i64,
                updated_at: index as i64,
            })
            .collect::<Vec<_>>();
        for annotation in annotations {
            app.pdf_annotations
                .entry(annotation.page_index)
                .or_default()
                .push(annotation);
        }
        app
    }

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
    fn indexable_markdown_link_target_filters_external_links() {
        let wiki = markdown_link("notes/topic", highlight::MarkdownLinkKind::Wiki);
        let local_inline =
            markdown_link("../paper.md#section", highlight::MarkdownLinkKind::Inline);
        let external = markdown_link("https://example.com", highlight::MarkdownLinkKind::Inline);
        let pdf = markdown_link(
            "pdf://papers/a.pdf?page=2",
            highlight::MarkdownLinkKind::Inline,
        );
        let reference = markdown_link("ref-id", highlight::MarkdownLinkKind::Reference);
        let anchor = markdown_link("#local", highlight::MarkdownLinkKind::Wiki);
        let resolved_reference = markdown_link(
            "papers/b.pdf",
            highlight::MarkdownLinkKind::ResolvedReference,
        );

        assert_eq!(
            indexable_markdown_link_target(&wiki).as_deref(),
            Some("notes/topic")
        );
        assert_eq!(
            indexable_markdown_link_target(&local_inline).as_deref(),
            Some("../paper.md#section")
        );
        assert!(indexable_markdown_link_target(&external).is_none());
        assert_eq!(
            indexable_markdown_link_target(&pdf).as_deref(),
            Some("papers/a.pdf")
        );
        assert!(indexable_markdown_link_target(&reference).is_none());
        assert!(indexable_markdown_link_target(&anchor).is_none());
        assert_eq!(
            indexable_markdown_link_target(&resolved_reference).as_deref(),
            Some("papers/b.pdf")
        );
    }

    fn markdown_link(
        target: &str,
        kind: highlight::MarkdownLinkKind,
    ) -> highlight::MarkdownLinkEntry {
        highlight::MarkdownLinkEntry {
            line: 0,
            target: target.to_string(),
            display_text: target.to_string(),
            source_text: target.to_string(),
            kind,
        }
    }

    #[test]
    fn ui_audit_fixture_no_vault_renders_welcome() {
        let app = app_without_vault();
        let mut ui = iced_test::simulator(app.view());

        ui.find("Open Vault")
            .expect("no-vault fixture should render vault opener");
        ui.find("Ctrl+O")
            .expect("no-vault fixture should expose keyboard path");
    }

    #[test]
    fn ui_audit_fixture_markdown_file_renders_shell_and_editor() {
        let app = app_with_markdown_file();
        let mut ui = iced_test::simulator(app.view());

        // Toolbar now shows basename only (B1: full path removed, "• Saved" removed)
        ui.find("research.md")
            .expect("markdown fixture should render active basename");
    }

    #[test]
    fn ui_audit_fixture_pdf_file_renders_pdf_toolbar() {
        let app = app_with_pdf_file();
        let mut ui = iced_test::simulator(app.view());

        // Toolbar shows basename only (B1). Page still visible in PDF toolbar (B5).
        ui.find("paper.pdf")
            .expect("PDF fixture should render active PDF basename");
        ui.find("1 / 3")
            .expect("PDF fixture should render page status with 1-based label");
    }

    #[test]
    fn ui_audit_fixture_split_research_renders_both_active_paths() {
        let app = app_with_split_research();
        let mut ui = iced_test::simulator(app.view());

        // Toolbar shows active-pane basename; PDF controls always visible when PDF open.
        ui.find("research.md")
            .expect("split fixture should keep markdown basename visible");
        ui.find("1 / 3")
            .expect("split fixture should keep PDF controls visible");
    }

    #[test]
    fn ui_audit_fixture_overlays_and_sidebars_render_stable_states() {
        let search_app = app_with_global_search();
        let mut search_ui = iced_test::simulator(search_app.view());
        search_ui
            .find("No results found")
            .expect("global-search fixture should render empty state");

        let command_app = app_with_command_palette();
        let mut command_ui = iced_test::simulator(command_app.view());
        command_ui
            .find("Navigate Back")
            .expect("command-palette fixture should render filtered command");

        let modal_app = app_with_active_modal();
        let mut modal_ui = iced_test::simulator(modal_app.view());
        modal_ui
            .find("Create New File")
            .expect("modal fixture should render create action");

        let annotation_app = app_with_annotation_heavy_pdf();
        let mut annotation_ui = iced_test::simulator(annotation_app.view());
        annotation_ui
            .find("\"Important quote 0\"")
            .expect("annotation-heavy fixture should render annotation row");
        annotation_ui
            .find("#review")
            .expect("annotation-heavy fixture should render tag metadata");
    }

    #[test]
    fn ui_audit_fixture_large_and_narrow_states_render_stable_shell() {
        let large_app = app_with_large_markdown_file();
        let mut large_ui = iced_test::simulator(large_app.view());
        large_ui
            .find("large.md")
            .expect("large markdown fixture should render active basename");

        let search_app = app_with_file_search();
        let mut search_ui = iced_test::simulator(search_app.view());
        search_ui
            .find("No matches")
            .expect("file-search fixture should render no-result state");

        let narrow_app = app_with_split_research();
        let mut narrow_ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(420.0, 720.0),
            narrow_app.view(),
        );
        narrow_ui
            .find("research.md")
            .expect("narrow split fixture should preserve markdown basename");
        narrow_ui
            .find("1 / 3")
            .expect("narrow split fixture should preserve PDF page status");
    }

    #[test]
    fn ui_audit_keyboard_shortcuts_expose_baseline_accessibility_paths() {
        let mut app = app_with_markdown_file();

        let _ = app.update(Message::KeyboardShortcut(Shortcut::CommandPalette));
        assert!(app.overlays.command_palette_visible);
        assert!(!app.overlays.citation_palette_visible);

        let _ = app.update(Message::KeyboardShortcut(Shortcut::CitationPalette));
        assert!(app.overlays.citation_palette_visible);
        assert!(!app.overlays.command_palette_visible);
        assert!(!app.search.visible);

        let _ = app.update(Message::KeyboardShortcut(Shortcut::Escape));
        assert!(!app.overlays.citation_palette_visible);

        let _ = app.update(Message::KeyboardShortcut(Shortcut::Search));
        assert!(app.search.editor.visible);
        assert!(!app.search.visible);
        assert!(!app.pdf_state.search.visible);

        let _ = app.update(Message::KeyboardShortcut(Shortcut::TableOfContents));
        assert!(app.toc_visible);

        let _ = app.update(Message::KeyboardShortcut(Shortcut::FocusMode));
        assert!(!app.sidebar_visible);
        assert!(!app.workspace.backlinks_visible);
        assert!(!app.toc_visible);
        assert!(!app.tracker.visible);
    }

    #[test]
    fn test_active_pane_shortcut_routing_and_switching() {
        let mut app = app_with_markdown_file();
        app.workspace.active_path = Some("notes/research.md".to_string());
        app.active_pdf_path = Some("vault/paper.pdf".to_string());
        app.split_view_active = true;
        app.active_panel = ActivePanel::Markdown;

        // Switch to PDF pane
        let _ = app.update(Message::KeyboardShortcut(Shortcut::SwitchPane));
        assert_eq!(app.active_panel, ActivePanel::Pdf);

        // Search is routed to PDF search
        let _ = app.update(Message::KeyboardShortcut(Shortcut::Search));
        assert!(app.pdf_state.search.visible);
        assert!(!app.search.editor.visible);

        // Switch back to Markdown pane
        let _ = app.update(Message::KeyboardShortcut(Shortcut::SwitchPane));
        assert_eq!(app.active_panel, ActivePanel::Markdown);

        // Search is routed to Markdown/editor search
        app.pdf_state.search.visible = false;
        let _ = app.update(Message::KeyboardShortcut(Shortcut::Search));
        assert!(app.search.editor.visible);
        assert!(!app.pdf_state.search.visible);
    }

    #[test]
    fn ui_audit_focus_targets_map_to_rendered_input_ids() {
        assert_eq!(
            FocusTarget::FileSearch.widget_id(),
            views::search::FILE_SEARCH_INPUT_ID
        );
        assert_eq!(
            FocusTarget::GlobalSearch.widget_id(),
            views::search::GLOBAL_SEARCH_INPUT_ID
        );
        assert_eq!(
            FocusTarget::PdfSearch.widget_id(),
            views::pdf_viewer::PDF_SEARCH_INPUT_ID
        );
        assert_eq!(
            FocusTarget::CommandPalette.widget_id(),
            views::command_palette::COMMAND_PALETTE_INPUT_ID
        );
        assert_eq!(
            FocusTarget::CitationPalette.widget_id(),
            views::citation_palette::CITATION_PALETTE_INPUT_ID
        );

        let mut command_app = app_with_markdown_file();
        let _ = command_app.update(Message::CommandPaletteOpen);
        let mut command_ui = iced_test::simulator(command_app.view());
        command_ui
            .find(iced_test::selector::id(
                FocusTarget::CommandPalette.widget_id(),
            ))
            .expect("command palette shortcut target should exist when open");

        let mut citation_app = app_with_annotation_heavy_pdf();
        let _ = citation_app.update(Message::CitationPaletteToggle);
        let mut citation_ui = iced_test::simulator(citation_app.view());
        citation_ui
            .find(iced_test::selector::id(
                FocusTarget::CitationPalette.widget_id(),
            ))
            .expect("citation palette shortcut target should exist when open");

        let file_search_app = app_with_file_search();
        let mut file_search_ui = iced_test::simulator(file_search_app.view());
        file_search_ui
            .find(iced_test::selector::id(FocusTarget::FileSearch.widget_id()))
            .expect("file search target should exist when open");

        let global_search_app = app_with_global_search();
        let mut global_search_ui = iced_test::simulator(global_search_app.view());
        global_search_ui
            .find(iced_test::selector::id(
                FocusTarget::GlobalSearch.widget_id(),
            ))
            .expect("global search target should exist when open");

        let pdf_search_app = app_with_pdf_search();
        let mut pdf_search_ui = iced_test::simulator(pdf_search_app.view());
        pdf_search_ui
            .find(iced_test::selector::id(FocusTarget::PdfSearch.widget_id()))
            .expect("PDF search target should exist when open");
    }

    #[test]
    fn ui_audit_escape_closes_modal_before_background_overlays() {
        let mut app = app_with_active_modal();
        app.search.visible = true;
        app.overlays.command_palette_visible = true;

        let _ = app.update(Message::KeyboardShortcut(Shortcut::Escape));
        assert!(app.overlays.active_modal.is_none());
        assert!(app.search.visible);
        assert!(app.overlays.command_palette_visible);

        let _ = app.update(Message::KeyboardShortcut(Shortcut::Escape));
        assert!(!app.search.visible);
        assert!(app.overlays.command_palette_visible);
    }

    #[test]
    fn ui_audit_shell_labels_do_not_overlap_in_baseline_layouts() {
        // Toolbar now shows basename + no "• Saved" text (B1).
        // Verify page status and filename do not overlap in PDF and split layouts.
        let pdf_app = app_with_pdf_file();
        let mut pdf_ui = iced_test::simulator(pdf_app.view());
        assert_no_text_overlap(&mut pdf_ui, "paper.pdf", "1 / 3");

        let narrow_app = app_with_split_research();
        let mut narrow_ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(420.0, 720.0),
            narrow_app.view(),
        );
        assert_no_text_overlap(&mut narrow_ui, "research.md", "1 / 3");
    }

    #[test]
    fn app_shell_state_matches_ui_audit_fixtures() {
        let no_vault = app_without_vault().app_shell_state();
        assert_eq!(no_vault.mode, AppShellMode::NoVault);
        assert_eq!(no_vault.active_pane, AppShellPane::None);

        let markdown = app_with_markdown_file().app_shell_state();
        assert_eq!(markdown.mode, AppShellMode::EditorOnly);
        assert_eq!(markdown.active_pane, AppShellPane::Markdown);

        let pdf = app_with_pdf_file().app_shell_state();
        assert_eq!(pdf.mode, AppShellMode::PdfOnly);
        assert_eq!(pdf.active_pane, AppShellPane::Pdf);

        let split = app_with_split_research().app_shell_state();
        assert_eq!(split.mode, AppShellMode::SplitResearch);
        assert_eq!(split.active_pane, AppShellPane::Markdown);
        assert!(split.uses_split_research_layout());
        assert!(
            split
                .command_groups()
                .contains(&crate::app_shell::CommandGroup::Research)
        );
        assert!(
            split
                .command_groups()
                .contains(&crate::app_shell::CommandGroup::Annotation)
        );

        let search = app_with_global_search().app_shell_state();
        assert_eq!(search.mode, AppShellMode::SearchHeavy);
        assert_eq!(search.active_pane, AppShellPane::Markdown);
    }

    #[test]
    fn app_shell_status_matches_document_and_pdf_state() {
        let mut app = app_with_split_research();
        app.buffer.dirty = true;
        app.pdf_current_page = 1;
        app.pdf_total_pages = 3;
        app.pdf_state.zoom = 1.5;
        app.search.global.searching = true;
        app.search.global.pdf_status = Some("Searched 2 PDFs".to_string());

        let shell_state = app.app_shell_state();
        let status = app.app_shell_status(shell_state);

        assert_eq!(status.save_status, crate::app_shell::SaveStatus::Unsaved);
        assert_eq!(status.search_status.as_deref(), Some("Searched 2 PDFs"));
        // pdf_status removed from status bar — page/zoom live in PDF toolbar only.
        assert_eq!(status.active_pane, AppShellPane::Markdown);
    }

    #[test]
    fn app_shell_status_surfaces_toast_before_background_error() {
        let mut app = app_with_pdf_file();
        app.overlays.toast = Some("Linked note created".to_string());
        app.search.pdf_error = Some("PDF search failed".to_string());

        let status = app.app_shell_status(app.app_shell_state());

        assert_eq!(status.save_status, crate::app_shell::SaveStatus::Saved);
        assert_eq!(status.message.as_deref(), Some("Linked note created"));
    }

    #[test]
    fn app_shell_status_bar_active_pane_indicators_render() {
        let app_md = app_with_markdown_file();
        let mut ui_md = iced_test::simulator(app_md.view());
        ui_md
            .find("EDITOR")
            .expect("should render EDITOR active pane indicator");

        let app_pdf = app_with_pdf_file();
        let mut ui_pdf = iced_test::simulator(app_pdf.view());
        ui_pdf
            .find("PDF")
            .expect("should render PDF active pane indicator");
    }

    #[test]
    fn app_shell_persistence_reflects_visible_panels_and_window_width() {
        let mut app = app_with_split_research();
        app.workspace.backlinks_visible = true;
        app.window_width = 900.0;
        let wide = app.app_shell_state();
        assert!(!wide.persistence.sidebar_collapsed);
        assert!(!wide.persistence.reference_collapsed);
        assert!(!wide.persistence.workflow_collapsed);
        assert_eq!(
            wide.persistence.active_workflow_tab,
            WorkflowSidebarTab::Backlinks
        );

        app.window_width = 600.0;
        let narrow = app.app_shell_state();
        assert!(!narrow.persistence.sidebar_collapsed);
        assert!(narrow.persistence.reference_collapsed);
        assert!(narrow.persistence.workflow_collapsed);
    }

    #[test]
    fn app_shell_persistence_round_trips_through_config() {
        let mut app = app_with_split_research();
        app.sidebar_visible = false;
        app.workspace.backlinks_visible = false;
        app.toc_visible = true;
        app.tracker.visible = false;
        app.pdf_annotations_visible = false;
        app.split_ratio = 0.62;
        app.pdf_split_ratio = 0.4;
        app.set_active_panel(ActivePanel::Pdf);
        app.persist_shell_state();

        let saved =
            md_editor_core::config::get_sys_config(&app.state, APP_SHELL_PERSISTENCE_CONFIG_KEY)
                .unwrap()
                .expect("shell persistence should be written");
        assert!(saved.contains("active_workflow_tab=outline"));
        assert!(saved.contains("last_focused_pane=pdf"));

        app.sidebar_visible = true;
        app.toc_visible = false;
        app.split_ratio = 0.5;
        app.pdf_split_ratio = 0.3;
        app.active_panel = ActivePanel::Markdown;
        app.load_shell_persistence();

        assert!(!app.sidebar_visible);
        assert!(app.toc_visible);
        assert_eq!(app.active_panel, ActivePanel::Pdf);
        assert!((app.split_ratio - 0.62).abs() < f32::EPSILON);
        assert!((app.pdf_split_ratio - 0.4).abs() < f32::EPSILON);
    }

    #[test]
    fn save_markdown_file_with_parser_targets_indexes_local_links() {
        let root = unique_temp_dir("native_parser_save");
        std::fs::create_dir_all(&root).unwrap();
        let state = md_editor_core::state::AppState::new_in_memory();
        md_editor_core::vault::set_vault_root(&state, root.to_str().unwrap()).unwrap();

        save_markdown_file_with_parser_targets(
            &state,
            "source.md",
            "See [[wiki-target]], [inline](inline-target), [pdf](pdf://papers/a.pdf?page=2), and [web](https://example.com).",
        )
        .unwrap();

        let wiki_backlinks =
            md_editor_core::vault::get_backlinks(&state, "wiki-target.md").unwrap();
        assert!(
            wiki_backlinks
                .iter()
                .any(|path| path.ends_with("source.md")),
            "wiki link should be indexed: {wiki_backlinks:?}"
        );
        let inline_backlinks =
            md_editor_core::vault::get_backlinks(&state, "inline-target.md").unwrap();
        assert!(
            inline_backlinks
                .iter()
                .any(|path| path.ends_with("source.md")),
            "local inline link should be indexed: {inline_backlinks:?}"
        );
        let pdf_backlinks = md_editor_core::vault::get_backlinks(&state, "papers/a.pdf").unwrap();
        assert!(
            pdf_backlinks.iter().any(|path| path.ends_with("source.md")),
            "pdf link should be indexed against vault PDF path: {pdf_backlinks:?}"
        );
        let external_backlinks =
            md_editor_core::vault::get_backlinks(&state, "https://example.com.md").unwrap();
        assert!(
            external_backlinks.is_empty(),
            "external URL should not be indexed: {external_backlinks:?}"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn save_markdown_file_with_reference_links_indexes_resolved_targets() {
        let root = unique_temp_dir("native_reference_save");
        std::fs::create_dir_all(&root).unwrap();
        let state = md_editor_core::state::AppState::new_in_memory();
        md_editor_core::vault::set_vault_root(&state, root.to_str().unwrap()).unwrap();

        save_markdown_file_with_parser_targets(
            &state,
            "source.md",
            "See [my text][ref1] and [shortcut_ref] and [unresolved_ref].\n\n[ref1]: papers/ref-target.pdf\n[shortcut_ref]: <another_note.md>",
        )
        .unwrap();

        let ref1_backlinks =
            md_editor_core::vault::get_backlinks(&state, "papers/ref-target.pdf").unwrap();
        assert!(
            ref1_backlinks
                .iter()
                .any(|path| path.ends_with("source.md")),
            "reference pdf link should be indexed: {ref1_backlinks:?}"
        );

        let shortcut_backlinks =
            md_editor_core::vault::get_backlinks(&state, "another_note.md").unwrap();
        assert!(
            shortcut_backlinks
                .iter()
                .any(|path| path.ends_with("source.md")),
            "shortcut reference link should be indexed: {shortcut_backlinks:?}"
        );

        let unresolved_backlinks =
            md_editor_core::vault::get_backlinks(&state, "unresolved_ref.md").unwrap();
        assert!(
            unresolved_backlinks.is_empty(),
            "unresolved reference ID should not be indexed: {unresolved_backlinks:?}"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reindex_vault_with_parser_targets_replaces_regex_backlinks() {
        let root = unique_temp_dir("native_parser_reindex");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("source.md"),
            "```md\n[[ignored-code-link]]\n```\nSee [inline](inline-target).",
        )
        .unwrap();

        let state = md_editor_core::state::AppState::new_in_memory();
        md_editor_core::vault::set_vault_root(&state, root.to_str().unwrap()).unwrap();
        let regex_backlinks =
            md_editor_core::vault::get_backlinks(&state, "ignored-code-link.md").unwrap();
        assert!(
            regex_backlinks
                .iter()
                .any(|path| path.ends_with("source.md")),
            "core fallback should see raw wiki text before native parser reindex"
        );

        reindex_vault_with_parser_targets(&state, &root).unwrap();

        let ignored_backlinks =
            md_editor_core::vault::get_backlinks(&state, "ignored-code-link.md").unwrap();
        assert!(
            ignored_backlinks.is_empty(),
            "parser reindex should drop links inside code blocks: {ignored_backlinks:?}"
        );
        let inline_backlinks =
            md_editor_core::vault::get_backlinks(&state, "inline-target.md").unwrap();
        assert!(
            inline_backlinks
                .iter()
                .any(|path| path.ends_with("source.md")),
            "parser reindex should add local inline links: {inline_backlinks:?}"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reindex_markdown_file_with_parser_targets_updates_opened_file() {
        let root = unique_temp_dir("native_parser_open_file");
        std::fs::create_dir_all(&root).unwrap();
        let state = md_editor_core::state::AppState::new_in_memory();
        md_editor_core::vault::set_vault_root(&state, root.to_str().unwrap()).unwrap();
        md_editor_core::vault::save_file(&state, "source.md", "See [[old-target]].").unwrap();

        reindex_markdown_file_with_parser_targets(
            &state,
            "source.md",
            "```md\n[[old-target]]\n```\nSee [new](new-target).",
        )
        .unwrap();

        let old_backlinks = md_editor_core::vault::get_backlinks(&state, "old-target.md").unwrap();
        assert!(
            old_backlinks.is_empty(),
            "parser reindex should remove stale/code-block links: {old_backlinks:?}"
        );
        let new_backlinks = md_editor_core::vault::get_backlinks(&state, "new-target.md").unwrap();
        assert!(
            new_backlinks.iter().any(|path| path.ends_with("source.md")),
            "parser reindex should add current local inline links: {new_backlinks:?}"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    fn unique_temp_dir(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("md_editor_{name}_{nanos}"))
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
            pdf_search_match_scroll_y_from(1000.0, Some(250.0), 20.0, 792.0, 2.0, 5000.0),
            1948.0
        );
        assert_eq!(
            pdf_search_match_scroll_y_from(20.0, Some(780.0), 10.0, 792.0, 1.0, 5000.0),
            0.0
        );
    }

    #[test]
    fn pdf_placeholder_size_scales_with_zoom() {
        assert_eq!(
            pdf_placeholder_display_size_from(Some((612.0, 792.0)), None, None, 2.0),
            (1224.0, 1584.0)
        );
    }

    #[test]
    fn pdf_placeholder_prefers_first_page_size_over_rendered_dimensions() {
        assert_eq!(
            pdf_placeholder_display_size_from(
                Some((612.0, 792.0)),
                Some((300.0, 300.0)),
                Some((5000, 5000)),
                1.5,
            ),
            (918.0, 1188.0)
        );
    }

    #[test]
    fn pdf_text_lru_keeps_fifty_pages() {
        let mut app = MdEditor::new().0;
        app.pdf_render_generation = 7;

        for page in 0..60 {
            let page_text = md_editor_core::pdf::PdfPageText {
                page_index: page,
                page_width: 612.0,
                page_height: 792.0,
                text: format!("page {page}"),
                chars: Vec::new(),
                lines: Vec::new(),
            };
            let _ = app.update(Message::PdfPageTextLoaded(7, page, Ok(page_text)));
        }

        assert_eq!(app.pdf_text_lru.len(), PDF_TEXT_PAGE_CACHE_LIMIT);
        assert_eq!(app.pdf_page_text.len(), PDF_TEXT_PAGE_CACHE_LIMIT);
        assert!(!app.pdf_page_text.contains_key(&0));
        assert!(!app.pdf_page_text.contains_key(&9));
        assert!(app.pdf_page_text.contains_key(&10));
        assert!(app.pdf_page_text.contains_key(&59));
    }

    #[test]
    fn pdf_render_page_range_caps_accidental_large_spans() {
        let mut app = MdEditor::new().0;
        app.pdf_total_pages = 1_000;
        app.pdf_pages = vec![None; 1_000];

        let _ = app.render_pdf_page_range(0, 999);

        assert_eq!(
            app.pdf_pending_pages.len(),
            PDF_RENDER_MAX_SCHEDULED_PAGES as usize
        );
        assert!(app.pdf_pending_pages.contains(&0));
        assert!(
            app.pdf_pending_pages
                .contains(&(PDF_RENDER_MAX_SCHEDULED_PAGES - 1))
        );
        assert!(
            !app.pdf_pending_pages
                .contains(&PDF_RENDER_MAX_SCHEDULED_PAGES)
        );
    }

    #[test]
    fn pdf_viewport_render_range_uses_visible_pages_plus_small_preload() {
        let mut app = MdEditor::new().0;
        app.pdf_total_pages = 100;
        app.pdf_pages = vec![None; 100];
        app.pdf_state.page_sizes = vec![Some((100.0, 100.0)); 100];
        app.pdf_state.layout = PdfLayout::rebuild(
            &app.pdf_state.page_sizes,
            app.pdf_state.zoom,
            app.pdf_placeholder_display_size(),
            PDF_PAGE_SPACING,
            PDF_PAGE_LIST_PADDING,
            app.pdf_rotation,
        );

        let scroll_y = app.pdf_page_offset(10);
        let _ = app.render_pdf_pages_for_viewport(scroll_y, 220.0);

        let expected =
            app.pdf_state
                .layout
                .visible_range(scroll_y, 220.0, PDF_RENDER_PRELOAD_PAGES);
        assert_eq!(expected, 7..15);
        assert_eq!(app.pdf_pending_pages.len(), expected.len());
        for page in expected {
            assert!(app.pdf_pending_pages.contains(&page));
        }
        assert!(!app.pdf_pending_pages.contains(&6));
        assert!(!app.pdf_pending_pages.contains(&15));
    }

    #[test]
    fn pdf_zoom_keeps_existing_pages_as_stale_placeholders() {
        let mut app = MdEditor::new().0;
        app.active_pdf_path = Some("dummy.pdf".to_string());
        app.showing_pdf = true;
        app.pdf_total_pages = 2;
        app.pdf_state.page_sizes = vec![Some((100.0, 200.0)); 2];
        app.pdf_state.layout = PdfLayout::rebuild(
            &app.pdf_state.page_sizes,
            app.pdf_state.zoom,
            app.pdf_placeholder_display_size(),
            PDF_PAGE_SPACING,
            PDF_PAGE_LIST_PADDING,
            app.pdf_rotation,
        );

        let handle = iced::widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 0]);
        app.pdf_pages = vec![Some(handle.clone()), Some(handle)];
        app.pdf_dimensions = vec![Some((100, 200)), Some((100, 200))];

        let _ = app.update(Message::PdfZoomChanged(2.0));

        assert!(app.pdf_pages.iter().all(Option::is_some));
        assert!(app.pdf_stale_pages.contains(&0));
        assert!(app.pdf_stale_pages.contains(&1));
        assert_eq!(app.pdf_state.zoom, 2.0);
    }

    #[test]
    fn closing_pdf_link_preview_clears_hidden_context_menu() {
        let mut app = MdEditor::new().0;
        app.pdf_link_preview = Some(iced::widget::image::Handle::from_rgba(
            1,
            1,
            vec![255, 255, 255, 255],
        ));
        app.overlays.active_modal = Some(views::modals::ModalType::PdfContextMenu(
            views::modals::PdfContextMenuState {
                absolute_pos: iced::Point::ORIGIN,
                items: Vec::new(),
            },
        ));

        let _ = app.update(Message::ClosePdfLinkPreview);

        assert!(app.pdf_link_preview.is_none());
        assert!(app.overlays.active_modal.is_none());
    }

    #[test]
    fn escape_closing_pdf_link_preview_clears_hidden_context_menu() {
        let mut app = MdEditor::new().0;
        app.pdf_link_preview = Some(iced::widget::image::Handle::from_rgba(
            1,
            1,
            vec![255, 255, 255, 255],
        ));
        app.overlays.active_modal = Some(views::modals::ModalType::PdfContextMenu(
            views::modals::PdfContextMenuState {
                absolute_pos: iced::Point::ORIGIN,
                items: Vec::new(),
            },
        ));

        let _ = app.update(Message::KeyboardShortcut(Shortcut::Escape));

        assert!(app.pdf_link_preview.is_none());
        assert!(app.overlays.active_modal.is_none());
    }

    #[test]
    fn split_view_places_pdf_before_markdown() {
        let source = include_str!("app.rs").replace("\r\n", "\n");
        let split_row = source
            .find("if shell_state.uses_split_research_layout()")
            .expect("split view branch should use app shell state");
        let pdf_pos = source[split_row..]
            .find("container(pdf_view)")
            .expect("PDF pane should exist in split row");
        let editor_pos = source[split_row..]
            .find("container(editor_view)")
            .expect("editor pane should exist in split row");

        assert!(
            pdf_pos < editor_pos,
            "split view should render PDF on the left and markdown on the right"
        );
    }

    #[test]
    fn split_view_toggle_works_from_markdown_view_with_loaded_pdf() {
        let mut app = MdEditor::new().0;
        app.workspace.active_path = Some("note.md".to_string());
        app.active_pdf_path = Some("paper.pdf".to_string());
        app.showing_pdf = false;
        app.active_panel = ActivePanel::Markdown;

        let _ = app.update(Message::SplitViewToggle);

        assert!(app.split_view_active);
    }

    #[test]
    fn pdf_ctrl_scroll_zoom_clamps_and_requires_modifier() {
        let mut app = MdEditor::new().0;
        app.active_pdf_path = Some("dummy.pdf".to_string());
        app.showing_pdf = true;
        app.pdf_state.zoom = 1.0;

        let _ = app.update(Message::PdfWheelScrolledForZoom(0.5));
        assert_eq!(app.pdf_state.zoom, 1.0);

        app.keyboard_modifiers = iced::keyboard::Modifiers::CTRL;
        let _ = app.update(Message::PdfWheelScrolledForZoom(10.0));
        assert_eq!(app.pdf_state.zoom, 1.0);

        let _ = app.update(Message::PdfZoomChanged(10.0));
        assert_eq!(app.pdf_state.zoom, 4.0);
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
            tags: Vec::new(),
            status: md_editor_core::pdf::PdfAnnotationStatus::Unresolved,
            created_at: 0,
            updated_at: 0,
        };

        let mut app = MdEditor::new().0;
        app.active_pdf_path = Some("papers/My PDF File.pdf".to_string());
        assert_eq!(
            app.default_pdf_note_path(&ann),
            "pdf-notes/my-pdf-file-p5-abcdef12.md"
        );
    }

    #[test]
    fn pdf_selection_quote_link_command_targets_page() {
        let mut app = MdEditor::new().0;
        app.active_pdf_path = Some("papers/paper.pdf".to_string());
        app.pdf_selection = Some(views::interactive_pdf::PdfSelection {
            page_index: 2,
            anchor_idx: 0,
            focus_idx: 9,
        });
        app.pdf_page_text.insert(
            2,
            md_editor_core::pdf::PdfPageText {
                page_index: 2,
                page_width: 612.0,
                page_height: 792.0,
                text: "Quoted PDF text".to_string(),
                chars: Vec::new(),
                lines: Vec::new(),
            },
        );

        let Some(EditorCommand::InsertPdfQuoteLink {
            selected_text,
            page_number,
            link,
        }) = app.pdf_selection_quote_link_command()
        else {
            panic!("expected PDF quote link command");
        };
        assert_eq!(selected_text, "Quoted PDF");
        assert_eq!(page_number, 3);
        assert_eq!(link, "pdf://papers/paper.pdf?page=3");
    }

    #[test]
    fn pdf_insert_annotation_link_uses_annotation_target() {
        let mut app = MdEditor::new().0;
        app.workspace.active_path = Some("notes/current.md".to_string());
        app.active_pdf_path = Some("papers/My PDF.pdf".to_string());
        app.pdf_annotations.insert(
            4,
            vec![md_editor_core::pdf::PdfAnnotation {
                id: "ann#1".to_string(),
                document_id: "doc".to_string(),
                page_index: 4,
                kind: md_editor_core::pdf::PdfAnnotationKind::Highlight,
                color: md_editor_core::pdf::PdfAnnotationColor::Yellow,
                selected_text: "Important highlighted text".to_string(),
                ranges: vec![],
                rects: vec![],
                note: None,
                linked_note_path: None,
                markdown_anchor: None,
                tags: Vec::new(),
                status: md_editor_core::pdf::PdfAnnotationStatus::Unresolved,
                created_at: 0,
                updated_at: 0,
            }],
        );

        let _ = app.update(Message::PdfInsertAnnotationLink("ann#1".to_string()));

        assert_eq!(
            app.buffer.text(),
            "[label](pdf://papers/My%20PDF.pdf?page=5&annotation=ann%231)"
        );
        assert!(app.buffer.undo());
        assert_eq!(app.buffer.text(), "");
    }

    #[test]
    fn command_palette_adds_pdf_insert_actions_only_when_available() {
        let mut app = MdEditor::new().0;
        assert!(!app.command_palette_commands().iter().any(|cmd| matches!(
            cmd.shortcut,
            Shortcut::InsertPdfQuote | Shortcut::InsertPdfHighlight
        )));

        app.workspace.active_path = Some("notes/current.md".to_string());
        app.active_pdf_path = Some("papers/paper.pdf".to_string());
        app.pdf_selection = Some(views::interactive_pdf::PdfSelection {
            page_index: 2,
            anchor_idx: 0,
            focus_idx: 9,
        });
        app.pdf_page_text.insert(
            2,
            md_editor_core::pdf::PdfPageText {
                page_index: 2,
                page_width: 612.0,
                page_height: 792.0,
                text: "Quoted PDF text".to_string(),
                chars: Vec::new(),
                lines: Vec::new(),
            },
        );
        app.focused_annotation_id = Some("ann#1".to_string());
        app.pdf_annotations.insert(
            4,
            vec![md_editor_core::pdf::PdfAnnotation {
                id: "ann#1".to_string(),
                document_id: "doc".to_string(),
                page_index: 4,
                kind: md_editor_core::pdf::PdfAnnotationKind::Highlight,
                color: md_editor_core::pdf::PdfAnnotationColor::Yellow,
                selected_text: "Important highlighted text".to_string(),
                ranges: vec![],
                rects: vec![],
                note: None,
                linked_note_path: None,
                markdown_anchor: None,
                tags: Vec::new(),
                status: md_editor_core::pdf::PdfAnnotationStatus::Unresolved,
                created_at: 0,
                updated_at: 0,
            }],
        );

        let shortcuts = app
            .command_palette_commands()
            .into_iter()
            .map(|cmd| cmd.shortcut)
            .collect::<Vec<_>>();

        assert!(shortcuts.contains(&Shortcut::InsertPdfQuote));
        assert!(shortcuts.contains(&Shortcut::InsertPdfHighlight));
    }

    #[test]
    fn pdf_quote_insert_requires_markdown_file() {
        let mut app = MdEditor::new().0;
        app.active_pdf_path = Some("papers/paper.pdf".to_string());
        app.pdf_selection = Some(views::interactive_pdf::PdfSelection {
            page_index: 2,
            anchor_idx: 0,
            focus_idx: 9,
        });
        app.pdf_page_text.insert(
            2,
            md_editor_core::pdf::PdfPageText {
                page_index: 2,
                page_width: 612.0,
                page_height: 792.0,
                text: "Quoted PDF text".to_string(),
                chars: Vec::new(),
                lines: Vec::new(),
            },
        );

        let _ = app.update(Message::PdfInsertQuoteLink);

        assert_eq!(
            app.overlays.toast.as_deref(),
            Some("Open a markdown file before inserting a quote link")
        );
        assert_eq!(app.buffer.text(), "");
    }

    #[test]
    fn pdf_highlight_shortcut_without_selection_shows_toast() {
        let mut app = app_with_pdf_file();

        let _ = app.update(Message::KeyboardShortcut(Shortcut::PdfHighlight));

        assert_eq!(
            app.overlays.toast.as_deref(),
            Some("Select PDF text before highlighting")
        );
        assert!(app.pdf_annotations.values().all(Vec::is_empty));
    }

    #[test]
    fn pdf_create_annotation_uses_selected_pdf_text() {
        let mut app = app_with_pdf_file();
        app.pdf_document_id = Some("doc-1".to_string());
        app.state
            .save_pdf_document("doc-1", "papers/paper.pdf", 100, Some(1))
            .expect("test PDF document should register");
        app.pdf_selection = Some(views::interactive_pdf::PdfSelection {
            page_index: 0,
            anchor_idx: 0,
            focus_idx: 8,
        });
        app.pdf_page_text.insert(
            0,
            md_editor_core::pdf::PdfPageText {
                page_index: 0,
                page_width: 612.0,
                page_height: 792.0,
                text: "Keyboard highlight text".to_string(),
                chars: Vec::new(),
                lines: Vec::new(),
            },
        );

        let _ = app.update(Message::PdfCreateAnnotation(
            md_editor_core::pdf::PdfAnnotationKind::Highlight,
            md_editor_core::pdf::PdfAnnotationColor::Yellow,
        ));

        let page_annotations = app
            .pdf_annotations
            .get(&0)
            .expect("highlight shortcut should create page annotation");
        assert_eq!(page_annotations.len(), 1);
        assert_eq!(page_annotations[0].selected_text, "Keyboard ");
        assert_eq!(
            page_annotations[0].kind,
            md_editor_core::pdf::PdfAnnotationKind::Highlight
        );
        assert_eq!(
            page_annotations[0].color,
            md_editor_core::pdf::PdfAnnotationColor::Yellow
        );
        assert!(app.pdf_selection.is_none());
    }

    #[test]
    fn pdf_companion_note_key_is_stable_for_path_separators() {
        assert_eq!(
            pdf_companion_note_key("papers\\paper.pdf"),
            "pdf_companion_note:papers/paper.pdf"
        );
    }

    #[test]
    fn test_pdf_navigation_history() {
        let mut history = NavigationHistory::default();
        let p1 = NavigationTarget::Pdf {
            path: "doc1.pdf".to_string(),
            page: 1,
            scroll_offset: 100.0,
            zoom: 1.0,
        };
        let p2 = NavigationTarget::Pdf {
            path: "doc1.pdf".to_string(),
            page: 2,
            scroll_offset: 200.0,
            zoom: 1.5,
        };
        let p3 = NavigationTarget::Markdown {
            path: "note.md".to_string(),
            line: 5,
            column: 10,
        };

        // Test push
        history.push(p1.clone());
        assert_eq!(history.entries.len(), 1);
        assert_eq!(history.current_index, 0);

        // Test duplicate push ignored
        history.push(p1.clone());
        assert_eq!(history.entries.len(), 1);

        // Push more
        history.push(p2.clone());
        history.push(p3.clone());
        assert_eq!(history.entries.len(), 3);
        assert_eq!(history.current_index, 2);

        // Test back
        assert_eq!(history.go_back(), Some(p2.clone()));
        assert_eq!(history.current_index, 1);
        assert_eq!(history.go_back(), Some(p1.clone()));
        assert_eq!(history.current_index, 0);
        assert_eq!(history.go_back(), None);

        // Test forward
        assert_eq!(history.go_forward(), Some(p2.clone()));
        assert_eq!(history.current_index, 1);
        assert_eq!(history.go_forward(), Some(p3.clone()));
        assert_eq!(history.current_index, 2);
        assert_eq!(history.go_forward(), None);

        // Test branch truncation on push
        assert_eq!(history.go_back(), Some(p2.clone())); // current_index = 1
        let p4 = NavigationTarget::Pdf {
            path: "doc2.pdf".to_string(),
            page: 4,
            scroll_offset: 400.0,
            zoom: 1.0,
        };
        history.push(p4.clone()); // truncates forward, adds p4 at index 2
        assert_eq!(history.entries.len(), 3);
        assert_eq!(history.entries[2].target, p4);
        assert_eq!(history.current_index, 2);
        assert_eq!(history.go_forward(), None);
    }

    #[test]
    fn test_pdf_page_rotation() {
        let mut app = MdEditor::new().0;
        app.active_pdf_path = Some("dummy.pdf".to_string());
        app.showing_pdf = true;
        app.pdf_total_pages = 1;
        app.pdf_state.page_sizes = vec![Some((100.0, 200.0))];
        app.pdf_state.zoom = 1.0;
        app.pdf_rotation = 0;

        app.pdf_state.layout = PdfLayout::rebuild(
            &app.pdf_state.page_sizes,
            app.pdf_state.zoom,
            app.pdf_placeholder_display_size(),
            PDF_PAGE_SPACING,
            PDF_PAGE_LIST_PADDING,
            app.pdf_rotation,
        );

        assert_eq!(app.pdf_state.layout.page_height(0), 200.0);
        assert_eq!(app.pdf_rotation, 0);

        let _ = app.update(Message::PdfRotateClockwise);
        assert_eq!(app.pdf_rotation, 90);
        assert_eq!(app.pdf_state.layout.page_height(0), 100.0);

        let _ = app.update(Message::PdfRotateClockwise);
        assert_eq!(app.pdf_rotation, 180);
        assert_eq!(app.pdf_state.layout.page_height(0), 200.0);

        let _ = app.update(Message::PdfRotateClockwise);
        assert_eq!(app.pdf_rotation, 270);
        assert_eq!(app.pdf_state.layout.page_height(0), 100.0);
    }

    #[test]
    fn test_pdf_link_click_in_split_view_navigates_and_preserves_scroll() {
        let mut app = MdEditor::new().0;
        app.split_view_active = true;
        app.showing_pdf = true;
        app.workspace.active_path = Some("note.md".to_string());
        app.active_pdf_path = Some("paper.pdf".to_string());
        app.pdf_total_pages = 10;
        app.pdf_state.page_sizes = vec![Some((500.0, 700.0)); 10];
        app.pdf_state.zoom = 1.0;

        app.pdf_state.layout = PdfLayout::rebuild(
            &app.pdf_state.page_sizes,
            app.pdf_state.zoom,
            app.pdf_placeholder_display_size(),
            PDF_PAGE_SPACING,
            PDF_PAGE_LIST_PADDING,
            app.pdf_rotation,
        );

        app.editor_scroll_y = 120.0;

        // Click on a relative link with hash delimiter and no schema prefix
        let _ = app.update(Message::SidebarFileClicked("paper.pdf#page=5".to_string()));

        // Assert editor scroll is preserved
        assert_eq!(app.editor_scroll_y, 120.0);
        // Assert PDF page navigated to page 4 (index of page 5)
        assert_eq!(app.pdf_current_page, 4);
    }

    #[test]
    fn test_pdf_open_race_condition_navigation() {
        let mut app = MdEditor::new().0;
        app.workspace.active_path = Some("note.md".to_string());
        app.active_pdf_path = Some("paper.pdf".to_string());

        // Initial target page starts at Some(4) when we click a link
        app.pdf_initial_target_page = Some(4);

        // Hashing finishes first before PDF pages count is loaded
        let _ = app.update(Message::PdfDocumentIdComputed(Some((
            "paper.pdf".to_string(),
            "dummyhash".to_string(),
            1000,
            Some(0),
        ))));

        // Verify target page was deferred and not clamped/consumed yet
        assert_eq!(app.pdf_initial_target_page, Some(4));

        // PDF total pages finishes loading
        let generation = app.pdf_render_generation;
        let _ = app.update(Message::PdfLoaded(generation, 10));
        assert_eq!(app.pdf_total_pages, 10);

        // Page sizes finish loading, which triggers layout rebuild and PdfFitToWidth
        let _ = app.update(Message::PdfPageSizesLoaded(
            generation,
            "paper.pdf".to_string(),
            vec![(500.0, 700.0); 10],
        ));

        // Under the hood, PdfPageSizesLoaded dispatches PdfFitToWidth, which we execute here
        let _ = app.update(Message::PdfFitToWidth);

        // Now it should be consumed and navigated to page 4
        assert_eq!(app.pdf_initial_target_page, None);
        assert_eq!(app.pdf_current_page, 4);
    }

    #[test]
    fn test_manual_scroll_clears_programmatic_scroll_target() {
        let mut app = MdEditor::new().0;
        app.pdf_total_pages = 10;
        app.pdf_pages = vec![None; 10];
        app.pdf_state.page_sizes = vec![Some((500.0, 700.0)); 10];
        app.pdf_state.layout = PdfLayout::rebuild(
            &app.pdf_state.page_sizes,
            app.pdf_state.zoom,
            app.pdf_placeholder_display_size(),
            PDF_PAGE_SPACING,
            PDF_PAGE_LIST_PADDING,
            app.pdf_rotation,
        );

        // 1. Programmatic scroll to page 5 when page 5 is NOT ready (still loading)
        app.pdf_toc_target_page = Some(5);
        app.pdf_programmatic_scroll = true;

        // Scroll event at expected placeholder position arrives
        let target_y = app.pdf_page_offset(5);
        let _ = app.update(Message::PdfScrolled {
            y: target_y,
            viewport_height: 500.0,
        });
        // Since page is not ready, programmatic scroll and target page are preserved
        assert!(app.pdf_programmatic_scroll);
        assert_eq!(app.pdf_toc_target_page, Some(5));

        // 2. Now simulate page 5 finishing loading/rendering
        let handle = iced::widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 0]);
        app.pdf_pages[5] = Some(handle);

        // Scroll event arrives now that the page is ready
        let _ = app.update(Message::PdfScrolled {
            y: target_y,
            viewport_height: 500.0,
        });
        // It arrives, so both flags are cleared
        assert!(!app.pdf_programmatic_scroll);
        assert_eq!(app.pdf_toc_target_page, None);

        // 3. Manual scroll clears target page (when pdf_programmatic_scroll is false)
        app.pdf_toc_target_page = Some(3);
        let _ = app.update(Message::PdfScrolled {
            y: 100.0,
            viewport_height: 500.0,
        });
        assert_eq!(app.pdf_toc_target_page, None);
    }

    #[test]
    fn test_split_view_width_calculations() {
        let mut app = MdEditor::new().0;
        app.window_width = 1200.0;
        app.sidebar_visible = false;
        app.toc_visible = false;
        app.workspace.backlinks_visible = false;
        app.pdf_annotations_visible = false;
        app.editor_viewport_width = 0.0;

        app.workspace.active_path = Some("note.md".to_string());
        app.active_pdf_path = Some("paper.pdf".to_string());
        app.split_view_active = true;
        app.split_ratio = 0.6; // PDF gets 60%, Editor gets 40%

        let pdf_width = app.pdf_available_width();
        let editor_width = app.estimated_editor_viewport_width();

        // 1200.0 * 0.6 = 720.0
        assert!((pdf_width - 720.0).abs() < 1e-3);
        // 1200.0 * 0.4 = 480.0
        assert!((editor_width - 480.0).abs() < 1e-3);
    }

    #[test]
    fn test_reference_link_resolves_and_preserves_scroll() {
        let mut app = MdEditor::new().0;
        app.workspace.active_path = Some("note.md".to_string());
        app.editor_scroll_y = 120.0;
        app.buffer = DocBuffer::from_text("# Heading 1\n\n[my-ref]\n\n[my-ref]: #heading-1\n");
        app.highlighted_lines = highlight::highlight_markdown(&app.buffer.text());

        // Click on the reference "my-ref"
        let _ = app.update(Message::SidebarFileClicked("my-ref".to_string()));

        // Active path should still be note.md
        assert_eq!(app.workspace.active_path.as_deref(), Some("note.md"));
        // Editor cursor should be moved to heading 1 (line 0)
        assert_eq!(app.buffer.cursor_line, 0);
    }

    #[test]
    fn test_ctrl_click_programmatic_scroll_bypasses_cancellation() {
        let mut app = MdEditor::new().0;
        app.pdf_total_pages = 10;
        app.pdf_pages = vec![None; 10];
        app.pdf_state.page_sizes = vec![Some((500.0, 700.0)); 10];
        app.pdf_state.layout = PdfLayout::rebuild(
            &app.pdf_state.page_sizes,
            app.pdf_state.zoom,
            app.pdf_placeholder_display_size(),
            PDF_PAGE_SPACING,
            PDF_PAGE_LIST_PADDING,
            app.pdf_rotation,
        );

        // Simulate Ctrl modifier active
        app.keyboard_modifiers = iced::keyboard::Modifiers::CTRL;

        // 1. Programmatic scroll is triggered
        app.pdf_toc_target_page = Some(5);
        app.pdf_programmatic_scroll = true;

        // Populate page 5 in cache to mark as ready
        let handle = iced::widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 0]);
        app.pdf_pages[5] = Some(handle);

        // Scroll event arrives (with Ctrl held down)
        let target_y = app.pdf_page_offset(5);
        let _ = app.update(Message::PdfScrolled {
            y: target_y,
            viewport_height: 500.0,
        });

        // Programmatic scroll bypasses Ctrl key cancellation, sets self.pdf_programmatic_scroll = false, and clears target
        assert!(!app.pdf_programmatic_scroll);
        assert_eq!(app.pdf_toc_target_page, None);
    }

    #[test]
    fn test_large_doc_highlight_debounce_and_reset() {
        let mut app = MdEditor::new().0;

        // Setup a buffer with more than LARGE_DOC_LINE_THRESHOLD (1,000) lines
        let mut text = String::new();
        for i in 0..1005 {
            text.push_str(&format!("Line {}\n", i));
        }
        app.buffer.set_text(&text);

        // 1. Initial edit (opened_file = false)
        let _task = app.refresh_highlighting_for_current_buffer(false);
        assert_eq!(app.highlight_generation, 1);
        assert_eq!(app.pending_highlight_generation, Some(1));
        assert!(app.pending_highlight_requested_at.is_some());
        assert!(app.pending_highlight_text.is_some());

        // 2. Second edit before debounce triggers resets and increments generation
        let _task2 = app.refresh_highlighting_for_current_buffer(false);
        assert_eq!(app.highlight_generation, 2);
        assert_eq!(app.pending_highlight_generation, Some(2));

        // 3. Mock time elapsed to trigger highlight debounce
        app.pending_highlight_requested_at =
            Some(std::time::Instant::now() - std::time::Duration::from_millis(300));
        let _debounce_task = app.update(Message::HighlightDebounceElapsed);

        // Debounce state cleared
        assert_eq!(app.pending_highlight_generation, None);
        assert!(app.pending_highlight_requested_at.is_none());
        assert!(app.pending_highlight_text.is_none());
    }

    #[test]
    fn test_stale_highlight_generation_handling() {
        let mut app = MdEditor::new().0;
        app.highlight_generation = 5;

        let dummy_lines_stale = vec![crate::editor::highlight::StyledLine::new()];
        let mut dummy_lines_newer = vec![crate::editor::highlight::StyledLine::new()];
        dummy_lines_newer[0]
            .spans
            .push(crate::editor::highlight::StyledSpan::plain("newer"));

        // 1. Stale highlight ready (generation 4 < 5) should be ignored
        let _ = app.update(Message::HighlightReady(4, dummy_lines_stale));
        assert!(app.highlighted_lines.is_empty());

        // 2. Newer highlight ready (generation 5 == 5) should be accepted
        let _ = app.update(Message::HighlightReady(5, dummy_lines_newer));
        assert_eq!(app.highlighted_lines.len(), 1);
        assert_eq!(app.highlighted_lines[0].spans[0].text, "newer");
    }

    #[test]
    fn test_pdf_open_clears_page_text_cache() {
        let mut app = MdEditor::new().0;

        // 1. Populate the page text cache with some dummy entries
        app.pdf_page_text.insert(
            0,
            md_editor_core::pdf::PdfPageText {
                page_index: 0,
                page_width: 500.0,
                page_height: 700.0,
                text: "Hello".to_string(),
                chars: vec![],
                lines: vec![],
            },
        );
        app.pdf_text_lru.push_back(0);

        // 2. Perform open_pdf (we'll set vault root first so path resolves)
        let root = unique_temp_dir("open_pdf_test");
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap().to_string();
        md_editor_core::vault::set_vault_root(&app.state, &root_str).unwrap();
        app.workspace.vault_root = Some(root_str);

        // Create a dummy pdf file so resolve_active_path works
        let pdf_path = root.join("test.pdf");
        std::fs::write(&pdf_path, "%PDF-1.4 ...").unwrap();

        let _task = app.open_pdf("test.pdf");

        // 3. Verify page text cache is cleared
        assert!(app.pdf_page_text.is_empty());
        assert!(app.pdf_text_lru.is_empty());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_sync_quick_note_to_linked_note_file() {
        let mut app = MdEditor::new().0;
        let root = unique_temp_dir("sync_quick_note_test");
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap().to_string();
        app.workspace.vault_root = Some(root_str.clone());
        md_editor_core::vault::set_vault_root(&app.state, &root_str).unwrap();

        // 1. Create a dummy linked note file
        let note_path = "linked-note.md";
        let doc_id = format!("doc-{}", uuid::Uuid::new_v4());
        let ann_id = format!("ann-{}", uuid::Uuid::new_v4());
        let pdf_path = "paper.pdf";

        let ann = md_editor_core::pdf::PdfAnnotation {
            id: ann_id.clone(),
            document_id: doc_id.clone(),
            page_index: 0,
            kind: md_editor_core::pdf::PdfAnnotationKind::Highlight,
            color: md_editor_core::pdf::PdfAnnotationColor::Yellow,
            selected_text: "Target Highlight Text".to_string(),
            ranges: vec![],
            rects: vec![],
            note: None,
            linked_note_path: Some(note_path.to_string()),
            markdown_anchor: None,
            tags: Vec::new(),
            status: md_editor_core::pdf::PdfAnnotationStatus::Unresolved,
            created_at: 0,
            updated_at: 0,
        };

        // Create the linked note file with initial empty content
        let initial_content =
            crate::pdf_notes::new_linked_pdf_note_content(note_path, pdf_path, &ann);
        std::fs::write(root.join(note_path), &initial_content).unwrap();

        app.state
            .save_pdf_document(&doc_id, pdf_path, 0, None)
            .unwrap();
        app.state.save_pdf_annotation(&ann).unwrap();

        // Setup app state
        app.active_pdf_path = Some(pdf_path.to_string());
        app.pdf_annotations.insert(0, vec![ann.clone()]);

        // Open the file as active in the editor so we test real-time buffer reload
        app.workspace.active_path = Some(note_path.to_string());
        app.buffer = crate::editor::buffer::DocBuffer::from_text(&initial_content);

        // 2. Fire PdfAddQuickNote
        let _ = app.update(Message::PdfAddQuickNote(
            ann_id.to_string(),
            "New note update from UI".to_string(),
        ));

        // 3. Verifications
        // Check annotation in app memory
        let updated_ann = app
            .pdf_annotations
            .get(&0)
            .unwrap()
            .iter()
            .find(|a| a.id == ann_id)
            .unwrap();
        assert_eq!(
            updated_ann.note,
            Some("New note update from UI".to_string())
        );

        // Check persisted annotation
        let db_note = app
            .state
            .get_pdf_annotations(&doc_id, Some(0))
            .unwrap()
            .into_iter()
            .find(|annotation| annotation.id == ann_id)
            .and_then(|annotation| annotation.note);
        assert_eq!(db_note, Some("New note update from UI".to_string()));

        // Check file on disk
        let disk_content = std::fs::read_to_string(root.join(note_path)).unwrap();
        assert!(disk_content.contains("### Notes\n\nNew note update from UI\n\n"));

        // Check active editor buffer reload
        assert!(
            app.buffer
                .text()
                .contains("### Notes\n\nNew note update from UI\n\n")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_cross_pane_navigation_history() {
        let mut app = app_without_vault();
        let root = unique_temp_dir("cross_pane_nav_test");
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap().to_string();
        app.workspace.vault_root = Some(root_str.clone());
        md_editor_core::vault::set_vault_root(&app.state, &root_str).unwrap();

        // Create a markdown file and a dummy PDF file
        let note_path = "document.md";
        let pdf_path = "document.pdf";
        std::fs::write(root.join(note_path), "# Title\nSome content here").unwrap();
        std::fs::write(root.join(pdf_path), "%PDF-1.4 ...").unwrap();

        // 1. Open the markdown file
        let _ = app.open_file(note_path);
        assert_eq!(app.workspace.active_path.as_deref(), Some(note_path));
        assert!(!app.showing_pdf);

        // 2. Open the PDF (this should trigger history push of markdown path)
        let _ = app.open_pdf(pdf_path);
        assert_eq!(app.active_pdf_path.as_deref(), Some(pdf_path));
        assert!(app.showing_pdf);

        // 3. Verify history has 1 entry (for Markdown)
        assert_eq!(app.workspace.navigation_history.entries.len(), 1);
        match &app.workspace.navigation_history.entries[0].target {
            NavigationTarget::Markdown { path, .. } => {
                assert_eq!(path, note_path);
            }
            _ => panic!("Expected Markdown target"),
        }

        // 4. Trigger PdfNavBack to return to Markdown
        let _ = app.update(Message::PdfNavBack);
        assert_eq!(app.workspace.active_path.as_deref(), Some(note_path));
        assert!(!app.showing_pdf);

        // 5. Trigger PdfNavForward to return to PDF
        let _ = app.update(Message::PdfNavForward);
        assert_eq!(app.active_pdf_path.as_deref(), Some(pdf_path));
        assert!(app.showing_pdf);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_follow_citation() {
        let mut app = MdEditor::new().0;

        let link_span = highlight::StyledSpan {
            text: "[citation](pdf://papers/a.pdf)".to_string(),
            display_text: Some("citation".to_string()),
            color: iced::Color::BLACK,
            bold: false,
            italic: false,
            font_size: 12.0,
            is_code: false,
            is_link: true,
            link_target: Some("pdf://papers/a.pdf".to_string()),
            is_heading: false,
            heading_level: 0,
            is_checkbox: false,
            is_checked: false,
            is_rule: false,
            is_image: false,
            image_path: None,
            image_alt: None,
            is_math: false,
            is_syntax: false,
            id: None,
        };
        app.highlighted_lines = vec![highlight::StyledLine {
            spans: vec![link_span],
            is_code_block: false,
            is_math_block: false,
            code_block_lang: None,
            is_blockquote: false,
            block_id: 1,
            is_block_fence: false,
            is_table_row: false,
            table_cells: vec![],
        }];

        app.buffer.cursor_line = 0;
        app.buffer.cursor_col = 5;
        let _task = app.follow_citation();

        app.buffer.cursor_col = 50;
        let _task_none = app.follow_citation();

        app.buffer.cursor_line = 10;
        let _task_oob = app.follow_citation();
    }

    #[test]
    fn test_show_usages() {
        let mut app = MdEditor::new().0;
        let root = unique_temp_dir("test_show_usages_dir");
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap().to_string();
        app.workspace.vault_root = Some(root_str.clone());
        md_editor_core::vault::set_vault_root(&app.state, &root_str).unwrap();

        save_markdown_file_with_parser_targets(
            &app.state,
            "source.md",
            "Refer to [doc](pdf://papers/a.pdf?page=2)",
        )
        .unwrap();

        app.active_pdf_path = Some("papers/a.pdf".to_string());
        app.showing_pdf = true;

        let _ = app.show_usages();

        assert!(app.workspace.backlinks_visible);
        assert!(!app.workspace.backlinks.is_empty());
        let backlink_labels = app
            .workspace
            .backlinks
            .iter()
            .map(|b| b.label.clone())
            .collect::<Vec<_>>();
        assert!(backlink_labels.contains(&"source.md".to_string()));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_combined_outline_toc_navigator() {
        let md_toc = vec![highlight::OutlineEntry {
            level: 1,
            text: "Heading 1".to_string(),
            line: 5,
        }];
        let pdf_toc = vec![highlight::OutlineEntry {
            level: 2,
            text: "Bookmark 2".to_string(),
            line: 12,
        }];

        let _element = views::toc::view(&md_toc, &pdf_toc, 250.0, None, None);
    }

    #[test]
    fn test_global_unified_search() {
        let mut app = app_without_vault();
        let root = unique_temp_dir("test_global_unified_search_dir");
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap().to_string();
        app.workspace.vault_root = Some(root_str.clone());
        md_editor_core::vault::set_vault_root(&app.state, &root_str).unwrap();

        app.search.visible = true;
        let _ = app.update(Message::SearchQueryChanged("vault".to_string()));

        assert_eq!(app.search.editor.query, "vault");
        assert!(app.search.global.searching);

        let match_item = md_editor_core::types::UnifiedSearchResult {
            group: md_editor_core::types::SearchResultGroup::Heading,
            path: "source.md".to_string(),
            line: 1,
            context: "# Welcome to the Vault".to_string(),
            score: 8.0,
            page_index: None,
            annotation_id: None,
        };

        let _ = app.update(Message::UnifiedSearchMatchesFound(
            app.search.global.id,
            vec![match_item],
        ));

        assert_eq!(app.search.global.results.len(), 1);
        assert_eq!(
            app.search.global.results[0].context,
            "# Welcome to the Vault"
        );
        let _ = app.update(Message::UnifiedPdfTextSearchMatchesFound(
            app.search.global.id,
            pdf_text_batch(Vec::new(), 0, 0, false, false),
        ));
        assert!(!app.search.global.searching);

        let _ = app.update(Message::UnifiedSearchFinished(app.search.global.id, Ok(())));
        assert!(!app.search.global.searching);

        let _click_task = app.update(Message::UnifiedSearchResultClicked(
            app.search.global.results[0].clone(),
        ));
        assert!(!app.search.visible);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_search_registered_pdf_text_results_does_not_deadlock() {
        let app = MdEditor::new().0;
        let root = unique_temp_dir("search_deadlock_test");
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap().to_string();

        md_editor_core::vault::set_vault_root(&app.state, &root_str).unwrap();

        // Register a pdf
        let pdf_path = "doc.pdf";
        let abs_path = root.join(pdf_path);
        std::fs::write(&abs_path, "PDF Dummy content").unwrap();

        let metadata = std::fs::metadata(&abs_path).unwrap();
        let size = metadata.len();
        let mtime = metadata
            .modified()
            .unwrap()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        app.state
            .save_pdf_document("doc-1", pdf_path, size, Some(mtime))
            .unwrap();

        let mut query = md_editor_core::types::UnifiedSearchQuery::all_sources("Dummy".to_string());
        query.sources = vec![md_editor_core::types::UnifiedSearchSource::PdfContent];

        // This would deadlock if state.vault_root lock guard was held and then validate_and_invalidate_pdf_cache tried to lock it again
        let _batch = search_registered_pdf_text_results(&app.state, &query, None);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_search_unopened_pdf_discovered_from_disk() {
        let app = MdEditor::new().0;
        let root = unique_temp_dir("unopened_pdf_test");
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap().to_string();

        md_editor_core::vault::set_vault_root(&app.state, &root_str).unwrap();

        // Write an unopened PDF file to disk (but DO NOT save it in the DB / register it)
        let pdf_path = "unopened.pdf";
        let abs_path = root.join(pdf_path);
        std::fs::write(&abs_path, "PDF Dummy content").unwrap();

        let pdf_paths = md_editor_core::vault::list_all_pdf_files(&root).unwrap();
        assert_eq!(pdf_paths.len(), 1);
        let rel_path = md_editor_core::vault::path_to_relative_string(&pdf_paths[0], &root);
        assert_eq!(rel_path, "unopened.pdf");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_pdf_toc_navigation_completes_if_already_scrolled() {
        let mut app = MdEditor::new().0;
        app.pdf_total_pages = 5;
        app.pdf_pages = vec![None; 5];
        app.pdf_dimensions = vec![Some((600, 800)); 5];

        // Setup state to be programmatically scrolling to page 2
        app.pdf_toc_target_page = Some(2);
        app.pdf_programmatic_scroll = true;

        // Mock scrollable position to be already at page 2 offset
        let scroll_y = app.pdf_page_offset(2);
        app.pdf_scroll_y = scroll_y;

        // Emit PdfRendered for page 2
        let _ = app.update(Message::PdfRendered(
            app.pdf_render_generation,
            2,
            image::DynamicImage::ImageRgba8(image::ImageBuffer::new(10, 10)),
        ));

        // Programmatic scroll flags should be cleared and page should be marked as current
        assert!(app.pdf_toc_target_page.is_none());
        assert!(!app.pdf_programmatic_scroll);
        assert_eq!(app.pdf_current_page, 2);
    }

    #[test]
    fn stale_pdf_matches_do_not_enter_global_results() {
        let mut app = MdEditor::new().0;
        app.search.visible = true;
        app.active_pdf_path = Some("paper.pdf".to_string());
        app.search.editor.query = "needle".to_string();
        app.search.pdf_active_id = 7;
        app.search.global.pdf_search_id = Some(8);

        let _ = app.update(Message::PdfSearchMatchesFound(
            7,
            vec![md_editor_core::pdf::PdfSearchMatch {
                page_index: 0,
                context: "needle context".to_string(),
                rects: Vec::new(),
            }],
        ));

        assert!(app.search.global.results.is_empty());
        assert_eq!(app.pdf_state.search.matches.len(), 1);
    }

    #[test]
    fn global_search_query_uses_source_toggles() {
        let mut app = MdEditor::new().0;
        let source = md_editor_core::types::UnifiedSearchSource::PdfContent;

        let _ = app.update(Message::UnifiedSearchSourceToggled(source, false));
        let query = app.build_global_search_query("needle".to_string());

        assert!(!query.includes(source));
        assert!(query.includes(md_editor_core::types::UnifiedSearchSource::MarkdownContent));
    }

    #[test]
    fn pdf_content_global_result_activates_matching_search_hit() {
        let mut app = MdEditor::new().0;
        app.active_pdf_path = Some("paper.pdf".to_string());
        app.showing_pdf = true;
        app.pdf_total_pages = 3;
        app.pdf_state.search.matches = vec![md_editor_core::pdf::PdfSearchMatch {
            page_index: 1,
            context: "needle context".to_string(),
            rects: vec![md_editor_core::pdf::PdfRect {
                x: 10.0,
                y: 20.0,
                width: 30.0,
                height: 10.0,
            }],
        }];
        app.rebuild_pdf_search_page_index();

        let _ = app.update(Message::UnifiedSearchResultClicked(
            md_editor_core::types::UnifiedSearchResult {
                group: md_editor_core::types::SearchResultGroup::PdfContent,
                path: "paper.pdf".to_string(),
                line: 2,
                context: "PDF text (1 areas): needle context".to_string(),
                score: 6.0,
                page_index: Some(1),
                annotation_id: Some("0".to_string()),
            },
        ));

        assert_eq!(app.pdf_state.search.active_index, Some(0));
        assert_eq!(app.pdf_current_page, 1);
        assert!(app.pdf_programmatic_scroll);
    }

    #[test]
    fn pdf_content_global_result_navigates_page_when_already_open_without_annotation_id() {
        let mut app = MdEditor::new().0;
        app.active_pdf_path = Some("paper.pdf".to_string());
        app.showing_pdf = true;
        app.pdf_total_pages = 3;
        app.pdf_pages = vec![None; 3];
        app.pdf_state.page_sizes = vec![Some((500.0, 700.0)); 3];
        app.pdf_state.layout = PdfLayout::rebuild(
            &app.pdf_state.page_sizes,
            app.pdf_state.zoom,
            app.pdf_placeholder_display_size(),
            PDF_PAGE_SPACING,
            PDF_PAGE_LIST_PADDING,
            app.pdf_rotation,
        );

        let _ = app.update(Message::UnifiedSearchResultClicked(
            md_editor_core::types::UnifiedSearchResult {
                group: md_editor_core::types::SearchResultGroup::PdfContent,
                path: "paper.pdf".to_string(),
                line: 2,
                context: "needle context".to_string(),
                score: 6.0,
                page_index: Some(1),
                annotation_id: None,
            },
        ));

        // It should navigate directly, setting current page to index 1 (page 2)
        assert_eq!(app.pdf_current_page, 1);
        assert!(app.pdf_programmatic_scroll);
        // It shouldn't clear pages/total pages since it was already open
        assert_eq!(app.pdf_pages.len(), 3);
        assert_eq!(app.pdf_total_pages, 3);
    }

    #[test]
    fn vault_pdf_text_results_merge_only_for_visible_current_search() {
        let mut app = MdEditor::new().0;
        app.search.visible = true;
        app.search.global.id = 5;
        app.search.global.pending_vault_pdf = true;

        let pdf_result = md_editor_core::types::UnifiedSearchResult {
            group: md_editor_core::types::SearchResultGroup::PdfContent,
            path: "other.pdf".to_string(),
            line: 3,
            context: "PDF text (1 areas): needle".to_string(),
            score: 4.0,
            page_index: Some(2),
            annotation_id: None,
        };

        let _ = app.update(Message::UnifiedPdfTextSearchMatchesFound(
            4,
            pdf_text_batch(vec![pdf_result.clone()], 1, 2, false, false),
        ));
        assert!(app.search.global.results.is_empty());
        assert!(app.search.global.pending_vault_pdf);

        let _ = app.update(Message::UnifiedPdfTextSearchMatchesFound(
            5,
            pdf_text_batch(vec![pdf_result], 1, 2, false, true),
        ));
        assert_eq!(app.search.global.results.len(), 1);
        assert!(!app.search.global.pending_vault_pdf);
        assert_eq!(
            app.search.global.pdf_status.as_deref(),
            Some("PDF text: searched 1 of 2 registered PDFs; document cap reached")
        );

        app.search.visible = false;
        app.search.global.id = 6;
        let _ = app.update(Message::UnifiedPdfTextSearchMatchesFound(
            6,
            pdf_text_batch(
                vec![md_editor_core::types::UnifiedSearchResult {
                    group: md_editor_core::types::SearchResultGroup::PdfContent,
                    path: "stale.pdf".to_string(),
                    line: 1,
                    context: "stale".to_string(),
                    score: 4.0,
                    page_index: Some(0),
                    annotation_id: None,
                }],
                1,
                1,
                false,
                false,
            ),
        ));
        assert_eq!(app.search.global.results.len(), 1);
    }

    #[test]
    fn registered_pdf_search_targets_skip_active_and_cap_work() {
        let paths = (0..40)
            .map(|idx| format!("paper-{idx}.pdf"))
            .collect::<Vec<_>>();

        let targets = registered_pdf_search_targets(paths, Some("paper-3.pdf"), 5);

        assert_eq!(targets.len(), 5);
        assert!(!targets.iter().any(|path| path == "paper-3.pdf"));
        assert_eq!(targets[0], "paper-0.pdf");
        assert_eq!(targets[4], "paper-5.pdf");
    }

    #[test]
    fn registered_pdf_index_targets_cap_documents() {
        let paths = (0..40)
            .map(|idx| format!("paper-{idx}.pdf"))
            .collect::<Vec<_>>();

        let targets = registered_pdf_index_targets(paths, 3);

        assert_eq!(targets, vec!["paper-0.pdf", "paper-1.pdf", "paper-2.pdf"]);
    }

    #[test]
    fn pdf_search_status_reports_result_cap_first() {
        let batch = pdf_text_batch(Vec::new(), 32, 100, true, true);

        assert_eq!(
            format_pdf_search_status(&batch),
            "PDF text: searched 32 of 100 registered PDFs; result cap reached"
        );
    }

    #[test]
    fn test_excerpt_mode_queue_and_batch_insert() {
        let mut app = MdEditor::new().0;
        app.workspace.active_path = Some("test_note.md".to_string());
        app.active_pdf_path = Some("document.pdf".to_string());

        // Toggle excerpt mode
        let _ = app.update(Message::ExcerptModeToggle);
        assert!(app.overlays.excerpt_mode_active);

        // Queue items using CitationPaletteChoose
        let item1 = crate::messages::CitationItem::Selection {
            text: "first queued excerpt".to_string(),
            page_index: 1, // page 2
        };
        let item2 = crate::messages::CitationItem::Annotation {
            id: "ann-456".to_string(),
            text: "second queued excerpt".to_string(),
            page_index: 4, // page 5
        };

        let _ = app.update(Message::CitationPaletteChoose(item1));
        let _ = app.update(Message::CitationPaletteChoose(item2));

        assert_eq!(app.overlays.excerpts_queue.len(), 2);

        // Insert batch
        let _ = app.update(Message::ExcerptQueueInsertBatch);

        // Queue should be cleared
        assert!(app.overlays.excerpts_queue.is_empty());

        // Document buffer should contain the citations
        let content = app.buffer.text();
        assert!(content.contains("> first queued excerpt"));
        assert!(content.contains("[Selection (Page 2)](pdf://document.pdf?page=2)"));
        assert!(content.contains("> second queued excerpt"));
        assert!(
            content.contains("[Highlight (Page 5)](pdf://document.pdf?page=5&annotation=ann-456)")
        );
    }

    #[test]
    fn citation_palette_submit_first_queues_first_item_in_excerpt_mode() {
        let mut app = MdEditor::new().0;
        app.workspace.active_path = Some("test_note.md".to_string());
        app.overlays.citation_palette_visible = true;
        app.overlays.excerpt_mode_active = true;
        app.pdf_annotations.insert(
            0,
            vec![md_editor_core::pdf::PdfAnnotation {
                id: "ann-keyboard".to_string(),
                document_id: "doc".to_string(),
                page_index: 0,
                kind: md_editor_core::pdf::PdfAnnotationKind::Highlight,
                color: md_editor_core::pdf::PdfAnnotationColor::Yellow,
                selected_text: "keyboard citation".to_string(),
                ranges: vec![],
                rects: vec![],
                note: None,
                linked_note_path: None,
                markdown_anchor: None,
                tags: vec![],
                status: md_editor_core::pdf::PdfAnnotationStatus::Unresolved,
                created_at: 0,
                updated_at: 0,
            }],
        );

        let _ = app.update(Message::CitationPaletteSubmitFirst);

        assert!(!app.overlays.citation_palette_visible);
        assert_eq!(app.overlays.excerpts_queue.len(), 1);
        assert!(matches!(
            app.overlays.excerpts_queue.as_slice(),
            [crate::messages::CitationItem::Annotation { id, .. }] if id == "ann-keyboard"
        ));
    }

    #[test]
    fn test_command_registry_is_enabled_context_rules() {
        use crate::command_registry::{CommandContext, get_command_registry};
        use crate::messages::Shortcut;

        let registry = get_command_registry();
        let save_cmd = registry.iter().find(|c| c.id == Shortcut::Save).unwrap();

        // 1. Save disabled when no markdown is open
        let ctx_no_md = CommandContext {
            markdown_open: false,
            pdf_open: false,
            image_open: false,
            active_pane: crate::app_shell::AppShellPane::None,
            has_vault: true,
            pdf_has_selection: false,
            has_focused_annotation: false,
        };
        assert_eq!(
            save_cmd.is_enabled(ctx_no_md),
            Err("No active markdown file to save")
        );

        // 2. Save enabled when markdown is open
        let ctx_md = CommandContext {
            markdown_open: true,
            pdf_open: false,
            image_open: false,
            active_pane: crate::app_shell::AppShellPane::Markdown,
            has_vault: true,
            pdf_has_selection: false,
            has_focused_annotation: false,
        };
        assert_eq!(save_cmd.is_enabled(ctx_md), Ok(()));
    }

    #[test]
    fn test_command_palette_context_aware_ranking_and_grouping() {
        let mut app = MdEditor::new().0;
        app.workspace.active_path = None; // No markdown file open

        // Querying commands with no markdown open
        let commands = app.command_palette_commands();

        // Verify Save command has a disabled reason
        let save_cmd = commands
            .iter()
            .find(|c| c.shortcut == crate::messages::Shortcut::Save)
            .unwrap();
        assert_eq!(
            save_cmd.disabled_reason,
            Some("No active markdown file to save")
        );

        // Open a markdown file
        app.workspace.active_path = Some("test.md".to_string());
        let commands_with_md = app.command_palette_commands();
        let save_cmd_enabled = commands_with_md
            .iter()
            .find(|c| c.shortcut == crate::messages::Shortcut::Save)
            .unwrap();
        assert_eq!(save_cmd_enabled.disabled_reason, None);
    }

    #[test]
    fn test_search_wrap_and_replace() {
        let mut app = MdEditor::new().0;
        app.workspace.active_path = Some("test.md".to_string());
        app.search.editor.visible = true;
        app.buffer.set_text("banana apple banana");
        app.highlighted_lines =
            crate::editor::highlight::highlight_markdown(app.buffer.text().as_str());

        // 1. Check search queries reset wrap_status
        let _ = app.update(Message::SearchQueryChanged("banana".to_string()));
        assert_eq!(app.current_document_match_count(), 2);
        assert_eq!(app.search.editor.wrap_status, None);

        // First Next: active_index goes to 0 (first match)
        let _ = app.update(Message::SearchNext);
        assert_eq!(app.search.editor.active_index, Some(0));
        assert_eq!(app.search.editor.wrap_status, None);

        // Second Next: active_index goes to 1 (second match)
        let _ = app.update(Message::SearchNext);
        assert_eq!(app.search.editor.active_index, Some(1));
        assert_eq!(app.search.editor.wrap_status, None);

        // Third Next: wraps around to 0
        let _ = app.update(Message::SearchNext);
        assert_eq!(app.search.editor.active_index, Some(0));
        assert_eq!(
            app.search.editor.wrap_status,
            Some(SearchWrapStatus::WrappedForward)
        );

        // Triggering SearchPrevious when active_index is 0 wraps to 1 (last match)
        let _ = app.update(Message::SearchPrevious);
        assert_eq!(app.search.editor.active_index, Some(1));
        assert_eq!(
            app.search.editor.wrap_status,
            Some(SearchWrapStatus::WrappedBackward)
        );

        // 2. Query changes reset wrap_status
        let _ = app.update(Message::SearchQueryChanged("apple".to_string()));
        assert_eq!(app.current_document_match_count(), 1);
        assert_eq!(app.search.editor.wrap_status, None);

        // 3. Single match replace
        let _ = app.update(Message::SearchReplaceChanged("pear".to_string()));
        let _ = app.update(Message::SearchNext); // Focus apple (index 0)
        assert_eq!(app.search.editor.active_index, Some(0));

        let _ = app.update(Message::SearchReplace);
        assert_eq!(app.buffer.text(), "banana pear banana");
        // After replacement, query "apple" has no matches in the document
        assert_eq!(app.current_document_match_count(), 0);
        assert_eq!(app.search.editor.active_index, None);
    }
}

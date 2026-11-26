//! The `MdEditor` shell: state, boot, window title and the message router. Handlers live in sibling files by feature.

#![warn(clippy::unwrap_used)]

use iced::widget::operation::{self, AbsoluteOffset};
use iced::widget::{Space, container, mouse_area, row, scrollable, stack, text};
use iced::{Alignment, Element, Length, Subscription, Task, Theme};

use image::GenericImageView;
use std::sync::Arc;

use crate::design::motion::{Motion, SCROLL_STIFFNESS, ScrollGlide, Spring};
use crate::editor::buffer::{Affinity, DocBuffer, EditorCommand};
use crate::editor::renderer::{CaretView, Reveal};
use crate::messages::{Message, Shortcut};
use crate::pdf_notes::{
    append_linked_pdf_note_section, new_linked_pdf_note_content, normalize_note_path, slug_fragment,
};
use crate::views;
use crate::views::pdf_viewer::PDF_PAGE_LIST_PADDING;

const ARROW_SCROLL: f32 = 64.0;
const EDITOR_PADDING: f32 = crate::design::metrics::SPACE_5;
const PDF_SUPERSAMPLE_MAX: f32 = 3.0;
/// The automatic fit-to-width stops here; a requested fit may go to `PDF_MAX_ZOOM`.
const PDF_FIT_MAX_ZOOM: f32 = 2.0;
/// The largest zoom the PDF view offers (toolbar, zoom field, fit).
const PDF_MAX_ZOOM: f32 = 4.0;
const PDF_PAGE_BITMAP_EVICT_MARGIN: u16 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivePanel {
    Markdown,
    Pdf,
    Ink,
}

enum SaveOutcome {
    Saved,
    NothingToDo,
    Failed(md_editor_core::vault::VaultError),
}

fn search_result_should_reload(active_path: Option<&str>, target_path: &str) -> bool {
    active_path != Some(target_path)
}

fn rename_open_path(path: &mut Option<String>, old_path: &str, new_path: &str) {
    let Some(current) = path.as_deref() else {
        return;
    };
    if current == old_path {
        *path = Some(new_path.to_string());
    } else if let Some(suffix) = current
        .strip_prefix(old_path)
        .filter(|suffix| suffix.starts_with('/'))
    {
        *path = Some(format!("{new_path}{suffix}"));
    }
}

/// Run blocking work — every `PdfRenderer` call waits on the PDFium worker, and
/// some read files — on tokio's blocking pool instead of inside an async task.
///
/// Blocking inside `Task::perform` futures parked the executor's worker
/// threads: opening a PDF queues a dozen renderer calls, and once every worker
/// waited on PDFium, nothing polled the subscriptions, so iced dropped input
/// events (`TrySendError { kind: Full }` in the log) and timers stalled.
pub(crate) async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    match tokio::task::spawn_blocking(work).await {
        Ok(value) => value,
        // Keep the panic a panic, with its original payload.
        Err(error) => std::panic::resume_unwind(error.into_panic()),
    }
}

/// Whether `path` is an image the vault lists (`IMAGE_EXTENSIONS`, any case).
pub(crate) fn is_supported_image_path(path: &str) -> bool {
    md_editor_core::vault::is_image_path(std::path::Path::new(path))
}

fn text_by_char_range(text: &str, start: usize, end: usize) -> String {
    if start >= end {
        return String::new();
    }
    text.chars().skip(start).take(end - start).collect()
}

pub struct MdEditor {
    state: Arc<md_editor_core::state::AppState>,
    vault: crate::vault_state::VaultState,
    tabs: crate::tabs::Tabs,
    active_path: Option<String>,
    editor: crate::editor_state::EditorPane,
    pdf: crate::pdf_pane::PdfPane,
    active_image_path: Option<String>,
    active_image: Option<(iced::widget::image::Handle, f32, f32)>,
    showing_pdf: bool,
    tracker: crate::tracker_state::TrackerState,
    graph: crate::graph_state::GraphState,
    ink: crate::ink::InkState,
    main_window: Option<iced::window::Id>,
    ink_window: Option<iced::window::Id>,
    ui: crate::ui_state::UiState,
    search: crate::search_state::SearchState,
    active_panel: ActivePanel,
    last_os_theme: crate::design::ActiveTheme,
    self_writes: std::collections::HashMap<String, std::time::Instant>,
    autosave_failure_reported: bool,
    motion: Motion,
    workspace: workspace::Workspace,
}

const SELF_WRITE_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

mod completion;
mod documents;
mod editor;
mod graph;
mod ink;
mod links;
mod overlays;
mod pane_view;
mod panes;
pub(crate) mod paste;
mod pdf;
mod pdf_nav;
mod pdf_pages;
mod pdf_render;
mod pdf_select;
mod search;
mod settings_handler;
mod shell;
mod shortcuts;
mod subscription;
mod tabs_handler;
mod vault;
pub(crate) mod vault_entries;
mod vault_links;
mod view;
pub(crate) mod workspace;

#[cfg(test)]
use editor::editor_command_keeps_cursor_visible;
#[cfg(test)]
use ink::derive_ink_path;
use links::*;
use search::{focus_file_search_input, focus_global_search_input, focus_pdf_search_input};

/// What an end-to-end test can observe about the app.
#[cfg(test)]
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct Probe {
    pub vault_root: Option<String>,
    pub active_path: Option<String>,
    pub open_tabs: Vec<String>,
    pub active_tab: Option<String>,
    pub buffer: String,
    pub dirty: bool,
    /// 0-based line of the caret in the open note.
    pub cursor_line: usize,
    /// Labels in the open completion popup.
    pub completion: Vec<String>,
    /// Decoded images of the open note: link text and pixel width.
    pub images: Vec<(String, f32)>,
    pub toast: Option<String>,
    pub modal_open: bool,
    pub palette_open: bool,
    pub settings_open: bool,
    pub graph_visible: bool,
    pub tracker_visible: bool,
    pub search_visible: bool,
    pub file_search_visible: bool,
    pub search_results: Vec<String>,
    pub pdf_path: Option<String>,
    pub pdf_pages: u16,
    pub pdf_rendered: usize,
    pub pdf_pending_pages: usize,
    pub pdf_zoom: f32,
    pub pdf_page: u16,
    pub pdf_search_results: usize,
    /// The picked search match, if any.
    pub pdf_match_index: Option<usize>,
    /// Highlights and notes on the open PDF.
    pub pdf_annotations: usize,
    /// Their ids, in page order.
    pub pdf_annotation_ids: Vec<String>,
    /// Pages whose text layer has loaded (needed to select and highlight).
    pub pdf_texts_loaded: usize,
    /// The document id is known, so highlights can be saved.
    pub pdf_document_ready: bool,
    /// The text under the current PDF selection.
    pub pdf_selected_text: Option<String>,
    pub split_view: bool,
    pub focused_pane: workspace::PaneId,
    /// What the pane without focus shows (note, else PDF).
    pub other_pane_path: Option<String>,
    pub pdf_panel_active: bool,
    pub palette_query: String,
    pub graph_nodes: usize,
    pub graph_local: bool,
    pub pdf_error: Option<String>,
    pub ink_window_open: bool,
    pub ink_path: Option<String>,
    pub ink_dirty: bool,
    pub ink_strokes: usize,
    pub entries: Vec<String>,
    /// Where the open context menu was placed, in window coordinates.
    pub context_menu: Option<(f32, f32)>,
    pub light_theme: bool,
    pub editor_font_size: f32,
    pub tracker_sessions: usize,
    /// What the session logged last says it worked on.
    pub tracker_last_notes: Option<String>,
}

#[cfg(test)]
impl MdEditor {
    pub(crate) fn probe(&self) -> Probe {
        Probe {
            vault_root: self.vault.root.clone(),
            active_path: self.active_path.clone(),
            open_tabs: self.tabs.list().iter().map(|t| t.path.clone()).collect(),
            active_tab: self.tabs.active_tab().map(|t| t.path.clone()),
            buffer: self.editor.buffer.text(),
            dirty: self.editor.buffer.dirty,
            cursor_line: self.editor.buffer.cursor_line,
            completion: self
                .editor
                .completion
                .iter()
                .flat_map(|c| c.items.iter().map(|i| i.label.clone()))
                .collect(),
            images: {
                let mut images: Vec<_> = self
                    .editor
                    .image_cache
                    .iter()
                    .map(|(key, (_, width, _))| (key.clone(), *width))
                    .collect();
                images.sort_by(|a, b| a.0.cmp(&b.0));
                images
            },
            toast: self.ui.toast.clone(),
            modal_open: self.ui.active_modal.is_some(),
            palette_open: self.ui.command_palette_visible,
            settings_open: self.ui.settings_visible,
            graph_visible: self.graph.visible,
            tracker_visible: self.tracker.visible,
            search_visible: self.search.visible,
            file_search_visible: self.search.file_visible,
            search_results: self.search.results.iter().map(|r| r.path.clone()).collect(),
            pdf_path: self.pdf.active_path.clone(),
            pdf_pages: self.pdf.total_pages,
            pdf_rendered: self.pdf.pages.iter().filter(|page| page.is_some()).count(),
            pdf_pending_pages: self.pdf.pending_pages.len(),
            pdf_zoom: self.pdf.zoom,
            pdf_page: self.pdf.current_page,
            pdf_search_results: self.search.pdf_results.len(),
            pdf_match_index: self.search.match_index,
            pdf_annotations: self.pdf.annotations.values().map(Vec::len).sum(),
            pdf_annotation_ids: {
                let mut pages: Vec<_> = self.pdf.annotations.iter().collect();
                pages.sort_by_key(|(page, _)| **page);
                pages
                    .into_iter()
                    .flat_map(|(_, anns)| anns.iter().map(|a| a.id.clone()))
                    .collect()
            },
            pdf_texts_loaded: self.pdf.page_text.len(),
            pdf_document_ready: self.pdf.document_id.is_some(),
            pdf_selected_text: self.pdf.selection.and_then(|sel| {
                let text = self.pdf.page_text.get(&sel.page_index)?;
                let start = sel.anchor_idx.min(sel.focus_idx);
                let end = sel.anchor_idx.max(sel.focus_idx) + 1;
                Some(text.text.chars().skip(start).take(end - start).collect())
            }),
            split_view: self.workspace.is_split(),
            focused_pane: self.workspace.focused,
            other_pane_path: self.parked().and_then(|docs| {
                docs.active_path
                    .clone()
                    .filter(|_| !docs.showing_pdf)
                    .or_else(|| docs.pdf.active_path.clone())
            }),
            pdf_panel_active: self.active_panel == ActivePanel::Pdf,
            palette_query: self.ui.command_palette_query.clone(),
            graph_nodes: self.graph.filtered().snapshot.nodes.len(),
            graph_local: matches!(
                self.graph.scope,
                crate::graph_state::GraphScope::Local { .. }
            ),
            pdf_error: self.pdf.load_error.clone(),
            ink_window_open: self.ink_window.is_some(),
            ink_path: self.ink.path.clone(),
            ink_dirty: self.ink.dirty,
            ink_strokes: self.ink.document.strokes().len(),
            entries: self.vault.entries.iter().map(|e| e.path.clone()).collect(),
            context_menu: self.ui.context_menu.as_ref().map(|menu| (menu.x, menu.y)),
            light_theme: crate::design::active_theme() == crate::design::ActiveTheme::Light,
            editor_font_size: crate::design::editor_font_size(),
            tracker_sessions: self.tracker.session_count(),
            tracker_last_notes: self.tracker.last_session_notes(),
        }
    }
}

impl MdEditor {
    /// An overlay with a text field that holds the keyboard is open.
    fn text_overlay_open(&self) -> bool {
        self.ui.command_palette_visible
            || self.search.visible
            || self.search.file_visible
            || self.ui.active_modal.is_some()
    }

    /// The focused pane shows its note.
    fn note_is_showing(&self) -> bool {
        self.active_path.is_some()
            && !self.showing_pdf
            && !self.graph.visible
            && !self.tracker.visible
            && !self.ui.settings_visible
    }

    pub fn boot(
        main_window: iced::window::Settings,
        startup_file: Option<std::path::PathBuf>,
    ) -> (Self, Task<Message>) {
        let (mut app, restore) = Self::new_with_startup_file(startup_file);
        let (id, open) = iced::window::open(main_window);
        app.main_window = Some(id);
        (app, Task::batch(vec![open.discard(), restore]))
    }

    pub fn new_with_startup_file(
        startup_file: Option<std::path::PathBuf>,
    ) -> (Self, Task<Message>) {
        #[cfg(test)]
        crate::test_support::isolate_data_dir();
        let state = Arc::new(md_editor_core::state::AppState::new());
        let startup_notice = state.take_startup_notice();
        let last_vault = md_editor_core::config::get_sys_config(&state, "last_vault")
            .ok()
            .flatten();
        let last_file = md_editor_core::config::get_sys_config(&state, "last_file")
            .ok()
            .flatten();
        let tracker = crate::tracker_state::TrackerState::new(&state);
        let graph = crate::graph_state::GraphState::new();

        let mut app = Self {
            state: state.clone(),
            vault: crate::vault_state::VaultState::new(),
            tabs: crate::tabs::Tabs::new(),
            active_path: None,
            editor: crate::editor_state::EditorPane::new(),
            pdf: crate::pdf_pane::PdfPane::new(),
            active_image_path: None,
            active_image: None,
            showing_pdf: false,
            tracker,
            graph,
            ink: crate::ink::InkState::new(),
            main_window: None,
            ink_window: None,
            ui: crate::ui_state::UiState::new(),
            search: crate::search_state::SearchState::new(),
            active_panel: ActivePanel::Markdown,
            last_os_theme: crate::design::ActiveTheme::Dark,
            self_writes: std::collections::HashMap::new(),
            autosave_failure_reported: false,
            motion: Motion::new(),
            workspace: workspace::Workspace::new(),
        };
        app.ui.toast = startup_notice;
        app.restore_design_settings();
        app.restore_layout();
        app.ui.sync_legacy_toast();

        let restored = crate::restore_window_size();
        app.ui.window_width = restored.width;
        app.ui.window_height = restored.height;

        let startup = startup_file.and_then(|p| resolve_startup_target(&p, last_vault.as_deref()));

        let mut task = Task::none();
        if let Some((vault_root, rel_file)) = startup {
            let index_task = app.open_vault(&vault_root);
            if let Some(file_path) = rel_file {
                task = app.open_startup_file(&file_path);
            }
            task = Task::batch(vec![index_task, task]);
        } else if let Some(path) = last_vault {
            if std::path::Path::new(&path).is_dir() {
                let index_task = app.open_vault(&path);
                if app.vault.root.is_some() {
                    task = app.restore_workspace(&path, last_file.as_deref());
                }
                task = Task::batch(vec![index_task, task]);
            } else {
                log::info!("The last vault, {path}, no longer exists");
                app.ui.toast = Some(format!(
                    "The last vault, {path}, was moved or deleted. Open a folder to continue."
                ));
                app.ui.sync_legacy_toast();
            }
        }

        (app, task)
    }

    fn open_startup_file(&mut self, file_path: &str) -> Task<Message> {
        self.vault.selected_path = Some(file_path.to_string());
        let lower = file_path.to_lowercase();
        if lower.ends_with(".md") || lower.ends_with(".markdown") {
            self.open_file(file_path)
        } else if lower.ends_with(".pdf") {
            self.pdf.active_path = Some(file_path.to_string());
            self.showing_pdf = true;
            self.open_pdf(file_path)
        } else if is_supported_image_path(&lower) {
            self.open_image(file_path)
        } else if lower.ends_with(".ink") {
            self.open_ink_page(file_path)
        } else {
            Task::none()
        }
    }

    pub fn title(&self, window: iced::window::Id) -> String {
        if Some(window) == self.ink_window {
            return format!(
                "{}Handwriting — {}",
                if self.ink.dirty { "● " } else { "" },
                self.ink.path.as_deref().unwrap_or("Unsaved page")
            );
        }
        if self.graph.visible {
            return format!(
                "{}Md-editor — Knowledge Graph",
                if self.editor.buffer.dirty { "● " } else { "" }
            );
        }
        if self.ui.settings_visible {
            return "Md-editor — Settings".to_string();
        }
        format!(
            "{}Md-editor — {}",
            if self.editor.buffer.dirty { "● " } else { "" },
            self.tabs
                .active_tab()
                .map(|t| t.path.as_str())
                .or(self.active_path.as_deref())
                .or(self.pdf.active_path.as_deref())
                .or(self.active_image_path.as_deref())
                .unwrap_or("Workspace")
        )
    }

    pub fn theme(&self) -> Theme {
        crate::design::iced_theme()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let overlay_was_open = self.text_overlay_open();
        let region_before = self.ui.focused_region;
        let mut task = self.update_inner(message);
        // Handlers may visit the other pane; between updates the focused
        // pane is the one swapped in.
        self.swap_in(self.workspace.focused);
        // Give the keyboard back to the note when the overlay that took it
        // closes, and when F6 lands on the main region.
        let entered_main = region_before != crate::ui::FocusRegion::Main
            && self.ui.focused_region == crate::ui::FocusRegion::Main;
        if (overlay_was_open && !self.text_overlay_open() || entered_main) && self.note_is_showing()
        {
            task = Task::batch(vec![
                task,
                iced::widget::operation::focus(self.workspace.focused.editor_widget_id()),
            ]);
        }

        self.ui.sync_legacy_toast();
        self.motion.sidebar_target_width = self.ui.sidebar_width;
        self.motion.toc_target_width = self.ui.side_panel_width;
        self.motion.backlinks_target_width = self.ui.side_panel_width;
        self.motion.sync(
            self.vault.sidebar_visible,
            self.editor.toc_visible,
            self.vault.backlinks_visible,
            self.ui.toast.as_deref(),
            self.ui.command_palette_visible,
        );
        self.search.ensure_matches(
            &self.editor.buffer,
            self.active_path.as_deref(),
            self.editor.buffer_revision,
        );
        if self.tracker.is_running() {
            let seen = self.study_observation();
            self.tracker.observe(&seen);
        }
        task
    }

    /// What a running study session records about this moment: the document
    /// in front, the note's word count, the PDF's page and highlights.
    fn study_observation(&self) -> crate::study_context::Observation {
        let focused = match self.active_panel {
            ActivePanel::Ink if self.ink_window.is_some() => self.ink.path.clone(),
            ActivePanel::Pdf if self.showing_pdf => self.pdf.active_path.clone(),
            _ => self.active_path.clone(),
        };
        crate::study_context::Observation {
            focused,
            note: self
                .active_path
                .clone()
                .map(|path| (path, self.editor.buffer.word_count())),
            pdf: self.pdf.active_path.clone().map(|path| {
                // Saved highlights load with the document id.
                let highlights = self
                    .pdf
                    .document_id
                    .is_some()
                    .then(|| self.pdf.annotations.values().map(Vec::len).sum());
                (path, self.pdf.current_page, highlights)
            }),
        }
    }

    fn update_inner(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Vault(m) => self.update_vault(m),
            Message::Tabs(m) => self.update_tabs(m),
            Message::Settings(m) => self.update_settings(m),
            Message::Graph(m) => self.update_graph(m),
            Message::Shell(m) => self.update_shell(m),
            Message::Editor(m) => self.update_editor(m),
            Message::Pdf(m) => self.update_pdf(m),
            Message::Search(m) => self.update_search(m),
            Message::Ink(m) => self.update_ink(m),
            Message::KeyboardShortcut(shortcut) => self.update_shortcuts(shortcut),
            Message::FromPane(pane, message) => self.update_from_pane(pane, *message),
            Message::ToPane(pane, message) => self.update_in_pane(pane, *message),
        }
    }
}

fn resolve_startup_target(
    path: &std::path::Path,
    last_vault: Option<&str>,
) -> Option<(String, Option<String>)> {
    let abs = std::path::absolute(path).ok()?;
    if abs.is_dir() {
        return Some((abs.to_string_lossy().to_string(), None));
    }
    if !abs.is_file() {
        return None;
    }
    let (root, rel) = match last_vault
        .map(std::path::Path::new)
        .and_then(|v| std::path::absolute(v).ok())
        .and_then(|v| {
            abs.strip_prefix(&v)
                .ok()
                .map(|rel| (v.clone(), rel.to_path_buf()))
        }) {
        Some((vault, rel)) => (vault, rel),
        None => {
            let parent = abs.parent()?.to_path_buf();
            let rel = abs.strip_prefix(&parent).ok()?.to_path_buf();
            (parent, rel)
        }
    };
    Some((
        root.to_string_lossy().to_string(),
        Some(rel.to_string_lossy().replace('\\', "/")),
    ))
}

#[cfg(test)]
mod tests;

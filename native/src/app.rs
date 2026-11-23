use iced::widget::operation::{self, AbsoluteOffset};
use iced::widget::{Space, column, container, mouse_area, row, scrollable, stack, text};
use iced::{Alignment, Element, Length, Subscription, Task, Theme};

use image::GenericImageView;
use std::sync::Arc;

use crate::editor::buffer::{Affinity, DocBuffer, EditorCommand};
use crate::editor::renderer::{CaretView, Reveal};
use crate::messages::{Message, Shortcut};
use crate::motion::{SCROLL_STIFFNESS, ScrollGlide, Spring};
use crate::pdf_notes::{
    append_linked_pdf_note_section, new_linked_pdf_note_content, normalize_note_path, slug_fragment,
};
use crate::theme as app_theme;
use crate::views;
use crate::views::pdf_viewer::PDF_PAGE_LIST_PADDING;

const PDF_SCROLLABLE_ID: &str = "pdf_scrollable";
const EDITOR_SCROLLABLE_ID: &str = "editor_scrollable";
/// How far one arrow key press scrolls a PDF or a presentation.
const ARROW_SCROLL: f32 = 64.0;
/// Space around the editor widget inside its scrollable.
const EDITOR_PADDING: f32 = app_theme::SPACE_6;
/// Upper bound on PDF supersampling. The actual factor tracks the display's
/// scale factor (see `MdEditor::pdf_supersample`); this caps bitmap cost on
/// unusually high-DPI displays.
const PDF_SUPERSAMPLE_MAX: f32 = 3.0;
/// Highest zoom fit-to-width will pick, whether automatic or from the fit
/// button. Manual zoom still goes to 4.0; this only stops a wide pane from
/// blowing a small page up past a comfortable reading size.
const PDF_FIT_MAX_ZOOM: f32 = 2.0;
/// How many pages beyond the rendered window keep their cached bitmap before
/// being evicted. Bounds resident bitmap memory for long PDFs while leaving
/// enough slack that ordinary scrolling reuses cached pages.
const PDF_PAGE_BITMAP_EVICT_MARGIN: u16 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivePanel {
    Markdown,
    Pdf,
    Ink,
}

/// Result of attempting to persist the active document. Callers decide how
/// loudly to report each case: an explicit `Ctrl+S` confirms success, autosave
/// stays silent unless it fails, and a file switch refuses to proceed on
/// failure rather than navigating away from unsaved work.
enum SaveOutcome {
    Saved,
    /// Nothing was dirty, or no document is open.
    NothingToDo,
    Failed(String),
}


fn search_result_should_reload(active_path: Option<&str>, target_path: &str) -> bool {
    active_path != Some(target_path)
}

fn pdf_search_generation_is_current(current: u64, incoming: u64) -> bool {
    current == incoming
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

pub(crate) fn is_supported_image_path(path: &str) -> bool {
    path.ends_with(".png")
        || path.ends_with(".jpg")
        || path.ends_with(".jpeg")
        || path.ends_with(".gif")
        || path.ends_with(".bmp")
        || path.ends_with(".webp")
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

    // Presentation viewer (prepared deck, zoom, scroll position)
    pptx: crate::pptx_pane::PptxPane,

    // Image viewer + active-viewer mode flag (shell-level routing)
    active_image_path: Option<String>,
    active_image: Option<(iced::widget::image::Handle, f32, f32)>,
    showing_pdf: bool,

    // Study tracker
    tracker: crate::tracker_state::TrackerState,

    // Research graph: cached vault topology, layout, filters, and selection
    graph: crate::graph_state::GraphState,
    // Handwritten notes surface, shown in its own window
    ink: crate::ink::InkState,

    /// The shell window. Closing it exits the app.
    main_window: Option<iced::window::Id>,
    /// The handwriting window, while it is open.
    ink_window: Option<iced::window::Id>,

    // Shell UI chrome: modals, command palette, toast, split view, window size
    ui: crate::ui_state::UiState,

    // Search (query/replace, vault + PDF results, in-document match cache)
    search: crate::search_state::SearchState,
    active_panel: ActivePanel,

    /// Paths this app wrote very recently, so the vault watcher can tell our
    /// own saves apart from genuine external edits.
    self_writes: std::collections::HashMap<String, std::time::Instant>,
    /// Whether the current run of autosave failures has already been reported.
    /// Retries continue quietly; only the first failure raises a toast.
    autosave_failure_reported: bool,

    /// In-flight UI transitions (panels opening, toasts fading). Mirrors the
    /// visibility flags that live on the sub-states; see [`crate::motion`].
    motion: crate::motion::Motion,
}

/// How long a path stays marked as "we just wrote this". Comfortably longer
/// than the vault watcher's debounce, short enough that a real external edit
/// arriving later is never mistaken for our own.
const SELF_WRITE_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

impl MdEditor {
    /// `startup_file` is a path passed on the command line (typically by the
    /// OS "open with" handler). When set it overrides the remembered
    /// last-vault/last-file: a file inside the last vault opens in that vault,
    /// any other file opens with its parent directory as the vault, and a
    /// directory opens as a vault.
    /// Build the app and open its shell window.
    ///
    /// The app runs as a daemon (the handwriting canvas has its own window),
    /// and a daemon starts with no windows, so opening the main one is part of
    /// booting rather than something the runtime does for us.
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
            active_path: None,
            editor: crate::editor_state::EditorPane::new(),
            pdf: crate::pdf_pane::PdfPane::new(),
            pptx: crate::pptx_pane::PptxPane::new(),
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
            self_writes: std::collections::HashMap::new(),
            autosave_failure_reported: false,
            motion: crate::motion::Motion::new(),
        };
        app.ui.toast = startup_notice;
        app.restore_layout();

        // Seed the geometry the window was actually restored at. Without this,
        // every layout computation (PDF fit-to-width, editor viewport, modal
        // sizing) would use the 1200x800 default until the first resize event
        // arrived — and on a platform that emits none, forever.
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
            let index_task = app.open_vault(&path);
            if let Some(file_path) = last_file {
                task = app.open_startup_file(&file_path);
            }
            task = Task::batch(vec![index_task, task]);
        }

        (app, task)
    }

    /// Open a vault-relative file by extension (markdown, PDF, or image);
    /// unsupported extensions are ignored.
    fn open_startup_file(&mut self, file_path: &str) -> Task<Message> {
        // Select the reopened file so its row offers delete without a click.
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
        } else if lower.ends_with(".pptx") {
            self.open_pptx(file_path)
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
        format!(
            "{}Md-editor — {}",
            if self.editor.buffer.dirty { "● " } else { "" },
            self.active_path
                .as_deref()
                .or(self.pdf.active_path.as_deref())
                .or(self.active_image_path.as_deref())
                .or(self.pptx.active_path.as_deref())
                .unwrap_or("New File")
        )
    }

    pub fn theme(&self) -> Theme {
        app_theme::md_editor_theme()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // App-level keyboard handling. Returns `None` for keys we don't act on
        // so unhandled keystrokes (ordinary text entry, handled by the focused
        // editor widget) don't each spawn a redundant update cycle. The
        // context-sensitive messages below are gated in their own handlers
        // (e.g. PdfScrollBy only scrolls when the PDF pane is the active
        // target, NameModalSubmitCurrent is a no-op without an open modal).
        let keyboard = iced::event::listen_with(|event, _status, _window_id| {
            use iced::keyboard::Key;
            use iced::keyboard::key::Named;

            let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) =
                event
            else {
                return None;
            };

            if key == Key::Named(Named::Escape) {
                return Some(Message::KeyboardShortcut(Shortcut::Escape));
            }
            if key == Key::Named(Named::Enter) {
                return Some(Message::OverlaySubmitCurrent);
            }
            if (modifiers.command() || modifiers.control())
                && let Key::Character(c) = key.as_ref()
            {
                return match c {
                    "s" => Some(Message::KeyboardShortcut(Shortcut::Save)),
                    "o" => Some(Message::KeyboardShortcut(Shortcut::OpenVault)),
                    "n" => Some(Message::KeyboardShortcut(Shortcut::NewFile)),
                    "f" => Some(Message::KeyboardShortcut(Shortcut::Search)),
                    "c" => Some(Message::PdfCopySelection),
                    "p" => Some(Message::KeyboardShortcut(Shortcut::CommandPalette)),
                    "b" => Some(Message::KeyboardShortcut(Shortcut::ToggleSidebar)),
                    "g" => Some(Message::KeyboardShortcut(Shortcut::KnowledgeGraph)),
                    "t" => Some(Message::KeyboardShortcut(Shortcut::TableOfContents)),
                    "i" => Some(Message::InkToggle),
                    // Undo/redo for the ink surface. These are no-ops while it
                    // is hidden, and the listener does not capture the event,
                    // so the editor's own handling of Ctrl+Z is unaffected.
                    "z" => Some(Message::InkUndo),
                    "y" => Some(Message::InkRedo),
                    _ => None,
                };
            }
            // Removes the ink selection; a no-op while the surface is hidden,
            // and the listener does not capture, so the editor still sees the
            // keystroke normally.
            if key == Key::Named(Named::Delete) {
                return Some(Message::InkDeleteSelection);
            }
            match key {
                Key::Named(Named::ArrowDown) => Some(Message::PdfScrollBy(ARROW_SCROLL)),
                Key::Named(Named::ArrowUp) => Some(Message::PdfScrollBy(-ARROW_SCROLL)),
                Key::Named(Named::PageDown) => Some(Message::PdfScrollBy(520.0)),
                Key::Named(Named::PageUp) => Some(Message::PdfScrollBy(-520.0)),
                _ => None,
            }
        });

        let toast = if self.ui.toast.is_some() {
            iced::time::every(std::time::Duration::from_secs(3)).map(|_| Message::ToastHide)
        } else {
            Subscription::none()
        };

        // Armed only while there are unwritten edits, so an idle app ticks for
        // nothing. The tick is finer than the debounce because its phase is
        // independent of the last keystroke: polling at the debounce itself
        // would let a write land up to two windows after typing stopped.
        let autosave = if self.editor.autosave_pending_since.is_some() {
            iced::time::every(crate::editor_state::AUTOSAVE_POLL).map(|_| Message::AutosaveElapsed)
        } else {
            Subscription::none()
        };

        let search_debounce = if self.search.pending_query_at.is_some() {
            iced::time::every(crate::search_state::SEARCH_DEBOUNCE)
                .map(|_| Message::SearchDebounceElapsed)
        } else {
            Subscription::none()
        };

        // Vault writes and watcher notifications often arrive in bursts. Wait
        // briefly, then build one graph snapshot away from the UI thread.
        let graph_refresh = if self.graph.visible && self.graph.refresh_due() {
            iced::time::every(std::time::Duration::from_millis(200))
                .map(|_| Message::GraphRefresh)
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

        let window_events = iced::event::listen_with(|event, _status, window_id| match event {
            iced::Event::Window(iced::window::Event::Resized(size)) => {
                Some(Message::WindowResized(window_id, size.width, size.height))
            }
            // Captured so we can query the initial scale factor — a static
            // HiDPI monitor may never emit `Rescaled`.
            iced::Event::Window(iced::window::Event::Opened { .. }) => {
                Some(Message::WindowOpened(window_id))
            }
            // A daemon keeps running with no windows, so closing has to be
            // handled explicitly or the process would linger invisibly.
            iced::Event::Window(iced::window::Event::Closed) => {
                Some(Message::WindowClosed(window_id))
            }
            iced::Event::Window(iced::window::Event::Rescaled(factor)) => {
                Some(Message::WindowRescaled(window_id, factor))
            }
            // Intercepted rather than letting the window close directly, so the
            // last few hundred milliseconds of typing are written before exit.
            iced::Event::Window(iced::window::Event::CloseRequested) => {
                Some(Message::WindowCloseRequested)
            }
            _ => None,
        });

        // Watch the vault root for external changes so the index, search, and
        // file tree stay current without reopening the vault. Keyed by the root
        // path so it re-subscribes (and re-watches) when the vault changes.
        let vault_watch = match self.vault.root.clone() {
            Some(root) => Subscription::run_with(root, vault_watch_stream),
            None => Subscription::none(),
        };

        // Redraw every frame only while something is actually moving; a
        // settled UI subscribes to nothing.
        let animation = if self.motion.is_animating() {
            iced::window::frames().map(Message::AnimationTick)
        } else {
            Subscription::none()
        };

        let physics_tick = if self.graph.visible && self.graph.physics.running {
            iced::time::every(std::time::Duration::from_millis(33))
                .map(|_| Message::GraphPhysicsTick)
        } else {
            Subscription::none()
        };

        // A wheel turn or a click takes the page back from a scroll in flight.
        let scroll_interrupt = if self.motion.editor_scroll.is_some() {
            iced::event::listen_with(|event, _status, _window_id| match event {
                iced::Event::Mouse(
                    iced::mouse::Event::WheelScrolled { .. } | iced::mouse::Event::ButtonPressed(_),
                ) => Some(Message::EditorScrollInterrupted),
                _ => None,
            })
        } else {
            Subscription::none()
        };

        // Captured pen samples. Deliberately always-on rather than gated on
        // the surface being visible: the stream consumes the process-wide
        // receiver, so tearing it down and rebuilding it would leave the
        // second instance with nothing to read. Nothing is produced while the
        // surface is hidden anyway — the window hook stops claiming pen input.
        let pen_input = Subscription::run_with("ink-pen", |_: &&str| crate::ink::pen::stream());

        Subscription::batch(vec![
            keyboard,
            animation,
            scroll_interrupt,
            toast,
            autosave,
            search_debounce,
            graph_refresh,
            physics_tick,
            mouse_drag,
            window_events,
            vault_watch,
            pen_input,
        ])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.update_inner(message);

        // Point every transition at the flags that own the real state. Doing
        // it here, once, means a flag flipped anywhere animates without the
        // code that flipped it having to know motion exists.
        self.motion.sync(
            self.vault.sidebar_visible,
            self.editor.toc_visible,
            self.vault.backlinks_visible,
            self.ui.toast.as_deref(),
            self.ui.command_palette_visible,
        );
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
            Message::VaultIndexed(generation, root, result) => {
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
                                Message::VaultIndexed(
                                    generation,
                                    message_root.clone(),
                                    result,
                                )
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
            m @ (Message::SidebarToggle
            | Message::SidebarFolderToggled(_)
            | Message::SidebarSelectionCleared) => self.vault.update(m),
            Message::SidebarFileClicked(path) => {
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
            Message::GraphToggle => {
                if self.vault.root.is_none() {
                    return Task::none();
                }
                if self.graph.visible {
                    self.graph.visible = false;
                } else {
                    self.graph.visible = true;
                    self.tracker.hide();
                    self.search.visible = false;
                    self.search.file_visible = false;
                    self.ui.command_palette_visible = false;
                    self.graph.retry_refresh();
                    let active = self.graph.active_path.clone();
                    self.graph.select(active);
                }
                Task::none()
            }
            Message::GraphRefresh => {
                if !self.graph.visible {
                    return Task::none();
                }
                let Some(generation) = self.graph.begin_refresh() else {
                    return Task::none();
                };
                let state = self.state.clone();
                Task::perform(
                    async move { md_editor_core::vault::get_graph_snapshot(&state) },
                    move |result| Message::GraphSnapshotLoaded(generation, result),
                )
            }
            Message::GraphSnapshotLoaded(generation, result) => {
                match result {
                    Ok(snapshot) => {
                        self.graph.complete_refresh(generation, snapshot);
                    }
                    Err(error) => {
                        if self.graph.fail_refresh(generation) && self.graph.visible {
                            self.ui.toast =
                                Some(format!("Could not refresh knowledge graph: {error}"));
                        }
                    }
                }
                Task::none()
            }
            Message::GraphScopeGlobal => {
                self.graph
                    .set_scope(crate::graph_state::GraphScope::Global);
                Task::none()
            }
            Message::GraphScopeLocal => {
                let depth = match self.graph.scope {
                    crate::graph_state::GraphScope::Local { depth } => depth,
                    crate::graph_state::GraphScope::Global => 1,
                };
                self.graph
                    .set_scope(crate::graph_state::GraphScope::Local { depth });
                Task::none()
            }
            Message::GraphLocalDepthChanged(depth) => {
                self.graph
                    .set_scope(crate::graph_state::GraphScope::Local { depth });
                Task::none()
            }
            Message::GraphQueryChanged(query) => {
                self.graph.set_query(query);
                Task::none()
            }
            Message::GraphShowPdfsToggled(show) => {
                self.graph.set_show_pdfs(show);
                Task::none()
            }
            Message::GraphShowMissingToggled(show) => {
                self.graph.set_show_missing(show);
                Task::none()
            }
            Message::GraphShowOrphansToggled(show) => {
                self.graph.set_show_orphans(show);
                Task::none()
            }
            Message::GraphNodeSelected(path) => {
                self.graph.select(path);
                Task::none()
            }
            Message::GraphNodeMoved { path, x, y } => {
                self.graph.move_node(&path, x, y);
                Task::none()
            }
            Message::GraphNodeOpen(path) => {
                let exists = self.graph.node(&path).is_some_and(|node| node.exists);
                if exists {
                    // The graph is a full-window workspace, so leaving it up
                    // would hide the document the user just asked to read.
                    self.graph.visible = false;
                    Task::done(Message::SidebarFileClicked(path))
                } else {
                    self.ui.toast = Some("That link target does not exist yet".to_string());
                    Task::none()
                }
            }
            Message::GraphNodeFocused(path) => {
                self.graph.focus_on(&path);
                Task::none()
            }
            Message::GraphPinToggled(path) => {
                self.graph.toggle_pin(&path);
                Task::none()
            }
            Message::GraphUnpinAll => {
                self.graph.unpin_all();
                Task::none()
            }
            Message::GraphZoomBy(factor) => {
                self.graph.zoom_by(factor);
                Task::none()
            }
            Message::GraphPhysicsToggled => {
                self.graph.toggle_physics();
                Task::none()
            }
            Message::GraphFiltersReset => {
                self.graph.reset_filters();
                Task::none()
            }
            Message::GraphInspectorToggled => {
                self.graph.inspector_visible = !self.graph.inspector_visible;
                Task::none()
            }
            Message::GraphFitView => {
                self.graph.fit_revision = self.graph.fit_revision.wrapping_add(1);
                Task::none()
            }
            Message::GraphResetLayout => {
                self.graph.reset_layout();
                Task::none()
            }
            Message::GraphPhysicsTick => {
                self.graph.physics_tick();
                Task::none()
            }
            Message::GraphSetView { pan_x, pan_y, zoom } => {
                self.graph.set_view(pan_x, pan_y, zoom);
                Task::none()
            }
            Message::GraphHovered(path) => {
                self.graph.set_hovered(path);
                Task::none()
            }
            // UI chrome arms that mutate only `self.ui` are routed to
            // `UiState::update`; see ui_state.rs.
            // Opening the palette also puts the caret in its field: a palette
            // you have to click into first is not reachable from the keyboard,
            // which is the only reason it exists.
            Message::CommandPaletteOpen => {
                let task = self.ui.update(Message::CommandPaletteOpen);
                Task::batch(vec![
                    task,
                    operation::focus(iced::advanced::widget::Id::new(
                        views::command_palette::PALETTE_INPUT_ID,
                    )),
                ])
            }
            m @ (Message::CreateFileDialog
            | Message::CreateFolderDialog
            | Message::DeleteFileDialog(_)
            | Message::RenameEntryDialog(_)
            | Message::NameModalInputChanged(_)
            | Message::PdfLinkNoteFolderSelected(_)
            | Message::PdfLinkNoteFileSelected(_)
            | Message::PdfLinkNotePickerSearchChanged(_)
            | Message::NameModalCancel
            | Message::CommandPaletteQueryChanged(_)
            | Message::ShowToast(_)
            | Message::ToastHide
            | Message::SplitViewDragStart) => self.ui.update(m),
            Message::NameModalSubmit(input) => {
                if let Some(views::modals::ModalType::QuickNote(id)) = self.ui.active_modal.clone()
                {
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
                        }
                        Err(error) => self.ui.toast = Some(error),
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
                        let result = md_editor_core::vault::create_file(&self.state, &path);
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
                        let result = md_editor_core::vault::create_dir(&self.state, &target_path);
                        self.reset_vault_ui(result);
                        self.invalidate_graph();
                        Task::none()
                    }
                    _ => Task::none(),
                }
            }
            Message::DeleteFile(path) => {
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
                    Err(err) => self.ui.toast = Some(err),
                }
                Task::none()
            }

            Message::EditorCommand(command) => self.run_editor_command(command),
            Message::EditorCommandNoScroll(command) => {
                self.run_editor_command_with_scroll(command, false)
            }
            // Editor arms that mutate only `self.editor` are routed to
            // `EditorPane::update`; see editor_state.rs.
            m @ Message::MathRendered(..) => self.editor.update(m),
            Message::EditorSave => {
                match self.save_active_document() {
                    SaveOutcome::Saved => self.ui.toast = Some("File saved".to_string()),
                    // An explicit save always answers, even when autosave got
                    // there first — pressing Ctrl+S and seeing nothing happen
                    // reads as the keystroke having been swallowed.
                    SaveOutcome::NothingToDo if self.active_path.is_some() => {
                        self.ui.toast = Some("No unsaved changes".to_string())
                    }
                    SaveOutcome::NothingToDo => {}
                    SaveOutcome::Failed(err) => self.ui.toast = Some(format!("Save failed: {err}")),
                }
                Task::none()
            }
            Message::AnimationTick(now) => {
                self.motion.now = now;
                self.step_editor_scroll(now)
            }
            Message::AutosaveElapsed => {
                if self.editor.autosave_due() {
                    // Autosave is silent on success — a toast on every pause in
                    // typing would be noise. Failures still surface, because a
                    // failed autosave is exactly when the user needs to know.
                    // Only the first failure in a run speaks up, though: the
                    // retry keeps going in the background, and a toast every
                    // few hundred milliseconds would bury the rest of the UI.
                    if let SaveOutcome::Failed(err) = self.save_active_document()
                        && !self.autosave_failure_reported
                    {
                        self.autosave_failure_reported = true;
                        self.ui.toast = Some(format!("Autosave failed: {err}"));
                    }
                }
                Task::none()
            }
            Message::EditorCheckboxToggle(line_idx) => {
                self.run_editor_command(EditorCommand::ToggleCheckbox { line: line_idx })
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
            Message::EditorCaretView(view) => self.reveal_caret(view),
            Message::EditorScrollInterrupted => {
                self.motion.editor_scroll = None;
                Task::none()
            }

            Message::PdfLoaded(generation, result) => {
                if generation != self.pdf.render_generation {
                    return Task::none();
                }
                let pages = match result {
                    Ok(pages) if pages > 0 => {
                        self.pdf.load_error = None;
                        pages
                    }
                    Ok(_) => {
                        let file = self.pdf.active_path.as_deref().unwrap_or("PDF");
                        self.pdf.load_error =
                            Some(format!("Could not open '{file}': the document has no pages"));
                        0
                    }
                    Err(error) => {
                        let file = self.pdf.active_path.as_deref().unwrap_or("PDF");
                        self.pdf.load_error = Some(format!("Could not open '{file}': {error}"));
                        0
                    }
                };
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
                if let Some(error) = self.pdf.load_error.clone() {
                    self.ui.toast = Some(error);
                }
                // A target page stashed by PdfDocumentIdComputed while the page
                // count was still unknown (see that handler) is resolved here.
                let target_task = if pages == 0 {
                    // Failed load: drop the target so it can't leak into the
                    // next document opened.
                    self.pdf.initial_target_page = None;
                    self.pdf.initial_target_annotation = None;
                    Task::none()
                } else if self.pdf.fit_to_width {
                    // Leave the stash for the PdfFitToWidth handler, whose
                    // `is_initial` path scrolls to the target at the post-fit
                    // zoom. Navigating here would use pre-fit page offsets and
                    // then be re-scrolled to page 0 by the fit pass.
                    Task::none()
                } else if let Some(page) = self.pdf.initial_target_page.take() {
                    self.navigate_pdf_page(page)
                } else {
                    Task::none()
                };
                let layout_task = if self.pdf.fit_to_width
                    && self
                        .pdf
                        .page_sizes
                        .iter()
                        .take(pages as usize)
                        .any(Option::is_some)
                {
                    Task::done(Message::PdfFitToWidth)
                } else if self.pdf.fit_to_width {
                    Task::none()
                } else {
                    self.render_all_pdf_pages()
                };
                Task::batch(vec![layout_task, target_task])
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
                    .pdf
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
                            .map(|(w, _)| (*w as f32 / self.pdf.zoom).max(1.0))
                    })
                    .unwrap_or(612.0);
                let next_zoom =
                    ((available_width - 48.0).max(240.0) / page_width).clamp(0.5, PDF_FIT_MAX_ZOOM);
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
            // PDF arms that mutate only `self.pdf` are routed to
            // `PdfPane::update`; see pdf_pane.rs.
            m @ (Message::PdfPageSizesLoaded(..)
            | Message::PdfRenderSkipped(..)
            | Message::ClosePdfLinkPreview
            | Message::PdfPageTextLoaded(..)
            | Message::PdfSelectionCleared) => self.pdf.update(m),
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
                let supersample = self.pdf_supersample();
                let logical_width = (width as f32 / supersample).round() as u32;
                let logical_height = (height as f32 / supersample).round() as u32;
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
            Message::TocClicked(index) => {
                if self.pdf.active_path.is_some() {
                    let target_page = (index as u16).min(self.pdf.total_pages.saturating_sub(1));
                    self.navigate_pdf_page(target_page)
                } else {
                    let task = self.run_editor_command(EditorCommand::SetCursor {
                        line: index,
                        col: 0,
                        affinity: Affinity::Downstream,
                    });
                    self.editor.request_reveal(Reveal::Center);
                    task
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
                            crate::platform::open_external(&uri);
                            self.ui.toast = Some(format!("Opening: {}", uri));
                        } else {
                            self.ui.toast =
                                Some(format!("External link (Ctrl+click to open): {}", uri));
                        }
                        Task::none()
                    } else {
                        Task::none()
                    }
                } else if let Some(ann) = self.pdf.annotation_at(page_idx, x, y) {
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
                    target_ann = self.pdf.annotation_at(page_idx, x, y);
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
                    let target_px = self.preview_target_px();

                    Task::perform(
                        async move {
                            let renderer = _state.pdf_renderer.as_ref()?;
                            renderer
                                .render_link_preview(&abs_path, dest_page, dest_y, target_px)
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
                    self.pdf.link_preview_size = Some((width as f32, height as f32));
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
            Message::PdfTocLoaded(path, entries, synthetic) => {
                // Path-gated, not generation-gated: fit-to-width bumps the
                // render generation right after open, and the TOC scan for
                // bookmark-less PDFs regularly finishes after that. Only a
                // result for a different document is stale.
                if self.pdf.active_path.as_deref() != Some(path.as_str()) {
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
                self.pdf.toc_entries = mapped;
                self.pdf.toc_is_synthetic = synthetic;
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
            Message::NameModalSubmitCurrent => {
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
                            Task::done(Message::CommandPaletteCommandClicked(shortcut))
                        }
                        Some(views::command_palette::PaletteAction::OpenFile(path)) => {
                            Task::done(Message::CommandPaletteFileClicked(path))
                        }
                        None => Task::none(),
                    };
                }
                self.ui.update(Message::NameModalSubmitCurrent)
            }
            Message::CommandPaletteFileClicked(path) => {
                self.ui.command_palette_visible = false;
                self.ui.command_palette_query.clear();
                // Reuse the sidebar's open path: it already routes markdown,
                // PDFs and images to the right viewer.
                Task::done(Message::SidebarFileClicked(path))
            }
            Message::CommandPaletteCommandClicked(shortcut) => {
                self.ui.command_palette_visible = false;
                self.ui.command_palette_query.clear();
                Task::done(Message::KeyboardShortcut(shortcut))
            }
            Message::OverlaySubmitCurrent => {
                if self.ui.command_palette_visible {
                    Task::done(Message::NameModalSubmitCurrent)
                } else if self.search.visible {
                    Task::done(Message::GlobalSearchSubmit)
                } else if self.graph.visible && self.ui.active_modal.is_none() {
                    // Enter opens whatever is selected on the canvas, so the
                    // graph is navigable without reaching for the mouse.
                    match self.graph.selected_path.clone() {
                        Some(path) => Task::done(Message::GraphNodeOpen(path)),
                        None => Task::none(),
                    }
                } else {
                    Task::done(Message::NameModalSubmitCurrent)
                }
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
            Message::GlobalSearchSubmit => {
                if let Some(result) = self.search.pdf_results.first() {
                    Task::done(Message::PdfSearchResultClicked(result.page_index))
                } else if let Some(result) = self.search.results.first() {
                    Task::done(Message::SearchResultClicked(result.path.clone()))
                } else {
                    Task::none()
                }
            }
            Message::SearchClose => {
                self.search.visible = false;
                self.search.file_visible = false;
                self.restore_scroll_positions()
            }
            Message::SearchQueryChanged(q) => {
                self.search.query = q;
                self.search.match_index = None;
                self.search.pdf_error = None;
                // Stale results are cleared immediately, but the actual vault
                // FTS query and full-document PDF scan run debounced (see
                // SearchDebounceElapsed) so they fire once per pause in
                // typing, not once per keystroke.
                if self.search.query.len() <= 2 || self.search.regex {
                    self.search.results.clear();
                    self.search.results_truncated = false;
                }
                if !((self.search.visible || self.pdf_search_is_active())
                    && self.pdf.active_path.is_some()
                    && self.search.query.len() > 1)
                {
                    self.search.pdf_results.clear();
                    self.search.pdf_indices_by_page.clear();
                }
                self.search.pending_query_at = Some(std::time::Instant::now());
                Task::none()
            }
            Message::SearchDebounceElapsed => {
                if self
                    .search
                    .pending_query_at
                    .is_some_and(|at| at.elapsed() < crate::search_state::SEARCH_DEBOUNCE)
                {
                    return Task::none();
                }
                if self.search.pending_query_at.take().is_none() {
                    return Task::none();
                }
                let q = self.search.query.clone();
                if q.len() > 2
                    && !self.search.regex
                    && let Ok(res) = md_editor_core::vault::search_vault(&self.state, &q)
                {
                    self.search.results = res.items;
                    self.search.results_truncated = res.truncated;
                }
                if (self.search.visible || self.pdf_search_is_active())
                    && self.pdf.active_path.is_some()
                    && q.len() > 1
                {
                    self.search_pdf()
                } else {
                    Task::none()
                }
            }
            // Search arms that mutate only `self.search` are routed to
            // `SearchState::update`; see search_state.rs.
            m @ Message::SearchReplaceChanged(_) => self.search.update(m),
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
            Message::PdfSearchLooseToggled(value) => {
                self.search.loose = value;
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
            Message::PdfSearchResult(generation, Ok(results)) => {
                if !pdf_search_generation_is_current(self.pdf.render_generation, generation) {
                    return Task::none();
                }
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
            Message::PdfSearchResult(generation, Err(err)) => {
                if !pdf_search_generation_is_current(self.pdf.render_generation, generation) {
                    return Task::none();
                }
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
                // The palette owns the arrow keys while it is open.
                if self.ui.command_palette_visible {
                    let count = views::command_palette::result_count(
                        &self.ui.command_palette_query,
                        &self.ui.commands,
                        &self.vault.entries,
                    );
                    if count > 0 {
                        let last = count - 1;
                        self.ui.palette_selected = if delta > 0.0 {
                            (self.ui.palette_selected + 1).min(last)
                        } else {
                            self.ui.palette_selected.saturating_sub(1)
                        };
                    }
                    return Task::none();
                }
                if self.pptx.active_path.is_some() {
                    if self.search.visible || self.ui.active_modal.is_some() {
                        return Task::none();
                    }
                    let width = self.pdf_available_width();
                    // Page Up and Page Down move a whole slide; the arrows scroll.
                    return if delta.abs() > ARROW_SCROLL {
                        self.pptx
                            .update(Message::PptxStep(delta.signum() as i32), width)
                    } else {
                        self.pptx.scroll_by(delta, width)
                    };
                }
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
            Message::PdfDocumentIdComputed(generation, Some((path, hash, len, mtime))) => {
                if generation != self.pdf.render_generation
                    || self.pdf.active_path.as_deref() != Some(path.as_str())
                {
                    return Task::none();
                }
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

                let resolved_target = if let Some(page) = target_page {
                    self.pdf.initial_target_annotation = None;
                    Some(page)
                } else {
                    self.pdf.initial_target_page
                };
                let scroll_task = match resolved_target {
                    Some(page) if self.pdf.total_pages > 0 => {
                        self.pdf.initial_target_page = None;
                        self.navigate_pdf_page(page)
                    }
                    Some(page) => {
                        // The hash task won the race against PdfLoaded, so the
                        // page count isn't known yet and navigating now would
                        // clamp the target to 0. Stash it; once the document is
                        // applied it is consumed by PdfFitToWidth's is_initial
                        // path (the normal fit-on-open flow) or by PdfLoaded
                        // directly when fit-to-width is off.
                        self.pdf.initial_target_page = Some(page);
                        Task::none()
                    }
                    None => Task::none(),
                };

                // Internal references: load the cached target map if this
                // document has already been scanned, otherwise resolve once in
                // the background and cache it. Gated on document_id (not render
                // generation): references are a document-level artifact, and the
                // scan outlives the fit-to-width zoom change that bumps the
                // generation.
                //
                // The scan extracts text one page at a time so the single
                // pdfium worker drains queued page renders *between* commands —
                // avoiding the ~0.5–1s stall a monolithic full-document command
                // would cause on first open of a bookmarked PDF.
                let refs_task = if let Some(links) = self.state.get_pdf_references(&hash) {
                    self.pdf.set_references(links);
                    Task::none()
                } else if let Some(abs) = self.resolve_active_path(&path) {
                    let abs = abs.to_string_lossy().to_string();
                    let state_refs = self.state.clone();
                    let doc_id = hash.clone();
                    let doc_id_msg = hash.clone();
                    Task::perform(
                        async move {
                            let renderer = state_refs.pdf_renderer.as_ref()?;
                            // Embedded outline first (cheap, no scan).
                            let mut toc = renderer.get_embedded_toc(&abs).unwrap_or_default();
                            let n = renderer.page_count(&abs).ok()?;
                            let mut pages = Vec::with_capacity(n as usize);
                            for i in 0..n {
                                if let Ok(pt) = renderer.get_page_text(&abs, i) {
                                    pages.push(pt);
                                }
                            }
                            // No bookmarks → recover section structure from the
                            // text just collected (no second scan).
                            if toc.is_empty() {
                                toc = md_editor_core::pdf::recover_toc_from_texts(&pages);
                            }
                            let links =
                                md_editor_core::references::resolve_references(&pages, &toc);
                            state_refs.put_pdf_references(&doc_id, &links);
                            Some(links)
                        },
                        move |res| {
                            Message::PdfReferencesLoaded(
                                doc_id_msg.clone(),
                                res.unwrap_or_default(),
                            )
                        },
                    )
                } else {
                    Task::none()
                };

                Task::batch(vec![scroll_task, refs_task])
            }
            Message::PdfReferencesLoaded(doc_id, links) => {
                // Apply only if still on the same document (the scan is async and
                // may finish after the user has navigated away).
                if self.pdf.document_id.as_deref() == Some(doc_id.as_str()) {
                    self.pdf.set_references(links);
                }
                Task::none()
            }
            Message::PdfDocumentIdComputed(_, None) => Task::none(),
            Message::PdfSelectionChanged(page, anchor, focus) => {
                self.active_panel = ActivePanel::Pdf;
                self.pdf.selection = Some(views::interactive_pdf::PdfSelection {
                    page_index: page,
                    anchor_idx: anchor,
                    focus_idx: focus,
                });
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
                if let Some(sel) = &self.pdf.selection
                    && let Some(page_text) = self.pdf.page_text.get(&sel.page_index)
                {
                    let start = sel.anchor_idx.min(sel.focus_idx);
                    let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
                    let selected = text_by_char_range(&page_text.text, start, end);
                    if !selected.is_empty() {
                        return iced::clipboard::write(selected);
                    }
                }
                Task::none()
            }
            Message::PdfCreateHighlight(color) => self.create_highlight(color),
            Message::PdfQuickHighlight => {
                // Only consume a palette color when there is actually a
                // selection to highlight, so the cycle doesn't silently
                // advance on empty clicks.
                if self.pdf.selection.is_none() {
                    Task::none()
                } else {
                    let color = self.pdf.next_highlight_color;
                    self.pdf.next_highlight_color = color.next();
                    self.create_highlight(color)
                }
            }
            Message::PdfCopyAnnotationText(id) => {
                if let Some(text) = self
                    .pdf
                    .annotations
                    .values()
                    .flatten()
                    .find(|a| a.id == id)
                    .map(|a| a.selected_text.clone())
                {
                    self.ui.toast = Some("Annotation text copied".to_string());
                    iced::clipboard::write(text)
                } else {
                    Task::none()
                }
            }
            Message::PdfOrphanReport => {
                let report = self.pdf_orphan_report();
                self.ui.toast = Some(report);
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
                    if let Some(views::modals::ModalType::QuickNote(ref mid)) = self.ui.active_modal
                        && mid == &id
                    {
                        self.ui.active_modal = None;
                        self.ui.modal_input.clear();
                    }
                    if let Some(ref path) = self.pdf.active_path {
                        self.vault.backlinks =
                            md_editor_core::vault::get_mixed_backlinks(&self.state, path)
                                .unwrap_or_default();
                    }
                    self.invalidate_graph();
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
                        self.ui.active_modal =
                            Some(views::modals::ModalType::LinkNote(annotation_id));
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
                        self.invalidate_graph();
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

                let split_changed = !self.ui.split_view_active;
                self.ui.split_view_active = true;
                self.showing_pdf = true;

                if self.pdf.active_path.as_deref() == Some(&resolved_pdf_path) {
                    self.pdf.focused_annotation_id = Some(annotation_id);
                    let nav_task = self.navigate_pdf_page(page.saturating_sub(1));
                    if split_changed {
                        Task::batch(vec![self.refit_pdf_if_needed(), nav_task])
                    } else {
                        nav_task
                    }
                } else {
                    self.pdf.initial_target_page = Some(page.saturating_sub(1));
                    self.pdf.initial_target_annotation = Some(annotation_id);
                    self.open_pdf(&resolved_pdf_path)
                }
            }
            Message::SearchResultClicked(path) => {
                if !search_result_should_reload(self.active_path.as_deref(), &path) {
                    // The open buffer is authoritative while dirty; a global
                    // search hit for this same file must not reload disk state.
                    self.search.visible = false;
                    self.vault.selected_path = Some(path);
                    return Task::none();
                }
                self.search.visible = false;
                self.vault.selected_path = Some(path.clone());
                if path.to_lowercase().ends_with(".pdf") {
                    self.pdf.active_path = Some(path.clone());
                    self.showing_pdf = true;
                    self.open_pdf(&path)
                } else {
                    self.open_file(&path)
                }
            }

            Message::KeyboardShortcut(s) => {
                match s {
                    Shortcut::Escape => {
                        // Close overlays in priority order. The ink surface is
                        // first: while it is up it owns the screen, and Escape
                        // should drop its selection before anything else.
                        if self.ink.is_visible() && self.ink.clear_selection() {
                            return Task::none();
                        }
                        if self.pdf.selection.is_some() {
                            self.pdf.selection = None;
                        } else if self.pdf.focused_annotation_id.is_some() {
                            self.pdf.focused_annotation_id = None;
                        } else if self.pdf.link_preview.is_some() {
                            self.pdf.link_preview = None;
                            self.pdf.link_preview_size = None;
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
                        } else if self.graph.visible {
                            self.graph.visible = false;
                        } else if self.editor.toc_visible {
                            self.editor.toc_visible = false;
                        }
                        Task::none()
                    }
                    Shortcut::ToggleSidebar => {
                        self.vault.sidebar_visible = !self.vault.sidebar_visible;
                        self.refit_pdf_if_needed()
                    }
                    // Ctrl+S saves whatever surface is in front.
                    Shortcut::Handwriting => Task::done(Message::InkToggle),
                    Shortcut::Save if self.ink.is_visible() => Task::done(Message::InkSave),
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
                    // Routed through the message rather than setting the flag
                    // here, so the shortcut and any other opener share one
                    // path — including focusing the query field.
                    Shortcut::CommandPalette => Task::done(Message::CommandPaletteOpen),
                    Shortcut::ToggleBacklinks => {
                        self.vault.backlinks_visible = !self.vault.backlinks_visible;
                        Task::none()
                    }
                    Shortcut::KnowledgeGraph => Task::done(Message::GraphToggle),
                    Shortcut::TableOfContents => Task::done(Message::ToggleTOC),
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
                        self.graph.visible = false;
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
            Message::SplitViewDragging(x_pos) => {
                if !self.ui.is_resizing_split {
                    return Task::none();
                }
                let side_width = if self.vault.sidebar_visible {
                    250.0
                } else {
                    0.0
                } + if self.tracker.visible { 300.0 } else { 0.0 }
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
                self.persist_layout();
                if self.pdf.fit_to_width && self.pdf.active_path.is_some() {
                    return Task::done(Message::PdfFitToWidth);
                }
                Task::none()
            }
            Message::WindowResized(id, width, height) => {
                // Only the shell's geometry is layout input and worth restoring;
                // the handwriting window sizes itself.
                if Some(id) != self.main_window {
                    return Task::none();
                }
                self.ui.window_width = width;
                self.ui.window_height = height;
                self.persist_window_size();
                if self.pdf.fit_to_width && self.pdf.active_path.is_some() {
                    return Task::done(Message::PdfFitToWidth);
                }
                Task::none()
            }
            // ── Ink (handwritten notes) ──────────────────────────
            Message::InkPenHookReady(raw_window_id) => {
                if let Err(e) = crate::ink::pen::install(raw_window_id) {
                    // Not fatal: the surface still works with a mouse.
                    eprintln!("Pen input unavailable: {e}");
                }
                Task::none()
            }
            Message::InkToggle => match self.ink_window {
                // Already open: closing the window tears down the surface via
                // the close event, so there is one path for both routes out.
                Some(id) => iced::window::close(id),
                None => self.open_ink_window(),
            },
            Message::InkPenSamples(samples) => {
                self.ink.apply_samples(&samples, self.ui.scale_factor);
                Task::none()
            }
            Message::InkPointerDown(x, y) => {
                // Mouse fallback: no pressure, so use a fixed mid-weight.
                self.ink.begin(
                    crate::ink::stroke::Vec2::new(x, y),
                    crate::ink::canvas::MOUSE_PRESSURE,
                    false,
                );
                Task::none()
            }
            Message::InkPointerMove(x, y) => {
                self.ink.extend(
                    crate::ink::stroke::Vec2::new(x, y),
                    crate::ink::canvas::MOUSE_PRESSURE,
                );
                Task::none()
            }
            Message::InkPointerUp => {
                self.ink.finish();
                Task::none()
            }
            Message::InkHoverMoved(x, y) => {
                self.ink.set_hover(crate::ink::stroke::Vec2::new(x, y));
                Task::none()
            }
            Message::InkToolSelected(tool) => {
                if self.ink.tool != tool {
                    // Leaving the lasso behind should not leave a selection
                    // highlighted with no way to act on it.
                    if tool != crate::ink::Tool::Lasso {
                        self.ink.clear_selection();
                    }
                    self.ink.tool = tool;
                }
                Task::none()
            }
            Message::InkEraserModeSelected(mode) => {
                self.ink.eraser_mode = mode;
                Task::none()
            }
            Message::InkPaperSelected(paper) => {
                self.ink.set_paper(paper);
                Task::none()
            }
            // Colour and width belong to the active tool: switching to the
            // highlighter and back must not clobber the pen's settings.
            Message::InkColorSelected(color) => {
                if self.ink.tool == crate::ink::Tool::Highlighter {
                    self.ink.highlighter_color = color;
                } else {
                    self.ink.color = color;
                }
                Task::none()
            }
            Message::InkSizeSelected(size) => {
                if self.ink.tool == crate::ink::Tool::Highlighter {
                    self.ink.highlighter_size = size;
                } else {
                    self.ink.options.size = size;
                }
                Task::none()
            }
            Message::InkPan { dx, dy } => {
                self.ink.pan_by(crate::ink::stroke::Vec2::new(dx, dy));
                Task::none()
            }
            Message::InkZoom { x, y, notches } => {
                self.ink
                    .zoom_by_wheel(crate::ink::stroke::Vec2::new(x, y), notches);
                Task::none()
            }
            Message::InkZoomStep(notches) => {
                self.ink.zoom_step(notches);
                Task::none()
            }
            Message::InkResetView => {
                self.ink.reset_view();
                Task::none()
            }
            Message::InkDeleteSelection => {
                if self.ink.is_visible() {
                    self.ink.delete_selection();
                }
                Task::none()
            }
            // Guarded so the shared Ctrl+Z / Ctrl+Y bindings only reach the ink
            // surface while it is the one on screen.
            Message::InkUndo => {
                if self.ink.is_visible() {
                    self.ink.undo();
                }
                Task::none()
            }
            Message::InkRedo => {
                if self.ink.is_visible() {
                    self.ink.redo();
                }
                Task::none()
            }
            Message::InkClear => {
                self.ink.clear();
                Task::none()
            }
            Message::InkSave => self.save_ink_page(),
            Message::InkSaved(result) => {
                match result {
                    Ok(path) => {
                        self.ink.dirty = false;
                        self.ui.toast = Some(format!("Saved {path}"));
                    }
                    Err(e) => self.ui.toast = Some(e),
                }
                Task::none()
            }

            Message::WindowOpened(id) => {
                // Pull the real scale factor; a static HiDPI monitor may never
                // emit a `Rescaled` event, so the listener alone isn't enough.
                let scale = iced::window::scale_factor(id)
                    .map(move |factor| Message::WindowRescaled(id, factor));

                if Some(id) == self.ink_window {
                    // The hook attaches to whichever window holds the canvas,
                    // so it has to move here — and it must re-attach each time
                    // the window is reopened, since the handle is new.
                    return Task::batch(vec![
                        scale,
                        iced::window::raw_id::<Message>(id).map(Message::InkPenHookReady),
                    ]);
                }
                scale
            }
            Message::WindowCloseRequested => {
                // Best-effort flush on the way out; the close is deliberately
                // *not* intercepted. Holding the window open on a failed write
                // would risk an app that cannot be closed at all, which is a
                // worse failure than the few hundred milliseconds of typing
                // that autosave has not yet committed.
                let _ = self.save_active_document();
                self.persist_session();
                self.persist_layout();
                Task::none()
            }
            Message::WindowClosed(id) => {
                if Some(id) == self.ink_window {
                    self.ink_window_closed();
                    return Task::none();
                }
                if Some(id) == self.main_window {
                    // The shell is the app; close it and everything goes.
                    return iced::exit();
                }
                Task::none()
            }
            Message::WindowRescaled(id, factor) => {
                // Clamp: below 1 makes no sense, and capping at 3 bounds the
                // bitmap cost on unusually high-DPI displays.
                let factor = factor.clamp(1.0, 3.0);

                // The two windows can sit on monitors with different scaling,
                // so the pen hook tracks the ink window's factor specifically —
                // using the shell's would misplace every stroke.
                if Some(id) == self.ink_window {
                    crate::ink::set_scale_factor(factor);
                    return Task::none();
                }
                if Some(id) != self.main_window {
                    return Task::none();
                }

                if (factor - self.ui.scale_factor).abs() < 0.01 {
                    return Task::none();
                }
                self.ui.scale_factor = factor;
                // Math bitmaps are rasterized at the display scale but cached
                // by TeX string only; flush and re-render at the new density.
                // load_math also records the new scale so results still in
                // flight from before the rescale get dropped on arrival.
                self.editor.math_cache.clear();
                let math_task = self.editor.load_math(factor);
                // Cached page bitmaps were rasterized at the old supersample;
                // drop them and re-render at the new device resolution.
                if self.pdf.active_path.is_some() && self.pdf.total_pages > 0 {
                    self.pdf.pages = vec![None; self.pdf.total_pages as usize];
                    self.pdf.dimensions = vec![None; self.pdf.total_pages as usize];
                    self.pdf.placeholder_page_size = self.pdf.first_page_size();
                    self.pdf.pending_pages.clear();
                    self.pdf.pending_links.clear();
                    self.pdf.render_generation = self.pdf.render_generation.wrapping_add(1);
                    return Task::batch(vec![math_task, self.render_visible_pdf_pages()]);
                }
                math_task
            }
            Message::VaultFilesChanged(paths) => {
                // Reconcile the index/search for changed markdown files. The
                // open editor buffer is intentionally NOT reloaded — that would
                // clobber unsaved edits and fire on our own saves.
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
            m @ (Message::PptxLoaded(..)
            | Message::PptxScrolled { .. }
            | Message::PptxZoomIn
            | Message::PptxZoomOut
            | Message::PptxZoomFit
            | Message::PptxStep(_)) => {
                let width = self.pdf_available_width();
                self.pptx.update(m, width)
            }
            Message::ToggleTOC => {
                if self.pdf.active_path.is_some()
                    && (self.showing_pdf
                        || (self.ui.split_view_active && self.active_path.is_some()))
                {
                    self.editor.toc_visible = !self.editor.toc_visible;
                    // The TOC panel changes the PDF pane width — re-fit so pages
                    // don't render stretched at the old width.
                    return self.refit_pdf_if_needed();
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// Settle the vault UI after a create: on success refresh the tree and
    /// close the modal, on failure surface the error and keep the modal open
    /// so the name can be corrected. Returns whether the create succeeded.
    fn reset_vault_ui(&mut self, result: Result<(), String>) -> bool {
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
    fn reveal_in_sidebar(&mut self, path: &str) {
        let mut ancestor = path;
        while let Some(idx) = ancestor.rfind('/') {
            ancestor = &ancestor[..idx];
            self.vault.expanded_folders.insert(ancestor.to_string());
        }
        self.vault.selected_path = Some(path.to_string());
    }

    /// Open the detached handwriting window.
    fn open_ink_window(&mut self) -> Task<Message> {
        if let Some(id) = self.ink_window {
            // Already open — surface it instead of opening a second one.
            return iced::window::gain_focus(id);
        }

        let (id, open) = iced::window::open(crate::ink_window_settings());
        self.ink_window = Some(id);
        self.ink.set_visible(true);
        self.active_panel = ActivePanel::Ink;
        open.discard()
    }

    /// Tear down the handwriting surface after its window goes away.
    fn ink_window_closed(&mut self) {
        self.ink_window = None;
        // Stops pen capture and drops any half-drawn stroke.
        self.ink.set_visible(false);
        self.active_panel = ActivePanel::Markdown;
    }

    /// Load a handwritten page from the vault and show it in its own window.
    fn open_ink_page(&mut self, path: &str) -> Task<Message> {
        let Some(root) = self.vault.root.clone() else {
            return Task::none();
        };

        match crate::ink::load_from_vault(&root, path) {
            Ok(document) => {
                self.ink.load(document, Some(path.to_string()));
                self.open_ink_window()
            }
            Err(e) => {
                self.ui.toast = Some(e);
                Task::none()
            }
        }
    }

    /// Write the handwriting surface to the vault, choosing a path on first
    /// save from the note being annotated, or the date when none is open.
    fn save_ink_page(&mut self) -> Task<Message> {
        let Some(root) = self.vault.root.clone() else {
            return Task::done(Message::InkSaved(Err(
                "Open a vault before saving handwritten notes".to_string(),
            )));
        };

        let path = match self.ink.path.clone() {
            Some(path) => path,
            None => {
                let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
                let derived = derive_ink_path(self.active_path.as_deref(), &stamp);
                self.ink.path = Some(derived.clone());
                derived
            }
        };

        let result = crate::ink::save_to_vault(&root, &path, &self.ink.document).map(|()| path);
        Task::done(Message::InkSaved(result))
    }

    pub fn view(&self, window: iced::window::Id) -> Element<'_, Message, Theme, iced::Renderer> {
        if Some(window) == self.ink_window {
            return self.ink_view();
        }
        self.shell_view()
    }

    /// The handwriting window: toolbar over a canvas that fills the rest.
    fn ink_view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        let (live_options, live_color) = self.ink.live_style();

        column![
            views::ink::toolbar(views::ink::ToolbarState {
                tool: self.ink.tool,
                eraser_mode: self.ink.eraser_mode,
                paper: self.ink.document.paper,
                size: if self.ink.tool == crate::ink::Tool::Highlighter {
                    self.ink.highlighter_size
                } else {
                    self.ink.options.size
                },
                zoom: self.ink.camera.zoom,
                can_undo: self.ink.document.can_undo(),
                can_redo: self.ink.document.can_redo(),
                has_strokes: !self.ink.document.is_empty(),
                has_selection: self.ink.has_selection(),
                dirty: self.ink.dirty,
                path: self.ink.path.as_deref(),
                pen_available: crate::ink::pen::is_available(),
            }),
            row![
                container(crate::ink::canvas::view(crate::ink::canvas::InkSurface {
                    document: &self.ink.document,
                    live: self.ink.live_points(),
                    lasso: self.ink.lasso(),
                    selection: self.ink.selection(),
                    selection_bounds: self.ink.selection_bounds(),
                    cache: &self.ink.cache,
                    camera: self.ink.camera,
                    options: self.ink.options,
                    live_options,
                    color: live_color,
                    tool: self.ink.tool,
                    eraser_radius: self.ink.eraser_screen_radius(),
                    drawing: self.ink.is_drawing(),
                    hover: self.ink.hover(),
                    radial: self.ink.radial_menu(),
                }))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(app_theme::BG_PRIMARY)),
                    ..Default::default()
                }),
                // Its own column rather than an overlay: the canvas claims all
                // pen input within its bounds, so a panel drawn on top of it
                // could never be pressed with the pen.
                views::ink::color_palette(
                    if self.ink.tool == crate::ink::Tool::Highlighter {
                        self.ink.highlighter_color
                    } else {
                        self.ink.color
                    }
                ),
            ]
            .height(Length::Fill),
        ]
        .height(Length::Fill)
        .into()
    }

    fn shell_view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        if self.vault.root.is_none() {
            return views::welcome::view();
        }

        let toolbar = views::toolbar::view(
            self.active_path.as_deref(),
            self.pdf
                .active_path
                .as_deref()
                .or(self.active_image_path.as_deref())
                .or(self.pptx.active_path.as_deref()),
            self.editor.buffer.dirty,
            self.vault.sidebar_visible,
            self.vault.backlinks_visible,
            self.graph.visible,
            self.tracker.visible,
            self.editor.toc_visible,
            self.pdf.active_path.is_some()
                && (self.showing_pdf || (self.ui.split_view_active && self.active_path.is_some())),
            self.ui.split_view_active,
            self.active_path.is_some(),
            self.ink.is_visible(),
        );

        // Panels are clipped to an animated width rather than swapped between
        // "full" and "absent", so opening and closing them reads as movement.
        // The content keeps its natural width throughout; only the window onto
        // it changes, which avoids re-laying-out the tree on every frame.
        let sidebar_width = self.motion.sidebar_width();
        let sidebar: Element<Message, Theme, iced::Renderer> = container(views::sidebar::view(
            &self.vault.entries,
            self.vault.selected_path.as_deref(),
            self.active_path
                .as_deref()
                .or(self.pdf.active_path.as_deref())
                .or(self.active_image_path.as_deref())
                .or(self.pptx.active_path.as_deref()),
            &self.vault.expanded_folders,
            sidebar_width < 1.0,
        ))
        .width(Length::Fixed(sidebar_width))
        .clip(true)
        .into();

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
                    &self.editor.highlight.lines,
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
                )
                .scale_factor(self.ui.scale_factor)
                .reveal_caret(self.editor.reveal_request, Message::EditorCaretView)
                .layout_revision(self.editor.layout_revision()),
            )
            .padding(EDITOR_PADDING)
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
            } else {
                container(Space::new())
                    .height(Length::Fixed(0.0))
                    .width(Length::Fill)
                    .into()
            };
            column![file_bar, editor_scroll].height(Length::Fill).into()
        };

        let pdf_view: Element<Message, Theme, iced::Renderer> = if self.pdf.active_path.is_some() {
            let focused_ann = self.pdf.focused_annotation_id.as_ref().and_then(|ann_id| {
                self.pdf
                    .annotations
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
                self.pdf.annotations.values().any(|v| !v.is_empty()),
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
                self.search.match_index,
                &self.pdf.page_text,
                &self.pdf.annotations,
                &self.pdf.references,
                self.pdf.selection,
                self.pdf.focused_annotation_id.as_deref(),
                self.pdf.load_error.as_deref(),
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
                    self.search.loose,
                    self.search.pdf_results.len(),
                    self.search.match_index,
                )
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
        let toc_width = self.motion.toc_width();
        let toc_view: Element<Message, Theme, iced::Renderer> =
            if toc_width >= 1.0 && pdf_toc_available {
                container(views::toc::view(
                    &self.pdf.toc_entries,
                    self.pdf.toc_is_synthetic,
                ))
                .width(Length::Fixed(toc_width))
                .clip(true)
                .into()
            } else {
                container(Space::new()).width(Length::Fixed(0.0)).into()
            };

        let image_view: Element<Message, Theme, iced::Renderer> =
            if let Some((handle, width, height)) = &self.active_image {
                let label = self.active_image_path.as_deref().unwrap_or("Image");
                container(
                    column![
                        text(label)
                            .size(app_theme::TEXT_BASE)
                            .color(app_theme::TEXT_MUTED),
                        iced::widget::image(handle.clone())
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .content_fit(iced::ContentFit::Contain),
                        text(format!("{:.0} x {:.0}", width, height))
                            .size(app_theme::TEXT_SM)
                            .color(app_theme::TEXT_MUTED),
                    ]
                    .spacing(app_theme::SPACE_4)
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

        let main_content: Element<Message, Theme, iced::Renderer> = if self.graph.visible {
            views::graph::view(&self.graph)
        } else if self.ui.split_view_active
            && self.active_path.is_some()
            && self.pdf.active_path.is_some()
        {
            let left_portion = (self.ui.split_ratio * 1000.0) as u16;
            let right_portion = ((1.0 - self.ui.split_ratio) * 1000.0) as u16;

            let divider = mouse_area(
                container(
                    text("⋮")
                        .size(app_theme::TEXT_BASE)
                        .color(app_theme::TEXT_MUTED),
                )
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
        } else if self.pptx.active_path.is_some() {
            views::pptx_viewer::view(&self.pptx, self.pdf_available_width())
        } else if self.active_image.is_some() {
            image_view
        } else {
            editor_view
        };

        let content = column![toolbar, main_content].height(Length::Fill);

        let backlinks_width = self.motion.backlinks_width();
        let backlinks_view: Element<Message, Theme, iced::Renderer> = container(
            views::backlinks::view(&self.vault.backlinks, backlinks_width >= 1.0 && !self.graph.visible),
        )
        .width(Length::Fixed(backlinks_width))
        .clip(true)
        .into();

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
                    self.search.results_truncated,
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

        // Kept mounted through the fade-out, so dismissing the palette is a
        // transition rather than a cut.
        let palette_opacity = self.motion.palette_opacity();
        if palette_opacity > 0.01 {
            layers.push(
                container(views::command_palette::view(
                    &self.ui.command_palette_query,
                    &self.ui.commands,
                    &self.vault.entries,
                    self.ui.palette_selected,
                    palette_opacity,
                ))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgba(
                        0.0,
                        0.0,
                        0.0,
                        0.58 * palette_opacity,
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
            // Size the box to the preview's aspect ratio, as large as fits in
            // ~85% of the window. Matching the aspect means the image fills the
            // box (no letterboxing) and the destination — centered in the
            // crop — sits centered in the modal, as large as the window allows.
            let (img_w, img_h) = self.pdf.link_preview_size.unwrap_or((800.0, 600.0));
            let aspect = (img_w / img_h).max(0.1);
            let max_w = self.preview_max_box_width();
            let max_h = (self.ui.window_height * 0.85).clamp(360.0, 1000.0);
            let mut preview_box_w = max_w;
            let mut preview_box_h = preview_box_w / aspect;
            if preview_box_h > max_h {
                preview_box_h = max_h;
                preview_box_w = preview_box_h * aspect;
            }

            let img = iced::widget::image(preview_handle.clone())
                .filter_method(iced::widget::image::FilterMethod::Linear)
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(iced::ContentFit::Contain);

            let modal = container(
                iced::widget::column![
                    iced::widget::row![
                        Space::new().width(Length::Fill),
                        iced::widget::button("✕")
                            .on_press(Message::ClosePdfLinkPreview)
                            .padding(app_theme::SPACE_3)
                    ],
                    container(img)
                        .width(Length::Fixed(preview_box_w))
                        .height(Length::Fixed(preview_box_h))
                        .style(|_| container::Style {
                            background: Some(iced::Background::Color(iced::Color::WHITE)),
                            border: iced::Border {
                                radius: app_theme::RADIUS_MD.into(),
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

        // Driven by the animation, not by `ui.toast` directly, so the toast
        // survives long enough to fade out after the message is cleared.
        let toast_opacity = self.motion.toast_opacity();
        if toast_opacity > 0.01 {
            layers.push(
                container(views::toast::view(&self.motion.toast_text, toast_opacity))
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

    fn invalidate_graph(&mut self) {
        self.graph.mark_dirty();
    }

    fn open_vault(&mut self, path: &str) -> Task<Message> {
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
            async move {
                md_editor_core::vault::index_vault_for_generation(&state, &path, generation)
            },
            move |result| Message::VaultIndexed(generation, result_path.clone(), result),
        )
    }

    fn new_entry_path(&self, name: &str) -> String {
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
    fn new_entry_parent(
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

    fn open_file(&mut self, path: &str) -> Task<Message> {
        self.open_file_extended(path, true)
    }

    /// Write the active markdown document to disk if it has unsaved edits.
    ///
    /// The single funnel for every write: `Ctrl+S`, the autosave debounce, and
    /// the flush before a file switch or quit all land here, so durability is
    /// decided in one place rather than at each call site.
    fn save_active_document(&mut self) -> SaveOutcome {
        if !self.editor.buffer.dirty {
            self.editor.autosave_pending_since = None;
            return SaveOutcome::NothingToDo;
        }

        // No document is open (an image or PDF is showing), so there is nothing
        // this buffer belongs to. Disarm, or the autosave subscription would
        // poll forever on a pending flag that can never be satisfied.
        let Some(path) = self.active_path.clone() else {
            self.editor.autosave_pending_since = None;
            return SaveOutcome::NothingToDo;
        };

        let content = self.editor.buffer.text();
        match md_editor_core::vault::save_file(&self.state, &path, &content) {
            Ok(()) => {
                self.editor.buffer.mark_saved();
                self.editor.autosave_pending_since = None;
                self.invalidate_graph();
                self.autosave_failure_reported = false;
                self.note_self_write(&path);
                self.persist_document_position();
                SaveOutcome::Saved
            }
            Err(err) => {
                // Leave `dirty` set — the edits are still only in memory, so
                // the title keeps its unsaved marker. Restart the debounce
                // rather than leaving it elapsed: otherwise every poll tick
                // would retry a failing write (a full create/write/fsync/rename
                // attempt) ten times a second for the rest of the session.
                self.editor.mark_dirty_now();
                SaveOutcome::Failed(err)
            }
        }
    }

    /// Save and park the active document before something replaces it.
    ///
    /// Every path that changes what the editor is showing — opening another
    /// note, an image, or clearing the pane — goes through here, so unsaved
    /// edits and undo history cannot be dropped by a code path that forgot to
    /// think about them. Returns the write error if the document could not be
    /// persisted, in which case the caller must not proceed.
    fn leave_active_document(&mut self) -> Result<(), String> {
        if let SaveOutcome::Failed(err) = self.save_active_document() {
            return Err(err);
        }
        self.park_active_buffer();
        Ok(())
    }

    /// Remember that we just wrote this path, so the vault watcher can ignore
    /// the change notification our own save is about to produce.
    fn note_self_write(&mut self, path: &str) {
        let now = std::time::Instant::now();
        self.self_writes
            .retain(|_, at| now.duration_since(*at) < SELF_WRITE_GRACE);
        self.self_writes.insert(path.to_string(), now);
    }

    /// Whether `path` was written by this app moments ago.
    fn is_recent_self_write(&self, path: &str) -> bool {
        self.self_writes
            .get(path)
            .is_some_and(|at| at.elapsed() < SELF_WRITE_GRACE)
    }

    /// Record where this session left off: window geometry, and the scroll and
    /// cursor position of the open document.
    ///
    /// Written once on exit rather than on every resize or scroll event, so
    /// dragging a window edge or flicking through a long note does not turn
    /// into a stream of database writes.
    fn persist_session(&self) {
        self.persist_window_size();
        self.persist_document_position();
    }

    /// Record the window geometry. Called as the window resizes rather than
    /// only at exit, because the app does not intercept its own close and so
    /// cannot rely on running code on the way out.
    fn persist_window_size(&self) {
        let _ = md_editor_core::config::set_sys_config(
            &self.state,
            "window_size",
            &format!("{}x{}", self.ui.window_width, self.ui.window_height),
        );
    }

    /// Record where the reader is in the active document. Rides along with
    /// each save, so it is written on the same schedule as the content it
    /// describes.
    fn persist_document_position(&self) {
        let Some(path) = self.active_path.as_deref() else {
            return;
        };
        let _ = md_editor_core::config::set_sys_config(
            &self.state,
            &format!("scroll:{path}"),
            &self.editor.scroll_y.to_string(),
        );
        let _ = md_editor_core::config::set_sys_config(
            &self.state,
            &format!("cursor:{path}"),
            &self.editor.buffer.cursor_offset().to_string(),
        );
    }

    /// Scroll offset stored for `path` by a previous session, if any.
    fn stored_scroll_for(&self, path: &str) -> Option<f32> {
        md_editor_core::config::get_sys_config(&self.state, &format!("scroll:{path}"))
            .ok()
            .flatten()
            .and_then(|raw| raw.parse::<f32>().ok())
            .filter(|y| y.is_finite() && *y >= 0.0)
    }

    /// Cursor offset stored for `path` by a previous session, if any.
    fn stored_cursor_for(&self, path: &str) -> Option<usize> {
        md_editor_core::config::get_sys_config(&self.state, &format!("cursor:{path}"))
            .ok()
            .flatten()
            .and_then(|raw| raw.parse::<usize>().ok())
    }

    /// Move the active buffer into the retained registry so a later visit to
    /// the same file resumes its undo history.
    fn park_active_buffer(&mut self) {
        let Some(path) = self.active_path.clone() else {
            return;
        };
        let outgoing = std::mem::replace(&mut self.editor.buffer, DocBuffer::new());
        let scroll_y = self.editor.scroll_y;
        self.editor.retain_buffer(path, outgoing, scroll_y);
    }

    /// Resolve an internal link target (wikilink / relative markdown link) to a
    /// vault file path. Tries the existing relative-path resolution first, then
    /// falls back to a vault-wide basename lookup so `[[NoteName]]` resolves to
    /// a note living in any subfolder.
    fn resolve_internal_link_path(&self, link_path: &str) -> String {
        let mut resolved = resolve_relative_link_path(
            self.vault.root.as_deref(),
            self.active_path.as_deref(),
            link_path,
        );
        if !md_editor_core::vault::is_supported_vault_path(std::path::Path::new(&resolved)) {
            resolved.push_str(".md");
        }
        let exists = self
            .vault
            .root
            .as_deref()
            .map(|root| std::path::Path::new(root).join(&resolved).exists())
            .unwrap_or(false);
        if !exists && let Some(found) = resolve_vault_note_by_name(&self.vault.entries, link_path) {
            return found;
        }
        resolved
    }

    fn open_file_extended(&mut self, path: &str, reset_scroll: bool) -> Task<Message> {
        let is_different = self.active_path.as_deref() != Some(path);

        // Read the incoming file *first*. Parking the outgoing buffer before
        // knowing the open will succeed would leave an empty buffer bound to
        // the still-active path, and the next autosave would write that empty
        // buffer over the previous note.
        let incoming = match md_editor_core::vault::open_file(&self.state, path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(content) => content,
                Err(_) => {
                    self.ui.toast = Some(format!(
                        "Cannot open {path}: not valid UTF-8 text (unsupported encoding?)"
                    ));
                    return Task::none();
                }
            },
            Err(err) => {
                self.ui.toast = Some(format!("Cannot open {path}: {err}"));
                return Task::none();
            }
        };

        // Only now is it safe to flush and park what is currently open.
        // Without this, switching files silently discarded both the unsaved
        // edits and the undo stack that could have recovered them.
        if is_different && let Err(err) = self.leave_active_document() {
            // Refuse to navigate away from work we could not persist.
            self.ui.toast = Some(format!(
                "Not switching files: {err}. Your edits are still open here."
            ));
            return Task::none();
        }

        let content = incoming;
        // Resume the parked buffer when it still matches disk, so
        // undo history and cursor survive a round trip; otherwise
        // the file changed underneath us and a fresh buffer is the
        // only honest option.
        let resumed = self.editor.take_retained_matching(path, &content);
        let resumed_scroll = resumed.as_ref().map(|(_, y)| *y);
        let is_resumed = resumed.is_some();
        self.editor.buffer = resumed
            .map(|(buffer, _)| buffer)
            .unwrap_or_else(|| DocBuffer::from_text(&content));

        // A buffer built fresh from disk starts at the top. If a
        // previous session left a cursor here, put it back.
        if !is_resumed && let Some(offset) = self.stored_cursor_for(path) {
            self.editor.buffer.set_cursor_offset(offset);
        }
        self.editor.autosave_pending_since = None;
        self.editor.buffer_revision = self.editor.buffer_revision.wrapping_add(1);
        self.active_path = Some(path.to_string());
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
        self.active_image_path = None;
        self.active_image = None;
        self.pptx.close();
        self.showing_pdf = false;
        self.active_panel = ActivePanel::Markdown;
        self.editor.toc_entries = views::toc::get_toc(&content);
        self.editor.toc_is_synthetic = false;
        let highlight_task = self.highlight_all();
        self.vault.backlinks =
            md_editor_core::vault::get_mixed_backlinks(&self.state, path).unwrap_or_default();
        if is_different && reset_scroll {
            // Return to where this file was last left, rather than
            // always to the top — finding your place again by hand
            // in a long note is the whole cost this avoids.
            let target_y = resumed_scroll
                .or_else(|| self.stored_scroll_for(path))
                .unwrap_or(0.0);
            self.editor.scroll_y = target_y;
            self.motion.editor_scroll = None;
            let scroll_task = operation::scroll_to(
                iced::advanced::widget::Id::new(EDITOR_SCROLLABLE_ID),
                AbsoluteOffset {
                    x: 0.0,
                    y: target_y,
                },
            );
            return Task::batch(vec![highlight_task, scroll_task]);
        }
        highlight_task
    }

    fn open_pdf(&mut self, path: &str) -> Task<Message> {
        let Some(abs_path) = self.resolve_active_path(path) else {
            self.ui.toast = Some("Open a vault before opening a PDF".to_string());
            return Task::none();
        };
        let path_str = abs_path.to_string_lossy().to_string();
        self.pdf.active_path = Some(path.to_string());
        self.graph.set_active_path(Some(path.to_string()));
        self.pdf.load_error = None;
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
        self.active_image_path = None;
        self.active_image = None;
        self.pptx.close();
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
        self.pdf.references.clear();
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
        self.pdf.toc_entries.clear();
        self.pdf.toc_is_synthetic = false;
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
            move |result| Message::PdfDocumentIdComputed(generation, result),
        );

        let _state = self.state.clone();
        let _state_toc = self.state.clone();
        let _state_sizes = self.state.clone();
        let path_clone = path_str.clone();
        let path_str_toc = path_str.clone();
        let path_for_toc = path.to_string();
        let path_for_sizes = path.to_string();
        let path_str_sizes = path_str.clone();

        Task::batch(vec![
            hash_task,
            Task::perform(
                async move {
                    let renderer = _state
                        .pdf_renderer
                        .as_ref()
                        .ok_or_else(|| "PDF renderer is unavailable".to_string())?;
                    renderer.page_count(&path_clone)
                },
                move |res| Message::PdfLoaded(generation, res),
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
                move |res| {
                    let (entries, synthetic) = res.unwrap_or_default();
                    Message::PdfTocLoaded(path_for_toc.clone(), entries, synthetic)
                },
            ),
        ])
    }

    /// Show a presentation in the viewer. Like an image, a presentation takes
    /// the pane from the open note, so the note is flushed and parked first.
    fn open_pptx(&mut self, path: &str) -> Task<Message> {
        if self.vault.root.is_none() {
            self.ui.toast = Some("Open a vault before opening a presentation".to_string());
            return Task::none();
        }
        if let Err(err) = self.leave_active_document() {
            self.ui.toast = Some(format!(
                "Not opening the presentation: {err}. Your edits are still open."
            ));
            return Task::none();
        }
        self.active_path = None;
        self.pdf.active_path = None;
        self.showing_pdf = false;
        self.active_image_path = None;
        self.active_image = None;
        self.active_panel = ActivePanel::Markdown;
        self.editor.toc_entries.clear();
        self.editor.toc_is_synthetic = false;
        self.pdf.toc_entries.clear();
        self.pdf.toc_is_synthetic = false;
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
        self.vault.backlinks =
            md_editor_core::vault::get_mixed_backlinks(&self.state, path).unwrap_or_default();
        self.load_pptx(path)
    }

    /// Read and lay out the presentation at `path` off the UI thread.
    fn load_pptx(&mut self, path: &str) -> Task<Message> {
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        let generation = self.pptx.begin_load(path);
        Task::perform(
            async move { crate::slides::load_deck(&abs_path).map(Arc::new) },
            move |result| Message::PptxLoaded(generation, result),
        )
    }

    fn open_image(&mut self, path: &str) -> Task<Message> {
        let Some(abs_path) = self.resolve_active_path(path) else {
            self.ui.toast = Some("Open a vault before opening an image".to_string());
            return Task::none();
        };

        match image::open(&abs_path) {
            Ok(img) => {
                // Showing an image clears `active_path`, which detaches the
                // open note from the buffer holding it. Flush and park it
                // first, or those edits have nowhere left to be written to.
                if let Err(err) = self.leave_active_document() {
                    self.ui.toast = Some(format!(
                        "Not opening the image: {err}. Your edits are still open."
                    ));
                    return Task::none();
                }

                let (width, height) = img.dimensions();
                let handle = iced::widget::image::Handle::from_rgba(
                    width,
                    height,
                    img.into_rgba8().into_raw(),
                );
                self.active_image_path = Some(path.to_string());
                self.graph.set_active_path(None);
                let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
                self.active_image = Some((handle, width as f32, height as f32));
                self.pdf.active_path = None;
                self.pptx.close();
                self.showing_pdf = false;
                self.active_panel = ActivePanel::Markdown;
                self.editor.toc_entries.clear();
                self.editor.toc_is_synthetic = false;
                self.pdf.toc_entries.clear();
                self.pdf.toc_is_synthetic = false;
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
        let zoom = md_editor_core::pdf::pdf_render_bucket(self.pdf.zoom) * self.pdf_supersample();
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
        let zoom = md_editor_core::pdf::pdf_render_bucket(self.pdf.zoom) * self.pdf_supersample();
        let generation = self.pdf.render_generation;
        let _state = self.state.clone();
        if self
            .pdf
            .pages
            .get(page as usize)
            .is_none_or(|p| p.is_none())
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

        if let Some(path) = &self.pdf.active_path
            && let Some(abs_path) = self.resolve_active_path(path)
        {
            let path_str = abs_path.to_string_lossy().to_string();
            if let Some(renderer) = self.state.pdf_renderer.as_ref() {
                renderer.set_visible_range(first_visible, last_visible, &path_str);
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

        if let Some(path) = &self.pdf.active_path
            && let Some(abs_path) = self.resolve_active_path(path)
        {
            let path_str = abs_path.to_string_lossy().to_string();
            if let Some(renderer) = self.state.pdf_renderer.as_ref() {
                renderer.set_visible_range(first_visible, last_visible, &path_str);
            }
        }

        let first = self
            .pdf
            .page_at_scroll((scroll_y - self.pdf.estimated_page_height()).max(0.0));
        let last = self
            .pdf
            .page_at_scroll(scroll_y + viewport_height + self.pdf.estimated_page_height());
        self.render_pdf_page_range(
            first.saturating_sub(2),
            (last + 2).min(self.pdf.total_pages.saturating_sub(1)),
        )
    }

    fn render_pdf_page_range(&mut self, start: u16, end: u16) -> Task<Message> {
        let mut tasks = Vec::new();
        for page_idx in start..=end {
            if self
                .pdf
                .pages
                .get(page_idx as usize)
                .is_none_or(|p| p.is_none())
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

        // Free bitmaps well outside the rendered window so a long PDF doesn't
        // keep every page resident in memory (each page is a full RGBA bitmap,
        // and supersampling multiplies that cost). Layout metadata in
        // `dimensions`/`page_sizes` is cheap and kept, so geometry/link
        // hit-testing still work; an evicted page simply re-renders when it
        // scrolls back into view.
        self.evict_distant_page_bitmaps(start, end);

        Task::batch(tasks)
    }

    /// Drop cached page bitmaps further than [`PDF_PAGE_BITMAP_EVICT_MARGIN`]
    /// from the `[start, end]` render window. The margin gives scroll
    /// hysteresis so small back-and-forth movements reuse cached pages.
    fn evict_distant_page_bitmaps(&mut self, start: u16, end: u16) {
        let keep_lo = start.saturating_sub(PDF_PAGE_BITMAP_EVICT_MARGIN);
        let keep_hi = end.saturating_add(PDF_PAGE_BITMAP_EVICT_MARGIN);
        for (idx, slot) in self.pdf.pages.iter_mut().enumerate() {
            let idx = idx as u16;
            if (idx < keep_lo || idx > keep_hi) && slot.is_some() {
                *slot = None;
            }
        }
    }

    /// Supersampling factor for PDF rasterization. Renders at the display's
    /// device-pixel ratio so pages are pixel-sharp on HiDPI/fractional displays
    /// while staying 1:1 (no wasted work) on standard 1× screens. With zoom
    /// bucketing always rounding up, this guarantees the rasterized bitmap is
    /// never undersampled relative to the physical pixels it lands on.
    fn pdf_supersample(&self) -> f32 {
        self.ui.scale_factor.clamp(1.0, PDF_SUPERSAMPLE_MAX)
    }

    /// Re-fit the PDF to the available width when fit-to-width is active and a
    /// PDF is open. Call after any change that alters the PDF pane's width (TOC,
    /// sidebar, split view) so pages re-render at the correct scale instead of
    /// being stretched from a stale zoom.
    /// Logical width the link-preview modal box is sized to (width-limited for
    /// the typical landscape crop). Shared by the render request and the view so
    /// the rendered bitmap matches the on-screen box.
    fn preview_max_box_width(&self) -> f32 {
        (self.ui.window_width * 0.85).clamp(480.0, 1400.0)
    }

    /// Physical pixel width to rasterize the link preview at: the logical box
    /// width times the display scale factor, so the enlarged preview renders
    /// pixel-sharp. Capped to bound the render cost.
    fn preview_target_px(&self) -> u32 {
        (self.preview_max_box_width() * self.ui.scale_factor).clamp(600.0, 4000.0) as u32
    }

    fn refit_pdf_if_needed(&self) -> Task<Message> {
        if self.pdf.fit_to_width && self.pdf.active_path.is_some() {
            Task::done(Message::PdfFitToWidth)
        } else {
            Task::none()
        }
    }

    fn pdf_available_width(&self) -> f32 {
        let sidebar_width = if self.vault.sidebar_visible {
            260.0
        } else {
            0.0
        };
        let toc_width = if self.editor.toc_visible { 260.0 } else { 0.0 };
        let backlinks_width = if self.vault.backlinks_visible {
            260.0
        } else {
            0.0
        };
        let chrome_width = sidebar_width + toc_width + backlinks_width;
        let content_width = (self.ui.window_width - chrome_width).max(320.0);

        if self.ui.split_view_active && self.active_path.is_some() && self.pdf.active_path.is_some()
        {
            (content_width * (1.0 - self.ui.split_ratio)).max(280.0)
        } else {
            content_width
        }
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
            .pdf
            .active_path
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

    /// Re-highlight the buffer and load the images and math it references.
    fn highlight_all(&mut self) -> Task<Message> {
        self.editor.refresh_highlighting();
        self.load_editor_resources()
    }

    /// Load images and math for the freshly highlighted lines. Images resolve
    /// relative to the active document, so the vault root + active path are
    /// supplied from the shell.
    fn load_editor_resources(&mut self) -> Task<Message> {
        if let (Some(root), Some(path)) = (self.vault.root.clone(), self.active_path.clone()) {
            self.editor.load_images(&root, &path);
        }
        self.editor.load_math(self.ui.scale_factor)
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
            affinity: Affinity::Downstream,
        });
        self.editor.request_reveal(Reveal::Center);
        Task::none()
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
        if let Some(path) = &self.pdf.active_path
            && let Some(abs_path) = self.resolve_active_path(path)
        {
            let path_str = abs_path.to_string_lossy().to_string();
            if let Some(renderer) = self.state.pdf_renderer.as_ref() {
                renderer.set_visible_range(
                    target_page.saturating_sub(1),
                    (target_page + 1).min(self.pdf.total_pages.saturating_sub(1)),
                    &path_str,
                );
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
        // No generation bump here: the document and zoom bucket are unchanged,
        // so in-flight results are still valid. Bumping would race the initial
        // PdfLoaded/PdfTocLoaded tasks (tagged with the open generation) and
        // silently drop them when a PDF is opened with a target page.
        self.pdf.toc_target_page = Some(target_page);

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
            self.editor.buffer.replace_all_text(&new_text);
            self.editor.buffer_revision = self.editor.buffer_revision.wrapping_add(1);
            // Replace-all is an edit like any other: it arms autosave, so the
            // result reaches disk without waiting for an explicit save.
            self.editor.mark_dirty_now();
            self.editor.toc_entries = views::toc::get_toc(&self.editor.buffer.text());
            let task = self.highlight_all();
            return Ok((count, task));
        }
        Ok((count, Task::none()))
    }

    /// Create a highlight annotation over the current PDF text selection.
    /// Shared by the explicit color buttons and the cycling quick highlight.
    fn create_highlight(
        &mut self,
        color: md_editor_core::pdf::PdfAnnotationColor,
    ) -> Task<Message> {
        if let (Some(sel), Some(doc_id)) = (&self.pdf.selection, &self.pdf.document_id)
            && let Some(page_text) = self.pdf.page_text.get(&sel.page_index)
        {
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
                self.pdf
                    .annotations
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
        Task::none()
    }

    /// Build a short report of "orphan" annotations: highlights whose stored
    /// `selected_text` no longer appears in the current text of their page,
    /// which means the underlying PDF text has drifted away from the anchor.
    fn pdf_orphan_report(&self) -> String {
        let mut total = 0usize;
        let mut orphans = 0usize;
        let mut checked = 0usize;
        for (page_index, anns) in &self.pdf.annotations {
            let page_text = self.pdf.page_text.get(page_index);
            for ann in anns {
                total += 1;
                let needle = ann.selected_text.trim();
                if needle.is_empty() {
                    continue;
                }
                // Only pages whose text has been loaded can be checked.
                if let Some(pt) = page_text {
                    checked += 1;
                    if !pt.text.contains(needle) {
                        orphans += 1;
                    }
                }
            }
        }
        if checked == 0 {
            format!(
                "Orphan report: 0 of {total} annotations checkable (scroll pages to load text first)"
            )
        } else if orphans == 0 {
            format!("Orphan report: no drift found ({checked} of {total} checked)")
        } else {
            format!("Orphan report: {orphans} drifted of {checked} checked ({total} total)")
        }
    }

    fn search_pdf(&self) -> Task<Message> {
        let Some(path) = &self.pdf.active_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        // Loose whitespace: build a regex that allows any whitespace run
        // (including PDF line breaks) between the query's tokens, so a phrase
        // wrapping across lines still matches. Reuses the existing regex path.
        let (query, regex) = if self.search.loose {
            let pattern = self
                .search
                .query
                .split_whitespace()
                .map(regex::escape)
                .collect::<Vec<_>>()
                .join(r"\s+");
            (pattern, true)
        } else {
            (self.search.query.clone(), self.search.regex)
        };
        let match_case = self.search.match_case;
        let _state = self.state.clone();
        let path_str = abs_path.to_string_lossy().to_string();
        let generation = self.pdf.render_generation;
        Task::perform(
            async move {
                let Some(renderer) = _state.pdf_renderer.as_ref() else {
                    return Err(
                        "PDF search is unavailable because PDFium is not loaded".to_string()
                    );
                };
                renderer.search_text(&path_str, &query, regex, match_case)
            },
            move |result| Message::PdfSearchResult(generation, result),
        )
    }

    /// Scroll the editor the way the answered reveal request asked, now that
    /// the widget has said exactly where the caret is.
    fn reveal_caret(&mut self, view: CaretView) -> Task<Message> {
        let Some(reveal) = self.editor.take_reveal(view.request) else {
            return Task::none();
        };
        let Some(top) = view.viewport_top_for(reveal) else {
            return Task::none();
        };
        // The widget sits inside the scrollable's padding: a widget-relative y
        // plus the padding is a scroll offset.
        let current = view.viewport_top + EDITOR_PADDING;
        let max = (view.height + 2.0 * EDITOR_PADDING - view.viewport_height).max(0.0);
        let target = (top + EDITOR_PADDING).clamp(0.0, max);
        if (target - current).abs() < 0.5 {
            return Task::none();
        }

        let now = std::time::Instant::now();
        let mut spring = match self.motion.editor_scroll {
            // Still gliding from where the page really is: keep the momentum.
            Some(glide) if (glide.applied - current).abs() < 1.0 => glide.spring,
            _ => Spring::at_rest(current, now, SCROLL_STIFFNESS),
        };
        // A far jump cuts to a screen short of the target and glides the rest.
        let reach = view.viewport_height.max(1.0);
        let mut applied = current;
        let mut task = Task::none();
        if (target - spring.value(now)).abs() > 2.0 * reach {
            applied = target - reach.copysign(target - current);
            spring = Spring::at_rest(applied, now, SCROLL_STIFFNESS);
            task = scroll_editor_to(applied);
        }
        spring.retarget(target, now);
        self.motion.editor_scroll = Some(ScrollGlide { spring, applied });
        self.motion.now = now;
        task
    }

    /// Move an animated editor scroll on to the frame drawn at `now`.
    fn step_editor_scroll(&mut self, now: std::time::Instant) -> Task<Message> {
        let Some(glide) = self.motion.editor_scroll else {
            return Task::none();
        };
        let y = if glide.spring.is_settled(now) {
            self.motion.editor_scroll = None;
            glide.spring.target()
        } else {
            let y = glide.spring.value(now);
            self.motion.editor_scroll = Some(ScrollGlide {
                applied: y,
                ..glide
            });
            y
        };
        scroll_editor_to(y)
    }

    fn run_editor_command(&mut self, command: EditorCommand) -> Task<Message> {
        let keep_cursor_visible = editor_command_keeps_cursor_visible(&command);
        self.run_editor_command_with_scroll(command, keep_cursor_visible)
    }

    fn restore_layout(&mut self) {
        let get = |key| {
            md_editor_core::config::get_sys_config(&self.state, key)
                .ok()
                .flatten()
        };
        self.vault.sidebar_visible = get("layout_sidebar_visible")
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.vault.sidebar_visible);
        self.vault.backlinks_visible = get("layout_backlinks_visible")
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.vault.backlinks_visible);
        self.editor.toc_visible = get("layout_toc_visible")
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.editor.toc_visible);
        self.tracker.visible = get("layout_tracker_visible")
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.tracker.visible);
        self.ui.split_view_active = get("layout_split_active")
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.ui.split_view_active);
        self.ui.split_ratio = get("layout_split_ratio")
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(self.ui.split_ratio)
            .clamp(0.25, 0.75);
    }

    fn persist_layout(&self) {
        let values = [
            ("layout_sidebar_visible", self.vault.sidebar_visible.to_string()),
            ("layout_backlinks_visible", self.vault.backlinks_visible.to_string()),
            ("layout_toc_visible", self.editor.toc_visible.to_string()),
            ("layout_tracker_visible", self.tracker.visible.to_string()),
            ("layout_split_active", self.ui.split_view_active.to_string()),
            ("layout_split_ratio", self.ui.split_ratio.to_string()),
        ];
        for (key, value) in values {
            let _ = md_editor_core::config::set_sys_config(&self.state, key, &value);
        }
    }

    fn run_editor_command_with_scroll(
        &mut self,
        command: EditorCommand,
        keep_cursor_visible: bool,
    ) -> Task<Message> {
        self.active_panel = ActivePanel::Markdown;
        let result = self.editor.buffer.execute(command);
        if result.text_changed {
            self.editor.buffer_revision = self.editor.buffer_revision.wrapping_add(1);
            // Restart the autosave countdown on every edit, so a write happens
            // once typing pauses rather than on each keystroke.
            self.editor.mark_dirty_now();
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
            self.editor.request_reveal(Reveal::Nearest);
        }
        content_task
    }
}

fn scroll_editor_to(y: f32) -> Task<Message> {
    operation::scroll_to(
        iced::advanced::widget::Id::new(EDITOR_SCROLLABLE_ID),
        AbsoluteOffset { x: 0.0, y },
    )
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

fn focus_file_search_input() -> Task<Message> {
    operation::focus(iced::advanced::widget::Id::new(
        views::search::FILE_SEARCH_INPUT_ID,
    ))
}

/// Debounce window for coalescing filesystem-watcher events. Editors and tools
/// often write a file as several rapid operations (write + rename of a temp
/// file); coalescing avoids a re-index storm.
const VAULT_WATCH_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(300);

/// Stream factory for [`iced::Subscription::run_with`]: watches `root`
/// recursively and emits a debounced [`Message::VaultFilesChanged`] listing the
/// vault-relative paths that changed. Must be a plain `fn` (not a closure) to
/// satisfy `run_with`; the root is threaded in via the `&String` argument.
fn vault_watch_stream(
    root: &String,
) -> std::pin::Pin<Box<dyn iced::futures::Stream<Item = Message> + Send>> {
    use iced::futures::SinkExt;
    use notify::{RecursiveMode, Watcher};

    let root = root.clone();
    Box::pin(iced::stream::channel(
        64,
        move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            let root_path = std::path::PathBuf::from(&root);
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<notify::Event>();

            let mut watcher = match notify::recommended_watcher(
                move |res: Result<notify::Event, notify::Error>| {
                    if let Ok(event) = res {
                        let _ = tx.send(event);
                    }
                },
            ) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to create vault watcher: {e}");
                    return;
                }
            };
            if let Err(e) = watcher.watch(&root_path, RecursiveMode::Recursive) {
                eprintln!("Failed to watch vault root {}: {e}", root_path.display());
                return;
            }
            // Hold the watcher for the lifetime of this stream; dropping it stops
            // delivery.
            let _watcher = watcher;

            loop {
                let Some(first) = rx.recv().await else {
                    break; // sender dropped — watcher gone
                };
                let mut changed = std::collections::BTreeSet::new();
                collect_changed_paths(&first, &root_path, &mut changed);
                // Drain the burst within the debounce window.
                while let Ok(Some(event)) =
                    tokio::time::timeout(VAULT_WATCH_DEBOUNCE, rx.recv()).await
                {
                    collect_changed_paths(&event, &root_path, &mut changed);
                }
                if !changed.is_empty() {
                    let paths: Vec<String> = changed.into_iter().collect();
                    if output
                        .send(Message::VaultFilesChanged(paths))
                        .await
                        .is_err()
                    {
                        break; // receiver dropped — subscription ended
                    }
                }
            }
        },
    ))
}

/// Collect the vault-relative paths touched by a watcher `event`, skipping
/// access-only events, dotfiles, and excluded directories.
fn collect_changed_paths(
    event: &notify::Event,
    root: &std::path::Path,
    out: &mut std::collections::BTreeSet<String>,
) {
    use notify::EventKind;
    if matches!(event.kind, EventKind::Access(_) | EventKind::Other) {
        return;
    }
    for path in &event.paths {
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        // Ignore dotfiles/dot-dirs and well-known heavy directories.
        let skip = rel.components().any(|c| {
            let s = c.as_os_str().to_string_lossy();
            s.starts_with('.')
                || matches!(
                    s.as_ref(),
                    "node_modules" | "target" | "build" | "dist" | "__pycache__"
                )
        });
        if skip {
            continue;
        }
        out.insert(rel.to_string_lossy().replace('\\', "/"));
    }
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

/// Resolve a path passed on the command line to `(vault_root, vault_relative_file)`.
/// A directory becomes the vault itself; a file inside `last_vault` reuses that
/// vault; any other file uses its parent directory as the vault.
fn resolve_startup_target(
    path: &std::path::Path,
    last_vault: Option<&str>,
) -> Option<(String, Option<String>)> {
    // `std::path::absolute` (not `canonicalize`) to avoid `\\?\` verbatim
    // prefixes on Windows, which would leak into stored vault paths.
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

/// A vault path with its extension removed.
///
/// Only a dot in the final component counts: a directory like `course-v1.2/`
/// must not be truncated.
fn strip_extension(path: &str) -> &str {
    match path.rfind('.').filter(|dot| !path[*dot..].contains('/')) {
        Some(dot) => &path[..dot],
        None => path,
    }
}

/// Vault-relative path for a handwritten page: beside the note it belongs to
/// when one is open, otherwise a timestamped page at the vault root.
fn derive_ink_path(active_path: Option<&str>, timestamp: &str) -> String {
    let extension = crate::ink::INK_EXTENSION;
    match active_path {
        Some(path) => format!("{}.{extension}", strip_extension(path)),
        None => format!("handwritten-{timestamp}.{extension}"),
    }
}

/// Lexically normalize a vault-relative path and render it with forward
/// slashes. Vault-relative paths are forward-slash separated everywhere else
/// (see `vault::path_to_relative_string`), so on Windows the `PathBuf`'s native
/// backslashes have to be converted back or the result stops matching indexed
/// entries, the active path, and the paths stored in the settings database.
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
    // Vault-relative strings use forward slashes everywhere (see core's
    // path_to_relative_string); PathBuf would yield `\` on Windows.
    normalized.to_string_lossy().replace('\\', "/")
}

fn resolve_relative_link_path(
    vault_root: Option<&str>,
    active_path: Option<&str>,
    link_path: &str,
) -> String {
    if link_path.starts_with('.')
        && let Some(active_file) = active_path
    {
        let active_path_buf = std::path::Path::new(active_file);
        if let Some(parent) = active_path_buf.parent() {
            let resolved = parent.join(link_path);
            return normalize_path(&resolved);
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

/// Resolve a bare wikilink name (e.g. `[[NoteName]]`) to a vault file path by
/// matching the link's final component against every file's basename, case-
/// insensitively, with or without a `.md`/`.markdown` extension. When several
/// files match, the shortest path wins (Obsidian-style "closest" resolution);
/// `None` is returned for no match or an ambiguous tie at the same depth.
fn resolve_vault_note_by_name(
    entries: &[md_editor_core::types::FileEntry],
    link_path: &str,
) -> Option<String> {
    // Use only the final path component as the target name.
    let target = link_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(link_path)
        .trim();
    if target.is_empty() {
        return None;
    }
    let target_lower = target.to_lowercase();

    let mut matches: Vec<&str> = Vec::new();
    for entry in entries {
        if entry.is_dir {
            continue;
        }
        let name_lower = entry.name.to_lowercase();
        let stem_lower = name_lower
            .strip_suffix(".md")
            .or_else(|| name_lower.strip_suffix(".markdown"))
            .unwrap_or(&name_lower);
        if name_lower == target_lower || stem_lower == target_lower {
            matches.push(entry.path.as_str());
        }
    }

    matches.sort_by_key(|p| p.len());
    match matches.as_slice() {
        [] => None,
        [only] => Some((*only).to_string()),
        // Prefer a strictly shortest path; bail if the two shortest tie.
        [a, b, ..] if a.len() != b.len() => Some((*a).to_string()),
        _ => None,
    }
}

fn slugify(s: &str) -> String {
    let mut result = String::new();
    let mut last_was_hyphen = false;
    for c in s.to_lowercase().chars() {
        if c.is_alphanumeric() || c == '_' {
            result.push(c);
            last_was_hyphen = false;
        } else if (c.is_whitespace() || c == '-') && !last_was_hyphen {
            result.push('-');
            last_was_hyphen = true;
        }
    }
    result.trim_matches('-').to_string()
}

fn find_heading_line(text: &str, target_slug: &str) -> Option<usize> {
    let mut in_code_block = false;
    for (line_idx, line_content) in text.split('\n').enumerate() {
        let trimmed = line_content.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }
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
    } else {
        target_slug
            .strip_prefix("code-")
            .map(|num_str| format!("listing-{}", num_str))
    };

    for (line_idx, line) in highlighted_lines.iter().enumerate() {
        for span in &line.spans {
            if let Some(ref span_id) = span.id {
                if span_id.eq_ignore_ascii_case(target_slug) {
                    return Some(line_idx);
                }
                if let Some(ref alt) = alternative_slug
                    && span_id.eq_ignore_ascii_case(alt)
                {
                    return Some(line_idx);
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
    fn test_resolve_vault_note_by_name() {
        use md_editor_core::types::FileEntry;
        let entry = |path: &str, name: &str, is_dir: bool| FileEntry {
            path: path.to_string(),
            name: name.to_string(),
            is_dir,
        };
        let entries = vec![
            entry("notes", "notes", true),
            entry("notes/Alpha.md", "Alpha.md", false),
            entry("archive/old/Beta.md", "Beta.md", false),
            entry("Gamma.pdf", "Gamma.pdf", false),
            entry("a/Dup.md", "Dup.md", false),
            entry("deep/nested/Dup.md", "Dup.md", false),
        ];

        // Bare name in a subfolder, case-insensitive, with/without extension.
        assert_eq!(
            resolve_vault_note_by_name(&entries, "Alpha").as_deref(),
            Some("notes/Alpha.md")
        );
        assert_eq!(
            resolve_vault_note_by_name(&entries, "beta").as_deref(),
            Some("archive/old/Beta.md")
        );
        // Non-md extension matches on full name.
        assert_eq!(
            resolve_vault_note_by_name(&entries, "Gamma.pdf").as_deref(),
            Some("Gamma.pdf")
        );
        // Only the final path component is used as the target name.
        assert_eq!(
            resolve_vault_note_by_name(&entries, "folder/Alpha").as_deref(),
            Some("notes/Alpha.md")
        );
        // Shortest path wins when paths differ in length.
        assert_eq!(
            resolve_vault_note_by_name(&entries, "Dup").as_deref(),
            Some("a/Dup.md")
        );
        // No match.
        assert_eq!(resolve_vault_note_by_name(&entries, "Missing"), None);
    }

    #[test]
    fn ink_path_is_derived_from_the_open_note() {
        assert_eq!(
            derive_ink_path(Some("notes/algebra.md"), "20260805-101500"),
            "notes/algebra.ink"
        );
    }

    #[test]
    fn ink_path_falls_back_to_a_timestamp_with_no_note_open() {
        assert_eq!(
            derive_ink_path(None, "20260805-101500"),
            "handwritten-20260805-101500.ink"
        );
    }

    #[test]
    fn ink_path_only_strips_an_extension_from_the_final_component() {
        // A dot in a directory name must not be mistaken for the extension.
        assert_eq!(
            derive_ink_path(Some("course-v1.2/lecture"), "20260805-101500"),
            "course-v1.2/lecture.ink"
        );
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
    fn test_new_entry_parent_follows_only_visible_selection() {
        let entry = |path: &str, is_dir: bool| md_editor_core::types::FileEntry {
            path: path.to_string(),
            name: path.rsplit('/').next().unwrap().to_string(),
            is_dir,
        };
        let entries = vec![
            entry("notes", true),
            entry("notes/a.md", false),
            entry("notes/deep", true),
            entry("root.md", false),
        ];
        let expanded: std::collections::BTreeSet<String> = ["notes".to_string()].into();
        let parent = |selected| MdEditor::new_entry_parent(selected, &entries, &expanded);

        // A visible file inside a folder: new entries go beside it.
        assert_eq!(parent(Some("notes/a.md")), Some("notes".to_string()));
        // A visible folder: new entries go inside it.
        assert_eq!(parent(Some("notes/deep")), Some("notes/deep".to_string()));
        // Root-level selection, or none: vault root.
        assert_eq!(parent(Some("root.md")), None);
        assert_eq!(parent(None), None);
        // A selection that no longer exists doesn't count.
        assert_eq!(parent(Some("notes/gone.md")), None);
        // Collapsing the folder hides the selection, so it doesn't count.
        let collapsed = std::collections::BTreeSet::new();
        assert_eq!(
            MdEditor::new_entry_parent(Some("notes/a.md"), &entries, &collapsed),
            None
        );
    }

    #[test]
    fn test_find_heading_or_widget_line() {
        let text = "Line 0\n$$E = mc^2$$ \\label{equation-1}\nLine 2\n<div id=\"figure-1\">\nLine 4\n$$E = h\\nu$$ { #equation-2 }";
        let highlighted = crate::editor::highlight::highlight_markdown(text);
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
        let dyn_highlighted = crate::editor::highlight::highlight_markdown(dynamic_text);
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
    fn rename_updates_open_descendant_paths() {
        let mut path = Some("folder/sub/note.md".to_string());
        rename_open_path(&mut path, "folder", "renamed");
        assert_eq!(path.as_deref(), Some("renamed/sub/note.md"));
    }

    #[test]
    fn pdf_search_scroll_targets_match_rect_not_just_page_top() {
        assert_eq!(
            crate::pdf_pane::search_match_scroll_y_from(
                1000.0,
                Some(250.0),
                20.0,
                792.0,
                2.0,
                5000.0
            ),
            1948.0
        );
        assert_eq!(
            crate::pdf_pane::search_match_scroll_y_from(
                20.0,
                Some(780.0),
                10.0,
                792.0,
                1.0,
                5000.0
            ),
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

        let mut app = MdEditor::new_with_startup_file(None).0;
        app.pdf.active_path = Some("papers/My PDF File.pdf".to_string());
        assert_eq!(
            app.default_pdf_note_path(&ann),
            "pdf-notes/my-pdf-file-p5-abcdef12.md"
        );
    }

    #[test]
    fn same_file_search_result_does_not_reload_open_buffer() {
        assert!(!search_result_should_reload(
            Some("notes/current.md"),
            "notes/current.md"
        ));
        assert!(search_result_should_reload(
            Some("notes/current.md"),
            "notes/other.md"
        ));
    }

    #[test]
    fn stale_pdf_search_generation_is_rejected() {
        assert!(!pdf_search_generation_is_current(8, 7));
        assert!(pdf_search_generation_is_current(8, 8));
    }

    #[test]
    fn editor_cursor_move_reclaims_active_panel_from_pdf() {
        let mut app = MdEditor::new_with_startup_file(None).0;
        app.pdf.render_generation = 8;
        app.search.pdf_error = Some("keep".to_string());
        let _ = app.update_inner(Message::PdfSearchResult(
            7,
            Err("stale result".to_string()),
        ));
        assert_eq!(app.search.pdf_error.as_deref(), Some("keep"));

        app.active_panel = ActivePanel::Pdf;
        let _ = app.update_inner(Message::EditorCommand(EditorCommand::SetCursor {
            line: 0,
            col: 0,
            affinity: crate::editor::buffer::Affinity::Downstream,
        }));
        assert_eq!(app.active_panel, ActivePanel::Markdown);
    }

    #[test]
    fn graph_toggle_and_escape_preserve_the_open_document_mode() {
        let mut app = MdEditor::new_with_startup_file(None).0;
        app.graph.visible = false;
        app.vault.root = Some("/test-vault".to_string());
        app.graph
            .set_snapshot(md_editor_core::types::GraphSnapshot::default());
        app.active_path = Some("notes/current.md".to_string());

        let _ = app.update_inner(Message::GraphToggle);
        assert!(app.graph.visible);

        let _ = app.update_inner(Message::KeyboardShortcut(Shortcut::Escape));
        assert!(!app.graph.visible);
        assert_eq!(app.active_path.as_deref(), Some("notes/current.md"));
    }

    #[test]
    fn graph_shortcut_is_a_noop_without_a_vault() {
        let mut app = MdEditor::new_with_startup_file(None).0;
        app.vault.root = None;
        app.graph.visible = false;

        let _ = app.update_inner(Message::GraphToggle);

        assert!(!app.graph.visible);
        assert!(!app.title(iced::window::Id::unique()).contains("Knowledge Graph"));
    }

    #[test]
    fn stale_pdf_identity_result_cannot_attach_to_another_document() {
        let mut app = MdEditor::new_with_startup_file(None).0;
        app.pdf.render_generation = 8;
        app.pdf.active_path = Some("current.pdf".to_string());
        app.pdf.document_id = None;

        let _ = app.update_inner(Message::PdfDocumentIdComputed(
            7,
            Some(("current.pdf".to_string(), "stale-id".to_string(), 10, Some(1))),
        ));
        assert!(app.pdf.document_id.is_none());

        let _ = app.update_inner(Message::PdfDocumentIdComputed(
            8,
            Some(("other.pdf".to_string(), "wrong-file".to_string(), 10, Some(1))),
        ));
        assert!(app.pdf.document_id.is_none());
    }
}

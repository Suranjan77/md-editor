//! The iced shell (ADR-0100): wires the kernel Workspace + InputRouter into
//! a real window. Structure of every frame:
//!
//! - **Input:** one `iced::keyboard::listen()` subscription feeds
//!   [`keys::normalize`] → [`md3_kernel::Workspace::handle_key`]. That is the
//!   *only* keyboard path — widgets bind nothing (BUG-A discipline). Chords
//!   no command claims fall through as raw text to the focused surface.
//! - **State:** kernel `Workspace` owns identity/layout/focus; [`session`]
//!   owns per-document content state (editor engine instances, PDF frames).
//! - **View:** the kernel's `Layout` tree is walked into iced rows/columns;
//!   documents are peers — any kind in any pane (BUG-C discipline).

pub mod editor_canvas;
pub mod file_tree;
pub mod keys;
pub mod overlay;
pub mod paint;
mod pdf_view;
pub mod session;
pub mod snapshot;
pub mod tokens;
pub mod tracker_view;

use std::path::{Path, PathBuf};

use iced::widget::{button, canvas, column, container, row, stack, text};
use iced::{Element, Fill, Subscription, Task};
use md3_editor::buffer::{Command, Movement};
use md3_kernel::input::{Chord, EditorKind, Key};
use md3_kernel::pane::{DocumentId, Layout, Pane, PaneId, TabId};
use md3_kernel::{CommandId, CommandRegistry, Keymap, SplitAxis, Workspace};
use md3_vault::{AnnotationStore, NewAnnotation, Quad, SearchIndex, SessionStore};

use editor_canvas::{EditorCanvas, palette as colors};
use overlay::{Overlay, PdfFindHit};
use session::{MdSession, PdfSelection, PdfSession, Sessions};
use snapshot::{NodeSnapshot, SessionSnapshot, TabSnapshot, ViewSnapshot};

/// Highlight color cycle (`pdf.highlight-color`); new annotations start at
/// the first entry. Stored per annotation (`#rrggbb`, schema column).
const HIGHLIGHT_PALETTE: [&str; 4] = ["#ffd866", "#a9dc76", "#78dce8", "#ab9df2"];

/// Default highlight color for new annotations.
const HIGHLIGHT_COLOR: &str = HIGHLIGHT_PALETTE[0];

pub fn run(registry: CommandRegistry, keymap: Keymap, vault_root: PathBuf) -> iced::Result {
    iced::application(
        move || Shell::new(registry.clone(), keymap.clone(), vault_root.clone()),
        Shell::update,
        Shell::view,
    )
    .title("md3")
    .subscription(Shell::subscription)
    .theme(Shell::theme)
    .window(iced::window::Settings {
        size: iced::Size::new(1200.0, 800.0),
        // The shell confirms close itself so the session is saved first
        // (Message::WindowCloseRequested → save → exit).
        exit_on_close_request: false,
        icon: iced::window::icon::from_file_data(include_bytes!("../../../../md-editor.png"), None)
            .ok(),
        ..Default::default()
    })
    .run()
}

#[derive(Debug, Clone)]
pub enum Message {
    Key(keys::KeyEvent),
    Ignored,
    TabSelected(TabId),
    EditorClicked {
        tab: TabId,
        line: usize,
        col: usize,
        viewport_h: f32,
    },
    EditorScrolled {
        tab: TabId,
        dy: f32,
        viewport_h: f32,
    },
    PdfScrolled {
        tab: TabId,
        dy: f32,
        viewport: (f32, f32),
    },
    PdfMouseDown {
        tab: TabId,
        pos: (f32, f32),
        viewport: (f32, f32),
    },
    PdfMouseDragged {
        tab: TabId,
        pos: (f32, f32),
        viewport: (f32, f32),
    },
    PdfMouseUp {
        tab: TabId,
    },
    PdfRightClick {
        tab: TabId,
        pos: (f32, f32),
        viewport: (f32, f32),
    },
    OverlayPick(usize),
    WindowCloseRequested,
    TreeFileClicked(String),
    TreeDirToggled(String),
    Tracker(tracker_view::TrackerMessage),
}

pub struct Shell {
    registry: CommandRegistry,
    keymap: Keymap,
    ws: Workspace,
    sessions: Sessions,
    vault_root: PathBuf,
    overlay: Option<Overlay>,
    /// Vault files (relative paths) for quick-open; rescanned on open.
    files: Vec<String>,
    /// FTS index, opened lazily on first vault search; persists in the
    /// vault sidecar so an unchanged vault re-reads nothing across runs.
    index: Option<SearchIndex>,
    /// Annotation store on the same sidecar, opened lazily on first PDF.
    annotations: Option<AnnotationStore>,
    /// Session store on the same sidecar; opened on startup for restore.
    session: Option<SessionStore>,
    status: String,
    last_command: Option<CommandId>,
    tree_open: bool,
    tree_expanded: std::collections::BTreeSet<String>,
    // Study Tracker State (Phase 4)
    tracker_open: bool,
    tracker_running: bool,
    tracker_started_at: Option<std::time::Instant>,
    tracker_sessions: Vec<md3_vault::tracker::StudySession>,
    tracker_kv: std::collections::HashMap<String, String>,
    tracker_active_tab: tracker_view::TrackerTab,
    tracker_config_json: iced::widget::text_editor::Content,
    tracker_manual_date: String,
    tracker_manual_hours: String,
    tracker_manual_notes: String,
}

impl Shell {
    pub fn new(registry: CommandRegistry, keymap: Keymap, vault_root: PathBuf) -> Shell {
        let tracker_db = directories::ProjectDirs::from("com", "Suranjan77", "md-editor")
            .map(|p| p.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let _ = std::fs::create_dir_all(&tracker_db);
        let tracker_db_path = tracker_db.join("tracker.db");

        let mut tracker_sessions = Vec::new();
        let mut tracker_kv = std::collections::HashMap::new();
        let mut tracker_config_json =
            iced::widget::text_editor::Content::with_text(&tracker_view::default_config_json());

        if let Ok(store) = md3_vault::tracker::TrackerStore::open(&tracker_db_path) {
            if let Ok(sessions) = store.get_sessions() {
                tracker_sessions = sessions;
            }
            if let Ok(kvs) = store.get_kv() {
                for kv in kvs {
                    tracker_kv.insert(kv.key, kv.value);
                }
            }
            if let Some(json) = tracker_kv.get("tracker_config") {
                tracker_config_json = iced::widget::text_editor::Content::with_text(json);
            }
        }

        let mut shell = Shell {
            registry,
            keymap,
            ws: Workspace::new(),
            sessions: Sessions::default(),
            vault_root,
            overlay: None,
            files: Vec::new(),
            index: None,
            annotations: None,
            session: None,
            status: "ctrl+p open file · ctrl+shift+p commands".to_string(),
            last_command: None,
            tree_open: false,
            tree_expanded: std::collections::BTreeSet::new(),
            // Study Tracker (Phase 4)
            tracker_open: false,
            tracker_running: false,
            tracker_started_at: None,
            tracker_sessions,
            tracker_kv,
            tracker_active_tab: tracker_view::TrackerTab::Dashboard,
            tracker_config_json,
            tracker_manual_date: String::new(),
            tracker_manual_hours: String::new(),
            tracker_manual_notes: String::new(),
        };
        shell.restore_session();
        shell
    }

    /// The vault's sidecar database — one SQLite file (plan §2 pillar 5)
    /// shared by the FTS index and the annotation store (disjoint tables).
    /// Lives in a dot-directory so every vault walk skips it.
    fn sidecar_path(&self) -> PathBuf {
        self.vault_root.join(".md3/sidecar.db")
    }

    fn open_tracker_store(
        &self,
    ) -> Result<md3_vault::tracker::TrackerStore, md3_vault::error::VaultError> {
        let proj = directories::ProjectDirs::from("com", "Suranjan77", "md-editor");
        let dir = proj
            .map(|p| p.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let _ = std::fs::create_dir_all(&dir);
        md3_vault::tracker::TrackerStore::open(&dir.join("tracker.db"))
    }

    // ----- read-only access for tests and the view -------------------------

    pub fn workspace(&self) -> &Workspace {
        &self.ws
    }

    pub fn overlay(&self) -> Option<&Overlay> {
        self.overlay.as_ref()
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn last_command(&self) -> Option<CommandId> {
        self.last_command
    }

    pub fn tree_open(&self) -> bool {
        self.tree_open
    }

    pub fn tracker_open(&self) -> bool {
        self.tracker_open
    }

    pub fn tree_expanded(&self) -> &std::collections::BTreeSet<String> {
        &self.tree_expanded
    }

    /// The focused tab's markdown session, if it is one.
    pub fn focused_md(&self) -> Option<&MdSession> {
        let tab = self.ws.focused_tab()?;
        let doc = self.tab_document(tab)?;
        self.sessions.md.get(&doc)
    }

    /// The focused tab's PDF session, if it is one.
    pub fn focused_pdf(&self) -> Option<&PdfSession> {
        let tab = self.ws.focused_tab()?;
        let doc = self.tab_document(tab)?;
        self.sessions.pdf.get(&doc)
    }

    /// Mutable access to the focused PDF session — a test seam, like
    /// [`Self::inject_pdf_session_layout`]: the canvas is the production
    /// writer of selection state; windowless suites inject it here.
    pub fn focused_pdf_session_mut(&mut self) -> Option<&mut PdfSession> {
        let tab = self.ws.focused_tab()?;
        let doc = self.tab_document(tab)?;
        self.sessions.pdf.get_mut(&doc)
    }

    pub fn inject_pdf_session_layout(&mut self, layout: md3_pdf::DocLayout) {
        let tab = self.ws.focused_tab();
        let doc = tab.and_then(|t| self.tab_document(t));
        if let Some(doc) = doc
            && let Some(session) = self.sessions.pdf.get_mut(&doc)
        {
            session.layout = Some(layout);
        }
    }

    fn theme(&self) -> iced::Theme {
        let t = tokens::dark();
        iced::Theme::custom_with_fn(
            "MD Editor Dark".to_string(),
            iced::theme::Palette {
                background: t.bg_primary,
                text: t.text_primary,
                primary: t.accent,
                success: t.success,
                danger: t.danger,
                warning: t.warning,
            },
            iced::theme::palette::Extended::generate,
        )
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::keyboard::listen().map(|event| match keys::normalize(&event) {
                Some(ev) => Message::Key(ev),
                None => Message::Ignored,
            }),
            iced::window::close_requests().map(|_| Message::WindowCloseRequested),
        ])
    }

    // ------------------------------------------------------------- update --

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Ignored => Task::none(),
            Message::Key(ev) => self.on_key(ev),
            Message::TabSelected(tab) => {
                if let Err(e) = self.ws.focus_tab(tab) {
                    self.status = e.to_string();
                }
                self.save_session();
                Task::none()
            }
            Message::WindowCloseRequested => {
                self.save_session();
                iced::exit()
            }
            Message::EditorClicked {
                tab,
                line,
                col,
                viewport_h,
            } => {
                if let Err(e) = self.ws.focus_tab(tab) {
                    self.status = e.to_string();
                }
                if let Some(session) = self.focused_md_mut() {
                    session.viewport_h = viewport_h;
                    session.apply(Command::SetCursor { line, col });
                }
                self.sync_status();
                Task::none()
            }
            Message::EditorScrolled {
                tab,
                dy,
                viewport_h,
            } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    session.viewport_h = viewport_h;
                    session.scroll_by(dy);
                }
                Task::none()
            }
            Message::PdfScrolled { tab, dy, viewport } => {
                if let Err(e) = self.ws.focus_tab(tab) {
                    self.status = e.to_string();
                }
                let root = self.vault_root.clone();
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.pdf.get_mut(&d)) {
                    session.viewport = viewport;
                    session.scroll_by(dy);
                    let abs = root.join(&session.rel_path);
                    pdf_view::ensure_tiles(session, &abs);
                }
                self.sync_status();
                Task::none()
            }
            Message::PdfMouseDown { tab, pos, viewport } => {
                if let Err(e) = self.ws.focus_tab(tab) {
                    self.status = e.to_string();
                }
                self.pdf_mouse_down(tab, pos, viewport);
                Task::none()
            }
            Message::PdfRightClick { tab, pos, viewport } => {
                if let Err(e) = self.ws.focus_tab(tab) {
                    self.status = e.to_string();
                }
                self.pdf_right_click(tab, pos, viewport);
                Task::none()
            }
            Message::PdfMouseDragged { tab, pos, viewport } => {
                self.pdf_mouse_dragged(tab, pos, viewport);
                Task::none()
            }
            Message::PdfMouseUp { tab } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.pdf.get_mut(&d)) {
                    match &session.selection {
                        Some(sel) if !sel.text.is_empty() => {
                            self.status = format!(
                                "{} chars selected · ctrl+h highlights",
                                sel.text.chars().count()
                            );
                        }
                        _ => session.selection = None,
                    }
                }
                Task::none()
            }
            Message::OverlayPick(i) => {
                if let Some(sel) = self.overlay.as_mut().and_then(Overlay::selected_mut) {
                    *sel = i;
                }
                self.confirm_overlay()
            }
            Message::TreeFileClicked(rel_path) => {
                self.open_document(&rel_path);
                Task::none()
            }
            Message::TreeDirToggled(dir_path) => {
                if self.tree_expanded.contains(&dir_path) {
                    self.tree_expanded.remove(&dir_path);
                } else {
                    self.tree_expanded.insert(dir_path);
                }
                self.save_session();
                Task::none()
            }
            Message::Tracker(msg) => {
                match msg {
                    tracker_view::TrackerMessage::Toggle => {
                        self.tracker_open = !self.tracker_open;
                        self.save_session();
                    }
                    tracker_view::TrackerMessage::Start => {
                        self.tracker_running = true;
                        self.tracker_started_at = Some(std::time::Instant::now());
                        self.status = "timer: started".to_string();
                    }
                    tracker_view::TrackerMessage::Stop => {
                        if self.tracker_running {
                            self.tracker_running = false;
                            if let Some(started_at) = self.tracker_started_at.take() {
                                let elapsed =
                                    std::time::Instant::now().saturating_duration_since(started_at);
                                let hours = (elapsed.as_secs_f32() / 3600.0).max(0.01);
                                let date =
                                    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
                                let session = md3_vault::tracker::StudySession {
                                    id: 0,
                                    date,
                                    hours,
                                    activity_type: "Study".to_string(),
                                    phase: "Focus".to_string(),
                                    notes: None,
                                };
                                if let Ok(mut store) = self.open_tracker_store()
                                    && store.save_session(&session).is_ok()
                                {
                                    if let Ok(sessions) = store.get_sessions() {
                                        self.tracker_sessions = sessions;
                                    }
                                    self.status = format!("timer: saved {:.2} hours", hours);
                                }
                            }
                        }
                    }
                    tracker_view::TrackerMessage::TabSelected(tab) => {
                        self.tracker_active_tab = tab;
                        self.save_session();
                    }
                    tracker_view::TrackerMessage::ProjectStatusChanged(id, val) => {
                        let key = format!("proj_{}", id);
                        if let Ok(mut store) = self.open_tracker_store()
                            && store.set_kv(&key, &val).is_ok()
                        {
                            self.tracker_kv.insert(key, val);
                        }
                    }
                    tracker_view::TrackerMessage::GateToggled(id, idx) => {
                        let key = format!("gate_{}_{}", id, idx);
                        let val = if self
                            .tracker_kv
                            .get(&key)
                            .map(|v| v == "true")
                            .unwrap_or(false)
                        {
                            "false"
                        } else {
                            "true"
                        };
                        if let Ok(mut store) = self.open_tracker_store()
                            && store.set_kv(&key, val).is_ok()
                        {
                            self.tracker_kv.insert(key, val.to_string());
                        }
                    }
                    tracker_view::TrackerMessage::ReadingToggled(section, idx) => {
                        let key = format!("read_{}_{}", section, idx);
                        let val = if self
                            .tracker_kv
                            .get(&key)
                            .map(|v| v == "true")
                            .unwrap_or(false)
                        {
                            "false"
                        } else {
                            "true"
                        };
                        if let Ok(mut store) = self.open_tracker_store()
                            && store.set_kv(&key, val).is_ok()
                        {
                            self.tracker_kv.insert(key, val.to_string());
                        }
                    }
                    tracker_view::TrackerMessage::ConfigEdited(action) => {
                        self.tracker_config_json.perform(action);
                    }
                    tracker_view::TrackerMessage::ConfigSave => {
                        let text = self.tracker_config_json.text();
                        match tracker_view::parse_config(&text) {
                            Ok(_) => {
                                if let Ok(mut store) = self.open_tracker_store()
                                    && store.set_kv("tracker_config", &text).is_ok()
                                {
                                    self.tracker_kv.insert("tracker_config".to_string(), text);
                                    self.status = "tracker: configuration saved".to_string();
                                }
                            }
                            Err(e) => {
                                self.status = format!("tracker: invalid config: {}", e);
                            }
                        }
                    }
                    tracker_view::TrackerMessage::ManualDateChanged(val) => {
                        self.tracker_manual_date = val;
                    }
                    tracker_view::TrackerMessage::ManualHoursChanged(val) => {
                        self.tracker_manual_hours = val;
                    }
                    tracker_view::TrackerMessage::ManualNotesChanged(val) => {
                        self.tracker_manual_notes = val;
                    }
                    tracker_view::TrackerMessage::ManualAdd => {
                        let Ok(hours) = self.tracker_manual_hours.trim().parse::<f32>() else {
                            self.status = "tracker: invalid hours".to_string();
                            return Task::none();
                        };
                        if hours <= 0.0 {
                            self.status = "tracker: invalid hours".to_string();
                            return Task::none();
                        }
                        let date = if self.tracker_manual_date.trim().is_empty() {
                            chrono::Local::now().format("%Y-%m-%d").to_string()
                        } else {
                            self.tracker_manual_date.trim().to_string()
                        };
                        let notes = (!self.tracker_manual_notes.trim().is_empty())
                            .then(|| self.tracker_manual_notes.trim().to_string());

                        let session = md3_vault::tracker::StudySession {
                            id: 0,
                            date,
                            hours,
                            activity_type: "Manual".to_string(),
                            phase: "Manual".to_string(),
                            notes,
                        };
                        if let Ok(mut store) = self.open_tracker_store()
                            && store.save_session(&session).is_ok()
                        {
                            if let Ok(sessions) = store.get_sessions() {
                                self.tracker_sessions = sessions;
                            }
                            self.tracker_manual_hours = String::new();
                            self.tracker_manual_notes = String::new();
                            self.status = "tracker: session logged manually".to_string();
                        }
                    }
                    tracker_view::TrackerMessage::SessionDelete(id) => {
                        if let Ok(mut store) = self.open_tracker_store()
                            && store.delete_session(id).is_ok()
                        {
                            if let Ok(sessions) = store.get_sessions() {
                                self.tracker_sessions = sessions;
                            }
                            self.status = "tracker: session deleted".to_string();
                        }
                    }
                }
                Task::none()
            }
        }
    }

    /// Press on the page strip: pick the annotation under the cursor, or
    /// anchor a new text selection there.
    fn pdf_mouse_down(&mut self, tab: TabId, pos: (f32, f32), viewport: (f32, f32)) {
        let root = self.vault_root.clone();
        let Some(session) = self
            .tab_document(tab)
            .and_then(|d| self.sessions.pdf.get_mut(&d))
        else {
            return;
        };
        session.viewport = viewport;
        let abs = root.join(&session.rel_path);
        pdf_view::ensure_tiles(session, &abs);
        let hit = session
            .layout
            .as_ref()
            .and_then(|l| l.page_at_point(session.scroll, viewport, pos));
        let Some((page, pt)) = hit else {
            session.selection = None;
            session.selected_annotation = None;
            self.sync_status();
            return;
        };
        let page = page as u32;
        if let Some(link) = session.link_at(page, pt) {
            if let Some((dest_page, dest_y)) = link.dest {
                session.record_jump();
                session.go_to_page(dest_page as usize);
                if let Some(y) = dest_y {
                    let zoom = session.zoom;
                    if let Some(layout) = &session.layout {
                        let max = layout.max_scroll(session.viewport.1);
                        let top = layout.page_top(dest_page as usize);
                        session.scroll = (top + y * zoom).clamp(0.0, max);
                    }
                }
                let abs = root.join(&session.rel_path);
                pdf_view::ensure_tiles(session, &abs);
                self.status = format!("→ p. {} · alt+left returns", dest_page + 1);
                return;
            } else if let Some(uri) = &link.uri {
                self.status = format!("link: {uri}");
                return;
            }
        }
        let picked = session
            .annotation_at(page, pt)
            .map(|a| (a.id, a.note.clone()));
        if let Some((id, note)) = picked {
            session.selected_annotation = Some(id);
            session.selection = None;
            self.status = if note.is_empty() {
                "highlight · ctrl+n adds a note · delete removes".to_string()
            } else {
                format!("note: {note} · ctrl+n edits · delete removes")
            };
            return;
        }
        session.selected_annotation = None;
        pdf_view::load_page_chars(session, &abs, page);
        session.selection = Some(PdfSelection {
            page,
            anchor: pt,
            quads: Vec::new(),
            text: String::new(),
        });
        self.sync_status();
    }

    fn pdf_right_click(&mut self, tab: TabId, pos: (f32, f32), viewport: (f32, f32)) {
        let root = self.vault_root.clone();
        let Some(session) = self
            .tab_document(tab)
            .and_then(|d| self.sessions.pdf.get_mut(&d))
        else {
            return;
        };
        session.viewport = viewport;
        let abs = root.join(&session.rel_path);
        pdf_view::ensure_tiles(session, &abs);
        let hit = session
            .layout
            .as_ref()
            .and_then(|l| l.page_at_point(session.scroll, viewport, pos));
        let Some((page, pt)) = hit else {
            return;
        };
        let page = page as u32;
        if let Some(link) = session.link_at(page, pt) {
            if let Some(uri) = &link.uri {
                self.status = format!("link: {uri}");
            } else if let Some((dest_page, dest_y)) = link.dest {
                #[cfg(feature = "pdfium")]
                {
                    if let Some(renderer) = pdf_view::renderer() {
                        let w_px = session
                            .layout
                            .as_ref()
                            .map(|l| l.page_size_px(dest_page as usize).0)
                            .unwrap_or(0.0);
                        let page_width_pts = w_px / session.zoom;
                        let scale = if page_width_pts > 0.0 {
                            (520.0 / page_width_pts).min(2.0)
                        } else {
                            1.0
                        };
                        match renderer.render_page(&abs, dest_page, scale) {
                            Ok(rendered) => {
                                let handle = iced::widget::image::Handle::from_rgba(
                                    rendered.width,
                                    rendered.height,
                                    rendered.rgba,
                                );
                                self.open_overlay(Overlay::PdfLinkPreview {
                                    dest_page,
                                    dest_y,
                                    image: handle,
                                    width: rendered.width,
                                    height: rendered.height,
                                });
                            }
                            Err(e) => {
                                self.status = format!("preview render failed: {e}");
                            }
                        }
                    }
                }
                #[cfg(not(feature = "pdfium"))]
                {
                    let _ = dest_page;
                    let _ = dest_y;
                    self.status = "preview: built without pdfium".to_string();
                }
            }
        }
    }

    /// Drag: extend the selection from its anchor to the cursor, expressed
    /// in the anchor page's coordinates (the engine clamps to its text).
    fn pdf_mouse_dragged(&mut self, tab: TabId, pos: (f32, f32), viewport: (f32, f32)) {
        let Some(session) = self
            .tab_document(tab)
            .and_then(|d| self.sessions.pdf.get_mut(&d))
        else {
            return;
        };
        let (Some(layout), Some(sel)) = (session.layout.as_ref(), session.selection.as_mut())
        else {
            return;
        };
        let head = layout.point_in_page(session.scroll, viewport, pos, sel.page as usize);
        let chars = session.chars.get(&sel.page).map_or(&[][..], Vec::as_slice);
        match md3_pdf::select::select(chars, sel.anchor, head) {
            Some(text_sel) => {
                sel.quads = text_sel.quads;
                sel.text = text_sel.text;
            }
            None => {
                sel.quads.clear();
                sel.text.clear();
            }
        }
    }

    /// The single keystroke entry point.
    fn on_key(&mut self, ev: keys::KeyEvent) -> Task<Message> {
        if let Some(chord) = ev.chord
            && let Some(cmd) = self.ws.handle_key(&self.keymap, chord)
        {
            return self.run_command(cmd);
        }
        // Not a command: raw input for the focused surface.
        if self.overlay.is_some() {
            return self.overlay_raw_input(&ev);
        }
        match self.ws.focused_editor_kind() {
            Some(EditorKind::Markdown) => self.editor_raw_input(&ev),
            Some(EditorKind::Pdf) => self.pdf_raw_input(&ev),
            _ => {}
        }
        self.sync_status();
        Task::none()
    }

    fn editor_raw_input(&mut self, ev: &keys::KeyEvent) {
        let Some(session) = self.focused_md_mut() else {
            return;
        };
        let command = match ev.chord {
            Some(Chord { mods, key }) if mods.ctrl || mods.alt || mods.meta => {
                let _ = (mods, key);
                None // unbound command-grade chords do nothing
            }
            Some(Chord { mods, key }) => {
                let extend = mods.shift;
                match key {
                    Key::Enter => Some(Command::Insert("\n".to_string())),
                    Key::Tab => Some(Command::Insert("  ".to_string())),
                    Key::Backspace => Some(Command::DeleteBackward),
                    Key::Delete => Some(Command::DeleteForward),
                    Key::Up => Some(Command::Move {
                        movement: Movement::Up,
                        extend,
                    }),
                    Key::Down => Some(Command::Move {
                        movement: Movement::Down,
                        extend,
                    }),
                    Key::Left => Some(Command::Move {
                        movement: Movement::Left,
                        extend,
                    }),
                    Key::Right => Some(Command::Move {
                        movement: Movement::Right,
                        extend,
                    }),
                    Key::Home => Some(Command::Move {
                        movement: Movement::Home,
                        extend,
                    }),
                    Key::End => Some(Command::Move {
                        movement: Movement::End,
                        extend,
                    }),
                    Key::PageUp | Key::PageDown => {
                        let dy = session.viewport_h * if key == Key::PageUp { -0.9 } else { 0.9 };
                        session.scroll_by(dy);
                        None
                    }
                    _ => ev.text.clone().map(Command::Insert),
                }
            }
            None => ev.text.clone().map(Command::Insert),
        };
        if let Some(cmd) = command {
            session.apply(cmd);
        }
    }

    fn pdf_raw_input(&mut self, ev: &keys::KeyEvent) {
        if ev.chord.map(|c| c.key) == Some(Key::Delete) {
            self.remove_selected_annotation();
            return;
        }
        let root = self.vault_root.clone();
        let Some(session) = self.focused_pdf_mut() else {
            return;
        };
        let screen = session.viewport.1 * 0.9;
        match ev.chord.map(|c| c.key) {
            Some(Key::PageDown) => session.scroll_by(screen),
            Some(Key::PageUp) => session.scroll_by(-screen),
            Some(Key::Down) => session.scroll_by(60.0),
            Some(Key::Up) => session.scroll_by(-60.0),
            Some(Key::Right) => session.go_to_page(session.current_page() + 1),
            Some(Key::Left) => session.go_to_page(session.current_page().saturating_sub(1)),
            Some(Key::Home) => session.go_to_page(0),
            Some(Key::End) => session.scroll_by(f32::MAX),
            _ => return,
        }
        let abs = root.join(&session.rel_path);
        pdf_view::ensure_tiles(session, &abs);
    }

    fn overlay_raw_input(&mut self, ev: &keys::KeyEvent) -> Task<Message> {
        let Some(overlay) = self.overlay.as_mut() else {
            return Task::none();
        };
        match ev.chord.map(|c| c.key) {
            Some(Key::Backspace) => {
                if let Some(input) = overlay.input_mut() {
                    input.pop();
                }
            }
            Some(Key::Up) => {
                if let Some(sel) = overlay.selected_mut() {
                    *sel = sel.saturating_sub(1);
                }
            }
            Some(Key::Down) => {
                if let Some(sel) = overlay.selected_mut() {
                    *sel += 1; // clamped against the live row count below
                }
            }
            _ => {
                if let Some(t) = &ev.text
                    && let Some(input) = overlay.input_mut()
                {
                    input.push_str(t);
                }
            }
        }
        // Live-update vault search results as the query changes.
        let query = match self.overlay.as_ref() {
            Some(Overlay::Search { input, .. }) => Some(input.clone()),
            _ => None,
        };
        if let Some(q) = query {
            let new_hits = self.search_vault(&q);
            if let Some(Overlay::Search { hits, selected, .. }) = self.overlay.as_mut() {
                *selected = (*selected).min(new_hits.len().saturating_sub(1));
                *hits = new_hits;
            }
        }
        // Same for pdf.find — matches recompute over the cached glyphs.
        let query = match self.overlay.as_ref() {
            Some(Overlay::PdfFind { input, .. }) => Some(input.clone()),
            _ => None,
        };
        if let Some(q) = query {
            let new_hits = self.pdf_find_hits(&q);
            if let Some(Overlay::PdfFind { hits, selected, .. }) = self.overlay.as_mut() {
                *selected = (*selected).min(new_hits.len().saturating_sub(1));
                *hits = new_hits;
            }
        }
        // Clamp selection against the rows actually displayed (typing can
        // shrink the list; down must not walk past it — what's on screen is
        // what enter picks), then keep the selected row scrolled into view.
        let rows = match self.overlay.as_ref() {
            Some(ov) => overlay::list_rows(ov, &self.registry, &self.files).len(),
            None => 0,
        };
        if let Some(sel) = self.overlay.as_mut().and_then(Overlay::selected_mut) {
            *sel = (*sel).min(rows.saturating_sub(1));
            return overlay::snap_selected(rows, *sel);
        }
        Task::none()
    }

    // --------------------------------------------------------- commands --

    fn run_command(&mut self, cmd: CommandId) -> Task<Message> {
        self.last_command = Some(cmd);
        self.status = format!("⌘ {}", cmd.0);
        match cmd.0 {
            "app.quit" => {
                self.save_session();
                return iced::exit();
            }
            "palette.open" => self.open_overlay(Overlay::Palette {
                input: String::new(),
                selected: 0,
            }),
            "file.quick-open" => {
                self.files = scan_vault(&self.vault_root);
                self.open_overlay(Overlay::QuickOpen {
                    input: String::new(),
                    selected: 0,
                });
            }
            "search.global" => {
                self.ensure_index();
                self.open_overlay(Overlay::Search {
                    input: String::new(),
                    selected: 0,
                    hits: Vec::new(),
                });
            }
            "note.backlinks" => {
                let Some((path, EditorKind::Markdown)) = self.focused_doc_info() else {
                    self.status = "backlinks: focus a note".to_string();
                    return Task::none();
                };
                let referrers = self.note_backlinks(&path);
                if referrers.is_empty() {
                    self.status = format!("no backlinks to {path}");
                    return Task::none();
                }
                self.open_overlay(Overlay::Backlinks {
                    input: String::new(),
                    selected: 0,
                    referrers,
                });
            }
            "workspace.split-right" => {
                let focused = self.focused_doc_info();
                match focused {
                    Some((path, kind)) => {
                        if let Err(e) =
                            self.ws
                                .open_in_new_split(&path, kind, SplitAxis::Horizontal)
                        {
                            self.status = e.to_string();
                        }
                    }
                    None => self.status = "nothing to split".to_string(),
                }
            }
            "workspace.close-tab" => {
                if let Some(tab) = self.ws.focused_tab() {
                    if let Err(e) = self.ws.close_tab(tab) {
                        self.status = e.to_string();
                    }
                    self.sessions.gc(&self.ws.docs);
                }
            }
            "workspace.next-tab" => self.cycle_tab(),
            "workspace.toggle-files" => {
                self.tree_open = !self.tree_open;
                if self.tree_open {
                    self.files = scan_vault(&self.vault_root);
                }
                self.save_session();
            }
            "workspace.toggle-tracker" => {
                self.tracker_open = !self.tracker_open;
                self.save_session();
            }
            "editor.undo" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(Command::Undo);
                }
            }
            "editor.redo" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(Command::Redo);
                }
            }
            "editor.select-all" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(Command::SelectAll);
                }
            }
            "editor.save" => self.save_focused(),
            "editor.find" => self.open_overlay(Overlay::Find {
                input: String::new(),
            }),
            "pdf.zoom-input" => self.open_overlay(Overlay::PdfZoom {
                input: String::new(),
            }),
            "pdf.go-to-page" => self.open_overlay(Overlay::PdfPage {
                input: String::new(),
            }),
            // Early return: the no-pdf guidance must survive the trailing
            // sync_status (which would repaint the md caret pill over it).
            "pdf.find" => {
                self.open_pdf_find();
                return Task::none();
            }
            "pdf.toc" => {
                self.open_pdf_toc();
                return Task::none();
            }
            "pdf.back" | "pdf.forward" => {
                self.pdf_nav_history(cmd.0 == "pdf.back");
                return Task::none();
            }
            "pdf.highlight" => self.highlight_selection(),
            "pdf.annotation-note" => {
                match self.focused_pdf().and_then(PdfSession::selected_annotation) {
                    Some(a) => {
                        let input = a.note.clone();
                        self.open_overlay(Overlay::AnnotationNote { input });
                    }
                    None => self.status = "click a highlight first".to_string(),
                }
            }
            "pdf.annotations-export" => self.export_annotations(),
            "pdf.copy-selection" => {
                let text = self
                    .focused_pdf()
                    .and_then(|s| s.selection.as_ref())
                    .map(|sel| sel.text.clone())
                    .filter(|t| !t.is_empty());
                return match text {
                    Some(text) => {
                        self.status = format!("{} chars copied", text.chars().count());
                        iced::clipboard::write(text)
                    }
                    None => {
                        self.status = "select text first (drag over the page)".to_string();
                        Task::none()
                    }
                };
            }
            "pdf.highlight-color" => {
                self.cycle_highlight_color();
                return Task::none();
            }
            "pdf.annotation-link-note" => {
                self.link_note_for_annotation();
                return Task::none();
            }
            "pdf.annotations-orphans" => {
                self.orphan_report();
                return Task::none();
            }
            "overlay.close" => self.close_overlay(),
            "overlay.confirm" => return self.confirm_overlay(),
            other => self.status = format!("unhandled command: {other}"),
        }
        if matches!(
            cmd.0,
            "workspace.split-right" | "workspace.close-tab" | "workspace.next-tab" | "editor.save"
        ) {
            self.save_session();
        }
        self.sync_status();
        Task::none()
    }

    fn open_overlay(&mut self, overlay: Overlay) {
        self.ws.open_overlay(overlay.kernel_name());
        self.overlay = Some(overlay);
    }

    fn close_overlay(&mut self) {
        self.ws.close_overlay();
        self.overlay = None;
    }

    fn confirm_overlay(&mut self) -> Task<Message> {
        let Some(overlay) = self.overlay.clone() else {
            return Task::none();
        };
        match overlay {
            Overlay::Palette { input, selected } => {
                let picked = self
                    .registry
                    .palette(&input)
                    .get(selected.min(self.registry.palette(&input).len().saturating_sub(1)))
                    .map(|s| s.id);
                self.close_overlay();
                if let Some(id) = picked {
                    return self.run_command(id);
                }
            }
            Overlay::QuickOpen { input, selected } => {
                let rows = overlay::list_rows(
                    &Overlay::QuickOpen { input, selected },
                    &self.registry,
                    &self.files,
                );
                let picked = rows
                    .get(selected.min(rows.len().saturating_sub(1)))
                    .map(|(p, _)| p.clone());
                self.close_overlay();
                if let Some(path) = picked {
                    self.open_document(&path);
                }
            }
            Overlay::Search { hits, selected, .. } => {
                let picked = hits
                    .get(selected.min(hits.len().saturating_sub(1)))
                    .map(|h| h.path.to_string_lossy().to_string());
                self.close_overlay();
                if let Some(path) = picked {
                    self.open_document(&path);
                }
            }
            Overlay::Backlinks {
                input,
                selected,
                referrers,
            } => {
                let rows = overlay::list_rows(
                    &Overlay::Backlinks {
                        input,
                        selected,
                        referrers,
                    },
                    &self.registry,
                    &self.files,
                );
                let picked = rows
                    .get(selected.min(rows.len().saturating_sub(1)))
                    .map(|(p, _)| p.clone());
                self.close_overlay();
                if let Some(path) = picked {
                    self.open_document(&path);
                }
            }
            Overlay::Find { input } => {
                self.close_overlay();
                self.find_in_note(&input);
            }
            Overlay::PdfFind {
                input,
                selected,
                hits,
            } => {
                self.close_overlay();
                match hits.get(selected.min(hits.len().saturating_sub(1))) {
                    Some(hit) => self.jump_to_pdf_match(hit),
                    None if !input.trim().is_empty() => {
                        self.status = format!("no matches for `{}`", input.trim());
                    }
                    None => {}
                }
                // Keep the jump/no-match message; sync_status would replace
                // it with the page pill.
                return Task::none();
            }
            Overlay::PdfToc {
                input,
                selected,
                entries,
            } => {
                self.close_overlay();
                let matches = overlay::toc_matches(&entries, &input);
                let picked = matches
                    .get(selected.min(matches.len().saturating_sub(1)))
                    .map(|(title, page)| (title.clone(), *page));
                if let Some((title, page)) = picked {
                    let root = self.vault_root.clone();
                    if let Some(session) = self.focused_pdf_mut() {
                        session.record_jump();
                        session.go_to_page(page as usize);
                        let abs = root.join(&session.rel_path);
                        pdf_view::ensure_tiles(session, &abs);
                        self.status = format!("§ {}", title.trim_start());
                        return Task::none();
                    }
                }
            }
            Overlay::PdfZoom { input } => {
                self.close_overlay();
                let root = self.vault_root.clone();
                if let (Ok(pct), Some(session)) =
                    (input.trim().parse::<f32>(), self.focused_pdf_mut())
                {
                    session.set_zoom(pct / 100.0);
                    let abs = root.join(&session.rel_path);
                    pdf_view::ensure_tiles(session, &abs);
                }
            }
            Overlay::PdfPage { input } => {
                self.close_overlay();
                let root = self.vault_root.clone();
                if let (Ok(page), Some(session)) =
                    (input.trim().parse::<u32>(), self.focused_pdf_mut())
                {
                    session.record_jump();
                    session.go_to_page((page.saturating_sub(1)) as usize);
                    let abs = root.join(&session.rel_path);
                    pdf_view::ensure_tiles(session, &abs);
                }
            }
            Overlay::AnnotationNote { input } => {
                self.close_overlay();
                self.set_annotation_note(&input);
            }
            // Read-only report: confirm = dismiss.
            Overlay::OrphanReport { .. } => self.close_overlay(),
            Overlay::PdfLinkPreview {
                dest_page, dest_y, ..
            } => {
                self.close_overlay();
                let root = self.vault_root.clone();
                if let Some(session) = self.focused_pdf_mut() {
                    session.record_jump();
                    session.go_to_page(dest_page as usize);
                    if let Some(y) = dest_y {
                        let zoom = session.zoom;
                        if let Some(layout) = &session.layout {
                            let max = layout.max_scroll(session.viewport.1);
                            let top = layout.page_top(dest_page as usize);
                            session.scroll = (top + y * zoom).clamp(0.0, max);
                        }
                    }
                    let abs = root.join(&session.rel_path);
                    pdf_view::ensure_tiles(session, &abs);
                    self.status = format!("→ p. {} · alt+left returns", dest_page + 1);
                    return Task::none();
                }
            }
        }
        self.sync_status();
        Task::none()
    }

    // ------------------------------------------------------------ actions --

    fn open_document(&mut self, rel: &str) {
        let pane = self
            .ws
            .focused_pane()
            .unwrap_or_else(|| self.ws.panes.first_pane());
        if self.open_document_in(pane, rel).is_some() {
            if self.tree_open {
                self.files = scan_vault(&self.vault_root);
            }
            self.save_session();
        }
    }

    /// Open `rel` as a tab of `pane` (the workhorse `open_document` and
    /// session restore share). Returns the tab on success.
    fn open_document_in(&mut self, pane: PaneId, rel: &str) -> Option<TabId> {
        let kind = if rel.ends_with(".pdf") {
            EditorKind::Pdf
        } else {
            EditorKind::Markdown
        };
        let tab = match self.ws.open_in(pane, rel, kind) {
            Ok(t) => t,
            Err(e) => {
                self.status = e.to_string();
                return None;
            }
        };
        let doc = self.tab_document(tab)?;
        let abs = self.vault_root.join(rel);
        match kind {
            EditorKind::Markdown => {
                if let std::collections::hash_map::Entry::Vacant(entry) =
                    self.sessions.md.entry(doc)
                {
                    match std::fs::read_to_string(&abs) {
                        Ok(text) => {
                            entry.insert(MdSession::new(rel, &text));
                        }
                        Err(e) => {
                            self.status = format!("open {rel}: {e}");
                            let _ = self.ws.close_tab(tab);
                            self.sessions.gc(&self.ws.docs);
                            return None;
                        }
                    }
                }
            }
            EditorKind::Pdf => {
                // Annotation identity first: SHA-256 of the bytes (vault
                // convention — survives rename/move; needs no pdfium).
                self.ensure_annotations();
                let hash = md3_vault::document_hash(&abs).ok();
                if let (Some(store), Some(h)) = (self.annotations.as_mut(), &hash) {
                    let _ = store.record_document(h, rel);
                }
                let entry = self
                    .sessions
                    .pdf
                    .entry(doc)
                    .or_insert_with(|| PdfSession::new(rel));
                entry.doc_hash = hash;
                pdf_view::load_geometry(entry, &abs);
                pdf_view::ensure_tiles(entry, &abs);
                if let Some(store) = self.annotations.as_ref() {
                    refresh_annotations(store, entry);
                }
            }
            _ => {}
        }
        self.sync_status();
        Some(tab)
    }

    // ------------------------------------------------------------ session --

    /// Open the session store on the sidecar (once).
    fn ensure_session_store(&mut self) {
        if self.session.is_some() {
            return;
        }
        let opened = self
            .open_sidecar_dir()
            .and_then(|_| SessionStore::open(&self.sidecar_path()));
        match opened {
            Ok(store) => self.session = Some(store),
            Err(e) => self.status = format!("session persistence unavailable: {e}"),
        }
    }

    /// Serialize the live workspace: pane tree with vault-relative paths,
    /// plus per-document view state.
    fn capture_session(&self) -> SessionSnapshot {
        let layout = self.snapshot_node(&self.ws.panes.layout());
        let mut views = std::collections::BTreeMap::new();
        for s in self.sessions.md.values() {
            let head = s.doc.buffer().primary().head;
            let caret = s.doc.buffer().offset_to_line_col(head);
            views.insert(
                s.rel_path.clone(),
                ViewSnapshot {
                    scroll: s.scroll,
                    zoom: None,
                    caret: Some(caret),
                },
            );
        }
        for s in self.sessions.pdf.values() {
            views.insert(
                s.rel_path.clone(),
                ViewSnapshot {
                    scroll: s.scroll,
                    zoom: Some(s.zoom),
                    caret: None,
                },
            );
        }
        SessionSnapshot {
            layout,
            views,
            tree_open: self.tree_open,
            tree_expanded: self.tree_expanded.iter().cloned().collect(),
            tracker_open: self.tracker_open,
            tracker_active_tab: Some(match self.tracker_active_tab {
                tracker_view::TrackerTab::Dashboard => "Dashboard".to_string(),
                tracker_view::TrackerTab::Log => "Log".to_string(),
                tracker_view::TrackerTab::Projects => "Projects".to_string(),
                tracker_view::TrackerTab::Gates => "Gates".to_string(),
                tracker_view::TrackerTab::Reading => "Reading".to_string(),
                tracker_view::TrackerTab::Config => "Config".to_string(),
            }),
        }
    }

    fn snapshot_node(&self, node: &Layout<'_>) -> NodeSnapshot {
        match node {
            Layout::Pane(pane) => {
                let active = pane
                    .active_tab()
                    .and_then(|a| pane.tabs().iter().position(|t| t.id == a.id))
                    .unwrap_or(0);
                let tabs = pane
                    .tabs()
                    .iter()
                    .filter_map(|t| {
                        let doc = self.ws.docs.get(t.document)?;
                        Some(TabSnapshot {
                            path: doc.path.clone(),
                            focused: self.ws.focused_tab() == Some(t.id),
                        })
                    })
                    .collect();
                NodeSnapshot::Pane { tabs, active }
            }
            Layout::Split {
                axis,
                ratio,
                first,
                second,
            } => NodeSnapshot::Split {
                vertical: matches!(axis, SplitAxis::Vertical),
                ratio: *ratio,
                first: Box::new(self.snapshot_node(first)),
                second: Box::new(self.snapshot_node(second)),
            },
        }
    }

    /// Persist the current session. Failures degrade to a status line —
    /// never block the action that triggered the save.
    fn save_session(&mut self) {
        let snapshot = self.capture_session();
        let json = match serde_json::to_string(&snapshot) {
            Ok(j) => j,
            Err(e) => {
                self.status = format!("session save failed: {e}");
                return;
            }
        };
        self.ensure_session_store();
        if let Some(store) = self.session.as_mut()
            && let Err(e) = store.save(&json)
        {
            self.status = format!("session save failed: {e}");
        }
    }

    /// Rebuild the previous session at startup: layout first (skipping
    /// files that vanished, collapsing splits that end up hollow), then
    /// view state, then focus.
    fn restore_session(&mut self) {
        self.ensure_session_store();
        let Some(json) = self.session.as_ref().and_then(|s| s.load().ok().flatten()) else {
            return;
        };
        let Ok(snap) = serde_json::from_str::<SessionSnapshot>(&json) else {
            self.status = "saved session was unreadable — starting fresh".to_string();
            return;
        };

        let root_pane = self.ws.panes.first_pane();
        let mut focus = None;
        self.restore_node(&snap.layout, root_pane, &mut focus);
        self.ws.panes.collapse_empty_panes();

        let root = self.vault_root.clone();
        for (path, view) in &snap.views {
            if let Some(s) = self.sessions.md.values_mut().find(|s| &s.rel_path == path) {
                if let Some((line, col)) = view.caret {
                    s.apply(Command::SetCursor { line, col });
                }
                s.scroll = view.scroll;
                s.scroll_by(0.0); // clamp against the loaded document
            }
            if let Some(s) = self.sessions.pdf.values_mut().find(|s| &s.rel_path == path) {
                if let Some(zoom) = view.zoom {
                    s.set_zoom(zoom);
                }
                s.scroll = view.scroll;
                if s.layout.is_some() {
                    s.scroll_by(0.0); // clamp; without geometry keep the value
                }
                let abs = root.join(&s.rel_path);
                pdf_view::ensure_tiles(s, &abs);
            }
        }

        if let Some(tab) = focus {
            let _ = self.ws.focus_tab(tab);
        }
        self.tree_open = snap.tree_open;
        self.tree_expanded = snap.tree_expanded.into_iter().collect();
        self.tracker_open = snap.tracker_open;
        if let Some(tab_str) = snap.tracker_active_tab {
            self.tracker_active_tab = match tab_str.as_str() {
                "Dashboard" => tracker_view::TrackerTab::Dashboard,
                "Log" => tracker_view::TrackerTab::Log,
                "Projects" => tracker_view::TrackerTab::Projects,
                "Gates" => tracker_view::TrackerTab::Gates,
                "Reading" => tracker_view::TrackerTab::Reading,
                "Config" => tracker_view::TrackerTab::Config,
                _ => tracker_view::TrackerTab::Dashboard,
            };
        }
        self.sync_status();
        if let Some(s) = self.focused_pdf()
            && s.layout.is_some()
        {
            self.status = format!("resumed at p. {}/{}", s.current_page() + 1, s.page_count());
        }
    }

    fn restore_node(&mut self, node: &NodeSnapshot, pane: PaneId, focus: &mut Option<TabId>) {
        match node {
            NodeSnapshot::Pane { tabs, active } => {
                let mut opened = Vec::new();
                for t in tabs {
                    if !self.vault_root.join(&t.path).exists() {
                        continue; // the file vanished between sessions
                    }
                    if let Some(id) = self.open_document_in(pane, &t.path) {
                        opened.push((id, t.focused));
                    }
                }
                if let Some(&(id, _)) = opened.get((*active).min(opened.len().saturating_sub(1))) {
                    let _ = self.ws.panes.set_active_tab(pane, id);
                }
                if let Some(&(id, _)) = opened.iter().find(|(_, focused)| *focused) {
                    *focus = Some(id);
                }
            }
            NodeSnapshot::Split {
                vertical,
                ratio,
                first,
                second,
            } => {
                let axis = if *vertical {
                    SplitAxis::Vertical
                } else {
                    SplitAxis::Horizontal
                };
                match self.ws.panes.split_with_ratio(pane, axis, *ratio) {
                    Ok(sibling) => {
                        self.restore_node(first, pane, focus);
                        self.restore_node(second, sibling, focus);
                    }
                    Err(_) => {
                        // Degenerate snapshot: flatten both sides into the
                        // surviving pane rather than dropping documents.
                        self.restore_node(first, pane, focus);
                        self.restore_node(second, pane, focus);
                    }
                }
            }
        }
    }

    fn save_focused(&mut self) {
        let root = self.vault_root.clone();
        let Some(session) = self.focused_md_mut() else {
            return;
        };
        let abs = root.join(&session.rel_path);
        let text = session.doc.buffer().text();
        match md3_vault::atomic_save(&abs, text.as_bytes()) {
            Ok(()) => {
                session.doc.mark_saved();
                let rel = session.rel_path.clone();
                self.status = format!("saved {rel}");
                // Keep the search index converged with the save.
                if let Some(index) = self.index.as_mut() {
                    let _ = index.sync_paths(&root, &[abs]);
                }
                if self.tree_open {
                    self.files = scan_vault(&root);
                }
            }
            Err(e) => self.status = format!("save failed: {e}"),
        }
    }

    /// `pdf.highlight`: persist the focused PDF's text selection as an
    /// annotation and pick it (so ctrl+n annotates it immediately).
    fn highlight_selection(&mut self) {
        self.ensure_annotations();
        let Some(doc) = self.ws.focused_tab().and_then(|t| self.tab_document(t)) else {
            return;
        };
        let (Some(store), Some(session)) =
            (self.annotations.as_mut(), self.sessions.pdf.get_mut(&doc))
        else {
            return;
        };
        let Some(hash) = session.doc_hash.clone() else {
            self.status = "cannot annotate: file was unreadable on open".to_string();
            return;
        };
        let Some(sel) = session.selection.take_if(|s| !s.text.is_empty()) else {
            self.status = "select text first (drag over the page)".to_string();
            return;
        };
        let new = NewAnnotation {
            doc_hash: hash,
            page: sel.page,
            quads: sel
                .quads
                .iter()
                .map(|q| Quad {
                    x0: f64::from(q.x0),
                    y0: f64::from(q.y0),
                    x1: f64::from(q.x1),
                    y1: f64::from(q.y1),
                })
                .collect(),
            color: HIGHLIGHT_COLOR.to_string(),
            note: String::new(),
            linked_note: None,
        };
        match store.add(new) {
            Ok(id) => {
                session.selected_annotation = Some(id);
                refresh_annotations(store, session);
                self.status = "highlighted · ctrl+n adds a note".to_string();
            }
            Err(e) => {
                session.selection = Some(sel); // keep it; the user can retry
                self.status = format!("highlight failed: {e}");
            }
        }
    }

    /// `pdf.annotations-export`: write the focused PDF's annotation summary
    /// as a sibling markdown note in the vault.
    fn export_annotations(&mut self) {
        self.ensure_annotations();
        let root = self.vault_root.clone();
        let Some(session) = self.focused_pdf() else {
            self.status = "focus a pdf to export its annotations".to_string();
            return;
        };
        let (Some(store), Some(hash)) = (self.annotations.as_ref(), session.doc_hash.as_ref())
        else {
            return;
        };
        let rel = format!(
            "{}-annotations.md",
            session.rel_path.trim_end_matches(".pdf")
        );
        let markdown = match store.export_markdown(hash) {
            Ok(md) => md,
            Err(e) => {
                self.status = format!("export failed: {e}");
                return;
            }
        };
        let abs = root.join(&rel);
        match md3_vault::atomic_save(&abs, markdown.as_bytes()) {
            Ok(()) => {
                if let Some(index) = self.index.as_mut() {
                    let _ = index.sync_paths(&root, &[abs]);
                }
                self.status = format!("annotations exported to {rel}");
            }
            Err(e) => self.status = format!("export failed: {e}"),
        }
    }

    /// `pdf.highlight-color`: step the picked highlight through the token
    /// palette (an unknown stored color restarts the cycle).
    fn cycle_highlight_color(&mut self) {
        let Some(doc) = self.ws.focused_tab().and_then(|t| self.tab_document(t)) else {
            return;
        };
        let (Some(store), Some(session)) =
            (self.annotations.as_mut(), self.sessions.pdf.get_mut(&doc))
        else {
            return;
        };
        let Some(current) = session.selected_annotation() else {
            self.status = "click a highlight first".to_string();
            return;
        };
        let id = current.id;
        let next = HIGHLIGHT_PALETTE
            .iter()
            .position(|c| *c == current.color)
            .map(|i| HIGHLIGHT_PALETTE[(i + 1) % HIGHLIGHT_PALETTE.len()])
            .unwrap_or(HIGHLIGHT_PALETTE[0]);
        match store.set_color(id, next) {
            Ok(()) => {
                refresh_annotations(store, session);
                self.status = format!("highlight color {next}");
            }
            Err(e) => self.status = format!("color failed: {e}"),
        }
    }

    /// `pdf.annotation-link-note`: open the picked highlight's linked note,
    /// creating `<stem>-notes.md` beside the PDF on first use (a vault
    /// citizen, like the annotations export) and recording it on the
    /// annotation.
    fn link_note_for_annotation(&mut self) {
        self.ensure_annotations();
        let root = self.vault_root.clone();
        let Some(doc) = self.ws.focused_tab().and_then(|t| self.tab_document(t)) else {
            return;
        };
        let (Some(store), Some(session)) =
            (self.annotations.as_mut(), self.sessions.pdf.get_mut(&doc))
        else {
            return;
        };
        let Some(current) = session.selected_annotation() else {
            self.status = "click a highlight first".to_string();
            return;
        };
        let id = current.id;
        let rel = current
            .linked_note
            .clone()
            .unwrap_or_else(|| format!("{}-notes.md", session.rel_path.trim_end_matches(".pdf")));
        let abs = root.join(&rel);
        if !abs.exists() {
            let seed = format!("# Notes — {}\n", session.rel_path);
            if let Err(e) = md3_vault::atomic_save(&abs, seed.as_bytes()) {
                self.status = format!("linked note failed: {e}");
                return;
            }
            if let Some(index) = self.index.as_mut() {
                let _ = index.sync_paths(&root, &[abs]);
            }
        }
        match store.set_linked_note(id, &rel) {
            Ok(()) => {
                refresh_annotations(store, session);
                self.open_document(&rel);
                self.status = format!("linked note {rel}");
            }
            Err(e) => self.status = format!("linked note failed: {e}"),
        }
    }

    /// `pdf.annotations-orphans`: list sidecar documents whose annotations
    /// no longer match any vault file's content (edited bytes = new
    /// identity; the old annotations stay reachable, never silently drop).
    fn orphan_report(&mut self) {
        self.ensure_annotations();
        let Some(store) = self.annotations.as_ref() else {
            self.status = "annotation store unavailable".to_string();
            return;
        };
        let known = match store.known_documents() {
            Ok(k) => k,
            Err(e) => {
                self.status = format!("orphan report failed: {e}");
                return;
            }
        };
        let live: std::collections::HashSet<String> = scan_vault(&self.vault_root)
            .iter()
            .filter(|rel| rel.ends_with(".pdf"))
            .filter_map(|rel| md3_vault::document_hash(&self.vault_root.join(rel)).ok())
            .collect();
        let rows: Vec<(String, String)> = known
            .iter()
            .filter(|d| d.annotation_count > 0 && !live.contains(&d.doc_hash))
            .map(|d| {
                (
                    d.last_path.clone(),
                    format!("{} annotations", d.annotation_count),
                )
            })
            .collect();
        if rows.is_empty() {
            self.status = "no orphaned annotations".to_string();
            return;
        }
        self.open_overlay(Overlay::OrphanReport { rows });
    }

    /// Confirm handler for the note overlay: overwrite the picked
    /// annotation's note.
    fn set_annotation_note(&mut self, note: &str) {
        let Some(doc) = self.ws.focused_tab().and_then(|t| self.tab_document(t)) else {
            return;
        };
        let (Some(store), Some(session)) =
            (self.annotations.as_mut(), self.sessions.pdf.get_mut(&doc))
        else {
            return;
        };
        let Some(id) = session.selected_annotation else {
            return;
        };
        match store.update_note(id, note) {
            Ok(()) => {
                refresh_annotations(store, session);
                self.status = "note saved".to_string();
            }
            Err(e) => self.status = format!("note failed: {e}"),
        }
    }

    /// Delete on a picked highlight (raw key in the PDF surface).
    fn remove_selected_annotation(&mut self) {
        let Some(doc) = self.ws.focused_tab().and_then(|t| self.tab_document(t)) else {
            return;
        };
        let (Some(store), Some(session)) =
            (self.annotations.as_mut(), self.sessions.pdf.get_mut(&doc))
        else {
            return;
        };
        let Some(id) = session.selected_annotation.take() else {
            return;
        };
        match store.remove(id) {
            Ok(()) => {
                refresh_annotations(store, session);
                self.status = "highlight removed".to_string();
            }
            Err(e) => self.status = format!("remove failed: {e}"),
        }
    }

    fn find_in_note(&mut self, needle: &str) {
        if needle.is_empty() {
            return;
        }
        let Some(session) = self.focused_md_mut() else {
            return;
        };
        let text = session.doc.buffer().text();
        let from = session.doc.buffer().primary().head;
        let hit = text[from.min(text.len())..]
            .find(needle)
            .map(|i| i + from)
            .or_else(|| text.find(needle));
        match hit {
            Some(offset) => {
                let (line, col) = session.doc.buffer().offset_to_line_col(offset);
                session.apply(Command::SetCursor { line, col });
            }
            None => self.status = format!("not found: {needle}"),
        }
    }

    /// `pdf.find`: load glyph geometry for every page once (cached on the
    /// session afterwards), then open the live-filtering match overlay.
    fn open_pdf_find(&mut self) {
        let root = self.vault_root.clone();
        let Some(session) = self.focused_pdf_mut() else {
            self.status = "find: no pdf focused".to_string();
            return;
        };
        if session.layout.is_none() {
            self.status = "find: pdf pages not loaded".to_string();
            return;
        }
        let abs = root.join(&session.rel_path);
        let count = session.page_count();
        let cap = count.min(200);
        for page in 0..cap as u32 {
            pdf_view::load_page_chars(session, &abs, page);
        }
        self.open_overlay(Overlay::PdfFind {
            input: String::new(),
            selected: 0,
            hits: Vec::new(),
        });
        if count > 200 {
            self.status = format!("find: searching first 200 of {count} pages");
        }
    }

    /// `pdf.toc`: the outline as a filterable jump list, depth as
    /// indentation. Loaded with the geometry on open, so this is a snapshot.
    fn open_pdf_toc(&mut self) {
        let Some(session) = self.focused_pdf() else {
            self.status = "toc: no pdf focused".to_string();
            return;
        };
        if session.outline.is_empty() {
            self.status = "this pdf has no table of contents".to_string();
            return;
        }
        let entries = session
            .outline
            .iter()
            .map(|e| {
                let indent = "  ".repeat(usize::from(e.depth));
                (format!("{indent}{}", e.title), e.page)
            })
            .collect();
        self.open_overlay(Overlay::PdfToc {
            input: String::new(),
            selected: 0,
            entries,
        });
    }

    /// `pdf.back` / `pdf.forward`: walk the focused PDF's jump history.
    fn pdf_nav_history(&mut self, back: bool) {
        let root = self.vault_root.clone();
        let Some(session) = self.focused_pdf_mut() else {
            self.status = "history: no pdf focused".to_string();
            return;
        };
        let moved = if back {
            session.nav_back()
        } else {
            session.nav_forward()
        };
        if !moved {
            self.status = format!(
                "nothing to go {}",
                if back { "back to" } else { "forward to" }
            );
            return;
        }
        let abs = root.join(&session.rel_path);
        pdf_view::ensure_tiles(session, &abs);
        self.sync_status();
    }

    /// All matches for `query` across the focused PDF, in page order,
    /// capped — enough to scroll a hit list, no reason to mine more.
    fn pdf_find_hits(&self, query: &str) -> Vec<PdfFindHit> {
        const CAP: usize = 100;
        let query = query.trim();
        let mut out = Vec::new();
        if query.is_empty() {
            return out;
        }
        let Some(session) = self.focused_pdf() else {
            return out;
        };
        let mut pages: Vec<u32> = session.chars.keys().copied().collect();
        pages.sort_unstable();
        for page in pages {
            let Some(chars) = session.chars.get(&page) else {
                continue;
            };
            for range in md3_pdf::select::find(chars, query) {
                let Some(sel) = md3_pdf::select::range_selection(chars, range.clone()) else {
                    continue;
                };
                let ctx = range.start.saturating_sub(12)..(range.end + 28).min(chars.len());
                out.push(PdfFindHit {
                    page,
                    quads: sel.quads,
                    text: sel.text,
                    preview: chars[ctx].iter().map(|c| c.ch).collect(),
                });
                if out.len() >= CAP {
                    return out;
                }
            }
        }
        out
    }

    /// Scroll the match a third down the viewport and plant it as the live
    /// selection — the tint marks it and `ctrl+h` can highlight it directly.
    fn jump_to_pdf_match(&mut self, hit: &PdfFindHit) {
        let root = self.vault_root.clone();
        let Some(session) = self.focused_pdf_mut() else {
            return;
        };
        let Some(target) = session.layout.as_ref().map(|layout| {
            let y = hit.quads.first().map_or(0.0, |q| q.y0);
            let top = layout.page_top(hit.page as usize) + y * layout.zoom();
            (top - session.viewport.1 / 3.0).clamp(0.0, layout.max_scroll(session.viewport.1))
        }) else {
            return;
        };
        session.record_jump();
        session.scroll = target;
        session.selected_annotation = None;
        session.selection = Some(PdfSelection {
            page: hit.page,
            anchor: hit.quads.first().map_or((0.0, 0.0), |q| (q.x0, q.y0)),
            quads: hit.quads.clone(),
            text: hit.text.clone(),
        });
        let abs = root.join(&session.rel_path);
        pdf_view::ensure_tiles(session, &abs);
        self.status = format!("match on p. {} · ctrl+h highlights", hit.page + 1);
    }

    fn cycle_tab(&mut self) {
        let Some(current) = self.ws.focused_tab() else {
            return;
        };
        let Some((pane, _)) = self.ws.panes.find_tab(current) else {
            return;
        };
        let tabs = pane.tabs();
        let Some(i) = tabs.iter().position(|t| t.id == current) else {
            return;
        };
        let next = tabs[(i + 1) % tabs.len()].id;
        if let Err(e) = self.ws.focus_tab(next) {
            self.status = e.to_string();
        }
    }

    fn ensure_index(&mut self) {
        if self.index.is_some() {
            return;
        }
        // Persistent sidecar first (incremental across runs); in-memory is
        // the read-only-vault fallback so search still works.
        let opened = self
            .open_sidecar_dir()
            .and_then(|_| SearchIndex::open(&self.sidecar_path()))
            .or_else(|_| SearchIndex::open_in_memory());
        match opened {
            Ok(mut index) => {
                let synced = self.sync_index(&mut index);
                if let Err(e) = synced {
                    self.status = format!("index: {e}");
                }
                self.index = Some(index);
            }
            Err(e) => self.status = format!("index: {e}"),
        }
    }

    /// Open the annotation store on the sidecar; report (once per attempt)
    /// rather than fail — annotations degrade, the document still opens.
    fn ensure_annotations(&mut self) {
        if self.annotations.is_some() {
            return;
        }
        let opened = self
            .open_sidecar_dir()
            .and_then(|_| AnnotationStore::open(&self.sidecar_path()));
        match opened {
            Ok(store) => self.annotations = Some(store),
            Err(e) => self.status = format!("annotations unavailable: {e}"),
        }
    }

    fn open_sidecar_dir(&self) -> Result<(), md3_vault::VaultError> {
        let dir = self.vault_root.join(".md3");
        std::fs::create_dir_all(&dir).map_err(|e| md3_vault::VaultError::io(&dir, e))
    }

    /// Sync the FTS index; with the pdfium feature on, PDFs are indexed
    /// through the vault's `TextExtractor` seam — the production composition
    /// the engine crates deliberately leave to the shell.
    fn sync_index(&self, index: &mut SearchIndex) -> Result<(), md3_vault::VaultError> {
        #[cfg(feature = "pdfium")]
        {
            if let Some(renderer) = pdf_view::renderer() {
                let extractor = PdfTextExtractor(renderer);
                index.sync_with(&self.vault_root, Some(&extractor))?;
                return Ok(());
            }
        }
        index.sync(&self.vault_root)?;
        Ok(())
    }

    /// Referrers of `rel_path` per the wikilink graph, built fresh from the
    /// vault's notes on every call (mirrors quick-open's rescan: always
    /// current, vault-sized work — a cached graph + watcher refresh is the
    /// upgrade path if vaults outgrow it).
    fn note_backlinks(&self, rel_path: &str) -> Vec<String> {
        let mut graph = md3_vault::LinkGraph::new();
        for rel in scan_vault(&self.vault_root) {
            if !rel.ends_with(".md") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(self.vault_root.join(&rel)) else {
                continue;
            };
            graph.update_file(Path::new(&rel), &content);
        }
        graph
            .backlinks(Path::new(rel_path))
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect()
    }

    fn search_vault(&mut self, query: &str) -> Vec<md3_vault::Hit> {
        let Some(index) = self.index.as_ref() else {
            return Vec::new();
        };
        // The overlay list scrolls, so this bound is about FTS query cost,
        // not what fits on screen.
        index.search(query, 50).unwrap_or_default()
    }

    // ------------------------------------------------------------ helpers --

    fn tab_document(&self, tab: TabId) -> Option<DocumentId> {
        self.ws.panes.find_tab(tab).map(|(_, t)| t.document)
    }

    fn focused_doc_info(&self) -> Option<(String, EditorKind)> {
        let tab = self.ws.focused_tab()?;
        let (_, tab) = self.ws.panes.find_tab(tab)?;
        let doc = self.ws.docs.get(tab.document)?;
        Some((doc.path.clone(), tab.editor))
    }

    fn focused_md_mut(&mut self) -> Option<&mut MdSession> {
        let tab = self.ws.focused_tab()?;
        let doc = self.tab_document(tab)?;
        self.sessions.md.get_mut(&doc)
    }

    fn focused_pdf_mut(&mut self) -> Option<&mut PdfSession> {
        let tab = self.ws.focused_tab()?;
        let doc = self.tab_document(tab)?;
        self.sessions.pdf.get_mut(&doc)
    }

    fn sync_status(&mut self) {
        // Status defaults to caret position; commands overwrite it for a beat.
        let Some(tab) = self.ws.focused_tab() else {
            return;
        };
        let Some(doc) = self.tab_document(tab) else {
            return;
        };
        if let Some(session) = self.sessions.md.get(&doc) {
            let head = session.doc.buffer().primary().head;
            let (line, col) = session.doc.buffer().offset_to_line_col(head);
            let dirty = if session.doc.buffer().is_dirty() {
                " ●"
            } else {
                ""
            };
            self.status = format!("{}{dirty} — Ln {}, Col {}", session.rel_path, line + 1, col);
        } else if let Some(session) = self.sessions.pdf.get(&doc)
            && session.layout.is_some()
        {
            let section = session
                .current_section()
                .map(|t| format!(" · § {t}"))
                .unwrap_or_default();
            self.status = format!(
                "{} — p. {}/{} · {:.0}%{section}",
                session.rel_path,
                session.current_page() + 1,
                session.page_count(),
                session.zoom * 100.0
            );
        }
    }

    // --------------------------------------------------------------- view --

    fn view(&self) -> Element<'_, Message> {
        let workspace = self.layout_view(&self.ws.panes.layout());

        let mut row_children = Vec::new();

        if self.tree_open {
            let rows = file_tree::visible_rows(&self.files, &self.tree_expanded);
            let focused_path = self
                .ws
                .focused_tab()
                .and_then(|t| self.tab_document(t))
                .and_then(|d| self.ws.docs.get(d))
                .map(|doc| doc.path.as_str());

            let mut col = column![].spacing(2);
            for row in rows {
                let indent = row.depth as f32 * 14.0;
                let marker = if row.is_dir {
                    if self.tree_expanded.contains(&row.rel_path) {
                        "▾ "
                    } else {
                        "▸ "
                    }
                } else {
                    "  "
                };

                let is_active = focused_path.is_some_and(|p| p == row.rel_path);
                let text_color = if is_active {
                    tokens::dark().accent
                } else {
                    colors::text()
                };

                let content = row![
                    iced::widget::Space::new().width(indent),
                    text(format!("{marker}{}", row.label))
                        .size(13)
                        .color(text_color)
                ]
                .align_y(iced::Alignment::Center)
                .spacing(4);

                let msg = if row.is_dir {
                    Message::TreeDirToggled(row.rel_path.clone())
                } else {
                    Message::TreeFileClicked(row.rel_path.clone())
                };

                let item = iced::widget::mouse_area(content).on_press(msg);
                col = col.push(item);
            }

            let sidebar_bg = tokens::dark().bg_secondary;
            let border_color = tokens::dark().border;

            let panel = container(iced::widget::scrollable(col))
                .width(240)
                .height(Fill)
                .padding(8)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(sidebar_bg)),
                    border: iced::Border {
                        color: border_color,
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..container::Style::default()
                });

            row_children.push(panel.into());
        }

        row_children.push(container(workspace).width(Fill).height(Fill).into());

        if self.tracker_open {
            let tracker_panel = tracker_view::view(
                self.tracker_open,
                self.tracker_running,
                &self.tracker_sessions,
                &self.tracker_kv,
                self.tracker_active_tab,
                &self.tracker_config_json,
                &self.tracker_manual_date,
                &self.tracker_manual_hours,
                &self.tracker_manual_notes,
            );
            row_children.push(tracker_panel);
        }

        let workspace_content = row(row_children).spacing(0);

        let status = container(text(self.status.clone()).size(13).color(colors::marker()))
            .padding([4, 10])
            .width(Fill);

        let base = column![container(workspace_content).height(Fill), status];
        match &self.overlay {
            Some(ov) => stack![base, overlay::view(ov, &self.registry, &self.files)].into(),
            None => base.into(),
        }
    }

    fn layout_view<'a>(&'a self, node: &Layout<'a>) -> Element<'a, Message> {
        match node {
            Layout::Pane(pane) => self.pane_view(pane),
            Layout::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let a = container(self.layout_view(first)).width(Fill).height(Fill);
                let b = container(self.layout_view(second)).width(Fill).height(Fill);
                let (pa, pb) = (
                    ((ratio * 1000.0) as u16).max(1),
                    (((1.0 - ratio) * 1000.0) as u16).max(1),
                );
                match axis {
                    SplitAxis::Horizontal => row![
                        a.width(iced::Length::FillPortion(pa)),
                        b.width(iced::Length::FillPortion(pb))
                    ]
                    .spacing(2)
                    .into(),
                    SplitAxis::Vertical => column![
                        a.height(iced::Length::FillPortion(pa)),
                        b.height(iced::Length::FillPortion(pb))
                    ]
                    .spacing(2)
                    .into(),
                }
            }
        }
    }

    fn pane_view<'a>(&'a self, pane: &Pane) -> Element<'a, Message> {
        let focused_tab = self.ws.focused_tab();
        let pane_focused = self.ws.focused_pane() == Some(pane.id);

        let mut strip = row![].spacing(2).padding(2);
        for tab in pane.tabs() {
            let title = self
                .ws
                .docs
                .get(tab.document)
                .map(|d| {
                    let name = d.path.rsplit('/').next().unwrap_or(&d.path);
                    let dirty = self
                        .sessions
                        .md
                        .get(&tab.document)
                        .is_some_and(|s| s.doc.buffer().is_dirty());
                    if dirty {
                        format!("{name} ●")
                    } else {
                        name.to_string()
                    }
                })
                .unwrap_or_else(|| "?".to_string());
            let active =
                focused_tab == Some(tab.id) || pane.active_tab().map(|t| t.id) == Some(tab.id);
            strip = strip.push(
                button(text(title).size(13))
                    .padding([3, 10])
                    .style(move |theme, status| {
                        if active && pane_focused {
                            button::primary(theme, status)
                        } else if active {
                            button::secondary(theme, status)
                        } else {
                            button::text(theme, status)
                        }
                    })
                    .on_press(Message::TabSelected(tab.id)),
            );
        }

        let content: Element<'_, Message> = match pane.active_tab() {
            None => container(
                text("ctrl+p to open a file · ctrl+shift+p for commands")
                    .size(14)
                    .color(colors::marker()),
            )
            .center(Fill)
            .into(),
            Some(tab) => {
                let focused = focused_tab == Some(tab.id);
                match tab.editor {
                    EditorKind::Markdown => match self.sessions.md.get(&tab.document) {
                        Some(session) => canvas(EditorCanvas {
                            tab: tab.id,
                            session,
                            focused,
                        })
                        .width(Fill)
                        .height(Fill)
                        .into(),
                        None => missing_session(),
                    },
                    EditorKind::Pdf => match self.sessions.pdf.get(&tab.document) {
                        Some(session) => pdf_view::view(session, tab.id),
                        None => missing_session(),
                    },
                    _ => container(text("unsupported editor kind").color(colors::marker()))
                        .center(Fill)
                        .into(),
                }
            }
        };

        let border_color = if pane_focused {
            tokens::dark().accent
        } else {
            tokens::dark().border
        };
        container(column![strip, container(content).height(Fill)])
            .style(move |_| container::Style {
                border: iced::Border {
                    color: border_color,
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..container::Style::default()
            })
            .width(Fill)
            .height(Fill)
            .into()
    }
}

/// Re-read a document's annotations from the store into its session — the
/// canvas paints only from the session cache, so every mutation refreshes.
fn refresh_annotations(store: &AnnotationStore, session: &mut PdfSession) {
    if let Some(hash) = &session.doc_hash {
        session.annotations = store.annotations_for(hash).unwrap_or_default();
    }
    // A removed/missing id must not linger as a phantom pick.
    if let Some(id) = session.selected_annotation
        && !session.annotations.iter().any(|a| a.id == id)
    {
        session.selected_annotation = None;
    }
}

/// Production PDF→FTS composition (the bridge the engines leave to the
/// shell): every page's text, concatenated; failures yield `None` so the
/// index records-without-retrying.
#[cfg(feature = "pdfium")]
struct PdfTextExtractor(&'static md3_pdf::render::PdfRenderer);

#[cfg(feature = "pdfium")]
impl md3_vault::TextExtractor for PdfTextExtractor {
    fn extract(&self, abs_path: &Path) -> Option<String> {
        let pages = self.0.page_count(abs_path).ok()?;
        let mut out = String::new();
        for page in 0..u32::from(pages) {
            out.push_str(&self.0.extract_text(abs_path, page).ok()?);
            out.push('\n');
        }
        Some(out)
    }
}

fn missing_session<'a>() -> Element<'a, Message> {
    container(text("document failed to load").color(colors::marker()))
        .center(Fill)
        .into()
}

/// Vault file scan for quick-open: `.md` and `.pdf`, vault-relative, sorted;
/// dot-directories skipped (mirrors the index walk).
fn scan_vault(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "md" || e == "pdf")
            && let Ok(rel) = path.strip_prefix(root)
        {
            out.push(rel.to_string_lossy().to_string());
        }
    }
}

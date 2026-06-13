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

mod chrome_context;
mod commands_file;
mod commands_md;
mod commands_pdf_annotations;
mod commands_pdf_nav;
pub mod drag;
pub mod editor_canvas;
pub mod file_tree;
pub mod icons;
pub mod keys;
pub mod menu;
pub mod overlay;
pub mod paint;
mod pdf_input;
mod pdf_view;
mod pdf_worker_events;
pub mod session;
mod session_persist;
pub mod snapshot;
mod status;
mod toast;
pub mod tokens;
pub mod tracker_view;
pub mod welcome;
pub mod worker;

use std::path::{Path, PathBuf};

use iced::widget::{button, canvas, column, container, mouse_area, row, stack, text, text_input};
use iced::{Element, Fill, Subscription, Task};
use md3_editor::buffer::{Command, Movement, Selection};
use md3_kernel::input::{Chord, EditorKind, Key};
use md3_kernel::pane::{DocumentId, Layout, Pane, PaneId, SplitPath, TabId};
use md3_kernel::{CommandId, CommandRegistry, Keymap, SplitAxis, Workspace};
use md3_vault::{AnnotationStore, SearchIndex, SessionStore};

use editor_canvas::{EditorCanvas, palette as colors};
use overlay::{NamePurpose, Overlay, PdfFindHit};
use session::{MdSession, PdfFitMode, PdfSelection, PdfSession, Sessions};
use snapshot::{NodeSnapshot, SessionSnapshot, TabSnapshot, ViewSnapshot};

/// Highlight color cycle (`pdf.highlight-color`); new annotations start at
/// the first entry. Stored per annotation (`#rrggbb`, schema column).
const HIGHLIGHT_PALETTE: [&str; 4] = ["#ffd866", "#a9dc76", "#78dce8", "#ab9df2"];

/// Default highlight color for new annotations.
const HIGHLIGHT_COLOR: &str = HIGHLIGHT_PALETTE[0];

const BOLD: iced::Font = iced::Font {
    weight: iced::font::Weight::Bold,
    ..iced::Font::DEFAULT
};

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
    TabCloseClicked(TabId),
    PaneCommand {
        pane: PaneId,
        command: CommandId,
    },
    SplitRatioDragged {
        path: SplitPath,
        ratio: f32,
    },
    SplitRatioDragFinished,
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
    EditorViewportChanged {
        tab: TabId,
        width: f32,
        height: f32,
    },
    PdfScrolled {
        tab: TabId,
        dy: f32,
        viewport: (f32, f32),
    },
    PdfViewportChanged {
        tab: TabId,
        width: f32,
        height: f32,
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
        abs_pos: (f32, f32),
        viewport: (f32, f32),
    },
    PdfCommand {
        tab: TabId,
        command: CommandId,
    },
    OverlayPick(usize),
    WindowCloseRequested,
    TreeFileClicked(String),
    TreeDirToggled(String),
    TreeContextRequested {
        rel_path: String,
        is_dir: bool,
    },
    TreeContextCommand(CommandId),
    TreeContextOpen {
        split: bool,
    },
    TreeContextClosed,
    TreeResizeStarted,
    TreeResized(f32),
    TreeResizeFinished,
    VaultPicked(Option<PathBuf>),
    Tracker(tracker_view::TrackerMessage),
    RunCommand(CommandId),
    MenuToggled(menu::MenuId),
    MenuClosed,
    MenuCommand(CommandId),
    MdJumpToLine {
        tab: TabId,
        line: usize,
    },
    MdFindQueryChanged {
        tab: TabId,
        query: String,
    },
    MdReplaceTextChanged {
        tab: TabId,
        text: String,
    },
    MdFindNext {
        tab: TabId,
    },
    MdFindPrev {
        tab: TabId,
    },
    MdReplace {
        tab: TabId,
    },
    MdReplaceAll {
        tab: TabId,
    },
    MdCloseFind {
        tab: TabId,
    },
    PdfWorkerReady(worker::WorkerHandle),
    PdfWorker(worker::PdfJobOutput),
    PdfJumpToPage {
        tab: TabId,
        page: usize,
    },
    PdfJumpToAnnotation {
        tab: TabId,
        annotation_id: i64,
    },
    PdfDeleteAnnotation {
        tab: TabId,
        annotation_id: i64,
    },
    PdfEditAnnotationNote {
        tab: TabId,
        annotation_id: i64,
    },
    PdfCycleAnnotationColor {
        tab: TabId,
        annotation_id: i64,
    },
    PanelResized {
        kind: drag::PanelKind,
        width: f32,
    },
    PanelResizeFinished {
        kind: drag::PanelKind,
    },
    PdfContextMenuClosed,
    PdfContextMenuCommand {
        tab: TabId,
        command: CommandId,
    },
    DismissToast(usize),
    CloseToastClicked(usize),
    SettingsThemeChanged(String),
    SettingsScopeChanged(usize, String),
    SettingsChordChanged(usize, String),
    SettingsCommandChanged(usize, String),
    SettingsAddRow,
    SettingsRemoveRow(usize),
    SettingsSave,
    SettingsCancel,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub id: usize,
    pub kind: ToastKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
    Warning,
}

pub struct PdfContextMenuState {
    pub tab: TabId,
    pub abs_pos: (f32, f32),
}

pub struct Shell {
    registry: CommandRegistry,
    keymap: Keymap,
    ws: Workspace,
    sessions: Sessions,
    vault_root: PathBuf,
    overlay: Option<Overlay>,
    open_menu: Option<menu::MenuId>,
    /// Vault files (relative paths) for quick-open; rescanned on open.
    files: Vec<String>,
    /// FTS index, opened lazily on first vault search; persists in the
    /// vault sidecar so an unchanged vault re-reads nothing across runs.
    index: Option<SearchIndex>,
    /// Annotation store on the same sidecar, opened lazily on first PDF.
    annotations: Option<AnnotationStore>,
    /// Session store on the same sidecar; opened on startup for restore.
    session: Option<SessionStore>,
    pdf_worker: Option<worker::WorkerHandle>,
    /// Transient user-facing command result, warning, or error.
    status: String,
    /// Focused document position. Only [`Self::sync_status`] writes this.
    position_status: String,
    last_command: Option<CommandId>,
    tree_open: bool,
    tree_expanded: std::collections::BTreeSet<String>,
    tree_selected: Option<String>,
    tree_width: f32,
    tree_resizing: bool,
    tree_context: Option<(String, bool)>,
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
    pdf_context_menu: Option<PdfContextMenuState>,
    toasts: Vec<Toast>,
    next_toast_id: usize,
    theme_name: String,
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
            open_menu: None,
            files: Vec::new(),
            index: None,
            annotations: None,
            session: None,
            pdf_worker: None,
            status: String::new(),
            position_status: String::new(),
            last_command: None,
            tree_open: true,
            tree_expanded: std::collections::BTreeSet::new(),
            tree_selected: None,
            tree_width: 240.0,
            tree_resizing: false,
            tree_context: None,
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
            pdf_context_menu: None,
            toasts: Vec::new(),
            next_toast_id: 0,
            theme_name: "dark".to_string(),
        };
        shell.restore_session();
        if shell.tree_open {
            shell.files = scan_vault(&shell.vault_root);
        }
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
        if self.status.is_empty() {
            &self.position_status
        } else {
            &self.status
        }
    }

    pub fn toasts(&self) -> &[Toast] {
        &self.toasts
    }

    pub fn last_command(&self) -> Option<CommandId> {
        self.last_command
    }

    pub fn tree_open(&self) -> bool {
        self.tree_open
    }

    pub fn tree_files(&self) -> &[String] {
        &self.files
    }

    pub fn tracker_open(&self) -> bool {
        self.tracker_open
    }

    pub fn open_menu(&self) -> Option<menu::MenuId> {
        self.open_menu
    }

    pub fn tree_expanded(&self) -> &std::collections::BTreeSet<String> {
        &self.tree_expanded
    }

    pub fn tree_width(&self) -> f32 {
        self.tree_width
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

    pub(crate) fn theme(&self) -> iced::Theme {
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

    pub(crate) fn subscription(&self) -> Subscription<Message> {
        let subscriptions = vec![
            iced::keyboard::listen().map(|event| match keys::normalize(&event) {
                Some(ev) => Message::Key(ev),
                None => Message::Ignored,
            }),
            iced::window::close_requests().map(|_| Message::WindowCloseRequested),
        ];
        #[cfg(feature = "pdfium")]
        {
            let mut subscriptions = subscriptions;
            subscriptions.push(Subscription::run(worker::subscribe));
            Subscription::batch(subscriptions)
        }
        #[cfg(not(feature = "pdfium"))]
        {
            Subscription::batch(subscriptions)
        }
    }

    // ------------------------------------------------------------- update --

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Ignored => Task::none(),
            Message::Key(ev) => self.on_key(ev),
            Message::RunCommand(command) => self.run_command(command),
            Message::DismissToast(id) => {
                self.toasts.retain(|t| t.id != id);
                Task::none()
            }
            Message::CloseToastClicked(id) => {
                self.toasts.retain(|t| t.id != id);
                Task::none()
            }
            Message::SettingsThemeChanged(theme) => {
                if let Some(Overlay::Settings { theme: t, .. }) = &mut self.overlay {
                    *t = theme;
                }
                Task::none()
            }
            Message::SettingsScopeChanged(idx, val) => {
                if let Some(Overlay::Settings { keymap, .. }) = &mut self.overlay
                    && let Some(row) = keymap.bindings.get_mut(idx)
                {
                    row.scope = val;
                }
                Task::none()
            }
            Message::SettingsChordChanged(idx, val) => {
                if let Some(Overlay::Settings { keymap, .. }) = &mut self.overlay
                    && let Some(row) = keymap.bindings.get_mut(idx)
                {
                    row.chord = val;
                }
                Task::none()
            }
            Message::SettingsCommandChanged(idx, val) => {
                if let Some(Overlay::Settings { keymap, .. }) = &mut self.overlay
                    && let Some(row) = keymap.bindings.get_mut(idx)
                {
                    row.command = if val.trim().is_empty() {
                        None
                    } else {
                        Some(val)
                    };
                }
                Task::none()
            }
            Message::SettingsAddRow => {
                if let Some(Overlay::Settings { keymap, .. }) = &mut self.overlay {
                    keymap.bindings.push(crate::settings::BindingRow {
                        scope: "workspace".to_string(),
                        chord: String::new(),
                        command: None,
                    });
                }
                Task::none()
            }
            Message::SettingsRemoveRow(idx) => {
                if let Some(Overlay::Settings { keymap, .. }) = &mut self.overlay
                    && idx < keymap.bindings.len()
                {
                    keymap.bindings.remove(idx);
                }
                Task::none()
            }
            Message::SettingsSave => {
                if let Some(Overlay::Settings {
                    theme,
                    keymap,
                    error: _,
                }) = self.overlay.clone()
                {
                    match crate::settings::validate_overrides(&self.registry, &keymap) {
                        Ok(()) => {
                            if let Err(e) =
                                crate::settings::save_keymap_overrides(&self.vault_root, &keymap)
                            {
                                if let Some(Overlay::Settings {
                                    error: err_field, ..
                                }) = &mut self.overlay
                                {
                                    *err_field = Some(e);
                                }
                            } else {
                                self.theme_name = theme;
                                tokens::set_light_theme(self.theme_name == "light");
                                self.reload_keymap();
                                self.close_overlay();
                                self.save_session();
                                return self.success("Settings saved successfully");
                            }
                        }
                        Err(e) => {
                            if let Some(Overlay::Settings {
                                error: err_field, ..
                            }) = &mut self.overlay
                            {
                                *err_field = Some(e);
                            }
                        }
                    }
                }
                Task::none()
            }
            Message::SettingsCancel => {
                self.close_overlay();
                Task::none()
            }
            Message::MenuToggled(menu) => {
                if self.open_menu == Some(menu) {
                    self.close_menu();
                } else {
                    self.close_overlay();
                    self.open_menu = Some(menu);
                    self.ws.open_overlay("menu");
                }
                Task::none()
            }
            Message::MenuClosed => {
                self.close_menu();
                Task::none()
            }
            Message::MenuCommand(command) => {
                self.close_menu();
                self.run_command(command)
            }
            Message::TabSelected(tab) => {
                if let Err(e) = self.ws.focus_tab(tab) {
                    self.status = e.to_string();
                }
                self.save_session();
                Task::none()
            }
            Message::TabCloseClicked(tab) => {
                self.close_tab(tab);
                Task::none()
            }
            Message::PaneCommand { pane, command } => self.run_pane_command(pane, command),
            Message::SplitRatioDragged { path, ratio } => {
                if let Err(error) = self.ws.panes.set_ratio(&path, ratio) {
                    self.status = error.to_string();
                }
                Task::none()
            }
            Message::SplitRatioDragFinished => {
                self.save_session();
                Task::none()
            }
            Message::WindowCloseRequested => {
                if self.is_any_tab_dirty() {
                    self.open_overlay(Overlay::Confirm {
                        message: "Abandon unsaved changes and quit?".to_string(),
                        on_confirm: CommandId("app.force-quit"),
                    });
                    Task::none()
                } else {
                    self.save_session();
                    iced::exit()
                }
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
            Message::EditorViewportChanged { tab, width, height } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    session.set_viewport(width, height);
                }
                Task::none()
            }
            Message::PdfViewportChanged { tab, width, height } => {
                let root = self.vault_root.clone();
                let worker = self.pdf_worker.clone();
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.pdf.get_mut(&d)) {
                    session.set_viewport((width, height));
                    let abs = root.join(&session.rel_path);
                    pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                }
                Task::none()
            }
            Message::PdfScrolled { tab, dy, viewport } => {
                if let Err(e) = self.ws.focus_tab(tab) {
                    self.status = e.to_string();
                }
                let root = self.vault_root.clone();
                let worker = self.pdf_worker.clone();
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.pdf.get_mut(&d)) {
                    session.viewport = viewport;
                    session.scroll_by(dy);
                    let abs = root.join(&session.rel_path);
                    pdf_view::ensure_tiles(session, &abs, worker.as_ref());
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
            Message::PdfRightClick {
                tab,
                pos,
                abs_pos,
                viewport,
            } => {
                if let Err(e) = self.ws.focus_tab(tab) {
                    self.status = e.to_string();
                }
                self.pdf_right_click(tab, pos, abs_pos, viewport);
                Task::none()
            }
            Message::MdJumpToLine { tab, line } => {
                if let Err(error) = self.ws.focus_tab(tab) {
                    self.status = error.to_string();
                    return Task::none();
                }
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    session.apply(Command::SetCursor { line, col: 0 });
                }
                self.sync_status();
                Task::none()
            }
            Message::MdFindQueryChanged { tab, query } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    session.find_query = query;
                    let text = session.doc.buffer().text();
                    let matches = find_all_matches(&text, &session.find_query);
                    if !matches.is_empty() {
                        let head = session.doc.buffer().primary().head;
                        let target = matches
                            .iter()
                            .find(|&&(start, _)| start >= head)
                            .or(matches.first())
                            .copied();
                        if let Some((start, end)) = target {
                            session.apply(Command::SetSelections(vec![Selection::new(start, end)]));
                        }
                    }
                }
                Task::none()
            }
            Message::MdReplaceTextChanged { tab, text } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    session.replace_text = text;
                }
                Task::none()
            }
            Message::MdFindNext { tab } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    let text = session.doc.buffer().text();
                    let matches = find_all_matches(&text, &session.find_query);
                    if !matches.is_empty() {
                        let head = session.doc.buffer().primary().head;
                        let target = matches
                            .iter()
                            .find(|&&(start, _)| start >= head)
                            .or(matches.first())
                            .copied();
                        if let Some((start, end)) = target {
                            session.apply(Command::SetSelections(vec![Selection::new(start, end)]));
                        }
                    }
                }
                Task::none()
            }
            Message::MdFindPrev { tab } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    let text = session.doc.buffer().text();
                    let matches = find_all_matches(&text, &session.find_query);
                    if !matches.is_empty() {
                        let primary = session.doc.buffer().primary();
                        let caret_start = primary.anchor.min(primary.head);
                        let target = matches
                            .iter()
                            .rfind(|&&(start, _)| start < caret_start)
                            .or(matches.last())
                            .copied();
                        if let Some((start, end)) = target {
                            session.apply(Command::SetSelections(vec![Selection::new(start, end)]));
                        }
                    }
                }
                Task::none()
            }
            Message::MdReplace { tab } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    let text = session.doc.buffer().text();
                    let primary = session.doc.buffer().primary();
                    let (caret_start, caret_end) = (
                        primary.anchor.min(primary.head),
                        primary.anchor.max(primary.head),
                    );
                    let matches = find_all_matches(&text, &session.find_query);
                    if matches
                        .iter()
                        .any(|&(s, e)| s == caret_start && e == caret_end)
                    {
                        session.apply(Command::Insert(session.replace_text.clone()));
                        let new_text = session.doc.buffer().text();
                        let new_matches = find_all_matches(&new_text, &session.find_query);
                        if !new_matches.is_empty() {
                            let new_head = session.doc.buffer().primary().head;
                            let next_match = new_matches
                                .iter()
                                .find(|&&(start, _)| start >= new_head)
                                .or(new_matches.first())
                                .copied();
                            if let Some((start, end)) = next_match {
                                session.apply(Command::SetSelections(vec![Selection::new(
                                    start, end,
                                )]));
                            }
                        }
                    } else if !matches.is_empty() {
                        let head = session.doc.buffer().primary().head;
                        let next_match = matches
                            .iter()
                            .find(|&&(start, _)| start >= head)
                            .or(matches.first())
                            .copied();
                        if let Some((start, end)) = next_match {
                            session.apply(Command::SetSelections(vec![Selection::new(start, end)]));
                        }
                    }
                    self.save_session();
                }
                Task::none()
            }
            Message::MdReplaceAll { tab } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    let text = session.doc.buffer().text();
                    let matches = find_all_matches(&text, &session.find_query);
                    if !matches.is_empty() {
                        let selections = matches
                            .into_iter()
                            .map(|(start, end)| Selection::new(start, end))
                            .collect::<Vec<_>>();
                        session.apply(Command::SetSelections(selections));
                        session.apply(Command::Insert(session.replace_text.clone()));
                        self.status = "replaced all occurrences".to_string();
                    }
                    self.save_session();
                }
                Task::none()
            }
            Message::MdCloseFind { tab } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    session.find_open = false;
                    self.save_session();
                }
                Task::none()
            }
            Message::PdfJumpToPage { tab, page } => {
                if let Err(error) = self.ws.focus_tab(tab) {
                    self.status = error.to_string();
                    return Task::none();
                }
                let root = self.vault_root.clone();
                let worker = self.pdf_worker.clone();
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.pdf.get_mut(&d)) {
                    session.record_jump();
                    session.go_to_page(page);
                    let abs = root.join(&session.rel_path);
                    pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                }
                self.sync_status();
                Task::none()
            }
            Message::PdfJumpToAnnotation { tab, annotation_id } => {
                if let Err(error) = self.ws.focus_tab(tab) {
                    self.status = error.to_string();
                    return Task::none();
                }
                let root = self.vault_root.clone();
                let worker = self.pdf_worker.clone();
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.pdf.get_mut(&d)) {
                    session.selected_annotation = Some(annotation_id);
                    if let Some(ann) = session.selected_annotation() {
                        let target = session.layout.as_ref().map(|layout| {
                            let y = ann.quads.first().map_or(0.0, |q| q.y0 as f32);
                            let top = layout.page_top(ann.page as usize) + y * layout.zoom();
                            (top - session.viewport.1 / 3.0)
                                .clamp(0.0, layout.max_scroll(session.viewport.1))
                        });
                        if let Some(scroll) = target {
                            session.record_jump();
                            session.scroll = scroll;
                            let abs = root.join(&session.rel_path);
                            pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                        }
                    }
                }
                self.sync_status();
                Task::none()
            }
            Message::PdfDeleteAnnotation { tab, annotation_id } => {
                if let Err(error) = self.ws.focus_tab(tab) {
                    self.status = error.to_string();
                    return Task::none();
                }
                self.ensure_annotations();
                let doc = self.tab_document(tab);
                if let (Some(store), Some(session)) = (
                    self.annotations.as_mut(),
                    doc.and_then(|d| self.sessions.pdf.get_mut(&d)),
                ) {
                    if session.selected_annotation == Some(annotation_id) {
                        session.selected_annotation = None;
                    }
                    match store.remove(annotation_id) {
                        Ok(()) => {
                            refresh_annotations(store, session);
                            self.status = "highlight removed".to_string();
                        }
                        Err(e) => self.status = format!("remove failed: {e}"),
                    }
                }
                Task::none()
            }
            Message::PdfEditAnnotationNote { tab, annotation_id } => {
                if let Err(error) = self.ws.focus_tab(tab) {
                    self.status = error.to_string();
                    return Task::none();
                }
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.pdf.get_mut(&d)) {
                    session.selected_annotation = Some(annotation_id);
                    if let Some(ann) = session.selected_annotation() {
                        let input = ann.note.clone();
                        self.open_overlay(Overlay::AnnotationNote { input });
                    }
                }
                Task::none()
            }
            Message::PdfCycleAnnotationColor { tab, annotation_id } => {
                if let Err(error) = self.ws.focus_tab(tab) {
                    self.status = error.to_string();
                    return Task::none();
                }
                self.ensure_annotations();
                let doc = self.tab_document(tab);
                if let (Some(store), Some(session)) = (
                    self.annotations.as_mut(),
                    doc.and_then(|d| self.sessions.pdf.get_mut(&d)),
                ) {
                    session.selected_annotation = Some(annotation_id);
                    if let Some(current) = session.selected_annotation() {
                        let next = match current.color.as_str() {
                            "#f1c40f" => "#e74c3c", // yellow -> red
                            "#e74c3c" => "#2ecc71", // red -> green
                            "#2ecc71" => "#3498db", // green -> blue
                            _ => "#f1c40f",         // blue (or other) -> yellow
                        };
                        match store.set_color(current.id, next) {
                            Ok(()) => {
                                refresh_annotations(store, session);
                                self.status = "color updated".to_string();
                            }
                            Err(e) => self.status = format!("update failed: {e}"),
                        }
                    }
                }
                Task::none()
            }
            Message::PanelResized { kind, width } => {
                let width = width.clamp(160.0, 480.0);
                match kind {
                    drag::PanelKind::Toc => {
                        if let Some(session) = self.focused_pdf_mut() {
                            session.toc_width = width;
                        }
                    }
                    drag::PanelKind::Annotations => {
                        if let Some(session) = self.focused_pdf_mut() {
                            session.annotations_width = width;
                        }
                    }
                    drag::PanelKind::Outline => {
                        if let Some(session) = self.focused_md_mut() {
                            session.outline_width = width;
                        }
                    }
                }
                Task::none()
            }
            Message::PanelResizeFinished { .. } => {
                self.save_session();
                Task::none()
            }
            Message::PdfContextMenuClosed => {
                self.pdf_context_menu = None;
                Task::none()
            }
            Message::PdfContextMenuCommand { tab, command } => {
                self.pdf_context_menu = None;
                if let Err(error) = self.ws.focus_tab(tab) {
                    self.status = error.to_string();
                    return Task::none();
                }
                self.run_command(command)
            }
            Message::PdfCommand { tab, command } => {
                if let Err(error) = self.ws.focus_tab(tab) {
                    self.status = error.to_string();
                    return Task::none();
                }
                self.run_command(command)
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
                self.tree_selected = Some(rel_path.clone());
                self.open_document(&rel_path);
                Task::none()
            }
            Message::TreeDirToggled(dir_path) => {
                self.tree_selected = Some(dir_path.clone());
                if self.tree_expanded.contains(&dir_path) {
                    self.tree_expanded.remove(&dir_path);
                } else {
                    self.tree_expanded.insert(dir_path);
                }
                self.save_session();
                Task::none()
            }
            Message::TreeContextRequested { rel_path, is_dir } => {
                self.close_overlay();
                self.close_menu();
                self.tree_selected = Some(rel_path.clone());
                self.tree_context = Some((rel_path, is_dir));
                self.ws.open_overlay("file-context");
                Task::none()
            }
            Message::TreeContextCommand(command) => {
                self.close_tree_context();
                self.run_command(command)
            }
            Message::TreeContextOpen { split } => {
                let target = self.tree_context.as_ref().map(|(path, _)| path.clone());
                self.close_tree_context();
                let Some(path) = target else {
                    return Task::none();
                };
                if split {
                    let pane = self
                        .ws
                        .focused_pane()
                        .unwrap_or_else(|| self.ws.panes.first_pane());
                    match self.ws.panes.split(pane, SplitAxis::Horizontal) {
                        Ok(pane) => {
                            let _ = self.open_document_in(pane, &path);
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                } else {
                    self.open_document(&path);
                }
                Task::none()
            }
            Message::TreeContextClosed => {
                self.close_tree_context();
                Task::none()
            }
            Message::TreeResizeStarted => {
                self.tree_resizing = true;
                Task::none()
            }
            Message::TreeResized(x) => {
                self.tree_width = x.clamp(160.0, 480.0);
                Task::none()
            }
            Message::TreeResizeFinished => {
                self.tree_resizing = false;
                self.save_session();
                Task::none()
            }
            Message::VaultPicked(path) => {
                let Some(path) = path else {
                    return Task::none();
                };
                match crate::vault_picker::launch_vault(&path) {
                    Ok(_) => iced::exit(),
                    Err(error) => {
                        self.status = format!("open vault {}: {error}", path.display());
                        Task::none()
                    }
                }
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
                            return self.success_toast("tracker: session logged manually");
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
            Message::PdfWorkerReady(handle) => {
                self.pdf_worker = Some(handle);
                self.schedule_open_pdf_work();
                Task::none()
            }
            Message::PdfWorker(output) => {
                self.apply_pdf_worker_output(output);
                Task::none()
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
        self.status.clear();
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
                    Key::Tab => Some(Command::TableTab { backward: extend }),
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
        let worker = self.pdf_worker.clone();
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
        pdf_view::ensure_tiles(session, &abs, worker.as_ref());
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
        self.status.clear();
        match cmd.0 {
            "app.quit" => {
                if self.is_any_tab_dirty() {
                    self.open_overlay(Overlay::Confirm {
                        message: "Abandon unsaved changes and quit?".to_string(),
                        on_confirm: CommandId("app.force-quit"),
                    });
                } else {
                    self.save_session();
                    return iced::exit();
                }
            }
            "app.force-quit" => {
                self.save_session();
                return iced::exit();
            }
            "app.settings" => {
                let overrides = crate::settings::read_keymap_overrides(&self.vault_root).unwrap_or(
                    crate::settings::KeymapFile {
                        bindings: Vec::new(),
                    },
                );
                self.open_overlay(Overlay::Settings {
                    theme: self.theme_name.clone(),
                    keymap: overrides,
                    error: None,
                });
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
            "vault.open" => {
                return Task::perform(
                    crate::vault_picker::pick_vault_async(),
                    Message::VaultPicked,
                );
            }
            "file.new-note" => {
                self.open_overlay(Overlay::NameInput {
                    purpose: NamePurpose::NewNote {
                        parent: self.selected_parent(),
                    },
                    input: String::new(),
                });
            }
            "file.new-folder" => {
                self.open_overlay(Overlay::NameInput {
                    purpose: NamePurpose::NewFolder {
                        parent: self.selected_parent(),
                    },
                    input: String::new(),
                });
            }
            "file.rename" => {
                let Some(target) = self.selected_target() else {
                    self.status = "rename: select a file or folder".to_string();
                    return Task::none();
                };
                let input = Path::new(&target)
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();
                self.open_overlay(Overlay::NameInput {
                    purpose: NamePurpose::Rename { target },
                    input,
                });
            }
            "file.delete" => {
                let Some(target) = self.selected_target() else {
                    self.status = "delete: select a file or folder".to_string();
                    return Task::none();
                };
                let is_dir = self.vault_root.join(&target).is_dir();
                self.open_overlay(Overlay::ConfirmDelete { target, is_dir });
            }
            "workspace.refresh-files" => {
                self.files = scan_vault(&self.vault_root);
                return self.success("File panel refreshed");
            }
            "workspace.collapse-files" => {
                self.tree_expanded.clear();
                self.save_session();
            }
            "search.global" => {
                self.ensure_index();
                self.open_overlay(Overlay::Search {
                    input: String::new(),
                    selected: 0,
                    hits: Vec::new(),
                });
            }
            "help.shortcuts" => self.open_overlay(Overlay::Help {
                input: String::new(),
                selected: 0,
            }),
            "note.outline-panel" => {
                if let Some(session) = self.focused_md_mut() {
                    session.outline_open = !session.outline_open;
                    self.save_session();
                }
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
            "workspace.split-right" | "workspace.split-down" => {
                let focused = self.focused_doc_info();
                let axis = if cmd.0 == "workspace.split-down" {
                    SplitAxis::Vertical
                } else {
                    SplitAxis::Horizontal
                };
                match focused {
                    Some((path, kind)) => {
                        if let Err(e) = self.ws.open_in_new_split(&path, kind, axis) {
                            self.status = e.to_string();
                        }
                    }
                    None => {
                        let pane = self.ws.panes.first_pane();
                        if let Err(error) = self.ws.panes.split(pane, axis) {
                            self.status = error.to_string();
                        }
                    }
                }
            }
            "workspace.close-tab" => {
                if let Some(tab) = self.ws.focused_tab() {
                    if self.is_tab_dirty(tab) {
                        let name = self.tab_name(tab);
                        self.open_overlay(Overlay::Confirm {
                            message: format!("Abandon unsaved changes in `{name}`?"),
                            on_confirm: CommandId("workspace.force-close-tab"),
                        });
                    } else {
                        self.close_tab(tab);
                    }
                }
            }
            "workspace.force-close-tab" => {
                if let Some(tab) = self.ws.focused_tab() {
                    self.close_tab(tab);
                }
            }
            "workspace.close-pane" => {
                let pane = self
                    .ws
                    .focused_pane()
                    .unwrap_or_else(|| self.ws.panes.first_pane());
                self.close_pane(pane);
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
            "editor.toggle-bold" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(Command::ToggleBold);
                }
            }
            "editor.toggle-italic" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(Command::ToggleItalic);
                }
            }
            "editor.toggle-code" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(Command::ToggleCode);
                }
            }
            "editor.heading-cycle" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(Command::HeadingCycle);
                }
            }
            "editor.heading-1" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(Command::SetHeading(1));
                }
            }
            "editor.heading-2" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(Command::SetHeading(2));
                }
            }
            "editor.heading-3" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(Command::SetHeading(3));
                }
            }
            "editor.heading-4" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(Command::SetHeading(4));
                }
            }
            "editor.heading-5" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(Command::SetHeading(5));
                }
            }
            "editor.heading-6" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(Command::SetHeading(6));
                }
            }
            "editor.toggle-bullet" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(Command::ToggleBullet);
                }
            }
            "editor.toggle-checkbox" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(Command::ToggleCheckbox);
                }
            }
            "editor.toggle-wikilink" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(Command::ToggleWikilink);
                }
            }
            "editor.save" => return self.save_focused(),
            "editor.find" => {
                if let Some(session) = self.focused_md_mut() {
                    session.find_open = !session.find_open;
                    self.save_session();
                }
            }
            "pdf.zoom-input" => self.open_overlay(Overlay::PdfZoom {
                input: String::new(),
            }),
            "pdf.zoom-in" => self.adjust_pdf_zoom(1.25),
            "pdf.zoom-out" => self.adjust_pdf_zoom(0.8),
            "pdf.go-to-page" => self.open_overlay(Overlay::PdfPage {
                input: String::new(),
            }),
            "pdf.previous-page" | "pdf.next-page" => {
                let root = self.vault_root.clone();
                let worker = self.pdf_worker.clone();
                if let Some(session) = self.focused_pdf_mut() {
                    session.record_jump();
                    let current = session.current_page();
                    let page = if cmd.0 == "pdf.next-page" {
                        current.saturating_add(1)
                    } else {
                        current.saturating_sub(1)
                    };
                    session.go_to_page(page);
                    let abs = root.join(&session.rel_path);
                    pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                }
            }
            "pdf.find" => self.open_pdf_find(),
            "pdf.toc" => self.open_pdf_toc(),
            "pdf.back" | "pdf.forward" => {
                self.pdf_nav_history(cmd.0 == "pdf.back");
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
            "pdf.annotations-export" => return self.export_annotations(),
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
            "pdf.fit-width" => {
                let root = self.vault_root.clone();
                let worker = self.pdf_worker.clone();
                if let Some(session) = self.focused_pdf_mut() {
                    session.set_fit_mode(PdfFitMode::Width);
                    let abs = root.join(&session.rel_path);
                    pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                }
                self.save_session();
            }
            "pdf.fit-page" => {
                let root = self.vault_root.clone();
                let worker = self.pdf_worker.clone();
                if let Some(session) = self.focused_pdf_mut() {
                    session.set_fit_mode(PdfFitMode::Page);
                    let abs = root.join(&session.rel_path);
                    pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                }
                self.save_session();
            }
            "pdf.toc-panel" => {
                if let Some(session) = self.focused_pdf_mut() {
                    session.toc_open = !session.toc_open;
                    self.save_session();
                }
            }
            "pdf.annotations-panel" => {
                if let Some(session) = self.focused_pdf_mut() {
                    session.annotations_open = !session.annotations_open;
                    self.save_session();
                }
            }
            "pdf.highlight-and-note" => {
                self.highlight_selection();
                if let Some(session) = self.focused_pdf()
                    && let Some(a) = session.selected_annotation()
                {
                    let input = a.note.clone();
                    self.open_overlay(Overlay::AnnotationNote { input });
                }
            }
            "overlay.close" => {
                if self.tree_context.is_some() {
                    self.close_tree_context();
                } else if self.open_menu.is_some() {
                    self.close_menu();
                } else {
                    self.close_overlay();
                }
            }
            "overlay.confirm" => return self.confirm_overlay(),
            other => self.status = format!("unhandled command: {other}"),
        }
        if matches!(
            cmd.0,
            "workspace.split-right"
                | "workspace.split-down"
                | "workspace.close-pane"
                | "workspace.close-tab"
                | "workspace.next-tab"
                | "editor.save"
        ) {
            self.save_session();
        }
        self.sync_status();
        Task::none()
    }

    fn open_overlay(&mut self, overlay: Overlay) {
        self.close_menu();
        self.close_tree_context();
        self.ws.open_overlay(overlay.kernel_name());
        self.overlay = Some(overlay);
    }

    fn close_overlay(&mut self) {
        self.ws.close_overlay();
        self.overlay = None;
    }

    fn close_menu(&mut self) {
        if self.open_menu.take().is_some() {
            self.ws.close_overlay();
        }
    }

    fn close_tree_context(&mut self) {
        if self.tree_context.take().is_some() {
            self.ws.close_overlay();
        }
    }

    fn adjust_pdf_zoom(&mut self, factor: f32) {
        let root = self.vault_root.clone();
        let worker = self.pdf_worker.clone();
        let Some(session) = self.focused_pdf_mut() else {
            return;
        };
        session.set_zoom(session.zoom * factor);
        let abs_path = root.join(&session.rel_path);
        pdf_view::ensure_tiles(session, &abs_path, worker.as_ref());
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
            Overlay::Help { input, selected } => {
                let picked = self
                    .registry
                    .palette(&input)
                    .get(selected.min(self.registry.palette(&input).len().saturating_sub(1)))
                    .map(|spec| spec.id);
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
                    let worker = self.pdf_worker.clone();
                    if let Some(session) = self.focused_pdf_mut() {
                        session.record_jump();
                        session.go_to_page(page as usize);
                        let abs = root.join(&session.rel_path);
                        pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                        self.status = format!("§ {}", title.trim_start());
                        return Task::none();
                    }
                }
            }
            Overlay::PdfZoom { input } => {
                self.close_overlay();
                let root = self.vault_root.clone();
                let worker = self.pdf_worker.clone();
                if let (Ok(pct), Some(session)) =
                    (input.trim().parse::<f32>(), self.focused_pdf_mut())
                {
                    session.set_zoom(pct / 100.0);
                    let abs = root.join(&session.rel_path);
                    pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                }
            }
            Overlay::PdfPage { input } => {
                self.close_overlay();
                let root = self.vault_root.clone();
                let worker = self.pdf_worker.clone();
                if let (Ok(page), Some(session)) =
                    (input.trim().parse::<u32>(), self.focused_pdf_mut())
                {
                    session.record_jump();
                    session.go_to_page((page.saturating_sub(1)) as usize);
                    let abs = root.join(&session.rel_path);
                    pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                }
            }
            Overlay::AnnotationNote { input } => {
                self.close_overlay();
                self.set_annotation_note(&input);
            }
            Overlay::NameInput { purpose, input } => {
                self.close_overlay();
                match purpose {
                    NamePurpose::NewNote { parent } => self.create_note(&parent, &input),
                    NamePurpose::NewFolder { parent } => self.create_folder(&parent, &input),
                    NamePurpose::Rename { target } => self.rename_path(&target, &input),
                }
            }
            Overlay::ConfirmDelete { target, .. } => {
                self.close_overlay();
                self.delete_path(&target);
            }
            Overlay::Confirm { on_confirm, .. } => {
                self.close_overlay();
                return self.run_command(on_confirm);
            }
            Overlay::Settings { .. } => {
                return self.update(Message::SettingsSave);
            }
            // Read-only report: confirm = dismiss.
            Overlay::OrphanReport { .. } => self.close_overlay(),
            Overlay::PdfLinkPreview {
                dest_page, dest_y, ..
            } => {
                self.close_overlay();
                let root = self.vault_root.clone();
                let worker = self.pdf_worker.clone();
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
                    pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                    self.status = format!("→ p. {} · alt+left returns", dest_page + 1);
                    return Task::none();
                }
            }
        }
        self.sync_status();
        Task::none()
    }

    fn reload_keymap(&mut self) {
        if let Ok(mut keymap) = self.registry.keymap() {
            let report = crate::settings::apply_keymap_overrides(
                &self.vault_root,
                &self.registry,
                &mut keymap,
            );
            self.keymap = keymap;
            if !report.warnings.is_empty() {
                let msg = report.warnings.join("\n");
                let _ = self.warning(msg);
            }
        }
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

    // --------------------------------------------------------------- view --

    pub(crate) fn view(&self) -> Element<'_, Message> {
        let workspace = self.layout_view(&self.ws.panes.layout());
        let focused_kind = self.ws.focused_editor_kind();
        let mut row_children = Vec::new();

        if self.tree_open {
            let rows = file_tree::visible_rows(&self.files, &self.tree_expanded);
            let focused_path = self
                .ws
                .focused_tab()
                .and_then(|t| self.tab_document(t))
                .and_then(|d| self.ws.docs.get(d))
                .map(|doc| doc.path.as_str());

            let vault_name = self
                .vault_root
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| self.vault_root.display().to_string());
            let header_button = |label: &'static str, command| {
                button(text(label).size(14))
                    .padding([3, 6])
                    .style(button::text)
                    .on_press(Message::RunCommand(command))
            };
            let header = row![
                text(vault_name).size(13).color(colors::heading()),
                iced::widget::Space::new().width(Fill),
                header_button("+N", CommandId("file.new-note")),
                header_button("+F", CommandId("file.new-folder")),
                header_button("−", CommandId("workspace.collapse-files")),
                header_button("↻", CommandId("workspace.refresh-files")),
            ]
            .spacing(2)
            .align_y(iced::Alignment::Center);

            let mut col = column![header].spacing(2);
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

                let kind = if row.is_dir {
                    ""
                } else if row.rel_path.ends_with(".pdf") {
                    "PDF "
                } else {
                    "MD "
                };
                let dirty = self.sessions.md.values().any(|session| {
                    session.rel_path == row.rel_path && session.doc.buffer().is_dirty()
                });
                let content = row![
                    iced::widget::Space::new().width(indent),
                    text(format!(
                        "{marker}{kind}{}{}",
                        row.label,
                        if dirty { " ●" } else { "" }
                    ))
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

                let item = button(content)
                    .width(Fill)
                    .padding([4, 3])
                    .style(move |_theme, status| {
                        let hovered =
                            matches!(status, button::Status::Hovered | button::Status::Pressed);
                        button::Style {
                            background: hovered
                                .then_some(iced::Background::Color(tokens::dark().bg_tertiary)),
                            text_color,
                            ..button::Style::default()
                        }
                    })
                    .on_press(msg);
                let item =
                    iced::widget::mouse_area(item).on_right_press(Message::TreeContextRequested {
                        rel_path: row.rel_path,
                        is_dir: row.is_dir,
                    });
                col = col.push(item);
            }

            let sidebar_bg = tokens::dark().bg_secondary;
            let border_color = tokens::dark().border;

            let panel = container(iced::widget::scrollable(col))
                .width(self.tree_width)
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

            let resize_handle = iced::widget::mouse_area(
                container(iced::widget::Space::new())
                    .width(6)
                    .height(Fill)
                    .style(|_| container::Style {
                        background: Some(iced::Background::Color(tokens::dark().border_subtle)),
                        ..container::Style::default()
                    }),
            )
            .on_press(Message::TreeResizeStarted);
            row_children.push(row![panel, resize_handle].spacing(0).into());
        }

        row_children.push(container(workspace).width(Fill).height(Fill).into());

        if let Some(tab) = self.ws.focused_tab() {
            if let Some(session) = self.focused_pdf() {
                if session.toc_open {
                    row_children.push(drag::panel_resizer(drag::PanelKind::Toc, session.toc_width));
                    row_children.push(self.view_pdf_toc_panel(session, tab));
                }
                if session.annotations_open {
                    row_children.push(drag::panel_resizer(
                        drag::PanelKind::Annotations,
                        session.annotations_width,
                    ));
                    row_children.push(self.view_pdf_annotations_panel(session, tab));
                }
            } else if let Some(session) = self.focused_md()
                && session.outline_open
            {
                row_children.push(drag::panel_resizer(
                    drag::PanelKind::Outline,
                    session.outline_width,
                ));
                row_children.push(self.view_md_outline_panel(session, tab));
            }
        }

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

        let status = container(
            row![
                text(self.status.clone()).size(13).color(colors::marker()),
                iced::widget::Space::new().width(Fill),
                text(self.position_status.clone())
                    .size(13)
                    .color(colors::marker())
            ]
            .width(Fill),
        )
        .padding([4, 10])
        .width(Fill);

        let base = column![
            menu::bar(self.open_menu),
            container(workspace_content).height(Fill),
            status
        ];
        let mut final_view: Element<'_, Message> = if let Some(overlay) = &self.overlay {
            stack![base, overlay::view(overlay, &self.registry, &self.files)].into()
        } else if let Some(ctx) = &self.pdf_context_menu {
            stack![base, self.view_pdf_context_menu(ctx)].into()
        } else if self.tree_resizing {
            let drag_layer = iced::widget::mouse_area(
                container(iced::widget::Space::new())
                    .width(Fill)
                    .height(Fill),
            )
            .on_move(|point| Message::TreeResized(point.x))
            .on_release(Message::TreeResizeFinished);
            stack![base, drag_layer].into()
        } else if let Some((_, is_dir)) = self.tree_context.as_ref() {
            stack![base, file_tree::context_popover(self.tree_width, *is_dir)].into()
        } else if let Some(open_menu) = self.open_menu {
            let model = menu::menu_model(
                &self.registry,
                focused_kind,
                self.ws.focused_tab().is_some(),
            );
            stack![base, menu::popover(open_menu, model, &self.registry)].into()
        } else {
            base.into()
        };

        if !self.toasts.is_empty() {
            final_view = stack![final_view, self.view_toasts()].into();
        }
        final_view
    }

    fn layout_view<'a>(&'a self, node: &Layout<'a>) -> Element<'a, Message> {
        self.layout_view_at(node, Vec::new())
    }

    fn layout_view_at<'a>(&'a self, node: &Layout<'a>, path: SplitPath) -> Element<'a, Message> {
        match node {
            Layout::Pane(pane) => self.pane_view(pane),
            Layout::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let mut first_path = path.clone();
                first_path.push(false);
                let mut second_path = path.clone();
                second_path.push(true);
                let a = container(self.layout_view_at(first, first_path))
                    .width(Fill)
                    .height(Fill);
                let b = container(self.layout_view_at(second, second_path))
                    .width(Fill)
                    .height(Fill);
                let divider = drag::divider(path, *axis, *ratio);
                let (pa, pb) = (
                    ((ratio * 1000.0) as u16).max(1),
                    (((1.0 - ratio) * 1000.0) as u16).max(1),
                );
                match axis {
                    SplitAxis::Horizontal => row![
                        a.width(iced::Length::FillPortion(pa)),
                        divider,
                        b.width(iced::Length::FillPortion(pb))
                    ]
                    .spacing(0)
                    .into(),
                    SplitAxis::Vertical => column![
                        a.height(iced::Length::FillPortion(pa)),
                        divider,
                        b.height(iced::Length::FillPortion(pb))
                    ]
                    .spacing(0)
                    .into(),
                }
            }
        }
    }

    fn pane_view<'a>(&'a self, pane: &Pane) -> Element<'a, Message> {
        let focused_tab = self.ws.focused_tab();
        let pane_focused = self.ws.focused_pane() == Some(pane.id);

        let mut tabs = row![].spacing(2);
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
            let select = button(text(title).size(13))
                .padding([3, 8])
                .style(move |theme, status| {
                    if active && pane_focused {
                        button::primary(theme, status)
                    } else if active {
                        button::secondary(theme, status)
                    } else {
                        button::text(theme, status)
                    }
                })
                .on_press(Message::TabSelected(tab.id));
            let close = button(text("×").size(13))
                .padding([3, 6])
                .style(button::text)
                .on_press(Message::TabCloseClicked(tab.id));
            tabs = tabs.push(
                iced::widget::mouse_area(row![select, close].spacing(0))
                    .on_middle_press(Message::TabCloseClicked(tab.id)),
            );
        }
        tabs = tabs.push(
            button(text("+").size(15))
                .padding([2, 8])
                .style(button::text)
                .on_press(Message::RunCommand(CommandId("file.quick-open"))),
        );
        let tabs = iced::widget::scrollable(tabs).direction(
            iced::widget::scrollable::Direction::Horizontal(
                iced::widget::scrollable::Scrollbar::default(),
            ),
        );
        let pane_action = |label, command| {
            button(text(label).size(13))
                .padding([3, 6])
                .style(button::text)
                .on_press(Message::PaneCommand {
                    pane: pane.id,
                    command,
                })
        };
        let strip = row![
            container(tabs).width(Fill),
            pane_action("⇥", CommandId("workspace.split-right")),
            pane_action("⇩", CommandId("workspace.split-down")),
            pane_action("×", CommandId("workspace.close-pane")),
        ]
        .spacing(2)
        .padding(2)
        .align_y(iced::Alignment::Center);

        let content: Element<'_, Message> = match pane.active_tab() {
            None => {
                let mut welcome = column![
                    text("MD Editor").size(24).color(colors::heading()),
                    text("Open a note or browse the vault to begin.")
                        .size(14)
                        .color(colors::marker())
                ]
                .spacing(10)
                .width(320);
                for item in welcome::welcome_rows(&self.registry) {
                    let chord = item.chord.unwrap_or_default();
                    welcome = welcome.push(
                        button(
                            row![
                                text(item.label).size(14),
                                iced::widget::Space::new().width(Fill),
                                text(chord).size(12).color(colors::marker())
                            ]
                            .width(Fill),
                        )
                        .width(Fill)
                        .padding([8, 12])
                        .on_press(Message::RunCommand(item.command)),
                    );
                }
                container(welcome).center(Fill).into()
            }
            Some(tab) => {
                let focused = focused_tab == Some(tab.id);
                match tab.editor {
                    EditorKind::Markdown => match self.sessions.md.get(&tab.document) {
                        Some(session) => {
                            let toolbar = row![
                                button(text("B").font(BOLD).size(12))
                                    .padding([3, 8])
                                    .style(button::text)
                                    .on_press(Message::RunCommand(CommandId("editor.toggle-bold"))),
                                button(text("I").size(12))
                                    .padding([3, 8])
                                    .style(button::text)
                                    .on_press(Message::RunCommand(CommandId(
                                        "editor.toggle-italic"
                                    ))),
                                button(text("Code").size(12))
                                    .padding([3, 8])
                                    .style(button::text)
                                    .on_press(Message::RunCommand(CommandId("editor.toggle-code"))),
                                button(text("H").font(BOLD).size(12))
                                    .padding([3, 8])
                                    .style(button::text)
                                    .on_press(Message::RunCommand(CommandId(
                                        "editor.heading-cycle"
                                    ))),
                                button(text("List").size(12))
                                    .padding([3, 8])
                                    .style(button::text)
                                    .on_press(Message::RunCommand(CommandId(
                                        "editor.toggle-bullet"
                                    ))),
                                button(text("Todo").size(12))
                                    .padding([3, 8])
                                    .style(button::text)
                                    .on_press(Message::RunCommand(CommandId(
                                        "editor.toggle-checkbox"
                                    ))),
                                button(text("Link").size(12))
                                    .padding([3, 8])
                                    .style(button::text)
                                    .on_press(Message::RunCommand(CommandId(
                                        "editor.toggle-wikilink"
                                    ))),
                            ]
                            .spacing(2)
                            .padding(2)
                            .align_y(iced::Alignment::Center);

                            let toolbar_container =
                                container(toolbar).width(Fill).style(|_| container::Style {
                                    background: Some(iced::Background::Color(
                                        tokens::dark().bg_secondary,
                                    )),
                                    border: iced::Border {
                                        color: tokens::dark().border_subtle,
                                        width: 1.0,
                                        radius: 0.0.into(),
                                    },
                                    ..container::Style::default()
                                });

                            let editor = canvas(EditorCanvas {
                                tab: tab.id,
                                session,
                                focused,
                            })
                            .width(Fill)
                            .height(Fill);

                            let mut view_col = column![toolbar_container];
                            if session.find_open {
                                view_col =
                                    view_col.push(self.view_md_find_replace_bar(session, tab.id));
                            }
                            view_col.push(editor).into()
                        }
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

    fn view_pdf_toc_panel(&self, session: &PdfSession, tab: TabId) -> Element<'_, Message> {
        let t = tokens::dark();
        let title = row![
            text("Table of Contents")
                .size(14)
                .color(t.text_primary)
                .font(BOLD),
            iced::widget::Space::new().width(iced::Length::Fill),
            button(text("✕").size(12).font(BOLD))
                .on_press(Message::PdfCommand {
                    tab,
                    command: CommandId("pdf.toc-panel"),
                })
                .style(button::text),
        ]
        .align_y(iced::Alignment::Center);

        let current_idx = session.current_section_index();

        let mut list = column![].spacing(2);
        for (i, entry) in session.outline.iter().enumerate() {
            let active = Some(i) == current_idx;
            let text_color = if active { t.accent } else { t.text_primary };
            let font = if active { BOLD } else { iced::Font::DEFAULT };

            let content = row![
                iced::widget::Space::new().width(iced::Length::Fixed(entry.depth as f32 * 12.0)),
                text(entry.title.clone())
                    .size(12)
                    .color(text_color)
                    .font(font),
                iced::widget::Space::new().width(iced::Length::Fill),
                text(format!("{}", entry.page + 1))
                    .size(11)
                    .color(t.text_muted),
            ]
            .align_y(iced::Alignment::Center)
            .padding([4, 6]);

            let item = button(content)
                .width(Fill)
                .style(move |_theme, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: if active {
                            Some(iced::Background::Color(t.bg_tertiary))
                        } else if hovered {
                            Some(iced::Background::Color(t.bg_surface))
                        } else {
                            None
                        },
                        ..button::Style::default()
                    }
                })
                .on_press(Message::PdfJumpToPage {
                    tab,
                    page: entry.page as usize,
                });

            list = list.push(item);
        }

        let panel_content = column![
            title,
            iced::widget::Space::new().height(8),
            iced::widget::scrollable(list).height(iced::Length::Fill)
        ]
        .spacing(4)
        .padding(10);

        container(panel_content)
            .width(iced::Length::Fixed(session.toc_width))
            .height(iced::Length::Fill)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(t.bg_secondary)),
                border: iced::Border {
                    color: t.border,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    fn view_md_outline_panel(&self, session: &MdSession, tab: TabId) -> Element<'_, Message> {
        let t = tokens::dark();
        let title = row![
            text("Outline").size(14).color(t.text_primary).font(BOLD),
            iced::widget::Space::new().width(iced::Length::Fill),
            button(text("✕").size(12).font(BOLD))
                .on_press(Message::RunCommand(CommandId("note.outline-panel")))
                .style(button::text),
        ]
        .align_y(iced::Alignment::Center);

        let caret_line = {
            let head = session.doc.buffer().primary().head;
            session.doc.buffer().offset_to_line_col(head).0
        };

        let headings = session.doc.headings();
        let active_idx = headings
            .iter()
            .enumerate()
            .rfind(|(_, (_, _, line_idx))| *line_idx <= caret_line)
            .map(|(i, _)| i);

        let mut list = column![].spacing(2);
        for (i, (level, title_text, line_idx)) in headings.into_iter().enumerate() {
            let active = Some(i) == active_idx;
            let text_color = if active { t.accent } else { t.text_primary };
            let font = if active { BOLD } else { iced::Font::DEFAULT };

            let indent = (level.saturating_sub(1) as f32) * 12.0;

            let content = row![
                iced::widget::Space::new().width(iced::Length::Fixed(indent)),
                text(title_text).size(12).color(text_color).font(font),
                iced::widget::Space::new().width(iced::Length::Fill),
            ]
            .align_y(iced::Alignment::Center)
            .padding([4, 6]);

            let item = button(content)
                .width(Fill)
                .style(move |_theme, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: if active {
                            Some(iced::Background::Color(t.bg_tertiary))
                        } else if hovered {
                            Some(iced::Background::Color(t.bg_surface))
                        } else {
                            None
                        },
                        ..button::Style::default()
                    }
                })
                .on_press(Message::MdJumpToLine {
                    tab,
                    line: line_idx,
                });

            list = list.push(item);
        }

        let panel_content = column![
            title,
            iced::widget::Space::new().height(8),
            iced::widget::scrollable(list).height(iced::Length::Fill)
        ]
        .spacing(4)
        .padding(10);

        container(panel_content)
            .width(iced::Length::Fixed(session.outline_width))
            .height(iced::Length::Fill)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(t.bg_secondary)),
                border: iced::Border {
                    color: t.border,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    fn view_md_find_replace_bar(&self, session: &MdSession, tab: TabId) -> Element<'_, Message> {
        let t = tokens::dark();

        let text_val = session.doc.buffer().text();
        let matches = find_all_matches(&text_val, &session.find_query);
        let primary = session.doc.buffer().primary();
        let (caret_start, caret_end) = (
            primary.anchor.min(primary.head),
            primary.anchor.max(primary.head),
        );
        let active_idx = matches
            .iter()
            .position(|&(start, end)| start == caret_start && end == caret_end);

        let count_text = if session.find_query.is_empty() {
            "0 of 0".to_string()
        } else {
            match active_idx {
                Some(idx) => format!("{} of {}", idx + 1, matches.len()),
                None => format!("0 of {}", matches.len()),
            }
        };

        let find_input = text_input("Find...", &session.find_query)
            .on_input(move |q| Message::MdFindQueryChanged { tab, query: q })
            .width(180)
            .padding(4)
            .size(13);

        let replace_input = text_input("Replace with...", &session.replace_text)
            .on_input(move |val| Message::MdReplaceTextChanged { tab, text: val })
            .width(180)
            .padding(4)
            .size(13);

        let bar_content = row![
            text("Find:").size(12).color(t.text_muted),
            find_input,
            text(count_text).size(12).color(t.text_muted),
            button(text("▲").size(10))
                .padding([2, 6])
                .on_press(Message::MdFindPrev { tab }),
            button(text("▼").size(10))
                .padding([2, 6])
                .on_press(Message::MdFindNext { tab }),
            iced::widget::Space::new().width(12),
            text("Replace:").size(12).color(t.text_muted),
            replace_input,
            button(text("Replace").size(12))
                .padding([3, 8])
                .on_press(Message::MdReplace { tab }),
            button(text("Replace All").size(12))
                .padding([3, 8])
                .on_press(Message::MdReplaceAll { tab }),
            iced::widget::Space::new().width(iced::Length::Fill),
            button(text("✕").size(12).font(BOLD))
                .style(button::text)
                .on_press(Message::MdCloseFind { tab })
        ]
        .spacing(8)
        .padding([4, 8])
        .align_y(iced::Alignment::Center);

        container(bar_content)
            .width(Fill)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(t.bg_secondary)),
                border: iced::Border {
                    color: t.border_subtle,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    fn view_pdf_annotations_panel(&self, session: &PdfSession, tab: TabId) -> Element<'_, Message> {
        let t = tokens::dark();
        let title = row![
            text("Annotations")
                .size(14)
                .color(t.text_primary)
                .font(BOLD),
            iced::widget::Space::new().width(iced::Length::Fill),
            button(text("✕").size(12).font(BOLD))
                .on_press(Message::PdfCommand {
                    tab,
                    command: CommandId("pdf.annotations-panel"),
                })
                .style(button::text),
        ]
        .align_y(iced::Alignment::Center);

        let mut list = column![].spacing(6);
        for ann in &session.annotations {
            let mut txt = session.annotation_text(ann);
            if txt.is_empty() {
                txt = "Highlight".to_string();
            }
            let truncated_text = if txt.chars().count() > 60 {
                format!("{}...", txt.chars().take(57).collect::<String>())
            } else {
                txt
            };

            let swatch_color = pdf_view::quad_color(&ann.color, 1.0);
            let swatch = button(
                container(iced::widget::Space::new())
                    .width(12)
                    .height(12)
                    .style(move |_| container::Style {
                        background: Some(iced::Background::Color(swatch_color)),
                        border: iced::Border {
                            color: t.border,
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
                    }),
            )
            .padding(0)
            .on_press(Message::PdfCycleAnnotationColor {
                tab,
                annotation_id: ann.id,
            });

            let note_preview = if !ann.note.is_empty() {
                let note_txt = if ann.note.chars().count() > 40 {
                    format!("{}...", ann.note.chars().take(37).collect::<String>())
                } else {
                    ann.note.clone()
                };
                Some(text(note_txt).size(11).color(t.accent_secondary))
            } else {
                None
            };

            let trash_btn = button(text("🗑").size(12).color(t.danger))
                .style(button::text)
                .on_press(Message::PdfDeleteAnnotation {
                    tab,
                    annotation_id: ann.id,
                });

            let note_btn = button(text("📝").size(12).color(t.text_primary))
                .style(button::text)
                .on_press(Message::PdfEditAnnotationNote {
                    tab,
                    annotation_id: ann.id,
                });

            let active = Some(ann.id) == session.selected_annotation;
            let text_color = if active { t.accent } else { t.text_primary };

            let mut content_col = column![
                row![
                    swatch,
                    iced::widget::Space::new().width(4),
                    text(format!("Page {}", ann.page + 1))
                        .size(11)
                        .color(t.text_muted),
                    iced::widget::Space::new().width(iced::Length::Fill),
                    note_btn,
                    trash_btn,
                ]
                .align_y(iced::Alignment::Center)
                .spacing(2),
                text(truncated_text).size(12).color(text_color)
            ]
            .spacing(4);

            if let Some(n_preview) = note_preview {
                content_col = content_col.push(n_preview);
            }

            let card = container(content_col)
                .padding(8)
                .width(Fill)
                .style(move |_| container::Style {
                    background: if active {
                        Some(iced::Background::Color(t.bg_tertiary))
                    } else {
                        Some(iced::Background::Color(t.bg_surface))
                    },
                    border: iced::Border {
                        color: if active { t.accent } else { t.border_subtle },
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..Default::default()
                });

            let item = button(card).width(Fill).style(button::text).on_press(
                Message::PdfJumpToAnnotation {
                    tab,
                    annotation_id: ann.id,
                },
            );

            list = list.push(item);
        }

        let panel_content = column![
            title,
            iced::widget::Space::new().height(8),
            iced::widget::scrollable(list).height(iced::Length::Fill)
        ]
        .spacing(4)
        .padding(10);

        container(panel_content)
            .width(iced::Length::Fixed(session.annotations_width))
            .height(iced::Length::Fill)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(t.bg_secondary)),
                border: iced::Border {
                    color: t.border,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
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

fn safe_relative(input: &str) -> Option<PathBuf> {
    let path = PathBuf::from(input.trim());
    if path.as_os_str().is_empty() || path.is_absolute() {
        return None;
    }
    path.components()
        .all(|component| matches!(component, std::path::Component::Normal(_)))
        .then_some(path)
}

/// Vault tree scan: directories plus `.md` and `.pdf` files, vault-relative,
/// sorted; directories carry a trailing slash so empty folders stay visible.
/// Dot-directories are skipped (mirrors the index walk).
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
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(format!("{}/", rel.to_string_lossy()));
            }
            walk(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "md" || e == "pdf")
            && let Ok(rel) = path.strip_prefix(root)
        {
            out.push(rel.to_string_lossy().to_string());
        }
    }
}

fn find_all_matches(text: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();
    let mut matches = Vec::new();
    let mut start = 0;
    while let Some(pos) = text_lower[start..].find(&query_lower) {
        let actual_pos = start + pos;
        matches.push((actual_pos, actual_pos + query.len()));
        start = actual_pos + query.len().max(1);
    }
    matches
}

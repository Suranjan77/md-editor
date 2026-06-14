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

mod chrome;
mod chrome_context;
mod chrome_panels;
mod commands_file;
mod commands_md;
mod commands_pdf;
mod commands_pdf_annotations;
mod commands_pdf_nav;
mod commands_settings;
pub mod drag;
pub mod editor_canvas;
pub mod file_tree;
pub mod icons;
mod input;
pub mod keys;
mod markdown_assets;
pub mod menu;
mod motion;
pub mod overlay;
pub mod paint;
mod pdf_input;
mod pdf_view;
mod pdf_worker_events;
pub mod session;
mod session_persist;
pub mod shaped_measurer;
pub mod snapshot;
mod status;
mod stores;
mod toast;
pub mod tokens;
pub mod tracker_view;
mod tracker_widgets;
pub mod welcome;
pub mod worker;

use std::path::{Path, PathBuf};

use iced::widget::{button, canvas, column, container, mouse_area, row, stack, text};
use iced::{Element, Fill, Subscription, Task};
use md3_editor::buffer::{Command, Movement, Selection};
use md3_kernel::input::{Chord, EditorKind, Key};
use md3_kernel::pane::{DocumentId, Layout, Pane, PaneId, SplitPath, TabId};
use md3_kernel::{CommandId, CommandRegistry, Keymap, SplitAxis, Workspace};
use md3_vault::{AnnotationStore, AssetSizeStore, SearchIndex, SessionStore};

use chrome_panels::find_all_matches;
use editor_canvas::EditorCanvas;
use overlay::{NamePurpose, Overlay, PdfFindHit};
use session::{MdSession, PdfSelection, PdfSession, Sessions};
use snapshot::{NodeSnapshot, SessionSnapshot, TabSnapshot, ViewSnapshot};
use stores::{refresh_annotations, scan_vault};
use toast::PdfContextMenuState;

pub use toast::{Toast, ToastKind};

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
        checkbox: bool,
        ctrl: bool,
    },
    /// A left-drag in the editor: select source offsets `anchor`→`head`.
    EditorDragSelect {
        tab: TabId,
        anchor: usize,
        head: usize,
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
    SettingsReduceMotionChanged(bool),
    SettingsScopeChanged(usize, String),
    SettingsChordChanged(usize, String),
    SettingsCommandChanged(usize, String),
    SettingsAddRow,
    SettingsRemoveRow(usize),
    SettingsSave,
    SettingsCancel,
    AnimationTick(std::time::Instant),
}

pub struct Shell {
    registry: CommandRegistry,
    keymap: Keymap,
    ws: Workspace,
    sessions: Sessions,
    measurer: shaped_measurer::ShapedMeasurer,
    vault_root: PathBuf,
    tracker_db_path: PathBuf,
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
    asset_sizes: Option<AssetSizeStore>,
    pdf_worker: Option<worker::WorkerHandle>,
    md_assets_pending: std::collections::HashSet<(PathBuf, String)>,
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
    reduce_motion: bool,
}

impl Shell {
    pub fn new(registry: CommandRegistry, keymap: Keymap, vault_root: PathBuf) -> Shell {
        let tracker_db_path = crate::paths::config_file("tracker.db");
        Self::new_with_tracker_db(registry, keymap, vault_root, tracker_db_path)
    }

    pub fn new_with_tracker_db(
        registry: CommandRegistry,
        keymap: Keymap,
        vault_root: PathBuf,
        tracker_db_path: PathBuf,
    ) -> Shell {
        let measurer = shaped_measurer::ShapedMeasurer::new(std::sync::Arc::new(
            std::sync::Mutex::new(cosmic_text::FontSystem::new()),
        ));
        if let Some(parent) = tracker_db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

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
            measurer,
            vault_root,
            tracker_db_path,
            overlay: None,
            open_menu: None,
            files: Vec::new(),
            index: None,
            annotations: None,
            session: None,
            asset_sizes: None,
            pdf_worker: None,
            md_assets_pending: std::collections::HashSet::new(),
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
            reduce_motion: false,
        };
        shell.restore_session();
        if shell.tree_open {
            shell.files = scan_vault(&shell.vault_root);
        }
        shell
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

    pub fn theme_name(&self) -> &str {
        &self.theme_name
    }

    pub fn theme_tokens(&self) -> &'static tokens::Tokens {
        self.tokens()
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

    pub(crate) fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            iced::keyboard::listen().map(|event| match keys::normalize(&event) {
                Some(ev) => Message::Key(ev),
                None => Message::Ignored,
            }),
            iced::window::close_requests().map(|_| Message::WindowCloseRequested),
            Subscription::run(worker::subscribe),
        ];
        if let Some(subscription) = self.motion_subscription() {
            subscriptions.push(subscription);
        }
        Subscription::batch(subscriptions)
    }

    // ------------------------------------------------------------- update --

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SettingsThemeChanged(_)
            | Message::SettingsReduceMotionChanged(_)
            | Message::SettingsScopeChanged(..)
            | Message::SettingsChordChanged(..)
            | Message::SettingsCommandChanged(..)
            | Message::SettingsAddRow
            | Message::SettingsRemoveRow(_)
            | Message::SettingsSave
            | Message::SettingsCancel => self.handle_settings_message(message),
            Message::AnimationTick(now) => self.advance_motion(now),
            Message::TreeFileClicked(_)
            | Message::TreeDirToggled(_)
            | Message::TreeContextRequested { .. }
            | Message::TreeContextCommand(_)
            | Message::TreeContextOpen { .. }
            | Message::TreeContextClosed
            | Message::TreeResizeStarted
            | Message::TreeResized(_)
            | Message::TreeResizeFinished => self.handle_tree_message(message),
            Message::EditorClicked { .. }
            | Message::EditorDragSelect { .. }
            | Message::EditorScrolled { .. }
            | Message::EditorViewportChanged { .. }
            | Message::MdJumpToLine { .. }
            | Message::MdFindQueryChanged { .. }
            | Message::MdReplaceTextChanged { .. }
            | Message::MdFindNext { .. }
            | Message::MdFindPrev { .. }
            | Message::MdReplace { .. }
            | Message::MdReplaceAll { .. }
            | Message::MdCloseFind { .. } => self.handle_md_message(message),
            Message::PdfViewportChanged { .. }
            | Message::PdfScrolled { .. }
            | Message::PdfMouseDown { .. }
            | Message::PdfRightClick { .. }
            | Message::PdfJumpToPage { .. }
            | Message::PdfJumpToAnnotation { .. }
            | Message::PdfDeleteAnnotation { .. }
            | Message::PdfEditAnnotationNote { .. }
            | Message::PdfCycleAnnotationColor { .. }
            | Message::PdfContextMenuClosed
            | Message::PdfContextMenuCommand { .. }
            | Message::PdfCommand { .. }
            | Message::PdfMouseDragged { .. }
            | Message::PdfMouseUp { .. }
            | Message::PdfWorkerReady(..)
            | Message::PdfWorker(..) => self.handle_pdf_message(message),
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
            Message::OverlayPick(i) => {
                if let Some(sel) = self.overlay.as_mut().and_then(Overlay::selected_mut) {
                    *sel = i;
                }
                self.confirm_overlay()
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
        }
    }

    // --------------------------------------------------------- commands --

    fn run_command(&mut self, cmd: CommandId) -> Task<Message> {
        self.last_command = Some(cmd);
        self.status.clear();

        if let Some(task) = self.run_file_command(cmd.0) {
            return task;
        }
        if let Some(task) = self.run_md_command(cmd.0) {
            return task;
        }
        if let Some(task) = self.run_pdf_command(cmd.0) {
            return task;
        }

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
                    reduce_motion: self.reduce_motion,
                    keymap: overrides,
                    error: None,
                });
            }
            "palette.open" => self.open_overlay(Overlay::Palette {
                input: String::new(),
                selected: 0,
            }),
            "help.shortcuts" => self.open_overlay(Overlay::Help {
                input: String::new(),
                selected: 0,
            }),
            "search.global" => {
                self.ensure_index();
                self.open_overlay(Overlay::Search {
                    input: String::new(),
                    selected: 0,
                    hits: Vec::new(),
                });
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
        let tokens = self.tokens();
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
            let header_icon = |icon, command| {
                button(icons::view(icon, tokens.text_secondary, 15.0))
                    .padding([4, 5])
                    .style(button::text)
                    .on_press(Message::RunCommand(command))
            };
            let header = row![
                text(vault_name).size(13).color(tokens.text_secondary),
                iced::widget::Space::new().width(Fill),
                header_icon(icons::Icon::NewNote, CommandId("file.new-note")),
                header_icon(icons::Icon::NewFolder, CommandId("file.new-folder")),
                header_icon(icons::Icon::Sidebar, CommandId("workspace.collapse-files")),
                header_icon(icons::Icon::Refresh, CommandId("workspace.refresh-files")),
            ]
            .spacing(2)
            .align_y(iced::Alignment::Center);

            let mut col = column![header].spacing(2);
            for row in rows {
                let indent = row.depth as f32 * 14.0;
                let expanded = row.is_dir && self.tree_expanded.contains(&row.rel_path);
                let chevron = if row.is_dir {
                    if expanded { "▾" } else { "▸" }
                } else {
                    ""
                };
                let is_active = focused_path.is_some_and(|p| p == row.rel_path);
                let text_color = if is_active {
                    tokens.accent
                } else {
                    tokens.text_primary
                };
                let type_icon = if row.is_dir {
                    icons::Icon::Folder
                } else if row.rel_path.ends_with(".pdf") {
                    icons::Icon::Pdf
                } else {
                    icons::Icon::File
                };
                let dirty = self.sessions.md.values().any(|session| {
                    session.rel_path == row.rel_path && session.doc.buffer().is_dirty()
                });
                let content = row![
                    iced::widget::Space::new().width(indent),
                    text(chevron)
                        .size(11)
                        .color(tokens.text_muted)
                        .width(iced::Length::Fixed(12.0)),
                    icons::view(type_icon, text_color, 15.0),
                    text(if dirty {
                        format!("{} ●", row.label)
                    } else {
                        row.label.clone()
                    })
                    .size(13)
                    .color(text_color)
                ]
                .align_y(iced::Alignment::Center)
                .spacing(5);

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
                                .then_some(iced::Background::Color(tokens.bg_tertiary)),
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

            let sidebar_bg = tokens.bg_secondary;
            let border_color = tokens.border;

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
                    .style(move |_| container::Style {
                        background: Some(iced::Background::Color(tokens.border_subtle)),
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
                    row_children.push(drag::panel_resizer(
                        drag::PanelKind::Toc,
                        session.toc_width,
                        tokens,
                    ));
                    row_children.push(self.view_pdf_toc_panel(session, tab));
                }
                if session.annotations_open {
                    row_children.push(drag::panel_resizer(
                        drag::PanelKind::Annotations,
                        session.annotations_width,
                        tokens,
                    ));
                    row_children.push(self.view_pdf_annotations_panel(session, tab));
                }
            } else if let Some(session) = self.focused_md()
                && session.outline_open
            {
                row_children.push(drag::panel_resizer(
                    drag::PanelKind::Outline,
                    session.outline_width,
                    tokens,
                ));
                row_children.push(self.view_md_outline_panel(session, tab));
            }
        }

        if self.tracker_open {
            let tracker_panel = tracker_view::view(
                tokens,
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
                text(self.status.clone()).size(13).color(tokens.text_muted),
                iced::widget::Space::new().width(Fill),
                text(self.position_status.clone())
                    .size(13)
                    .color(tokens.text_muted)
            ]
            .width(Fill),
        )
        .padding([4, 10])
        .width(Fill);

        let base = column![
            menu::bar(self.open_menu, tokens),
            container(workspace_content).height(Fill),
            status
        ];
        let mut final_view: Element<'_, Message> = if let Some(overlay) = &self.overlay {
            stack![
                base,
                overlay::view(overlay, &self.registry, &self.files, tokens)
            ]
            .into()
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
            stack![
                base,
                file_tree::context_popover(self.tree_width, *is_dir, tokens)
            ]
            .into()
        } else if let Some(open_menu) = self.open_menu {
            let model = menu::menu_model(
                &self.registry,
                focused_kind,
                self.ws.focused_tab().is_some(),
            );
            stack![
                base,
                menu::popover(open_menu, model, &self.registry, tokens)
            ]
            .into()
        } else {
            base.into()
        };

        if !self.toasts.is_empty() {
            final_view = stack![final_view, self.view_toasts()].into();
        }
        final_view
    }
}

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
pub mod keys;
pub mod overlay;
mod pdf_view;
pub mod session;
pub mod snapshot;

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

/// Default highlight color for new annotations.
const HIGHLIGHT_COLOR: &str = "#ffd866";

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
    OverlayPick(usize),
    WindowCloseRequested,
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
}

impl Shell {
    pub fn new(registry: CommandRegistry, keymap: Keymap, vault_root: PathBuf) -> Shell {
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

    fn theme(&self) -> iced::Theme {
        iced::Theme::Dark
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
            self.overlay_raw_input(&ev);
            return Task::none();
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

    fn overlay_raw_input(&mut self, ev: &keys::KeyEvent) {
        let Some(overlay) = self.overlay.as_mut() else {
            return;
        };
        match ev.chord.map(|c| c.key) {
            Some(Key::Backspace) => {
                overlay.input_mut().pop();
            }
            Some(Key::Up) => {
                if let Some(sel) = overlay.selected_mut() {
                    *sel = sel.saturating_sub(1);
                }
            }
            Some(Key::Down) => {
                if let Some(sel) = overlay.selected_mut() {
                    *sel += 1; // clamped against the row count on confirm/draw
                }
            }
            _ => {
                if let Some(t) = &ev.text {
                    overlay.input_mut().push_str(t);
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
        SessionSnapshot { layout, views }
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
        for page in 0..session.page_count() as u32 {
            pdf_view::load_page_chars(session, &abs, page);
        }
        self.open_overlay(Overlay::PdfFind {
            input: String::new(),
            selected: 0,
            hits: Vec::new(),
        });
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

    fn search_vault(&mut self, query: &str) -> Vec<md3_vault::Hit> {
        let Some(index) = self.index.as_ref() else {
            return Vec::new();
        };
        index.search(query, 12).unwrap_or_default()
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
        let status = container(text(self.status.clone()).size(13).color(colors::MARKER))
            .padding([4, 10])
            .width(Fill);

        let base = column![container(workspace).height(Fill), status];
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
                    .color(colors::MARKER),
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
                    _ => container(text("unsupported editor kind").color(colors::MARKER))
                        .center(Fill)
                        .into(),
                }
            }
        };

        let border_color = if pane_focused {
            iced::Color::from_rgb(0.45, 0.55, 0.85)
        } else {
            iced::Color::from_rgb(0.25, 0.25, 0.33)
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
    container(text("document failed to load").color(colors::MARKER))
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

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

use std::path::{Path, PathBuf};

use iced::widget::{button, canvas, column, container, row, stack, text};
use iced::{Element, Fill, Subscription, Task};
use md3_editor::buffer::{Command, Movement};
use md3_kernel::input::{Chord, EditorKind, Key};
use md3_kernel::pane::{DocumentId, Layout, Pane, TabId};
use md3_kernel::{CommandId, CommandRegistry, Keymap, SplitAxis, Workspace};
use md3_vault::SearchIndex;

use editor_canvas::{EditorCanvas, palette as colors};
use overlay::Overlay;
use session::{MdSession, PdfSession, Sessions};

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
    OverlayPick(usize),
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
    /// FTS index, built lazily on first vault search.
    index: Option<SearchIndex>,
    status: String,
    last_command: Option<CommandId>,
}

impl Shell {
    pub fn new(registry: CommandRegistry, keymap: Keymap, vault_root: PathBuf) -> Shell {
        Shell {
            registry,
            keymap,
            ws: Workspace::new(),
            sessions: Sessions::default(),
            vault_root,
            overlay: None,
            files: Vec::new(),
            index: None,
            status: "ctrl+p open file · ctrl+shift+p commands".to_string(),
            last_command: None,
        }
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
        iced::keyboard::listen().map(|event| match keys::normalize(&event) {
            Some(ev) => Message::Key(ev),
            None => Message::Ignored,
        })
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
                Task::none()
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
            Message::OverlayPick(i) => {
                if let Some(sel) = self.overlay.as_mut().and_then(Overlay::selected_mut) {
                    *sel = i;
                }
                self.confirm_overlay()
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
    }

    // --------------------------------------------------------- commands --

    fn run_command(&mut self, cmd: CommandId) -> Task<Message> {
        self.last_command = Some(cmd);
        self.status = format!("⌘ {}", cmd.0);
        match cmd.0 {
            "app.quit" => return iced::exit(),
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
            "pdf.find" => self.status = "pdf find: not yet implemented".to_string(),
            "overlay.close" => self.close_overlay(),
            "overlay.confirm" => return self.confirm_overlay(),
            other => self.status = format!("unhandled command: {other}"),
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
                    session.go_to_page((page.saturating_sub(1)) as usize);
                    let abs = root.join(&session.rel_path);
                    pdf_view::ensure_tiles(session, &abs);
                }
            }
        }
        self.sync_status();
        Task::none()
    }

    // ------------------------------------------------------------ actions --

    fn open_document(&mut self, rel: &str) {
        let kind = if rel.ends_with(".pdf") {
            EditorKind::Pdf
        } else {
            EditorKind::Markdown
        };
        let tab = match self.ws.open(rel, kind) {
            Ok(t) => t,
            Err(e) => {
                self.status = e.to_string();
                return;
            }
        };
        let Some(doc) = self.tab_document(tab) else {
            return;
        };
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
                            return;
                        }
                    }
                }
            }
            EditorKind::Pdf => {
                let entry = self
                    .sessions
                    .pdf
                    .entry(doc)
                    .or_insert_with(|| PdfSession::new(rel));
                pdf_view::load_geometry(entry, &abs);
                pdf_view::ensure_tiles(entry, &abs);
            }
            _ => {}
        }
        self.sync_status();
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
        match SearchIndex::open_in_memory() {
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
            self.status = format!(
                "{} — p. {}/{} · {:.0}%",
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

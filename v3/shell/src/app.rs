//! The iced application: the kernel `Workspace` wired into a real window
//! (plan §5 M1 "dogfood-internal"; ADR-0100).
//!
//! Input discipline — the whole point of the kernel — holds here:
//!
//! 1. ONE keyboard subscription. No widget binds keys; every press is
//!    normalized by [`crate::keys`] and resolved by `Workspace::handle_key`
//!    against the *derived* scope stack (the status bar displays it live).
//! 2. Resolved commands are dispatched on the kernel `CommandBus` and drained
//!    in `update` — palette clicks and key chords take the identical path.
//! 3. Unresolved chords are raw text. Today the only raw-text consumers are
//!    the overlays (palette query, paths, digits); the editor widget joins
//!    them when the buffer engine lands.
//!
//! Engine surfaces (markdown buffer, pdf tiles) render as placeholders —
//! those are later sessions; this one makes the workspace chrome real.

use iced::widget::{
    button, center, column, container, mouse_area, opaque, row, space, stack, text,
};
use iced::{Element, Length, Subscription, Task, Theme};

use md3_kernel::command::Invocation;
use md3_kernel::defaults::default_registry;
use md3_kernel::pane::{Layout, Pane, PaneError, Tab};
use md3_kernel::{
    Chord, CommandBus, CommandId, CommandRegistry, EditorKind, Key, Keymap, PaneId, SplitAxis,
    TabId, Workspace,
};

use crate::keys;

/// Shell-side state for the kernel's modal overlay. The kernel only knows an
/// overlay is open (the scope fence); the text being typed into it lives here.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum OverlayUi {
    #[default]
    None,
    Palette {
        query: String,
        selected: usize,
    },
    QuickOpen {
        path: String,
    },
    GoToPage {
        digits: String,
    },
    Zoom {
        digits: String,
    },
}

impl OverlayUi {
    /// The name the kernel sees (its overlay state is `Option<&'static str>`).
    fn kernel_name(&self) -> Option<&'static str> {
        match self {
            OverlayUi::None => None,
            OverlayUi::Palette { .. } => Some("palette"),
            OverlayUi::QuickOpen { .. } => Some("quick-open"),
            OverlayUi::GoToPage { .. } => Some("go-to-page"),
            OverlayUi::Zoom { .. } => Some("zoom"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Message {
    /// A normalized key press — the only keyboard entry point.
    Key(Chord),
    /// A keyboard event that doesn't normalize to a chord (bare modifier,
    /// key release): ignored, but the subscription must map to *something*.
    Noop,
    FocusTab(TabId),
    CloseTab(TabId),
    FocusPane(PaneId),
    /// A palette entry was clicked (Enter goes through `overlay.confirm`).
    PaletteEntry(CommandId),
}

pub struct App {
    registry: CommandRegistry,
    keymap: Keymap,
    ws: Workspace,
    bus: CommandBus,
    overlay: OverlayUi,
    status: String,
    last_command: Option<CommandId>,
}

/// Run the windowed shell. The caller (main) has already verified the
/// registry and keymap build cleanly — a conflict exits non-zero *before*
/// any window opens.
pub fn run() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(|_app: &App| Theme::TokyoNightStorm)
        .window(iced::window::Settings {
            size: iced::Size::new(1200.0, 800.0),
            ..Default::default()
        })
        .run()
}

impl App {
    pub fn new() -> (App, Task<Message>) {
        // main() verified these before launching the UI (startup conflict
        // gate), so the fallbacks are unreachable; they exist because boot
        // cannot report errors and the unwrap lint is law in v3.
        let registry = default_registry().unwrap_or_default();
        let keymap = registry.keymap().unwrap_or_default();
        let mut ws = Workspace::new();
        let status = match ws.open("notes/welcome.md", EditorKind::Markdown) {
            Ok(_) => "ready — ctrl+shift+p opens the command palette".to_string(),
            Err(e) => e.to_string(),
        };
        (
            App {
                registry,
                keymap,
                ws,
                bus: CommandBus::new(),
                overlay: OverlayUi::None,
                status,
                last_command: None,
            },
            Task::none(),
        )
    }

    // ----- read-only access for tests and the view ---------------------------

    pub fn workspace(&self) -> &Workspace {
        &self.ws
    }

    pub fn overlay_ui(&self) -> &OverlayUi {
        &self.overlay
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn last_command(&self) -> Option<CommandId> {
        self.last_command
    }

    // ----- update loop --------------------------------------------------------

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Key(chord) => match self.ws.handle_key(&self.keymap, chord) {
                Some(cmd) => {
                    self.bus.dispatch(cmd);
                    self.drain_bus()
                }
                None => {
                    self.raw_input(chord);
                    Task::none()
                }
            },
            Message::Noop => Task::none(),
            Message::FocusTab(tab) => {
                let r = self.ws.focus_tab(tab);
                self.report(r);
                Task::none()
            }
            Message::CloseTab(tab) => {
                let r = self.ws.close_tab(tab);
                self.report(r);
                Task::none()
            }
            Message::FocusPane(pane) => {
                let active = self
                    .ws
                    .panes
                    .pane(pane)
                    .and_then(|p| p.active_tab())
                    .map(|t| t.id);
                if let Some(tab) = active {
                    let r = self.ws.focus_tab(tab);
                    self.report(r);
                }
                Task::none()
            }
            Message::PaletteEntry(id) => {
                self.close_overlay();
                self.bus.dispatch(id);
                self.drain_bus()
            }
        }
    }

    /// Execute queued invocations until the bus is empty. Commands may
    /// enqueue more (palette confirm dispatches its selection); the loop
    /// keeps draining until quiescent.
    fn drain_bus(&mut self) -> Task<Message> {
        let mut task = Task::none();
        while !self.bus.is_empty() {
            let batch: Vec<Invocation> = self.bus.drain().collect();
            for inv in batch {
                if let Some(t) = self.execute(inv.id) {
                    task = t;
                }
            }
        }
        task
    }

    /// The command handlers. Returns a task only for commands that must talk
    /// to the runtime (quit).
    fn execute(&mut self, cmd: CommandId) -> Option<Task<Message>> {
        self.last_command = Some(cmd);
        match cmd.0 {
            "app.quit" => return Some(iced::exit()),
            "palette.open" => self.open_overlay(OverlayUi::Palette {
                query: String::new(),
                selected: 0,
            }),
            "file.quick-open" => self.open_overlay(OverlayUi::QuickOpen {
                path: String::new(),
            }),
            "workspace.split-right" => self.split_right(),
            "workspace.close-tab" => {
                if let Some(tab) = self.ws.focused_tab() {
                    let r = self.ws.close_tab(tab);
                    self.report(r);
                }
            }
            "workspace.next-tab" => self.next_tab(),
            "pdf.zoom-input" => self.open_overlay(OverlayUi::Zoom {
                digits: String::new(),
            }),
            "pdf.go-to-page" => self.open_overlay(OverlayUi::GoToPage {
                digits: String::new(),
            }),
            "overlay.close" => {
                self.close_overlay();
                self.status = "overlay dismissed".to_string();
            }
            "overlay.confirm" => self.confirm_overlay(),
            // Engine-backed commands: routed correctly today, executed when
            // their engine lands (buffer: next session; vault index, pdf
            // renderer after that).
            "editor.undo" | "editor.redo" | "editor.save" | "editor.find" => {
                self.status = format!(
                    "{cmd}: routed to markdown editor (buffer engine lands in a later session)"
                );
            }
            "pdf.find" | "search.global" => {
                self.status = format!("{cmd}: routed (engine lands in a later session)");
            }
            other => self.status = format!("`{other}` has no handler yet"),
        }
        None
    }

    /// Unresolved chord → raw text input. Only overlays consume raw text
    /// today. Characters arrive lowercased from [`keys`]; queries are matched
    /// case-insensitively and demo paths are lowercase, so nothing is lost.
    fn raw_input(&mut self, chord: Chord) {
        if chord.mods.ctrl || chord.mods.alt || chord.mods.meta {
            return;
        }
        match &mut self.overlay {
            OverlayUi::None => {}
            OverlayUi::Palette { query, selected } => match chord.key {
                Key::Char(c) => {
                    query.push(c);
                    *selected = 0;
                }
                Key::Space => {
                    query.push(' ');
                    *selected = 0;
                }
                Key::Backspace => {
                    query.pop();
                    *selected = 0;
                }
                Key::Down => {
                    let n = self.registry.palette(query).len();
                    if n > 0 {
                        *selected = (*selected + 1).min(n - 1);
                    }
                }
                Key::Up => *selected = selected.saturating_sub(1),
                _ => {}
            },
            OverlayUi::QuickOpen { path } => match chord.key {
                Key::Char(c) => path.push(c),
                Key::Space => path.push(' '),
                Key::Backspace => {
                    path.pop();
                }
                _ => {}
            },
            OverlayUi::GoToPage { digits } | OverlayUi::Zoom { digits } => match chord.key {
                Key::Char(c) if c.is_ascii_digit() => digits.push(c),
                Key::Backspace => {
                    digits.pop();
                }
                _ => {}
            },
        }
    }

    fn open_overlay(&mut self, ui: OverlayUi) {
        if let Some(name) = ui.kernel_name() {
            self.ws.open_overlay(name);
        }
        self.overlay = ui;
    }

    fn close_overlay(&mut self) {
        self.overlay = OverlayUi::None;
        self.ws.close_overlay();
    }

    fn confirm_overlay(&mut self) {
        let overlay = std::mem::take(&mut self.overlay);
        self.ws.close_overlay();
        match overlay {
            OverlayUi::None => {}
            OverlayUi::Palette { query, selected } => {
                let target = self.registry.palette(&query).get(selected).map(|s| s.id);
                match target {
                    // Picked up by the ongoing drain_bus() loop.
                    Some(id) => self.bus.dispatch(id),
                    None => self.status = format!("palette: nothing matches `{query}`"),
                }
            }
            OverlayUi::QuickOpen { path } => {
                let path = path.trim();
                if path.is_empty() {
                    self.status = "quick open: empty path".to_string();
                } else {
                    match self.ws.open(path, kind_for_path(path)) {
                        Ok(_) => self.status = format!("opened {path}"),
                        Err(e) => self.status = e.to_string(),
                    }
                }
            }
            OverlayUi::GoToPage { digits } => {
                self.status = match digits.parse::<u32>() {
                    Ok(n) => format!("go to page {n} (pdf renderer lands in a later session)"),
                    Err(_) => "go to page: no page number entered".to_string(),
                };
            }
            OverlayUi::Zoom { digits } => {
                self.status = match digits.parse::<u32>() {
                    Ok(n) => format!("zoom {n}% (pdf renderer lands in a later session)"),
                    Err(_) => "zoom: no level entered".to_string(),
                };
            }
        }
    }

    fn split_right(&mut self) {
        let Some(tab) = self.ws.focused_tab() else {
            self.status = "split: nothing focused".to_string();
            return;
        };
        let doc = self
            .ws
            .panes
            .find_tab(tab)
            .and_then(|(_, t)| self.ws.docs.get(t.document))
            .map(|d| (d.path.clone(), d.kind));
        let Some((path, kind)) = doc else {
            return;
        };
        match self
            .ws
            .open_in_new_split(&path, kind, SplitAxis::Horizontal)
        {
            Ok(_) => self.status = format!("split right — {path} in both panes"),
            Err(e) => self.status = e.to_string(),
        }
    }

    fn next_tab(&mut self) {
        let Some(pane_id) = self.ws.focused_pane() else {
            return;
        };
        let next = self.ws.panes.pane(pane_id).and_then(|p| {
            let tabs = p.tabs();
            let active = p.active_tab()?;
            let i = tabs.iter().position(|t| t.id == active.id)?;
            Some(tabs[(i + 1) % tabs.len()].id)
        });
        if let Some(tab) = next {
            let r = self.ws.focus_tab(tab);
            self.report(r);
        }
    }

    fn report(&mut self, result: Result<(), PaneError>) {
        if let Err(e) = result {
            self.status = e.to_string();
        }
    }

    // ----- view ----------------------------------------------------------------

    pub fn title(&self) -> String {
        let focused = self
            .ws
            .focused_tab()
            .and_then(|tab| self.ws.panes.find_tab(tab))
            .and_then(|(_, t)| self.ws.docs.get(t.document));
        match focused {
            Some(doc) => format!("md3 — {}", doc.path),
            None => "md3".to_string(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        iced::keyboard::listen().map(on_keyboard_event)
    }

    pub fn view(&self) -> Element<'_, Message> {
        let body = self.layout_view(self.ws.panes.layout());
        let base = column![
            container(body)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(6),
            self.status_bar(),
        ];
        if self.overlay == OverlayUi::None {
            base.into()
        } else {
            stack![base, self.overlay_view()].into()
        }
    }

    fn layout_view<'a>(&'a self, layout: Layout<'a>) -> Element<'a, Message> {
        match layout {
            Layout::Pane(pane) => self.pane_view(pane),
            Layout::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let a = (ratio * 100.0).round().clamp(1.0, 99.0) as u16;
                let b = 100 - a;
                let first = container(self.layout_view(*first))
                    .width(Length::Fill)
                    .height(Length::Fill);
                let second = container(self.layout_view(*second))
                    .width(Length::Fill)
                    .height(Length::Fill);
                match axis {
                    SplitAxis::Horizontal => row![
                        first.width(Length::FillPortion(a)),
                        second.width(Length::FillPortion(b)),
                    ]
                    .spacing(6)
                    .into(),
                    SplitAxis::Vertical => column![
                        first.height(Length::FillPortion(a)),
                        second.height(Length::FillPortion(b)),
                    ]
                    .spacing(6)
                    .into(),
                }
            }
        }
    }

    fn pane_view<'a>(&'a self, pane: &'a Pane) -> Element<'a, Message> {
        let focused = self.ws.focused_pane() == Some(pane.id);
        let active = pane.active_tab().map(|t| t.id);

        let mut strip = row![].spacing(4);
        for tab in pane.tabs() {
            let name = self
                .ws
                .docs
                .get(tab.document)
                .map(|d| file_name(&d.path).to_string())
                .unwrap_or_else(|| "?".to_string());
            let style = if active == Some(tab.id) {
                button::primary
            } else {
                button::secondary
            };
            strip = strip.push(
                button(text(format!("{} {name}", kind_badge(tab.editor))).size(13))
                    .style(style)
                    .on_press(Message::FocusTab(tab.id)),
            );
            strip = strip.push(
                button(text("×").size(13))
                    .style(button::text)
                    .on_press(Message::CloseTab(tab.id)),
            );
        }

        let content: Element<'a, Message> = match pane.active_tab() {
            Some(tab) => self.tab_content(tab),
            None => center(text("empty pane — ctrl+p to open a file").size(14)).into(),
        };

        let inner = column![
            strip,
            container(content).width(Length::Fill).height(Length::Fill),
        ]
        .spacing(8);
        let framed = container(inner)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(8)
            .style(move |theme: &Theme| pane_frame(theme, focused));
        mouse_area(framed)
            .on_press(Message::FocusPane(pane.id))
            .into()
    }

    /// Engine placeholder: identifies the document and which scoped bindings
    /// are live for it. Replaced by real engine surfaces in later sessions.
    fn tab_content<'a>(&'a self, tab: &'a Tab) -> Element<'a, Message> {
        let path = self
            .ws
            .docs
            .get(tab.document)
            .map(|d| d.path.as_str())
            .unwrap_or("?");
        let (surface, hint) = match tab.editor {
            EditorKind::Markdown => (
                "markdown editor",
                "ctrl+z undo · ctrl+s save · ctrl+f find (buffer engine lands next)",
            ),
            EditorKind::Pdf => (
                "pdf viewer",
                "ctrl+z zoom · ctrl+g go to page · ctrl+f find (tile renderer lands later)",
            ),
            EditorKind::Image => ("image viewer", ""),
            EditorKind::Graph => ("graph view", ""),
        };
        center(
            column![
                text(kind_badge(tab.editor)).size(28),
                text(path).size(16),
                text(surface).size(13),
                text(hint).size(11),
            ]
            .spacing(6)
            .align_x(iced::Alignment::Center),
        )
        .into()
    }

    /// The scope stack on the left is computed by `Workspace::scope_stack()`
    /// per frame — the live proof there is no hand-synced focus flag anywhere.
    fn status_bar(&self) -> Element<'_, Message> {
        let scopes = self
            .ws
            .scope_stack()
            .iter()
            .map(scope_name)
            .collect::<Vec<_>>()
            .join(" ▸ ");
        let last = match self.last_command {
            Some(cmd) => format!("⌁ {cmd}"),
            None => String::new(),
        };
        container(
            row![
                text(scopes).size(12),
                space().width(Length::Fill),
                text(&self.status).size(12),
                text(last).size(12),
            ]
            .spacing(16),
        )
        .width(Length::Fill)
        .padding(6)
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style {
                background: Some(palette.background.weak.color.into()),
                ..container::Style::default()
            }
        })
        .into()
    }

    fn overlay_view(&self) -> Element<'_, Message> {
        let card: Element<'_, Message> = match &self.overlay {
            OverlayUi::None => text("").into(), // unreachable: view() checks first
            OverlayUi::Palette { query, selected } => self.palette_card(query, *selected),
            OverlayUi::QuickOpen { path } => prompt_card(
                "Quick Open",
                path,
                "type a path — .pdf opens a pdf viewer tab, anything else markdown",
            ),
            OverlayUi::GoToPage { digits } => prompt_card("Go to Page", digits, "digits · enter"),
            OverlayUi::Zoom { digits } => prompt_card("Set Zoom Level", digits, "digits · enter"),
        };
        // `opaque` swallows clicks: the workspace underneath is inert while
        // the modal fence is up — pointer events obey the fence like keys do.
        opaque(center(card).style(|_theme: &Theme| container::Style {
            background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
            ..container::Style::default()
        }))
    }

    fn palette_card<'a>(&'a self, query: &'a str, selected: usize) -> Element<'a, Message> {
        let hits = self.registry.palette(query);
        let mut list = column![].spacing(2);
        for (i, spec) in hits.iter().take(8).enumerate() {
            let chord_hint = spec
                .bindings
                .first()
                .map(|b| b.chord.to_string())
                .unwrap_or_default();
            let entry = row![
                text(spec.title).size(13).width(Length::Fill),
                text(spec.id.0).size(11),
                text(chord_hint).size(11),
            ]
            .spacing(12);
            let style = if i == selected {
                button::primary
            } else {
                button::text
            };
            list = list.push(
                button(entry)
                    .width(Length::Fill)
                    .style(style)
                    .on_press(Message::PaletteEntry(spec.id)),
            );
        }
        if hits.is_empty() {
            list = list.push(text("no matching command").size(12));
        }
        card(
            column![
                text("Command Palette").size(12),
                text(format!("› {query}▏")).size(15),
                list,
                text("type to filter · ↑↓ select · enter run · esc dismiss").size(11),
            ]
            .spacing(10),
        )
    }
}

fn on_keyboard_event(event: iced::keyboard::Event) -> Message {
    match event {
        iced::keyboard::Event::KeyPressed { key, modifiers, .. } => keys::chord(key, modifiers)
            .map(Message::Key)
            .unwrap_or(Message::Noop),
        _ => Message::Noop,
    }
}

fn prompt_card<'a>(title: &'a str, value: &'a str, hint: &'a str) -> Element<'a, Message> {
    card(
        column![
            text(title).size(12),
            text(format!("› {value}▏")).size(15),
            text(hint).size(11),
        ]
        .spacing(10),
    )
}

fn card(content: iced::widget::Column<'_, Message>) -> Element<'_, Message> {
    container(content)
        .width(560)
        .padding(16)
        .style(container::rounded_box)
        .into()
}

fn pane_frame(theme: &Theme, focused: bool) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(palette.background.base.color.into()),
        border: iced::Border {
            color: if focused {
                palette.primary.strong.color
            } else {
                palette.background.strong.color
            },
            width: 1.0,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    }
}

fn scope_name(scope: &md3_kernel::Scope) -> String {
    match scope {
        md3_kernel::Scope::Global => "Global".to_string(),
        md3_kernel::Scope::Workspace => "Workspace".to_string(),
        md3_kernel::Scope::Pane => "Pane".to_string(),
        md3_kernel::Scope::Editor(kind) => format!("Editor({kind:?})"),
        md3_kernel::Scope::Overlay => "Overlay".to_string(),
    }
}

fn kind_badge(kind: EditorKind) -> &'static str {
    match kind {
        EditorKind::Markdown => "MD",
        EditorKind::Pdf => "PDF",
        EditorKind::Image => "IMG",
        EditorKind::Graph => "GRAPH",
    }
}

fn kind_for_path(path: &str) -> EditorKind {
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext.to_ascii_lowercase().as_str() {
        "pdf" => EditorKind::Pdf,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => EditorKind::Image,
        _ => EditorKind::Markdown,
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

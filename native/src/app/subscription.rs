//! Subscriptions: keyboard, timers, OS theme, drag-and-drop, window events, pen input and vault watcher.

use super::*;
use crate::messages::{
    EditorMessage, GraphMessage, InkMessage, PdfMessage, SearchMessage, SettingsMessage,
    ShellMessage, SidebarNavKey, TabsMessage, VaultMessage,
};

impl MdEditor {
    pub fn subscription(&self) -> Subscription<Message> {
        let sidebar_focused = self.ui.focused_region == crate::ui::FocusRegion::Sidebar
            && self.vault.sidebar_visible
            && self.ui.active_modal.is_none()
            && !self.ui.command_palette_visible
            && !self.search.visible;
        let global_search_open = self.search.visible;
        let completion_open = self.editor.completion.is_some();
        let palette_open = self.ui.command_palette_visible;
        let graph_nav_active = self.graph.visible
            && self.ui.active_modal.is_none()
            && !self.ui.command_palette_visible
            && !self.search.visible
            && !self.ui.settings_visible
            && self.ui.context_menu.is_none();

        // App-level keyboard handling. Returns `None` for keys we don't act on
        // so ordinary text entry (handled by the focused editor widget) doesn't
        // spawn a redundant update cycle. `listen_with` takes a plain `fn`, so
        // the context flags ride along via `with` instead of a capture.
        let keyboard = iced::event::listen_with(|event, status, _window_id| match event {
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                Some((key, modifiers, status))
            }
            _ => None,
        })
        .with((
            sidebar_focused,
            global_search_open,
            completion_open,
            palette_open,
            graph_nav_active,
        ))
        .filter_map(
            |(
                (
                    sidebar_focused,
                    global_search_open,
                    completion_open,
                    palette_open,
                    graph_nav_active,
                ),
                (key, modifiers, status),
            )| {
                if completion_open && let Some(message) = completion_key(&key, modifiers) {
                    return Some(message);
                }
                keyboard_message(
                    &key,
                    modifiers,
                    status,
                    sidebar_focused,
                    global_search_open,
                    palette_open,
                    graph_nav_active,
                )
            },
        );

        let os_theme = iced::system::theme_changes()
            .map(|mode| Message::Settings(SettingsMessage::SystemThemeChanged(mode)));

        let vault_watch = match self.vault.root.clone() {
            Some(root) => Subscription::run_with(root, vault_watch_stream),
            None => Subscription::none(),
        };

        let scroll_interrupt = if self.motion.editor_scroll.is_some() {
            iced::event::listen_with(|event, _status, _window_id| match event {
                iced::Event::Mouse(
                    iced::mouse::Event::WheelScrolled { .. } | iced::mouse::Event::ButtonPressed(_),
                ) => Some(Message::Editor(EditorMessage::ScrollInterrupted)),
                _ => None,
            })
        } else {
            Subscription::none()
        };

        let pen_input = Subscription::run_with("ink-pen", |_: &&str| crate::ink::pen::stream());

        Subscription::batch(vec![
            keyboard,
            self.timer_subscriptions(),
            scroll_interrupt,
            self.pointer_drag_subscription(),
            window_events(),
            os_theme,
            vault_watch,
            pen_input,
        ])
    }

    /// Timers and frame ticks that run only while something is pending:
    /// toast expiry, autosave, search debounce, graph refresh and physics,
    /// and motion frames.
    fn timer_subscriptions(&self) -> Subscription<Message> {
        let toast = if let Some(text) = &self.ui.toast
            && self.ui.toast_queue.last().is_none_or(|t| !t.persistent)
        {
            iced::time::every(std::time::Duration::from_secs(3))
                .with(text.clone())
                .map(|_| Message::Shell(ShellMessage::ToastHide))
        } else {
            Subscription::none()
        };

        let autosave = if self.editor.autosave_pending_since.is_some() {
            iced::time::every(crate::editor_state::AUTOSAVE_POLL)
                .map(|_| Message::Editor(EditorMessage::AutosaveElapsed))
        } else {
            Subscription::none()
        };

        let search_debounce = if self.search.pending_query_at.is_some() {
            iced::time::every(crate::search_state::SEARCH_DEBOUNCE)
                .map(|_| Message::Search(SearchMessage::DebounceElapsed))
        } else {
            Subscription::none()
        };

        let graph_refresh = if self.graph.visible && self.graph.refresh_due() {
            iced::time::every(std::time::Duration::from_millis(200))
                .map(|_| Message::Graph(GraphMessage::Refresh))
        } else {
            Subscription::none()
        };

        let animation = if self.motion.is_animating() {
            iced::window::frames().map(|value| Message::Editor(EditorMessage::AnimationTick(value)))
        } else {
            Subscription::none()
        };

        let ink_autosave = if self.ink.dirty && self.vault.root.is_some() {
            iced::time::every(crate::ink::INK_AUTOSAVE_DELAY)
                .map(|_| Message::Ink(InkMessage::AutosaveElapsed))
        } else {
            Subscription::none()
        };

        let physics_tick = if self.graph.visible && self.graph.physics.running {
            iced::time::every(std::time::Duration::from_millis(33))
                .map(|_| Message::Graph(GraphMessage::PhysicsTick))
        } else {
            Subscription::none()
        };

        Subscription::batch(vec![
            toast,
            autosave,
            search_debounce,
            graph_refresh,
            physics_tick,
            animation,
            ink_autosave,
        ])
    }

    /// Mouse tracking while a split, panel or tab is dragged, or files hover.
    fn pointer_drag_subscription(&self) -> Subscription<Message> {
        use iced::mouse::{Button, Event as Mouse};

        // Each listener reports the window an event came from; `main_window_only`
        // drops cursor motion from the ink window, where the pen moving over the
        // canvas would otherwise drag the shell's split or panel edge. Releases
        // are accepted from any window, so a drag can always end.
        let listener = if self.ui.is_resizing_split {
            iced::event::listen_with(|event, _status, window| match event {
                iced::Event::Mouse(Mouse::CursorMoved { position }) => Some((
                    Message::Shell(ShellMessage::SplitViewDragging(position)),
                    Some(window),
                )),
                iced::Event::Mouse(Mouse::ButtonReleased(Button::Left)) => {
                    Some((Message::Shell(ShellMessage::SplitViewDragEnd), None))
                }
                _ => None,
            })
        } else if self.ui.resizing_panel.is_some() {
            iced::event::listen_with(|event, _status, window| match event {
                iced::Event::Mouse(Mouse::CursorMoved { position }) => Some((
                    Message::Shell(ShellMessage::PanelResizeDrag(position.x)),
                    Some(window),
                )),
                iced::Event::Mouse(Mouse::ButtonReleased(Button::Left)) => {
                    Some((Message::Shell(ShellMessage::PanelResizeEnd), None))
                }
                _ => None,
            })
        } else if self.ui.dragging_tab.is_some() {
            iced::event::listen_with(|event, _status, _window| match event {
                iced::Event::Mouse(Mouse::ButtonReleased(Button::Left)) => {
                    Some((Message::Tabs(TabsMessage::DragEnd), None))
                }
                _ => None,
            })
        } else if !self.ui.hovered_drop_files.is_empty() {
            iced::event::listen_with(|event, _status, window| match event {
                iced::Event::Mouse(Mouse::CursorMoved { position }) => Some((
                    Message::Shell(ShellMessage::CursorMoved(position.x, position.y)),
                    Some(window),
                )),
                _ => None,
            })
        } else {
            return Subscription::none();
        };
        main_window_only(listener, self.main_window)
    }
}

/// Keep a listener's messages from the main window, plus those tagged `None`
/// (any window). Before the main window id is known, everything passes.
fn main_window_only(
    listener: Subscription<(Message, Option<iced::window::Id>)>,
    main_window: Option<iced::window::Id>,
) -> Subscription<Message> {
    listener
        .with(main_window)
        .filter_map(
            |(main_window, (message, window))| match (main_window, window) {
                (Some(main), Some(window)) if main != window => None,
                _ => Some(message),
            },
        )
}

/// Window lifecycle, resize, rescale and file drag-and-drop for both windows.
fn window_events() -> Subscription<Message> {
    iced::event::listen_with(|event, _status, window_id| match event {
        iced::Event::Window(iced::window::Event::Resized(size)) => Some(Message::Shell(
            ShellMessage::WindowResized(window_id, size.width, size.height),
        )),
        iced::Event::Window(iced::window::Event::Opened { .. }) => {
            Some(Message::Shell(ShellMessage::WindowOpened(window_id)))
        }
        iced::Event::Window(iced::window::Event::Closed) => {
            Some(Message::Shell(ShellMessage::WindowClosed(window_id)))
        }
        iced::Event::Window(iced::window::Event::Rescaled(factor)) => Some(Message::Shell(
            ShellMessage::WindowRescaled(window_id, factor),
        )),
        iced::Event::Window(iced::window::Event::CloseRequested) => Some(Message::Shell(
            ShellMessage::WindowCloseRequested(window_id),
        )),
        iced::Event::Window(iced::window::Event::FileHovered(path)) => {
            Some(Message::Shell(ShellMessage::FileHovered(window_id, path)))
        }
        iced::Event::Window(iced::window::Event::FilesHoveredLeft) => {
            Some(Message::Shell(ShellMessage::FilesHoveredLeft))
        }
        iced::Event::Window(iced::window::Event::FileDropped(path)) => {
            Some(Message::Shell(ShellMessage::FileDropped(window_id, path)))
        }
        _ => None,
    })
}

pub(super) const VAULT_WATCH_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(300);

#[allow(clippy::ptr_arg)]
pub(super) fn vault_watch_stream(
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
                    log::warn!("Failed to create vault watcher: {e}");
                    return;
                }
            };
            if let Err(e) = watcher.watch(&root_path, RecursiveMode::Recursive) {
                log::warn!("Failed to watch vault root {}: {e}", root_path.display());
                return;
            }
            let _watcher = watcher;

            loop {
                let Some(first) = rx.recv().await else {
                    break;
                };
                let mut changed = std::collections::BTreeSet::new();
                collect_changed_paths(&first, &root_path, &mut changed);
                while let Ok(Some(event)) =
                    tokio::time::timeout(VAULT_WATCH_DEBOUNCE, rx.recv()).await
                {
                    collect_changed_paths(&event, &root_path, &mut changed);
                }
                if !changed.is_empty() {
                    let paths: Vec<String> = changed.into_iter().collect();
                    if output
                        .send(Message::Vault(VaultMessage::VaultFilesChanged(paths)))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        },
    ))
}

pub(super) fn collect_changed_paths(
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
        let rel = rel.to_string_lossy().replace('\\', "/");
        // The same folders the vault walk skips, the user's own included.
        if md_editor_core::vault::is_excluded_rel_path(&rel) {
            continue;
        }
        out.insert(rel);
    }
}

/// Keys that drive an open completion popup: ↑/↓ move, Enter/Tab insert,
/// Escape closes. The editor widget leaves these alone while it is open.
pub(super) fn completion_key(
    key: &iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
) -> Option<Message> {
    use iced::keyboard::Key;
    use iced::keyboard::key::Named;
    if modifiers.command() || modifiers.control() || modifiers.alt() {
        return None;
    }
    let message = match key {
        Key::Named(Named::ArrowDown) => EditorMessage::CompletionMove(1),
        Key::Named(Named::ArrowUp) => EditorMessage::CompletionMove(-1),
        Key::Named(Named::Enter | Named::Tab) => EditorMessage::CompletionAccept,
        Key::Named(Named::Escape) => EditorMessage::CompletionClose,
        _ => return None,
    };
    Some(Message::Editor(message))
}

pub(super) fn keyboard_message(
    key: &iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
    status: iced::event::Status,
    sidebar_focused: bool,
    global_search_open: bool,
    palette_open: bool,
    graph_nav_active: bool,
) -> Option<Message> {
    use iced::keyboard::Key;
    use iced::keyboard::key::Named;

    if *key == Key::Named(Named::Escape) {
        return Some(Message::KeyboardShortcut(Shortcut::Escape));
    }
    if *key == Key::Named(Named::F6) {
        return if modifiers.shift() {
            Some(Message::KeyboardShortcut(Shortcut::CycleFocusPrev))
        } else {
            Some(Message::KeyboardShortcut(Shortcut::CycleFocusNext))
        };
    }
    if *key == Key::Named(Named::F11) {
        return Some(Message::KeyboardShortcut(Shortcut::FocusMode));
    }
    if *key == Key::Named(Named::F2) && status != iced::event::Status::Captured {
        return Some(Message::KeyboardShortcut(Shortcut::RenameEntry));
    }

    let ctrl = modifiers.command() || modifiers.control();
    if ctrl {
        match key {
            Key::Named(Named::Tab) => {
                return if modifiers.shift() {
                    Some(Message::Tabs(TabsMessage::MruPrevious))
                } else {
                    Some(Message::Tabs(TabsMessage::MruNext))
                };
            }
            Key::Named(Named::PageUp) => {
                return if modifiers.shift() {
                    Some(Message::Tabs(TabsMessage::MoveBy(-1)))
                } else {
                    Some(Message::Tabs(TabsMessage::Previous))
                };
            }
            Key::Named(Named::PageDown) => {
                return if modifiers.shift() {
                    Some(Message::Tabs(TabsMessage::MoveBy(1)))
                } else {
                    Some(Message::Tabs(TabsMessage::Next))
                };
            }
            _ => {}
        }
    }

    if *key == Key::Named(Named::Enter) {
        if ctrl && palette_open {
            return Some(Message::Shell(ShellMessage::CommandPaletteOpenToSide));
        }
        if sidebar_focused && status != iced::event::Status::Captured {
            return Some(Message::Vault(VaultMessage::SidebarKeyNav(
                SidebarNavKey::Enter,
            )));
        }
        return Some(Message::Shell(ShellMessage::OverlaySubmitCurrent));
    }

    if ctrl && let Key::Character(c) = key.as_ref() {
        if status == iced::event::Status::Captured {
            return None;
        }
        return ctrl_character_message(c, modifiers.shift(), modifiers.alt());
    }

    if *key == Key::Named(Named::Delete) {
        if sidebar_focused && status != iced::event::Status::Captured {
            return None;
        }
        return Some(Message::Ink(InkMessage::DeleteSelection));
    }

    if graph_nav_active && status != iced::event::Status::Captured && !ctrl && !modifiers.alt() {
        match key {
            Key::Named(Named::Tab) => {
                let delta = if modifiers.shift() { -1 } else { 1 };
                return Some(Message::Graph(GraphMessage::CycleNode(delta)));
            }
            Key::Named(Named::ArrowRight) => {
                return Some(Message::Graph(GraphMessage::CycleNode(1)));
            }
            Key::Named(Named::ArrowLeft) => {
                return Some(Message::Graph(GraphMessage::CycleNode(-1)));
            }
            Key::Character(c) if c.eq_ignore_ascii_case("p") => {
                return Some(Message::Graph(GraphMessage::ToggleSelectedPin));
            }
            _ => {}
        }
    }

    arrow_message(key, status, sidebar_focused, global_search_open)
}

/// Ctrl (or Cmd) plus a character key, with optional Shift or Alt.
fn ctrl_character_message(c: &str, shift: bool, alt: bool) -> Option<Message> {
    let lower = c.to_ascii_lowercase();
    if alt {
        return match lower.as_str() {
            "d" if !shift => Some(Message::KeyboardShortcut(Shortcut::SwitchDesk)),
            "r" if !shift => Some(Message::KeyboardShortcut(Shortcut::RunCodeBlock)),
            _ => None,
        };
    }
    if shift {
        return match lower.as_str() {
            "d" => Some(Message::KeyboardShortcut(Shortcut::SaveDesk)),
            "t" => Some(Message::KeyboardShortcut(Shortcut::ReopenTab)),
            "f" => Some(Message::KeyboardShortcut(Shortcut::GlobalSearch)),
            "b" => Some(Message::KeyboardShortcut(Shortcut::ToggleBacklinks)),
            "o" => Some(Message::KeyboardShortcut(Shortcut::TableOfContents)),
            "i" => Some(Message::KeyboardShortcut(Shortcut::InsertSketch)),
            "m" => Some(Message::KeyboardShortcut(Shortcut::MoveTabToOtherPane)),
            "l" => Some(Message::KeyboardShortcut(Shortcut::ToggleSplitAxis)),
            // Redo as most apps spell it; Ctrl+Y works too.
            "z" => Some(Message::Ink(InkMessage::Redo)),
            _ => None,
        };
    }
    if let Some(digit) = lower
        .chars()
        .next()
        .and_then(|ch| ch.to_digit(10))
        .filter(|d| (1..=9).contains(d))
    {
        return Some(Message::Tabs(TabsMessage::SelectOrdinal(digit as usize)));
    }
    match lower.as_str() {
        "s" => Some(Message::KeyboardShortcut(Shortcut::Save)),
        "o" => Some(Message::KeyboardShortcut(Shortcut::OpenVault)),
        "n" => Some(Message::KeyboardShortcut(Shortcut::NewFile)),
        "d" => Some(Message::KeyboardShortcut(Shortcut::DailyNote)),
        "f" => Some(Message::KeyboardShortcut(Shortcut::Search)),
        "c" => Some(Message::Pdf(PdfMessage::CopySelection)),
        "p" => Some(Message::KeyboardShortcut(Shortcut::CommandPalette)),
        "b" => Some(Message::KeyboardShortcut(Shortcut::ToggleSidebar)),
        "g" => Some(Message::KeyboardShortcut(Shortcut::KnowledgeGraph)),
        "t" => Some(Message::KeyboardShortcut(Shortcut::StudyTracker)),
        "w" => Some(Message::KeyboardShortcut(Shortcut::CloseTab)),
        "," => Some(Message::KeyboardShortcut(Shortcut::Settings)),
        "\\" => Some(Message::KeyboardShortcut(Shortcut::SplitView)),
        "j" => Some(Message::KeyboardShortcut(Shortcut::FocusOtherPane)),
        "i" => Some(Message::Ink(InkMessage::Toggle)),
        "z" => Some(Message::Ink(InkMessage::Undo)),
        "y" => Some(Message::Ink(InkMessage::Redo)),
        "=" => Some(Message::KeyboardShortcut(Shortcut::EvaluateMath)),
        _ => None,
    }
}

/// Arrow and page keys: global search results, the sidebar tree, or PDF scroll.
fn arrow_message(
    key: &iced::keyboard::Key,
    status: iced::event::Status,
    sidebar_focused: bool,
    global_search_open: bool,
) -> Option<Message> {
    use iced::keyboard::Key;
    use iced::keyboard::key::Named;

    if global_search_open {
        return match key {
            Key::Named(Named::ArrowDown) => {
                Some(Message::Search(SearchMessage::GlobalSearchMoveSelection(1)))
            }
            Key::Named(Named::ArrowUp) => Some(Message::Search(
                SearchMessage::GlobalSearchMoveSelection(-1),
            )),
            _ => None,
        };
    }

    if sidebar_focused && status != iced::event::Status::Captured {
        return match key {
            Key::Named(Named::ArrowDown) => Some(Message::Vault(VaultMessage::SidebarKeyNav(
                SidebarNavKey::Down,
            ))),
            Key::Named(Named::ArrowUp) => Some(Message::Vault(VaultMessage::SidebarKeyNav(
                SidebarNavKey::Up,
            ))),
            Key::Named(Named::ArrowLeft) => Some(Message::Vault(VaultMessage::SidebarKeyNav(
                SidebarNavKey::Left,
            ))),
            Key::Named(Named::ArrowRight) => Some(Message::Vault(VaultMessage::SidebarKeyNav(
                SidebarNavKey::Right,
            ))),
            _ => None,
        };
    }

    match key {
        Key::Named(Named::ArrowDown) => Some(Message::Pdf(PdfMessage::ScrollBy(ARROW_SCROLL))),
        Key::Named(Named::ArrowUp) => Some(Message::Pdf(PdfMessage::ScrollBy(-ARROW_SCROLL))),
        Key::Named(Named::PageDown) => Some(Message::Pdf(PdfMessage::ScrollBy(520.0))),
        Key::Named(Named::PageUp) => Some(Message::Pdf(PdfMessage::ScrollBy(-520.0))),
        _ => None,
    }
}

//! Subscriptions: keyboard, timers, window events, pen input and the vault watcher.

use super::*;

impl MdEditor {
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
}

/// Debounce window for coalescing filesystem-watcher events. Editors and tools
/// often write a file as several rapid operations (write + rename of a temp
/// file); coalescing avoids a re-index storm.
pub(super) const VAULT_WATCH_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(300);

/// Stream factory for [`iced::Subscription::run_with`]: watches `root`
/// recursively and emits a debounced [`Message::VaultFilesChanged`] listing the
/// vault-relative paths that changed. Must be a plain `fn` (not a closure) to
/// satisfy `run_with`; the root is threaded in via the `&String` argument.
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

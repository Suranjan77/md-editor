//! Handwritten notes: the ink window, pen and pointer input, tools and saving pages.

use super::*;
use crate::messages::InkMessage;

impl MdEditor {
    /// Handle a message routed here by [`MdEditor::update_inner`].
    pub(super) fn update_ink(&mut self, message: InkMessage) -> Task<Message> {
        match message {
            // ── Ink (handwritten notes) ──────────────────────────
            InkMessage::PenHookReady(raw_window_id) => {
                match crate::ink::pen::install(raw_window_id) {
                    Ok(()) => log::info!("Pen input ready (pressure, eraser, barrel button)"),
                    // Not fatal: the surface still works with a mouse.
                    Err(e) => log::warn!("Pen input unavailable: {e}"),
                }
                Task::none()
            }
            InkMessage::Toggle => match self.ink_window {
                // Already open: closing the window tears down the surface via
                // the close event, so there is one path for both routes out.
                Some(id) => iced::window::close(id),
                None => self.open_ink_window(),
            },
            InkMessage::PenSamples(samples) => {
                // The ink window's own factor: the two windows can sit on
                // monitors with different scaling.
                self.ink.apply_samples(&samples, crate::ink::scale_factor());
                Task::none()
            }
            InkMessage::PointerDown(x, y) => {
                // Mouse fallback: no pressure, so use a fixed mid-weight.
                self.ink.begin(
                    crate::ink::stroke::Vec2::new(x, y),
                    crate::ink::canvas::MOUSE_PRESSURE,
                    false,
                );
                Task::none()
            }
            InkMessage::PointerMove(x, y) => {
                self.ink.extend(
                    crate::ink::stroke::Vec2::new(x, y),
                    crate::ink::canvas::MOUSE_PRESSURE,
                );
                Task::none()
            }
            InkMessage::PointerUp => {
                self.ink.finish();
                Task::none()
            }
            InkMessage::HoverMoved(x, y) => {
                self.ink.set_hover(crate::ink::stroke::Vec2::new(x, y));
                Task::none()
            }
            InkMessage::ToolSelected(tool) => {
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
            InkMessage::EraserModeSelected(mode) => {
                self.ink.eraser_mode = mode;
                Task::none()
            }
            InkMessage::PaperSelected(paper) => {
                self.ink.set_paper(paper);
                Task::none()
            }
            // Colour and width belong to the active tool: switching to the
            // highlighter and back must not clobber the pen's settings.
            InkMessage::ColorSelected(color) => {
                if self.ink.tool == crate::ink::Tool::Highlighter {
                    self.ink.highlighter_color = color;
                } else {
                    self.ink.color = color;
                }
                Task::none()
            }
            InkMessage::SizeSelected(size) => {
                if self.ink.tool == crate::ink::Tool::Highlighter {
                    self.ink.highlighter_size = size;
                } else {
                    self.ink.options.size = size;
                }
                Task::none()
            }
            InkMessage::Pan { dx, dy } => {
                self.ink.pan_by(crate::ink::stroke::Vec2::new(dx, dy));
                Task::none()
            }
            InkMessage::Zoom { x, y, notches } => {
                self.ink
                    .zoom_by_wheel(crate::ink::stroke::Vec2::new(x, y), notches);
                Task::none()
            }
            InkMessage::ZoomStep(notches) => {
                self.ink.zoom_step(notches);
                Task::none()
            }
            InkMessage::ResetView => {
                self.ink.reset_view();
                Task::none()
            }
            InkMessage::DeleteSelection => {
                if self.ink.is_visible() {
                    self.ink.delete_selection();
                }
                Task::none()
            }
            // Guarded so the shared Ctrl+Z / Ctrl+Y bindings only reach the ink
            // surface while it is the one on screen.
            InkMessage::Undo => {
                if self.ink.is_visible() {
                    self.ink.undo();
                }
                Task::none()
            }
            InkMessage::Redo => {
                if self.ink.is_visible() {
                    self.ink.redo();
                }
                Task::none()
            }
            InkMessage::Clear => {
                self.ink.clear();
                Task::none()
            }
            InkMessage::Save => self.save_ink_page(false),
            InkMessage::AutosaveElapsed => {
                // Never mid-stroke: the live stroke is not in the document yet,
                // and a write is a stall the pen would feel.
                if self.ink.is_drawing() {
                    Task::none()
                } else {
                    self.flush_ink_page()
                }
            }
            // `dirty` is settled when the write happens, not here: strokes
            // drawn before this message arrives must stay unsaved.
            InkMessage::Saved(result) => {
                self.ui.toast = Some(match result {
                    Ok(path) => format!("Saved {path}"),
                    Err(e) => e,
                });
                Task::none()
            }
        }
    }

    /// Open the detached handwriting window.
    pub(super) fn open_ink_window(&mut self) -> Task<Message> {
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

    /// Tear down the handwriting surface after its window goes away, saving
    /// the page first.
    pub(super) fn ink_window_closed(&mut self) -> Task<Message> {
        let saved = self.flush_ink_page();
        self.ink_window = None;
        // Stops pen capture and drops any half-drawn stroke.
        self.ink.set_visible(false);
        self.active_panel = ActivePanel::Markdown;
        saved
    }

    /// Load a handwritten page from the vault and show it in its own window.
    pub(super) fn open_ink_page(&mut self, path: &str) -> Task<Message> {
        let Some(root) = self.vault.root.clone() else {
            return Task::none();
        };

        if self.ink.path.as_deref() == Some(path) {
            // Already on the canvas, possibly with unsaved strokes: reloading
            // would throw them away.
            return self.open_ink_window();
        }
        // The canvas holds one page; write the current one before replacing it.
        let saved = self.flush_ink_page();
        if self.ink.dirty {
            self.ui.toast = Some(format!(
                "Not opening {path}: the current handwritten page could not be saved"
            ));
            return saved;
        }

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

    /// Save the page if it has unsaved strokes and there is a vault to save
    /// it in. Used by autosave and before the page is closed or replaced.
    pub(super) fn flush_ink_page(&mut self) -> Task<Message> {
        if self.ink.dirty && self.vault.root.is_some() {
            self.save_ink_page(true)
        } else {
            Task::none()
        }
    }

    /// Write the handwriting surface to the vault, choosing a path on first
    /// save from the note being annotated, or the date when none is open.
    ///
    /// `quiet` (autosave) skips the success toast and reports a failure only
    /// when it differs from the last one.
    pub(super) fn save_ink_page(&mut self, quiet: bool) -> Task<Message> {
        let Some(root) = self.vault.root.clone() else {
            if quiet {
                return Task::none();
            }
            return Task::done(Message::Ink(InkMessage::Saved(Err(
                "Open a vault before saving handwritten notes".to_string(),
            ))));
        };

        let path = match self.ink.path.clone() {
            Some(path) => path,
            None => {
                let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
                let derived = derive_ink_path(self.active_path.as_deref(), &stamp);
                // A note can have several pages; never overwrite one that
                // already exists.
                let path = unique_ink_path(&derived, |candidate| {
                    crate::ink::page_exists(&root, candidate)
                });
                self.ink.path = Some(path.clone());
                path
            }
        };

        let created = !crate::ink::page_exists(&root, &path);
        match crate::ink::save_to_vault(&root, &path, &self.ink.document) {
            Ok(()) => {
                self.ink.dirty = false;
                self.ink.last_save_error = None;
                if created {
                    // List the new page now rather than when the watcher's
                    // debounce gets round to it.
                    self.vault.entries =
                        md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                }
                if quiet {
                    Task::none()
                } else {
                    Task::done(Message::Ink(InkMessage::Saved(Ok(path))))
                }
            }
            Err(e) => {
                log::warn!("Saving handwritten page {path} failed: {e}");
                let repeat = self.ink.last_save_error.as_deref() == Some(e.as_str());
                self.ink.last_save_error = Some(e.clone());
                if quiet && repeat {
                    Task::none()
                } else {
                    Task::done(Message::Ink(InkMessage::Saved(Err(e))))
                }
            }
        }
    }
}

/// A vault path with its extension removed.
///
/// Only a dot in the final component counts: a directory like `course-v1.2/`
/// must not be truncated.
pub(super) fn strip_extension(path: &str) -> &str {
    match path.rfind('.').filter(|dot| !path[*dot..].contains('/')) {
        Some(dot) => &path[..dot],
        None => path,
    }
}

/// `derived`, or the first of `stem-2.ink`, `stem-3.ink`, … not yet `taken`.
pub(super) fn unique_ink_path(derived: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(derived) {
        return derived.to_string();
    }
    let stem = strip_extension(derived);
    let extension = crate::ink::INK_EXTENSION;
    (2..10_000)
        .map(|n| format!("{stem}-{n}.{extension}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| format!("{stem}-{}.{extension}", uuid::Uuid::new_v4()))
}

/// Vault-relative path for a handwritten page: beside the note it belongs to
/// when one is open, otherwise a timestamped page at the vault root.
pub(super) fn derive_ink_path(active_path: Option<&str>, timestamp: &str) -> String {
    let extension = crate::ink::INK_EXTENSION;
    match active_path {
        Some(path) => format!("{}.{extension}", strip_extension(path)),
        None => format!("handwritten-{timestamp}.{extension}"),
    }
}

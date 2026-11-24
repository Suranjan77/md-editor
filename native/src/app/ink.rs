//! Handwritten notes: the ink window, pen and pointer input, tools and saving pages.

use super::*;
use crate::messages::InkMessage;

impl MdEditor {
    /// Handle a message routed here by [`MdEditor::update_inner`].
    pub(super) fn update_ink(&mut self, message: InkMessage) -> Task<Message> {
        match message {
            // ── Ink (handwritten notes) ──────────────────────────
            InkMessage::PenHookReady(raw_window_id) => {
                if let Err(e) = crate::ink::pen::install(raw_window_id) {
                    // Not fatal: the surface still works with a mouse.
                    log::warn!("Pen input unavailable: {e}");
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
                self.ink.apply_samples(&samples, self.ui.scale_factor);
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
            InkMessage::Save => self.save_ink_page(),
            InkMessage::Saved(result) => {
                match result {
                    Ok(path) => {
                        self.ink.dirty = false;
                        self.ui.toast = Some(format!("Saved {path}"));
                    }
                    Err(e) => self.ui.toast = Some(e),
                }
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

    /// Tear down the handwriting surface after its window goes away.
    pub(super) fn ink_window_closed(&mut self) {
        self.ink_window = None;
        // Stops pen capture and drops any half-drawn stroke.
        self.ink.set_visible(false);
        self.active_panel = ActivePanel::Markdown;
    }

    /// Load a handwritten page from the vault and show it in its own window.
    pub(super) fn open_ink_page(&mut self, path: &str) -> Task<Message> {
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
    pub(super) fn save_ink_page(&mut self) -> Task<Message> {
        let Some(root) = self.vault.root.clone() else {
            return Task::done(Message::Ink(InkMessage::Saved(Err(
                "Open a vault before saving handwritten notes".to_string(),
            ))));
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
        Task::done(Message::Ink(InkMessage::Saved(result)))
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

/// Vault-relative path for a handwritten page: beside the note it belongs to
/// when one is open, otherwise a timestamped page at the vault root.
pub(super) fn derive_ink_path(active_path: Option<&str>, timestamp: &str) -> String {
    let extension = crate::ink::INK_EXTENSION;
    match active_path {
        Some(path) => format!("{}.{extension}", strip_extension(path)),
        None => format!("handwritten-{timestamp}.{extension}"),
    }
}

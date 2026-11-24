//! Window lifecycle, split view, the command palette, overlays, the tracker and presentations.

use super::*;
use crate::messages::{
    GraphMessage, InkMessage, PdfMessage, SearchMessage, ShellMessage, VaultMessage,
};

impl MdEditor {
    /// Handle a message routed here by [`MdEditor::update_inner`].
    pub(super) fn update_shell(&mut self, message: ShellMessage) -> Task<Message> {
        match message {
            m @ (ShellMessage::CommandPaletteQueryChanged(_)
            | ShellMessage::ShowToast(_)
            | ShellMessage::ToastHide
            | ShellMessage::SplitViewDragStart) => self.ui.update(Message::Shell(m)),
            // UI chrome arms that mutate only `self.ui` are routed to
            // `UiState::update`; see ui_state.rs.
            // Opening the palette also puts the caret in its field: a palette
            // you have to click into first is not reachable from the keyboard,
            // which is the only reason it exists.
            ShellMessage::CommandPaletteOpen => {
                let task = self
                    .ui
                    .update(Message::Shell(ShellMessage::CommandPaletteOpen));
                Task::batch(vec![
                    task,
                    operation::focus(iced::advanced::widget::Id::new(
                        views::command_palette::PALETTE_INPUT_ID,
                    )),
                ])
            }
            m @ ShellMessage::TrackerToggle => self.tracker.update(Message::Shell(m), &self.state),
            ShellMessage::CommandPaletteFileClicked(path) => {
                self.ui.command_palette_visible = false;
                self.ui.command_palette_query.clear();
                // Reuse the sidebar's open path: it already routes markdown,
                // PDFs and images to the right viewer.
                Task::done(Message::Vault(VaultMessage::SidebarFileClicked(path)))
            }
            ShellMessage::CommandPaletteCommandClicked(shortcut) => {
                self.ui.command_palette_visible = false;
                self.ui.command_palette_query.clear();
                Task::done(Message::KeyboardShortcut(shortcut))
            }
            ShellMessage::OverlaySubmitCurrent => {
                if self.ui.command_palette_visible {
                    Task::done(Message::Vault(VaultMessage::NameModalSubmitCurrent))
                } else if self.search.visible {
                    Task::done(Message::Search(SearchMessage::GlobalSearchSubmit))
                } else if self.graph.visible && self.ui.active_modal.is_none() {
                    // Enter opens whatever is selected on the canvas, so the
                    // graph is navigable without reaching for the mouse.
                    match self.graph.selected_path.clone() {
                        Some(path) => Task::done(Message::Graph(GraphMessage::NodeOpen(path))),
                        None => Task::none(),
                    }
                } else {
                    Task::done(Message::Vault(VaultMessage::NameModalSubmitCurrent))
                }
            }
            m @ (ShellMessage::TrackerStart
            | ShellMessage::TrackerStop
            | ShellMessage::TrackerTabSelected(_)
            | ShellMessage::TrackerProjectStatusChanged(..)
            | ShellMessage::TrackerGateToggled(..)
            | ShellMessage::TrackerReadingToggled(..)
            | ShellMessage::TrackerConfigEdited(_)
            | ShellMessage::TrackerConfigSave
            | ShellMessage::TrackerManualDateChanged(_)
            | ShellMessage::TrackerManualHoursChanged(_)
            | ShellMessage::TrackerManualNotesChanged(_)
            | ShellMessage::TrackerManualAdd
            | ShellMessage::TrackerSessionDelete(_)) => {
                self.tracker.update(Message::Shell(m), &self.state)
            }
            ShellMessage::SplitViewToggle => {
                if self.active_path.is_some() && self.pdf.active_path.is_some() {
                    self.ui.split_view_active = !self.ui.split_view_active;
                    if self.pdf.fit_to_width {
                        return Task::done(Message::Pdf(PdfMessage::FitToWidth));
                    }
                } else {
                    self.ui.toast =
                        Some("Open a markdown file and a PDF to use split view".to_string());
                }
                Task::none()
            }
            ShellMessage::SplitViewDragging(x_pos) => {
                if !self.ui.is_resizing_split {
                    return Task::none();
                }
                let side_width = if self.vault.sidebar_visible {
                    250.0
                } else {
                    0.0
                } + if self.tracker.visible { 300.0 } else { 0.0 }
                    + if self.editor.toc_visible { 250.0 } else { 0.0 };
                let content_width = (self.ui.window_width - side_width).max(480.0);
                let x_min = side_width + 240.0;
                let x_max = side_width + content_width - 240.0;
                let total_width = x_max - x_min;
                if total_width > 1.0 {
                    self.ui.split_ratio = ((x_pos - x_min) / total_width).clamp(0.25, 0.75);
                }
                Task::none()
            }
            ShellMessage::SplitViewDragEnd => {
                self.ui.is_resizing_split = false;
                self.persist_layout();
                if self.pdf.fit_to_width && self.pdf.active_path.is_some() {
                    return Task::done(Message::Pdf(PdfMessage::FitToWidth));
                }
                Task::none()
            }
            ShellMessage::WindowResized(id, width, height) => {
                // Only the shell's geometry is layout input and worth restoring;
                // the handwriting window sizes itself.
                if Some(id) != self.main_window {
                    return Task::none();
                }
                self.ui.window_width = width;
                self.ui.window_height = height;
                self.persist_window_size();
                if self.pdf.fit_to_width && self.pdf.active_path.is_some() {
                    return Task::done(Message::Pdf(PdfMessage::FitToWidth));
                }
                Task::none()
            }
            ShellMessage::WindowOpened(id) => {
                // Pull the real scale factor; a static HiDPI monitor may never
                // emit a `Rescaled` event, so the listener alone isn't enough.
                let scale = iced::window::scale_factor(id)
                    .map(move |factor| Message::Shell(ShellMessage::WindowRescaled(id, factor)));

                if Some(id) == self.ink_window {
                    // The hook attaches to whichever window holds the canvas,
                    // so it has to move here — and it must re-attach each time
                    // the window is reopened, since the handle is new.
                    return Task::batch(vec![
                        scale,
                        iced::window::raw_id::<Message>(id)
                            .map(|value| Message::Ink(InkMessage::PenHookReady(value))),
                    ]);
                }
                scale
            }
            ShellMessage::WindowCloseRequested => {
                // Best-effort flush on the way out; the close is deliberately
                // *not* intercepted. Holding the window open on a failed write
                // would risk an app that cannot be closed at all, which is a
                // worse failure than the few hundred milliseconds of typing
                // that autosave has not yet committed.
                let _ = self.save_active_document();
                self.persist_session();
                self.persist_layout();
                Task::none()
            }
            ShellMessage::WindowClosed(id) => {
                if Some(id) == self.ink_window {
                    self.ink_window_closed();
                    return Task::none();
                }
                if Some(id) == self.main_window {
                    // The shell is the app; close it and everything goes.
                    return iced::exit();
                }
                Task::none()
            }
            ShellMessage::WindowRescaled(id, factor) => {
                // Clamp: below 1 makes no sense, and capping at 3 bounds the
                // bitmap cost on unusually high-DPI displays.
                let factor = factor.clamp(1.0, 3.0);

                // The two windows can sit on monitors with different scaling,
                // so the pen hook tracks the ink window's factor specifically —
                // using the shell's would misplace every stroke.
                if Some(id) == self.ink_window {
                    crate::ink::set_scale_factor(factor);
                    return Task::none();
                }
                if Some(id) != self.main_window {
                    return Task::none();
                }

                if (factor - self.ui.scale_factor).abs() < 0.01 {
                    return Task::none();
                }
                self.ui.scale_factor = factor;
                // Math bitmaps are rasterized at the display scale but cached
                // by TeX string only; flush and re-render at the new density.
                // load_math also records the new scale so results still in
                // flight from before the rescale get dropped on arrival.
                self.editor.math_cache.clear();
                let math_task = self.editor.load_math(factor);
                // Cached page bitmaps were rasterized at the old supersample;
                // drop them and re-render at the new device resolution.
                if self.pdf.active_path.is_some() && self.pdf.total_pages > 0 {
                    self.pdf.pages = vec![None; self.pdf.total_pages as usize];
                    self.pdf.dimensions = vec![None; self.pdf.total_pages as usize];
                    self.pdf.placeholder_page_size = self.pdf.first_page_size();
                    self.pdf.pending_pages.clear();
                    self.pdf.pending_links.clear();
                    self.pdf.render_generation = self.pdf.render_generation.wrapping_add(1);
                    return Task::batch(vec![math_task, self.render_visible_pdf_pages()]);
                }
                math_task
            }
            m @ (ShellMessage::PptxLoaded(..)
            | ShellMessage::PptxScrolled { .. }
            | ShellMessage::PptxZoomIn
            | ShellMessage::PptxZoomOut
            | ShellMessage::PptxZoomFit
            | ShellMessage::PptxStep(_)) => {
                let width = self.pdf_available_width();
                self.pptx.update(Message::Shell(m), width)
            }
        }
    }
}

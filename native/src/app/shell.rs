//! Window lifecycle, split view, panel resize, context menus, drag-and-drop, tracker and presentations.

use super::*;
use crate::design::metrics::{
    SIDE_PANEL_DEFAULT_WIDTH, SIDE_PANEL_MAX_WIDTH, SIDE_PANEL_MIN_WIDTH, SIDEBAR_DEFAULT_WIDTH,
    SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH,
};
use crate::messages::{
    EditorMessage, GraphMessage, InkMessage, PdfMessage, SearchMessage, ShellMessage, VaultMessage,
};
use crate::tabs::TabKind;
use crate::ui_state::ResizingPanel;

impl MdEditor {
    /// Handle a message routed here by [`MdEditor::update_inner`].
    pub(super) fn update_shell(&mut self, message: ShellMessage) -> Task<Message> {
        match message {
            m @ (ShellMessage::CommandPaletteQueryChanged(_)
            | ShellMessage::ShowToast(_)
            | ShellMessage::ToastHide
            | ShellMessage::DismissToast(_)
            | ShellMessage::SplitViewDragStart
            | ShellMessage::SplitViewReset
            | ShellMessage::OpenContextMenu(..)
            | ShellMessage::CloseContextMenu) => self.ui.update(Message::Shell(m)),
            ShellMessage::RetrySave => Task::done(Message::Editor(EditorMessage::Save)),
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
            m @ ShellMessage::TrackerToggle => {
                self.graph.visible = false;
                self.ui.settings_visible = false;
                self.tracker.update(Message::Shell(m), &self.state)
            }
            ShellMessage::CommandPaletteFileClicked(path) => {
                self.ui.command_palette_visible = false;
                self.ui.command_palette_query.clear();
                Task::done(Message::Vault(VaultMessage::SidebarFileClicked(path)))
            }
            ShellMessage::CommandPaletteCommandClicked(shortcut) => {
                self.ui.command_palette_visible = false;
                self.ui.command_palette_query.clear();
                Task::done(Message::KeyboardShortcut(shortcut))
            }
            ShellMessage::OverlaySubmitCurrent => self.on_overlay_submit_current(),
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
            ShellMessage::SplitViewToggle => self.on_split_view_toggle(),
            ShellMessage::SplitViewDragging(x_pos) => {
                self.on_split_view_dragging(x_pos);
                Task::none()
            }
            ShellMessage::SplitViewDragEnd => {
                self.ui.is_resizing_split = false;
                self.settle_layout()
            }
            ShellMessage::SidebarResizeStart => self.start_panel_resize(ResizingPanel::Sidebar),
            ShellMessage::SidePanelResizeStart => self.start_panel_resize(ResizingPanel::SidePanel),
            ShellMessage::PanelResizeDrag(x_pos) => {
                self.on_panel_resize_drag(x_pos);
                Task::none()
            }
            ShellMessage::PanelResizeEnd => {
                self.ui.resizing_panel = None;
                self.settle_layout()
            }
            ShellMessage::ResetSidebarWidth => {
                self.ui.sidebar_width = SIDEBAR_DEFAULT_WIDTH;
                self.settle_layout()
            }
            ShellMessage::ResetSidePanelWidth => {
                self.ui.side_panel_width = SIDE_PANEL_DEFAULT_WIDTH;
                self.settle_layout()
            }
            ShellMessage::ImageZoomIn => self.set_image_zoom(Some(self.image_zoom() * 1.25)),
            ShellMessage::ImageZoomOut => self.set_image_zoom(Some(self.image_zoom() / 1.25)),
            ShellMessage::ImageZoomFit => self.set_image_zoom(None),
            ShellMessage::ImageZoomActual => self.set_image_zoom(Some(1.0)),
            ShellMessage::PptxSlideInputChanged(raw) => {
                self.ui.pptx_slide_input = Some(raw);
                Task::none()
            }
            ShellMessage::PptxSlideInputSubmit => self.on_pptx_slide_input_submit(),
            m @ (ShellMessage::PptxLoaded(..)
            | ShellMessage::PptxScrolled { .. }
            | ShellMessage::PptxZoomIn
            | ShellMessage::PptxZoomOut
            | ShellMessage::PptxZoomFit
            | ShellMessage::PptxStep(_)) => {
                let width = self.pdf_available_width();
                self.pptx.update(Message::Shell(m), width)
            }
            ShellMessage::CursorMoved(x, y) => {
                self.ui.cursor_pos = (x, y);
                Task::none()
            }
            ShellMessage::FileHovered(path) => {
                if !self.ui.hovered_drop_files.contains(&path) {
                    self.ui.hovered_drop_files.push(path);
                }
                Task::none()
            }
            ShellMessage::FilesHoveredLeft => {
                self.ui.hovered_drop_files.clear();
                Task::none()
            }
            ShellMessage::FileDropped(path) => self.on_file_dropped(path),
            ShellMessage::FocusRegionNext => {
                self.cycle_focus_region(true);
                Task::none()
            }
            ShellMessage::FocusRegionPrev => {
                self.cycle_focus_region(false);
                Task::none()
            }
            ShellMessage::WindowResized(id, width, height) => {
                self.on_window_resized(id, width, height)
            }
            ShellMessage::WindowOpened(id) => self.on_window_opened(id),
            ShellMessage::WindowCloseRequested => {
                let _ = self.save_active_document();
                self.persist_session();
                self.persist_layout();
                Task::none()
            }
            ShellMessage::WindowClosed(id) => self.on_window_closed(id),
            ShellMessage::WindowRescaled(id, factor) => self.on_window_rescaled(id, factor),
        }
    }

    /// Enter in an overlay: submit whichever overlay is on top.
    fn on_overlay_submit_current(&mut self) -> Task<Message> {
        if self.ui.command_palette_visible {
            Task::done(Message::Vault(VaultMessage::NameModalSubmitCurrent))
        } else if self.search.visible {
            Task::done(Message::Search(SearchMessage::GlobalSearchSubmit))
        } else if self.graph.visible && self.ui.active_modal.is_none() {
            match self.graph.selected_path.clone() {
                Some(path) => Task::done(Message::Graph(GraphMessage::NodeOpen(path))),
                None => Task::none(),
            }
        } else {
            Task::done(Message::Vault(VaultMessage::NameModalSubmitCurrent))
        }
    }

    /// A drag or reset changed the layout: save it and refit the PDF.
    fn settle_layout(&mut self) -> Task<Message> {
        self.persist_layout();
        self.refit_pdf_if_needed()
    }

    fn start_panel_resize(&mut self, panel: ResizingPanel) -> Task<Message> {
        self.ui.resizing_panel = Some(panel);
        Task::none()
    }

    fn on_panel_resize_drag(&mut self, x_pos: f32) {
        match self.ui.resizing_panel {
            Some(ResizingPanel::Sidebar) => {
                self.ui.sidebar_width = x_pos.clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
            }
            Some(ResizingPanel::SidePanel) => {
                let from_right = (self.ui.window_width - x_pos)
                    .clamp(SIDE_PANEL_MIN_WIDTH, SIDE_PANEL_MAX_WIDTH);
                self.ui.side_panel_width = from_right;
            }
            None => {}
        }
    }

    fn on_split_view_dragging(&mut self, x_pos: f32) {
        if !self.ui.is_resizing_split {
            return;
        }
        let left_chrome = if self.vault.sidebar_visible {
            self.ui.sidebar_width
        } else {
            0.0
        };
        let right_chrome = if self.editor.toc_visible {
            self.ui.side_panel_width
        } else {
            0.0
        } + if self.vault.backlinks_visible {
            self.ui.side_panel_width
        } else {
            0.0
        };
        let content_width = (self.ui.window_width - left_chrome - right_chrome).max(480.0);
        let x_min = left_chrome + 220.0;
        let x_max = left_chrome + content_width - 220.0;
        let total_width = x_max - x_min;
        if total_width > 1.0 {
            self.ui.split_ratio = ((x_pos - x_min) / total_width).clamp(0.25, 0.75);
        }
    }

    /// Image zoom and the presentation viewer.
    /// The image viewer's zoom; `None` (fit) counts as 1.0 for stepping.
    fn image_zoom(&self) -> f32 {
        self.ui.image_zoom.unwrap_or(1.0)
    }

    fn set_image_zoom(&mut self, zoom: Option<f32>) -> Task<Message> {
        self.ui.image_zoom = zoom.map(|z| z.clamp(0.25, 4.0));
        Task::none()
    }

    fn on_pptx_slide_input_submit(&mut self) -> Task<Message> {
        if let Some(raw) = self.ui.pptx_slide_input.take()
            && let Ok(slide_1) = raw.trim().parse::<i32>()
        {
            let width = self.pdf_available_width();
            let cur = self.pptx.current_slide(width) as i32 + 1;
            return self
                .pptx
                .update(Message::Shell(ShellMessage::PptxStep(slide_1 - cur)), width);
        }
        Task::none()
    }

    fn on_window_resized(
        &mut self,
        id: iced::window::Id,
        width: f32,
        height: f32,
    ) -> Task<Message> {
        if Some(id) != self.main_window {
            return Task::none();
        }
        self.ui.window_width = width;
        self.ui.window_height = height;
        self.persist_window_size();
        self.refit_pdf_if_needed()
    }

    fn on_window_opened(&mut self, id: iced::window::Id) -> Task<Message> {
        let scale = iced::window::scale_factor(id)
            .map(move |factor| Message::Shell(ShellMessage::WindowRescaled(id, factor)));
        if Some(id) == self.ink_window {
            return Task::batch(vec![
                scale,
                iced::window::raw_id::<Message>(id)
                    .map(|value| Message::Ink(InkMessage::PenHookReady(value))),
            ]);
        }
        scale
    }

    fn on_window_closed(&mut self, id: iced::window::Id) -> Task<Message> {
        if Some(id) == self.ink_window {
            self.ink_window_closed();
            return Task::none();
        }
        if Some(id) == self.main_window {
            return iced::exit();
        }
        Task::none()
    }
    fn on_split_view_toggle(&mut self) -> Task<Message> {
        if self.ui.split_view_active {
            self.ui.split_view_active = false;
            return self.refit_pdf_if_needed();
        }
        // Pair MRU Note tab + MRU PDF tab when available.
        let mru_note = self.tabs.mru_of_kind(TabKind::Note).cloned();
        let mru_pdf = self.tabs.mru_of_kind(TabKind::Pdf).cloned();
        let mut tasks = Vec::new();
        if self.active_path.is_none()
            && let Some(ref note) = mru_note
        {
            tasks.push(self.open_file(&note.path));
        }
        if self.pdf.active_path.is_none()
            && let Some(ref pdf) = mru_pdf
        {
            tasks.push(self.open_pdf_restoring_view(&pdf.path, Some(pdf.view.clone())));
        }
        if self.active_path.is_some() && self.pdf.active_path.is_some() {
            self.ui.split_view_active = true;
            if self.pdf.fit_to_width {
                tasks.push(Task::done(Message::Pdf(PdfMessage::FitToWidth)));
            }
        } else {
            self.ui.toast = Some("Open a markdown file and a PDF to use split view".to_string());
        }
        Task::batch(tasks)
    }

    fn on_window_rescaled(&mut self, id: iced::window::Id, factor: f32) -> Task<Message> {
        let factor = factor.clamp(1.0, 3.0);
        if Some(id) == self.ink_window {
            crate::ink::set_scale_factor(factor);
            return Task::none();
        }
        if Some(id) != self.main_window || (factor - self.ui.scale_factor).abs() < 0.01 {
            return Task::none();
        }
        self.ui.scale_factor = factor;
        self.editor.math_cache.clear();
        let math_task = self.editor.load_math(factor);
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

    pub(super) fn cycle_focus_region(&mut self, forward: bool) {
        let has_side_panel = self.editor.toc_visible || self.vault.backlinks_visible;
        self.ui.focused_region =
            self.ui
                .focused_region
                .step(forward, self.vault.sidebar_visible, has_side_panel, true);
    }
}

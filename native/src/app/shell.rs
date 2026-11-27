//! Window lifecycle, split view, panel resize, context menus, drag-and-drop, tracker and image zoom.

use super::workspace::SplitAxis;
use super::*;
use crate::design::metrics::{
    SIDE_PANEL_DEFAULT_WIDTH, SIDE_PANEL_MAX_WIDTH, SIDE_PANEL_MIN_WIDTH, SIDEBAR_DEFAULT_WIDTH,
    SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH,
};
use crate::messages::{
    EditorMessage, GraphMessage, InkMessage, SearchMessage, ShellMessage, VaultMessage,
};
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
            ShellMessage::CommandPaletteOpenToSide => self.on_palette_open_to_side(),
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
            ShellMessage::SplitViewToggle => self.toggle_split(),
            ShellMessage::ClosePane(pane) => self.close_pane(pane),
            ShellMessage::WorkspaceMeasured(size) => {
                self.ui.workspace_size = size;
                Task::none()
            }
            ShellMessage::SplitViewDragging(position) => {
                self.on_split_view_dragging(position);
                Task::none()
            }
            ShellMessage::SplitViewDragEnd => {
                self.ui.is_resizing_split = false;
                self.ui.split_drag_origin = None;
                self.persist_layout();
                Task::batch(vec![self.settle_layout(), self.refit_all_pdfs()])
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
            ShellMessage::CursorMoved(x, y) => {
                self.ui.cursor_pos = (x, y);
                Task::none()
            }
            ShellMessage::FileHovered(id, _) if Some(id) == self.ink_window => Task::none(),
            ShellMessage::FileHovered(_, path) => {
                if !self.ui.hovered_drop_files.contains(&path) {
                    self.ui.hovered_drop_files.push(path);
                }
                Task::none()
            }
            ShellMessage::FilesHoveredLeft => {
                self.ui.hovered_drop_files.clear();
                Task::none()
            }
            ShellMessage::FileDropped(id, _) if Some(id) == self.ink_window => {
                self.ui.toast = Some("Drop files on the main window to add them".to_string());
                Task::none()
            }
            ShellMessage::FileDropped(_, path) => self.on_file_dropped(path),
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
            ShellMessage::WindowCloseRequested(id) if Some(id) == self.ink_window => {
                self.flush_ink_page()
            }
            ShellMessage::WindowCloseRequested(_) => {
                let _ = self.save_all_panes();
                let ink = self.flush_ink_page();
                self.persist_session();
                self.persist_layout();
                ink
            }
            ShellMessage::WindowClosed(id) => self.on_window_closed(id),
            ShellMessage::WindowRescaled(id, factor) => self.on_window_rescaled(id, factor),
        }
    }

    /// `Ctrl+Enter` in the command palette: open the highlighted file to the side.
    fn on_palette_open_to_side(&mut self) -> Task<Message> {
        if !self.ui.command_palette_visible {
            return Task::none();
        }
        let action = views::command_palette::action_at(
            &self.ui.command_palette_query,
            &self.ui.commands,
            &self.vault.entries,
            self.desks.list(),
            self.ui.palette_selected,
        );
        if let Some(views::command_palette::PaletteAction::OpenFile(path)) = action {
            self.ui.command_palette_visible = false;
            self.ui.command_palette_query.clear();
            return Task::done(Message::Vault(VaultMessage::OpenToSide(path)));
        }
        Task::none()
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

    /// The divider follows the pointer: the ratio moves by the pointer's
    /// travel along the split, as a share of the measured panes area.
    fn on_split_view_dragging(&mut self, position: iced::Point) {
        if !self.ui.is_resizing_split {
            return;
        }
        let along = |p: iced::Point| match self.workspace.axis {
            SplitAxis::Vertical => p.x,
            SplitAxis::Horizontal => p.y,
        };
        let length = match self.workspace.axis {
            SplitAxis::Vertical => self.ui.workspace_size.width,
            SplitAxis::Horizontal => self.ui.workspace_size.height,
        };
        let (start, ratio) = *self
            .ui
            .split_drag_origin
            .get_or_insert((along(position), self.ui.split_ratio));
        if length > 1.0 {
            self.ui.split_ratio = (ratio + (along(position) - start) / length).clamp(0.2, 0.8);
        }
    }

    /// Image zoom.
    /// The image viewer's zoom; `None` (fit) counts as 1.0 for stepping.
    fn image_zoom(&self) -> f32 {
        self.ui.image_zoom.unwrap_or(1.0)
    }

    fn set_image_zoom(&mut self, zoom: Option<f32>) -> Task<Message> {
        self.ui.image_zoom = zoom.map(|z| z.clamp(0.25, 4.0));
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
            return self.ink_window_closed();
        }
        if Some(id) == self.main_window {
            // The ink window may still hold unsaved strokes; the write is
            // synchronous, so it lands before the exit.
            let _ = self.flush_ink_page();
            return iced::exit();
        }
        Task::none()
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
        self.for_each_pane(|app| {
            app.editor.forget_math();
            let math_task = app.editor.load_math(factor);
            if app.pdf.active_path.is_some() && app.pdf.total_pages > 0 {
                app.pdf.pages = vec![None; app.pdf.total_pages as usize];
                app.pdf.dimensions = vec![None; app.pdf.total_pages as usize];
                app.pdf.placeholder_page_size = app.pdf.first_page_size();
                app.pdf.pending_pages.clear();
                app.pdf.pending_links.clear();
                app.pdf.render_generation = app.pdf.render_generation.wrapping_add(1);
                return Task::batch(vec![math_task, app.render_visible_pdf_pages()]);
            }
            math_task
        })
    }

    pub(super) fn cycle_focus_region(&mut self, forward: bool) {
        let has_side_panel = self.editor.toc_visible || self.vault.backlinks_visible;
        self.ui.focused_region =
            self.ui
                .focused_region
                .step(forward, self.vault.sidebar_visible, has_side_panel, true);
    }
}

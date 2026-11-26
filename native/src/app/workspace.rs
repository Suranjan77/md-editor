//! The two-pane workspace: each pane's documents, focus, and routing messages to the pane they belong to.
//!
//! The pane whose state is *swapped in* keeps it in `MdEditor`'s own fields
//! (`tabs`, `active_path`, `editor`, `pdf`, …), so every handler works on it
//! unchanged. The other pane's state waits in [`Workspace::other`]. See
//! `docs/split-layouts-plan.md`.

use std::cell::Cell;

use super::*;
use crate::messages::PdfMessage;

/// One of the two panes. `A` always exists; `B` exists while the workspace is split.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PaneId {
    A,
    B,
}

impl PaneId {
    pub fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }

    fn tag(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
        }
    }

    pub fn editor_widget_id(self) -> iced::advanced::widget::Id {
        iced::advanced::widget::Id::from(format!("md_editor_note_{}", self.tag()))
    }

    pub fn editor_scrollable_id(self) -> iced::advanced::widget::Id {
        iced::advanced::widget::Id::from(format!("editor_scrollable_{}", self.tag()))
    }

    pub fn pdf_scrollable_id(self) -> iced::advanced::widget::Id {
        iced::advanced::widget::Id::from(format!("pdf_scrollable_{}", self.tag()))
    }
}

/// How the two panes share the main area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SplitAxis {
    /// Side by side.
    #[default]
    Vertical,
    /// One above the other.
    Horizontal,
}

thread_local! {
    /// The pane swapped in on this (the UI) thread: the widget ids tasks built
    /// by handlers target.
    static CURRENT: Cell<PaneId> = const { Cell::new(PaneId::A) };
}

/// The pane whose state handlers are working on right now.
pub fn current_pane() -> PaneId {
    CURRENT.with(Cell::get)
}

pub(super) fn set_current(pane: PaneId) {
    CURRENT.with(|c| c.set(pane));
}

/// A pane's documents: everything that differs between the two panes.
pub struct PaneDocs {
    pub tabs: crate::tabs::Tabs,
    pub active_path: Option<String>,
    pub editor: crate::editor_state::EditorPane,
    pub pdf: crate::pdf_pane::PdfPane,
    pub active_image_path: Option<String>,
    pub active_image: Option<(iced::widget::image::Handle, f32, f32)>,
    pub showing_pdf: bool,
    pub image_zoom: Option<f32>,
}

impl PaneDocs {
    /// An empty pane. Its PDF generations start far from pane A's, so a
    /// result meant for one can never pass the other's generation check.
    pub fn new(id: PaneId) -> Self {
        let mut pdf = crate::pdf_pane::PdfPane::new();
        if id == PaneId::B {
            pdf.render_generation = 1 << 40;
            pdf.document_generation = 1 << 40;
        }
        Self {
            tabs: crate::tabs::Tabs::new(),
            active_path: None,
            editor: crate::editor_state::EditorPane::new(),
            pdf,
            active_image_path: None,
            active_image: None,
            showing_pdf: false,
            image_zoom: None,
        }
    }
}

pub struct Workspace {
    /// The pane not swapped in, while the workspace is split.
    pub other: Option<PaneDocs>,
    /// The pane swapped in.
    pub current: PaneId,
    /// The pane with focus: keyboard, side panels, status bar.
    pub focused: PaneId,
    pub axis: SplitAxis,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            other: None,
            current: PaneId::A,
            focused: PaneId::A,
            axis: SplitAxis::default(),
        }
    }

    pub fn is_split(&self) -> bool {
        self.other.is_some()
    }

    /// The panes that exist, A first.
    pub fn panes(&self) -> Vec<PaneId> {
        if self.is_split() {
            vec![PaneId::A, PaneId::B]
        } else {
            vec![PaneId::A]
        }
    }
}

/// Messages a pane's view sends that do not mean the reader turned to it:
/// they run in that pane without moving focus.
fn is_passive(message: &Message) -> bool {
    use crate::messages::EditorMessage;
    matches!(
        message,
        Message::Editor(EditorMessage::Scrolled { .. } | EditorMessage::CaretView(_))
            | Message::Pdf(PdfMessage::Scrolled { .. } | PdfMessage::ViewportMeasured(_))
    )
}

impl MdEditor {
    /// Exchange the swapped-in pane's state with the other pane's.
    fn swap_docs(&mut self) {
        let Some(other) = self.workspace.other.as_mut() else {
            return;
        };
        std::mem::swap(&mut self.tabs, &mut other.tabs);
        std::mem::swap(&mut self.active_path, &mut other.active_path);
        std::mem::swap(&mut self.editor, &mut other.editor);
        // The outline panel is the window's, not a pane's.
        std::mem::swap(&mut self.editor.toc_visible, &mut other.editor.toc_visible);
        std::mem::swap(&mut self.pdf, &mut other.pdf);
        std::mem::swap(&mut self.active_image_path, &mut other.active_image_path);
        std::mem::swap(&mut self.active_image, &mut other.active_image);
        std::mem::swap(&mut self.showing_pdf, &mut other.showing_pdf);
        std::mem::swap(&mut self.ui.image_zoom, &mut other.image_zoom);
        self.workspace.current = self.workspace.current.other();
        set_current(self.workspace.current);
    }

    /// Swap `pane`'s state in (a no-op if it already is, or does not exist).
    pub(super) fn swap_in(&mut self, pane: PaneId) {
        if self.workspace.current != pane && self.workspace.is_split() {
            self.swap_docs();
        }
    }

    /// The other pane's documents, read-only (`None` unless split).
    pub(super) fn parked(&self) -> Option<&PaneDocs> {
        self.workspace.other.as_ref()
    }

    /// Run `f` with `pane` swapped in, then swap the focused pane back.
    pub(super) fn with_pane<T>(&mut self, pane: PaneId, f: impl FnOnce(&mut Self) -> T) -> T {
        let back = self.workspace.current;
        self.swap_in(pane);
        let result = f(self);
        self.swap_in(back);
        result
    }

    /// Handle `message` in `pane` and route the tasks it starts back there.
    pub(super) fn update_in_pane(&mut self, pane: PaneId, message: Message) -> Task<Message> {
        // A pane closed since: run in the focused one, so global results
        // (indexing, toasts) are not lost.
        let pane = if pane == PaneId::B && !self.workspace.is_split() {
            PaneId::A
        } else {
            pane
        };
        let task = self.with_pane(pane, |app| app.update_inner(message));
        route_to(pane, task)
    }

    /// A message from `pane`'s view: an interaction focuses the pane first.
    pub(super) fn update_from_pane(&mut self, pane: PaneId, message: Message) -> Task<Message> {
        if !is_passive(&message) && pane != self.workspace.focused {
            let focus = self.focus_pane(pane);
            let task = self.update_in_pane(pane, message);
            return Task::batch(vec![focus, task]);
        }
        self.update_in_pane(pane, message)
    }

    /// Give `pane` the focus: the note the old pane was editing is saved
    /// first, and find (tied to one document) closes.
    pub(super) fn focus_pane(&mut self, pane: PaneId) -> Task<Message> {
        if pane == self.workspace.focused || (pane == PaneId::B && !self.workspace.is_split()) {
            return Task::none();
        }
        if let SaveOutcome::Failed(err) = self.save_active_document() {
            self.ui.toast = Some(format!("Could not save before switching panes: {err}"));
        }
        self.search.file_visible = false;
        self.search.pdf_results.clear();
        self.search.pdf_indices_by_page.clear();
        self.search.match_index = None;
        self.search.pdf_search_seq = self.search.pdf_search_seq.wrapping_add(1);
        self.ui.pdf_page_input = None;
        self.ui.pdf_zoom_input = None;
        self.motion.editor_scroll = None;
        self.swap_in(pane);
        self.workspace.focused = pane;
        self.sync_focused_pane_metadata()
    }

    /// Refresh panel/sidebar/backlinks/graph state and editor focus for `self.workspace.focused`.
    pub(super) fn sync_focused_pane_metadata(&mut self) -> Task<Message> {
        self.active_panel = if self.showing_pdf && self.pdf.active_path.is_some() {
            ActivePanel::Pdf
        } else {
            ActivePanel::Markdown
        };
        let path = self
            .active_path
            .clone()
            .filter(|_| !self.showing_pdf)
            .or_else(|| self.pdf.active_path.clone())
            .or_else(|| self.active_image_path.clone());
        if let Some(path) = path {
            self.vault.selected_path = Some(path.clone());
            self.vault.backlinks =
                md_editor_core::vault::get_mixed_backlinks(&self.state, &path).unwrap_or_default();
            self.graph.set_active_path(Some(path));
        } else {
            self.vault.backlinks.clear();
            self.graph.set_active_path(None);
        }
        if self.active_panel == ActivePanel::Markdown && self.active_path.is_some() {
            iced::widget::operation::focus(self.workspace.focused.editor_widget_id())
        } else {
            Task::none()
        }
    }

    /// Run `f` once in every pane, focused pane last (so it ends swapped in).
    pub(super) fn for_each_pane(
        &mut self,
        mut f: impl FnMut(&mut Self) -> Task<Message>,
    ) -> Task<Message> {
        let focused = self.workspace.focused;
        let mut tasks = Vec::new();
        for pane in self.workspace.panes() {
            if pane != focused {
                let task = self.with_pane(pane, &mut f);
                tasks.push(route_to(pane, task));
            }
        }
        self.swap_in(focused);
        tasks.push(f(self));
        Task::batch(tasks)
    }
}

/// `task`'s messages, delivered to `pane`.
pub(super) fn route_to(pane: PaneId, task: Task<Message>) -> Task<Message> {
    task.map(move |message| address(pane, message))
}

/// `message` addressed to `pane`, unless it already names one.
fn address(pane: PaneId, message: Message) -> Message {
    match message {
        Message::ToPane(..) | Message::FromPane(..) => message,
        other => Message::ToPane(pane, Box::new(other)),
    }
}

#[cfg(test)]
mod tests;

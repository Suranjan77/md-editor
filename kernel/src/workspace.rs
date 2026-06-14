//! Workspace façade: ties DocumentStore + PaneTree + FocusModel + overlay
//! state together and derives the input scope stack from focus. This is the
//! type the shell holds; it upholds the kernel invariants:
//!
//! 1. Exactly one focused editor whenever any tab is open.
//! 2. Opening a document never changes the pane layout (BUG-C fix).
//! 3. The scope stack is *derived* from focus — never stored, never synced
//!    (BUG-A fix: the v2 bug was five hand-synced flags deciding who owns a
//!    keystroke).
//! 4. Documents are garbage-collected when their last tab closes.

use crate::command::CommandId;
use crate::focus::FocusModel;
use crate::input::{Chord, EditorKind, Keymap, Scope};
use crate::pane::{DocumentStore, PaneError, PaneId, PaneTree, SplitAxis, TabId};

#[derive(Debug, Default)]
pub struct Workspace {
    pub docs: DocumentStore,
    pub panes: PaneTree,
    focus: FocusModel,
    /// Open modal overlay (palette, go-to-page, dialog), if any. A scope
    /// fence: see [`Scope::Overlay`].
    overlay: Option<&'static str>,
}

impl Workspace {
    pub fn new() -> Workspace {
        Workspace::default()
    }

    /// Open `path` in the focused pane (or the first pane when nothing is
    /// focused yet) and focus it. Works identically for every document kind —
    /// a PDF opens standalone exactly like a markdown note (BUG-C).
    pub fn open(&mut self, path: &str, kind: EditorKind) -> Result<TabId, PaneError> {
        let pane = self
            .focused_pane()
            .unwrap_or_else(|| self.panes.first_pane());
        self.open_in(pane, path, kind)
    }

    /// Open `path` in a specific pane and focus it.
    pub fn open_in(
        &mut self,
        pane: PaneId,
        path: &str,
        kind: EditorKind,
    ) -> Result<TabId, PaneError> {
        let doc = self.docs.open(path, kind);
        let tab = self.panes.open_tab(pane, doc, kind)?;
        self.focus.focus(tab);
        Ok(tab)
    }

    /// Split the focused pane and open `path` in the new sibling — the
    /// *explicit* way to get a side-by-side layout.
    pub fn open_in_new_split(
        &mut self,
        path: &str,
        kind: EditorKind,
        axis: SplitAxis,
    ) -> Result<TabId, PaneError> {
        let pane = self
            .focused_pane()
            .unwrap_or_else(|| self.panes.first_pane());
        let new_pane = self.panes.split(pane, axis)?;
        self.open_in(new_pane, path, kind)
    }

    /// Close a tab; focus moves to the owning pane's next active tab, or the
    /// first pane's active tab if the pane collapsed. Unreferenced documents
    /// are garbage-collected.
    pub fn close_tab(&mut self, tab: TabId) -> Result<(), PaneError> {
        let closed = self.panes.close_tab(tab)?;
        if !self.panes.references_document(closed.tab.document) {
            self.docs.close(closed.tab.document);
        }
        if self.focus.focused() == Some(tab) {
            let next = if closed.pane_removed {
                self.panes
                    .pane(self.panes.first_pane())
                    .and_then(|p| p.active_tab())
                    .map(|t| t.id)
            } else {
                self.panes
                    .pane(closed.pane)
                    .and_then(|p| p.active_tab())
                    .map(|t| t.id)
            };
            match next {
                Some(t) => self.focus.focus(t),
                None => self.focus.clear(),
            }
        }
        Ok(())
    }

    pub fn focus_tab(&mut self, tab: TabId) -> Result<(), PaneError> {
        let (pane, _) = self.panes.find_tab(tab).ok_or(PaneError::NoSuchTab(tab))?;
        let pane_id = pane.id;
        self.panes.set_active_tab(pane_id, tab)?;
        self.focus.focus(tab);
        Ok(())
    }

    pub fn focused_tab(&self) -> Option<TabId> {
        self.focus.focused()
    }

    pub fn focused_pane(&self) -> Option<PaneId> {
        let tab = self.focus.focused()?;
        self.panes.find_tab(tab).map(|(p, _)| p.id)
    }

    pub fn focused_editor_kind(&self) -> Option<EditorKind> {
        let tab = self.focus.focused()?;
        self.panes.find_tab(tab).map(|(_, t)| t.editor)
    }

    pub fn open_overlay(&mut self, name: &'static str) {
        self.overlay = Some(name);
    }

    pub fn close_overlay(&mut self) {
        self.overlay = None;
    }

    pub fn overlay(&self) -> Option<&'static str> {
        self.overlay
    }

    /// The active input scope stack, outermost first — *derived* from focus
    /// and overlay state on every call.
    pub fn scope_stack(&self) -> Vec<Scope> {
        let mut stack = vec![Scope::Global, Scope::Workspace];
        if self.focus.focused().is_some() {
            stack.push(Scope::Pane);
            if let Some(kind) = self.focused_editor_kind() {
                stack.push(Scope::Editor(kind));
            }
        }
        if self.overlay.is_some() {
            stack.push(Scope::Overlay);
        }
        stack
    }

    /// The one keystroke entry point: resolve a chord against the current
    /// scope stack. `None` means "not a command — deliver as raw text input
    /// to the focused widget".
    pub fn handle_key(&self, keymap: &Keymap, chord: Chord) -> Option<CommandId> {
        keymap.resolve(&self.scope_stack(), chord)
    }
}

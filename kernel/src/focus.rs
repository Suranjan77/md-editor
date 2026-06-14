//! FocusModel (plan §3.1): exactly one focused editor at any instant. All
//! input flows through it first; panes render focus visibly.
//!
//! The model itself is small on purpose — the invariant ("focused tab exists
//! whenever any tab exists") is maintained by [`crate::workspace::Workspace`],
//! which is the only writer.

use crate::pane::TabId;

#[derive(Debug, Default)]
pub struct FocusModel {
    focused: Option<TabId>,
}

impl FocusModel {
    pub fn new() -> FocusModel {
        FocusModel::default()
    }

    /// The focused editor's tab, if any tab is open at all.
    pub fn focused(&self) -> Option<TabId> {
        self.focused
    }

    pub fn focus(&mut self, tab: TabId) {
        self.focused = Some(tab);
    }

    pub fn clear(&mut self) {
        self.focused = None;
    }
}

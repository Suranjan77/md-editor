//! Session snapshot wire format (plan §5 M2 "session restore"): the shape
//! the shell serializes into the vault's `SessionStore`. Paths are
//! vault-relative, so a vault move restores cleanly; every field the shell
//! might not know on an older/newer snapshot is `#[serde(default)]` —
//! restore degrades, it never refuses.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub layout: NodeSnapshot,
    /// View state per vault-relative path (BTreeMap: stable JSON output).
    #[serde(default)]
    pub views: BTreeMap<String, ViewSnapshot>,
    #[serde(default)]
    pub tree_open: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tree_expanded: Vec<String>,
    #[serde(default)]
    pub tracker_open: bool,
    #[serde(default)]
    pub tracker_active_tab: Option<String>,
}

/// Mirror of the kernel's `Layout` tree in serializable form.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum NodeSnapshot {
    Pane {
        tabs: Vec<TabSnapshot>,
        #[serde(default)]
        active: usize,
    },
    Split {
        /// `true` stacks the children (kernel `SplitAxis::Vertical`).
        vertical: bool,
        ratio: f32,
        first: Box<NodeSnapshot>,
        second: Box<NodeSnapshot>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabSnapshot {
    pub path: String,
    /// The workspace-wide focused tab (at most one across the whole tree).
    #[serde(default)]
    pub focused: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ViewSnapshot {
    #[serde(default)]
    pub scroll: f32,
    /// PDF zoom factor (1.0 = 100%).
    #[serde(default)]
    pub zoom: Option<f32>,
    /// Markdown caret as (line, col).
    #[serde(default)]
    pub caret: Option<(usize, usize)>,
}

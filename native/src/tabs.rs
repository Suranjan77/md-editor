//! Document tabs state: strip order, active tab, MRU order, closed-tab reopen stack, duplicate-name disambiguation, and session persistence.

use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

pub type TabId = u64;

const MAX_REOPEN_STACK: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabKind {
    Note,
    Pdf,
    Image,
}

impl TabKind {
    pub fn from_path(path: &str) -> Option<Self> {
        if crate::ink::is_sketch_path(path) {
            return None;
        }
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".md") || lower.ends_with(".markdown") {
            Some(Self::Note)
        } else if lower.ends_with(".pdf") {
            Some(Self::Pdf)
        } else if lower.ends_with(".png")
            || lower.ends_with(".jpg")
            || lower.ends_with(".jpeg")
            || lower.ends_with(".gif")
            || lower.ends_with(".bmp")
            || lower.ends_with(".webp")
            || lower.ends_with(".svg")
        {
            Some(Self::Image)
        } else {
            None
        }
    }
}

/// Per-tab view state restored when switching back to a tab (`feature-plan-tabs-and-images.md` §1.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TabView {
    /// Buffer, scroll and caret live in `EditorPane::retained`.
    Note,
    Pdf {
        page: u16,
        zoom: f32,
        fit_to_width: bool,
        /// See `PdfPane::fit_fills_pane`; absent in sessions saved before it.
        #[serde(default)]
        fit_fills_pane: bool,
        scroll_y: f32,
    },
    Image {
        zoom: f32,
        fit: bool,
    },
}

impl TabView {
    pub fn default_for(kind: TabKind) -> Self {
        match kind {
            TabKind::Note => Self::Note,
            TabKind::Pdf => Self::Pdf {
                page: 0,
                zoom: 1.5,
                fit_to_width: true,
                fit_fills_pane: false,
                scroll_y: 0.0,
            },
            TabKind::Image => Self::Image {
                zoom: 1.0,
                fit: true,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tab {
    pub id: TabId,
    pub kind: TabKind,
    pub path: String,
    pub view: TabView,
    /// Set when an external delete removed the backing file on disk.
    pub missing: bool,
}

impl Tab {
    pub fn basename(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    pub fn display_stem(&self) -> &str {
        let base = self.basename();
        if self.kind == TabKind::Note {
            base.strip_suffix(".md")
                .or_else(|| base.strip_suffix(".markdown"))
                .unwrap_or(base)
        } else {
            base
        }
    }

    pub fn parent_folder(&self) -> Option<&str> {
        self.path
            .rsplit_once('/')
            .map(|(dir, _)| dir)
            .filter(|dir| !dir.is_empty())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PersistedTabs {
    tabs: Vec<PersistedTab>,
    active_path: Option<String>,
    recent_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedTab {
    path: String,
    /// `None` when a saved view no longer parses (e.g. a removed tab kind), so one stale
    /// entry drops that tab instead of the whole session.
    #[serde(deserialize_with = "lenient_view")]
    view: Option<TabView>,
}

fn lenient_view<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<TabView>, D::Error> {
    let raw = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(raw).ok())
}

#[derive(Debug, Clone)]
pub struct Tabs {
    tabs: Vec<Tab>,
    active: Option<TabId>,
    /// Most-recently-used order (most recent last).
    recent: Vec<TabId>,
    /// Closed tabs for `Ctrl+Shift+T`, capped at 10.
    closed: Vec<Tab>,
    next_id: TabId,
    /// Tab currently being dragged for strip reordering.
    pub dragging: Option<TabId>,
    /// Whether the overflow `▾` menu listing all tabs is open.
    pub overflow_open: bool,
}

impl Default for Tabs {
    fn default() -> Self {
        Self::new()
    }
}

impl Tabs {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: None,
            recent: Vec::new(),
            closed: Vec::new(),
            next_id: 1,
            dragging: None,
            overflow_open: false,
        }
    }

    pub fn clear(&mut self) {
        self.tabs.clear();
        self.active = None;
        self.recent.clear();
        self.closed.clear();
        self.dragging = None;
        self.overflow_open = false;
    }

    pub fn list(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn active_id(&self) -> Option<TabId> {
        self.active
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        let id = self.active?;
        self.tabs.iter().find(|t| t.id == id)
    }

    pub fn get(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn find_by_path(&self, path: &str) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.path == path)
    }

    /// Most recently active tab of `kind`, used by split view (left = MRU Note, right = MRU Pdf).
    pub fn mru_of_kind(&self, kind: TabKind) -> Option<&Tab> {
        for &id in self.recent.iter().rev() {
            if let Some(tab) = self.tabs.iter().find(|t| t.id == id && t.kind == kind) {
                return Some(tab);
            }
        }
        self.tabs.iter().find(|t| t.kind == kind)
    }

    /// Disambiguated label for `tab`: appends ` — folder/` when two open tabs
    /// share the same display stem (`Index — notes/`).
    pub fn label_for(&self, tab: &Tab) -> String {
        let stem = tab.display_stem();
        let duplicate = self
            .tabs
            .iter()
            .filter(|other| other.display_stem() == stem)
            .count()
            > 1;
        if duplicate {
            match tab.parent_folder() {
                Some(folder) => format!("{stem} — {folder}/"),
                None => format!("{stem} — /"),
            }
        } else {
            stem.to_string()
        }
    }

    /// Open `path` (or focus it if already open). When `background` is false,
    /// activates the tab and updates MRU; when true, opens to the right of the
    /// active tab without changing focus.
    pub fn open(&mut self, path: &str, kind: TabKind, background: bool) -> TabId {
        if let Some(existing) = self.tabs.iter_mut().find(|t| t.path == path) {
            existing.missing = false;
            let id = existing.id;
            if !background {
                self.activate(id);
            }
            return id;
        }

        let id = self.alloc_id();
        let tab = Tab {
            id,
            kind,
            path: path.to_string(),
            view: TabView::default_for(kind),
            missing: false,
        };

        let insert_idx = self
            .active
            .and_then(|act| self.tabs.iter().position(|t| t.id == act))
            .map_or(self.tabs.len(), |pos| pos + 1);
        self.tabs.insert(insert_idx, tab);
        self.closed.retain(|c| c.path != path);

        if background && self.active.is_some() {
            if !self.recent.contains(&id) {
                self.recent.insert(0, id);
            }
        } else {
            self.activate(id);
        }
        id
    }

    pub fn activate(&mut self, id: TabId) -> bool {
        if !self.tabs.iter().any(|t| t.id == id) {
            return false;
        }
        self.active = Some(id);
        self.recent.retain(|&r| r != id);
        self.recent.push(id);
        true
    }

    /// Close `id`, recording it on the reopen stack and activating the most
    /// recently visited remaining tab (or an adjacent neighbour).
    pub fn close(&mut self, id: TabId) -> Option<Tab> {
        let pos = self.tabs.iter().position(|t| t.id == id)?;
        let removed = self.tabs.remove(pos);
        self.recent.retain(|&r| r != id);
        if self.dragging == Some(id) {
            self.dragging = None;
        }

        if !removed.missing {
            self.closed.retain(|t| t.path != removed.path);
            self.closed.push(removed.clone());
            if self.closed.len() > MAX_REOPEN_STACK {
                self.closed.remove(0);
            }
        }

        if self.active == Some(id) {
            self.active = self.recent.last().copied().or_else(|| {
                if self.tabs.is_empty() {
                    None
                } else {
                    Some(self.tabs[pos.min(self.tabs.len() - 1)].id)
                }
            });
        }
        Some(removed)
    }

    /// Close every tab except `keep_id`. Returns the closed tabs in strip order.
    pub fn close_others(&mut self, keep_id: TabId) -> Vec<Tab> {
        let ids: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|t| t.id != keep_id)
            .map(|t| t.id)
            .collect();
        let mut closed = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(tab) = self.close(id) {
                closed.push(tab);
            }
        }
        self.activate(keep_id);
        closed
    }

    /// Pop and reopen the most recently closed tab, activating it.
    pub fn reopen_closed(&mut self) -> Option<TabId> {
        let mut tab = self.closed.pop()?;
        if let Some(existing) = self.tabs.iter().find(|t| t.path == tab.path) {
            let id = existing.id;
            self.activate(id);
            return Some(id);
        }
        let id = self.alloc_id();
        tab.id = id;
        tab.missing = false;
        let insert_idx = self
            .active
            .and_then(|act| self.tabs.iter().position(|t| t.id == act))
            .map_or(self.tabs.len(), |pos| pos + 1);
        self.tabs.insert(insert_idx, tab);
        self.activate(id);
        Some(id)
    }

    /// Like [`Self::reopen_closed`], but first drops closed tabs whose file no
    /// longer `exists`, so `Ctrl+Shift+T` never reopens a deleted file.
    pub fn reopen_last(&mut self, exists: impl Fn(&str) -> bool) -> Option<TabId> {
        while self.closed.last().is_some_and(|tab| !exists(&tab.path)) {
            self.closed.pop();
        }
        self.reopen_closed()
    }

    /// Remember the viewer state of the tab showing `path` (no-op if none is open).
    pub fn save_view_by_path(&mut self, path: &str, view: TabView) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.path == path) {
            tab.view = view;
        }
    }

    /// Cycle in strip order (`delta = 1` for `Ctrl+PageDown`, `-1` for `Ctrl+PageUp`).
    pub fn step_strip(&mut self, delta: isize) -> Option<TabId> {
        if self.tabs.is_empty() {
            return None;
        }
        let cur = self
            .active
            .and_then(|id| self.tabs.iter().position(|t| t.id == id))
            .unwrap_or(0);
        let len = self.tabs.len() as isize;
        let next = (cur as isize + delta).rem_euclid(len) as usize;
        let id = self.tabs[next].id;
        self.activate(id);
        Some(id)
    }

    /// Cycle in MRU order (`forward = true` for `Ctrl+Tab` -> previous recent).
    pub fn step_mru(&mut self, forward: bool) -> Option<TabId> {
        if self.tabs.len() <= 1 {
            return self.active;
        }
        let target = if forward {
            // `recent` has active at the end; pick the second-most-recent and rotate.
            self.recent
                .get(self.recent.len().saturating_sub(2))
                .copied()
                .or_else(|| self.tabs.first().map(|t| t.id))?
        } else {
            self.recent
                .first()
                .copied()
                .or_else(|| self.tabs.last().map(|t| t.id))?
        };
        if forward && self.recent.len() >= 2 {
            // Rotate the old active to the front so repeated Ctrl+Tab walks the whole MRU ring.
            if let Some(prev_active) = self.active {
                self.recent.retain(|&id| id != prev_active && id != target);
                self.recent.insert(0, prev_active);
                self.recent.push(target);
                self.active = Some(target);
                return Some(target);
            }
        }
        self.activate(target);
        Some(target)
    }

    /// Select by 1-based shortcut `1..=9` (`9` always selects the last tab, per §1.2).
    pub fn select_ordinal(&mut self, n: usize) -> Option<TabId> {
        if self.tabs.is_empty() || n == 0 {
            return None;
        }
        let idx = if n >= 9 {
            self.tabs.len() - 1
        } else {
            n.checked_sub(1).filter(|&i| i < self.tabs.len())?
        };
        let id = self.tabs[idx].id;
        self.activate(id);
        Some(id)
    }

    /// Move `id` to `to_index` in the strip (drag reorder or `Ctrl+Shift+PageUp/Down`).
    pub fn move_tab(&mut self, id: TabId, to_index: usize) -> bool {
        let Some(from) = self.tabs.iter().position(|t| t.id == id) else {
            return false;
        };
        let clamped = to_index.min(self.tabs.len().saturating_sub(1));
        if from == clamped {
            return false;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(clamped, tab);
        true
    }

    pub fn move_active_by(&mut self, delta: isize) -> bool {
        let Some(active) = self.active else {
            return false;
        };
        let Some(pos) = self.tabs.iter().position(|t| t.id == active) else {
            return false;
        };
        let target = (pos as isize + delta).clamp(0, self.tabs.len() as isize - 1) as usize;
        self.move_tab(active, target)
    }

    /// Update tab paths on file or folder rename (`old_path` -> `new_path`).
    pub fn rename_path(&mut self, old_path: &str, new_path: &str) {
        for tab in &mut self.tabs {
            if rewrite_rel_path(&mut tab.path, old_path, new_path)
                && let Some(kind) = TabKind::from_path(&tab.path)
            {
                tab.kind = kind;
            }
        }
        for tab in &mut self.closed {
            let _ = rewrite_rel_path(&mut tab.path, old_path, new_path);
        }
    }

    /// Close tabs matching a user-deleted file or folder.
    pub fn remove_deleted_path(&mut self, deleted_path: &str) {
        let ids: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|t| path_matches_or_descendant(&t.path, deleted_path))
            .map(|t| t.id)
            .collect();
        self.closed
            .retain(|t| !path_matches_or_descendant(&t.path, deleted_path));
        for id in ids {
            let _ = self.close(id);
        }
        self.closed
            .retain(|t| !path_matches_or_descendant(&t.path, deleted_path));
    }

    /// Count valid tabs serialized inside a `Tabs::to_json()` snapshot.
    pub fn json_tab_count(raw: &str) -> usize {
        serde_json::from_str::<PersistedTabs>(raw)
            .map(|persisted| {
                persisted
                    .tabs
                    .iter()
                    .filter(|t| TabKind::from_path(&t.path).is_some() && t.view.is_some())
                    .count()
            })
            .unwrap_or(0)
    }

    /// Rewrite `old_path` (or descendants under it) inside a serialized tab snapshot.
    /// Returns `Some(updated_json)` only if any path changed.
    pub fn rewrite_json_paths(raw: &str, old_path: &str, new_path: &str) -> Option<String> {
        let mut persisted = serde_json::from_str::<PersistedTabs>(raw).ok()?;
        let mut changed = false;
        for tab in &mut persisted.tabs {
            changed |= rewrite_rel_path(&mut tab.path, old_path, new_path);
        }
        if let Some(ref mut active) = persisted.active_path {
            changed |= rewrite_rel_path(active, old_path, new_path);
        }
        for path in &mut persisted.recent_paths {
            changed |= rewrite_rel_path(path, old_path, new_path);
        }
        changed.then(|| serde_json::to_string(&persisted).ok())?
    }

    /// Remove `deleted_path` (or descendants under it) from a serialized tab snapshot.
    /// Returns `Some(updated_json)` only if anything was removed.
    pub fn remove_path_from_json(raw: &str, deleted_path: &str) -> Option<String> {
        let mut persisted = serde_json::from_str::<PersistedTabs>(raw).ok()?;
        let prev_tabs = persisted.tabs.len();
        let prev_recent = persisted.recent_paths.len();
        persisted
            .tabs
            .retain(|t| !path_matches_or_descendant(&t.path, deleted_path));
        persisted
            .recent_paths
            .retain(|p| !path_matches_or_descendant(p, deleted_path));
        let mut changed =
            persisted.tabs.len() != prev_tabs || persisted.recent_paths.len() != prev_recent;
        if persisted
            .active_path
            .as_deref()
            .is_some_and(|p| path_matches_or_descendant(p, deleted_path))
        {
            persisted.active_path = persisted
                .recent_paths
                .last()
                .cloned()
                .or_else(|| persisted.tabs.first().map(|t| t.path.clone()));
            changed = true;
        }
        changed.then(|| serde_json::to_string(&persisted).ok())?
    }

    /// Sync missing flags with on-disk files (external delete marks `missing = true`).
    pub fn sync_missing(&mut self, exists: impl Fn(&str) -> bool) {
        for tab in &mut self.tabs {
            tab.missing = !exists(&tab.path);
        }
    }

    pub fn to_json(&self) -> String {
        let active_path = self.active_tab().map(|t| t.path.clone());
        let recent_paths = self
            .recent
            .iter()
            .filter_map(|&id| self.get(id).map(|t| t.path.clone()))
            .collect();
        let persisted = PersistedTabs {
            tabs: self
                .tabs
                .iter()
                .map(|t| PersistedTab {
                    path: t.path.clone(),
                    view: Some(t.view.clone()),
                })
                .collect(),
            active_path,
            recent_paths,
        };
        serde_json::to_string(&persisted).unwrap_or_default()
    }

    pub fn restore_from_json(&mut self, raw: &str, exists: impl Fn(&str) -> bool) -> bool {
        let Ok(persisted) = serde_json::from_str::<PersistedTabs>(raw) else {
            return false;
        };
        self.clear();
        for item in persisted.tabs {
            let (Some(kind), Some(view)) = (TabKind::from_path(&item.path), item.view) else {
                continue;
            };
            if !exists(&item.path) {
                continue;
            }
            let id = self.alloc_id();
            self.tabs.push(Tab {
                id,
                kind,
                path: item.path,
                view,
                missing: false,
            });
        }
        for path in &persisted.recent_paths {
            if let Some(id) = self.tabs.iter().find(|t| &t.path == path).map(|t| t.id)
                && !self.recent.contains(&id)
            {
                self.recent.push(id);
            }
        }
        for tab in &self.tabs {
            if !self.recent.contains(&tab.id) {
                self.recent.insert(0, tab.id);
            }
        }
        if let Some(active_path) = persisted.active_path
            && let Some(id) = self
                .tabs
                .iter()
                .find(|t| t.path == active_path)
                .map(|t| t.id)
        {
            self.activate(id);
        } else if let Some(first) = self.tabs.first().map(|t| t.id) {
            self.activate(first);
        }
        !self.tabs.is_empty()
    }

    fn alloc_id(&mut self) -> TabId {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        id
    }
}

fn rewrite_rel_path(p: &mut String, old_path: &str, new_path: &str) -> bool {
    if p == old_path {
        *p = new_path.to_string();
        true
    } else if let Some(suffix) = p.strip_prefix(old_path).filter(|s| s.starts_with('/')) {
        *p = format!("{new_path}{suffix}");
        true
    } else {
        false
    }
}

fn path_matches_or_descendant(p: &str, target: &str) -> bool {
    p == target || p.strip_prefix(target).is_some_and(|s| s.starts_with('/'))
}

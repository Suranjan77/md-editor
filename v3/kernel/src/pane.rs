//! PaneTree + DocumentStore (plan §3.1).
//!
//! A workspace is a binary split tree of panes; each pane holds a tab strip of
//! editors; an editor is a view onto a document. Document state is owned by
//! the [`DocumentStore`] and shared by reference — two panes can show the same
//! document.
//!
//! This is the structural fix for BUG-C: v2 had no workspace model, so "open a
//! PDF" was `split_view_active = true; showing_pdf = true` plus three more
//! flags hand-synced across ~40 call sites. Here *any* document opens as a tab
//! in *any* pane; splitting is an explicit, separate layout operation.

use std::collections::HashMap;

use crate::input::EditorKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PaneId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocumentId(pub u64);

/// Metadata for an open document. Buffer/annotation state will hang off this
/// (owned here, never by a pane) when the editor engine is wired in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub id: DocumentId,
    pub path: String,
    pub kind: EditorKind,
}

/// Owns all open documents; panes refer to them by [`DocumentId`].
#[derive(Debug, Default)]
pub struct DocumentStore {
    docs: HashMap<DocumentId, Document>,
    by_path: HashMap<String, DocumentId>,
    next: u64,
}

impl DocumentStore {
    pub fn new() -> DocumentStore {
        DocumentStore::default()
    }

    /// Open (or return the already-open) document at `path`. Deduplicated by
    /// path so two tabs on the same file share state by construction.
    pub fn open(&mut self, path: &str, kind: EditorKind) -> DocumentId {
        if let Some(&id) = self.by_path.get(path) {
            return id;
        }
        self.next += 1;
        let id = DocumentId(self.next);
        self.docs.insert(
            id,
            Document {
                id,
                path: path.to_string(),
                kind,
            },
        );
        self.by_path.insert(path.to_string(), id);
        id
    }

    pub fn get(&self, id: DocumentId) -> Option<&Document> {
        self.docs.get(&id)
    }

    /// Drop a document nothing references anymore (the workspace decides when).
    pub fn close(&mut self, id: DocumentId) -> Option<Document> {
        let doc = self.docs.remove(&id)?;
        self.by_path.remove(&doc.path);
        Some(doc)
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
}

/// `Horizontal` lays children side by side (a "vertical divider"); `Vertical`
/// stacks them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

/// One tab: an editor of `editor` kind viewing `document`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tab {
    pub id: TabId,
    pub document: DocumentId,
    pub editor: EditorKind,
}

/// A leaf of the pane tree: a tab strip with at most one active tab.
#[derive(Debug)]
pub struct Pane {
    pub id: PaneId,
    tabs: Vec<Tab>,
    active: usize,
}

impl Pane {
    fn new(id: PaneId) -> Pane {
        Pane {
            id,
            tabs: Vec::new(),
            active: 0,
        }
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    fn tab_index(&self, tab: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == tab)
    }
}

#[derive(Debug)]
enum Node {
    Leaf(Pane),
    Split {
        axis: SplitAxis,
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

/// Read-only view of the tree for the shell's layout pass.
#[derive(Debug)]
pub enum Layout<'a> {
    Pane(&'a Pane),
    Split {
        axis: SplitAxis,
        ratio: f32,
        first: Box<Layout<'a>>,
        second: Box<Layout<'a>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PaneError {
    #[error("no such pane: {0:?}")]
    NoSuchPane(PaneId),
    #[error("no such tab: {0:?}")]
    NoSuchTab(TabId),
    #[error("cannot remove the last pane")]
    LastPane,
}

/// What [`PaneTree::close_tab`] did, so the workspace can fix focus and
/// garbage-collect documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClosedTab {
    pub tab: Tab,
    /// Pane the tab lived in.
    pub pane: PaneId,
    /// True if the pane became empty and was collapsed out of the tree.
    pub pane_removed: bool,
}

#[derive(Debug)]
pub struct PaneTree {
    root: Node,
    next_pane: u64,
    next_tab: u64,
}

impl Default for PaneTree {
    fn default() -> PaneTree {
        PaneTree::new()
    }
}

impl PaneTree {
    /// A tree starts as a single empty pane — not a split (BUG-C).
    pub fn new() -> PaneTree {
        PaneTree {
            root: Node::Leaf(Pane::new(PaneId(1))),
            next_pane: 1,
            next_tab: 0,
        }
    }

    /// Panes in layout order (left-to-right / top-to-bottom traversal).
    pub fn panes(&self) -> Vec<&Pane> {
        let mut out = Vec::new();
        collect_leaves(&self.root, &mut out);
        out
    }

    pub fn pane_count(&self) -> usize {
        self.panes().len()
    }

    /// Leftmost pane — the default target when nothing is focused.
    pub fn first_pane(&self) -> PaneId {
        let mut node = &self.root;
        loop {
            match node {
                Node::Leaf(p) => return p.id,
                Node::Split { first, .. } => node = first,
            }
        }
    }

    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        find_pane(&self.root, id)
    }

    /// Pane and tab for a [`TabId`].
    pub fn find_tab(&self, tab: TabId) -> Option<(&Pane, &Tab)> {
        self.panes()
            .into_iter()
            .find_map(|p| p.tabs.iter().find(|t| t.id == tab).map(|t| (p, t)))
    }

    pub fn layout(&self) -> Layout<'_> {
        build_layout(&self.root)
    }

    /// Open a document as a new tab in `pane` and make it the active tab.
    /// No split, no mode flag — this is the whole story of opening a document.
    pub fn open_tab(
        &mut self,
        pane: PaneId,
        document: DocumentId,
        editor: EditorKind,
    ) -> Result<TabId, PaneError> {
        let p = find_pane_mut(&mut self.root, pane).ok_or(PaneError::NoSuchPane(pane))?;
        // Re-activating an existing tab on the same document+editor instead of
        // duplicating it matches every tabbed editor users know.
        if let Some(i) = p
            .tabs
            .iter()
            .position(|t| t.document == document && t.editor == editor)
        {
            p.active = i;
            return Ok(p.tabs[i].id);
        }
        self.next_tab += 1;
        let id = TabId(self.next_tab);
        p.tabs.push(Tab {
            id,
            document,
            editor,
        });
        p.active = p.tabs.len() - 1;
        Ok(id)
    }

    /// Split `pane`, returning the new (empty) sibling pane. An explicit
    /// layout choice, never a side effect of opening a document.
    pub fn split(&mut self, pane: PaneId, axis: SplitAxis) -> Result<PaneId, PaneError> {
        self.split_with_ratio(pane, axis, 0.5)
    }

    /// [`Self::split`] with an explicit divider ratio — what session restore
    /// uses to rebuild a saved layout. Clamped away from degenerate slivers.
    pub fn split_with_ratio(
        &mut self,
        pane: PaneId,
        axis: SplitAxis,
        ratio: f32,
    ) -> Result<PaneId, PaneError> {
        self.next_pane += 1;
        let new_id = PaneId(self.next_pane);
        let ratio = if ratio.is_finite() {
            ratio.clamp(0.05, 0.95)
        } else {
            0.5
        };
        if split_leaf(&mut self.root, pane, axis, ratio, Pane::new(new_id)) {
            Ok(new_id)
        } else {
            self.next_pane -= 1;
            Err(PaneError::NoSuchPane(pane))
        }
    }

    /// Close a tab. An empty non-last pane collapses out of the tree, undoing
    /// its split.
    pub fn close_tab(&mut self, tab: TabId) -> Result<ClosedTab, PaneError> {
        let pane_id = self
            .panes()
            .iter()
            .find(|p| p.tab_index(tab).is_some())
            .map(|p| p.id)
            .ok_or(PaneError::NoSuchTab(tab))?;
        let (removed, now_empty) = {
            let p = find_pane_mut(&mut self.root, pane_id).ok_or(PaneError::NoSuchPane(pane_id))?;
            let idx = p.tab_index(tab).ok_or(PaneError::NoSuchTab(tab))?;
            let removed = p.tabs.remove(idx);
            if p.active >= idx && p.active > 0 {
                p.active -= 1;
            }
            (removed, p.tabs.is_empty())
        };
        let mut pane_removed = false;
        if now_empty && self.pane_count() > 1 {
            self.remove_pane(pane_id)?;
            pane_removed = true;
        }
        Ok(ClosedTab {
            tab: removed,
            pane: pane_id,
            pane_removed,
        })
    }

    /// Move a tab to another pane (activating it there).
    pub fn move_tab(&mut self, tab: TabId, target: PaneId) -> Result<(), PaneError> {
        if self.pane(target).is_none() {
            return Err(PaneError::NoSuchPane(target));
        }
        let closed = self.close_tab(tab)?;
        let p = find_pane_mut(&mut self.root, target).ok_or(PaneError::NoSuchPane(target))?;
        p.tabs.push(closed.tab);
        p.active = p.tabs.len() - 1;
        Ok(())
    }

    pub fn set_active_tab(&mut self, pane: PaneId, tab: TabId) -> Result<(), PaneError> {
        let p = find_pane_mut(&mut self.root, pane).ok_or(PaneError::NoSuchPane(pane))?;
        let idx = p.tab_index(tab).ok_or(PaneError::NoSuchTab(tab))?;
        p.active = idx;
        Ok(())
    }

    fn remove_pane(&mut self, pane: PaneId) -> Result<(), PaneError> {
        if matches!(&self.root, Node::Leaf(p) if p.id == pane) {
            return Err(PaneError::LastPane);
        }
        if collapse_pane(&mut self.root, pane) {
            Ok(())
        } else {
            Err(PaneError::NoSuchPane(pane))
        }
    }

    /// Collapse every empty pane out of the tree (the last pane always
    /// survives). Session restore uses this after skipping tabs whose files
    /// vanished — a split must not outlive its content.
    pub fn collapse_empty_panes(&mut self) {
        loop {
            if self.pane_count() <= 1 {
                return;
            }
            let empty = self.panes().iter().find(|p| p.is_empty()).map(|p| p.id);
            match empty {
                Some(id) => {
                    if self.remove_pane(id).is_err() {
                        return;
                    }
                }
                None => return,
            }
        }
    }

    /// True if any tab in any pane references `doc` — used by the workspace
    /// for document garbage collection.
    pub fn references_document(&self, doc: DocumentId) -> bool {
        self.panes()
            .iter()
            .any(|p| p.tabs.iter().any(|t| t.document == doc))
    }
}

fn collect_leaves<'a>(node: &'a Node, out: &mut Vec<&'a Pane>) {
    match node {
        Node::Leaf(p) => out.push(p),
        Node::Split { first, second, .. } => {
            collect_leaves(first, out);
            collect_leaves(second, out);
        }
    }
}

fn find_pane(node: &Node, id: PaneId) -> Option<&Pane> {
    match node {
        Node::Leaf(p) => (p.id == id).then_some(p),
        Node::Split { first, second, .. } => find_pane(first, id).or_else(|| find_pane(second, id)),
    }
}

fn find_pane_mut(node: &mut Node, id: PaneId) -> Option<&mut Pane> {
    match node {
        Node::Leaf(p) => (p.id == id).then_some(p),
        Node::Split { first, second, .. } => {
            if find_pane(first, id).is_some() {
                find_pane_mut(first, id)
            } else {
                find_pane_mut(second, id)
            }
        }
    }
}

fn split_leaf(
    node: &mut Node,
    target: PaneId,
    axis: SplitAxis,
    ratio: f32,
    new_pane: Pane,
) -> bool {
    match node {
        Node::Leaf(p) if p.id == target => {
            let old = std::mem::replace(node, Node::Leaf(Pane::new(PaneId(0))));
            *node = Node::Split {
                axis,
                ratio,
                first: Box::new(old),
                second: Box::new(Node::Leaf(new_pane)),
            };
            true
        }
        Node::Leaf(_) => false,
        Node::Split { first, second, .. } => {
            if find_pane(first, target).is_some() {
                split_leaf(first, target, axis, ratio, new_pane)
            } else {
                split_leaf(second, target, axis, ratio, new_pane)
            }
        }
    }
}

fn collapse_pane(node: &mut Node, target: PaneId) -> bool {
    if let Node::Split { first, second, .. } = node {
        let take_second = matches!(first.as_ref(), Node::Leaf(p) if p.id == target);
        let take_first = matches!(second.as_ref(), Node::Leaf(p) if p.id == target);
        if take_second || take_first {
            let survivor = if take_second {
                std::mem::replace(second, Box::new(Node::Leaf(Pane::new(PaneId(0)))))
            } else {
                std::mem::replace(first, Box::new(Node::Leaf(Pane::new(PaneId(0)))))
            };
            *node = *survivor;
            return true;
        }
        return collapse_pane(first, target) || collapse_pane(second, target);
    }
    false
}

fn build_layout(node: &Node) -> Layout<'_> {
    match node {
        Node::Leaf(p) => Layout::Pane(p),
        Node::Split {
            axis,
            ratio,
            first,
            second,
        } => Layout::Split {
            axis: *axis,
            ratio: *ratio,
            first: Box::new(build_layout(first)),
            second: Box::new(build_layout(second)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok<T>(r: Result<T, PaneError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("unexpected pane error: {e}"),
        }
    }

    #[test]
    fn new_tree_is_a_single_empty_pane() {
        let tree = PaneTree::new();
        assert_eq!(tree.pane_count(), 1);
        let panes = tree.panes();
        assert!(panes[0].is_empty());
    }

    #[test]
    fn open_tab_activates_and_dedups_per_document() {
        let mut tree = PaneTree::new();
        let pane = tree.first_pane();
        let t1 = ok(tree.open_tab(pane, DocumentId(1), EditorKind::Markdown));
        let t2 = ok(tree.open_tab(pane, DocumentId(2), EditorKind::Pdf));
        assert_ne!(t1, t2);
        let again = ok(tree.open_tab(pane, DocumentId(1), EditorKind::Markdown));
        assert_eq!(
            again, t1,
            "re-opening the same document re-activates its tab"
        );
        let p = tree.pane(pane).map(|p| p.tabs().len());
        assert_eq!(p, Some(2));
    }

    #[test]
    fn split_then_close_collapses_back() {
        let mut tree = PaneTree::new();
        let left = tree.first_pane();
        let t_md = ok(tree.open_tab(left, DocumentId(1), EditorKind::Markdown));
        let right = ok(tree.split(left, SplitAxis::Horizontal));
        let t_pdf = ok(tree.open_tab(right, DocumentId(2), EditorKind::Pdf));
        assert_eq!(tree.pane_count(), 2);

        let closed = ok(tree.close_tab(t_pdf));
        assert!(closed.pane_removed, "empty split pane collapses");
        assert_eq!(tree.pane_count(), 1);
        assert!(tree.find_tab(t_md).is_some());
    }

    #[test]
    fn closing_last_tab_of_last_pane_keeps_the_pane() {
        let mut tree = PaneTree::new();
        let pane = tree.first_pane();
        let tab = ok(tree.open_tab(pane, DocumentId(1), EditorKind::Markdown));
        let closed = ok(tree.close_tab(tab));
        assert!(!closed.pane_removed);
        assert_eq!(tree.pane_count(), 1);
    }

    #[test]
    fn split_with_ratio_lands_in_layout_and_clamps() {
        let mut tree = PaneTree::new();
        let left = tree.first_pane();
        ok(tree.split_with_ratio(left, SplitAxis::Horizontal, 0.7));
        match tree.layout() {
            Layout::Split { ratio, .. } => assert!((ratio - 0.7).abs() < f32::EPSILON),
            Layout::Pane(_) => panic!("expected a split"),
        }
        // Degenerate inputs are tamed, not stored.
        let inner = tree.panes()[1].id;
        ok(tree.split_with_ratio(inner, SplitAxis::Vertical, f32::NAN));
        let all_sane = tree_ratios(&tree.layout())
            .iter()
            .all(|r| r.is_finite() && (0.05..=0.95).contains(r));
        assert!(all_sane);
    }

    fn tree_ratios(layout: &Layout<'_>) -> Vec<f32> {
        match layout {
            Layout::Pane(_) => Vec::new(),
            Layout::Split {
                ratio,
                first,
                second,
                ..
            } => {
                let mut out = vec![*ratio];
                out.extend(tree_ratios(first));
                out.extend(tree_ratios(second));
                out
            }
        }
    }

    #[test]
    fn collapse_empty_panes_undoes_hollow_splits() {
        let mut tree = PaneTree::new();
        let a = tree.first_pane();
        ok(tree.open_tab(a, DocumentId(1), EditorKind::Markdown));
        let b = ok(tree.split(a, SplitAxis::Horizontal));
        let _c = ok(tree.split(b, SplitAxis::Vertical)); // b and c both empty
        assert_eq!(tree.pane_count(), 3);
        tree.collapse_empty_panes();
        assert_eq!(tree.pane_count(), 1, "only the populated pane survives");
        // A fully empty tree keeps its one pane.
        let mut fresh = PaneTree::new();
        fresh.collapse_empty_panes();
        assert_eq!(fresh.pane_count(), 1);
    }

    #[test]
    fn move_tab_between_panes() {
        let mut tree = PaneTree::new();
        let left = tree.first_pane();
        let tab = ok(tree.open_tab(left, DocumentId(1), EditorKind::Markdown));
        ok(tree.open_tab(left, DocumentId(2), EditorKind::Pdf));
        let right = ok(tree.split(left, SplitAxis::Vertical));
        ok(tree.move_tab(tab, right));
        let in_right = tree.pane(right).and_then(|p| p.active_tab()).map(|t| t.id);
        assert_eq!(in_right, Some(tab));
    }

    #[test]
    fn document_store_dedups_by_path_and_gc_checks_references() {
        let mut docs = DocumentStore::new();
        let a = docs.open("notes/a.md", EditorKind::Markdown);
        let a2 = docs.open("notes/a.md", EditorKind::Markdown);
        assert_eq!(a, a2);
        assert_eq!(docs.len(), 1);

        let mut tree = PaneTree::new();
        let pane = tree.first_pane();
        let tab = ok(tree.open_tab(pane, a, EditorKind::Markdown));
        assert!(tree.references_document(a));
        ok(tree.close_tab(tab));
        assert!(!tree.references_document(a));
    }

    #[test]
    fn same_document_can_be_shown_in_two_panes() {
        let mut docs = DocumentStore::new();
        let doc = docs.open("notes/shared.md", EditorKind::Markdown);
        let mut tree = PaneTree::new();
        let left = tree.first_pane();
        ok(tree.open_tab(left, doc, EditorKind::Markdown));
        let right = ok(tree.split(left, SplitAxis::Horizontal));
        ok(tree.open_tab(right, doc, EditorKind::Markdown));
        assert!(
            tree.panes()
                .iter()
                .all(|p| p.tabs().iter().any(|t| t.document == doc))
        );
    }
}

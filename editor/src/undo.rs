//! Undo **tree** (plan §3.2): edits never destroy history. Where v2 kept a
//! linear undo/redo stack pair (redo cleared on every new edit), v3 keeps
//! every transaction as a node in a tree:
//!
//! - `commit` adds a child of the current node and moves to it;
//! - `undo` steps to the parent (returning the transaction to invert);
//! - `redo` follows the *active* child — the most recently taken branch —
//!   and `select_branch` re-aims it, so abandoned branches stay reachable.
//!
//! The tree is plain data (no I/O, no serde): [`UndoTree::snapshot`] /
//! [`UndoTree::from_snapshot`] convert to/from an index-based flat form the
//! vault sidecar can store per document hash ("persistent undo", plan M3).

/// One primitive, invertible text mutation in **char** offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOp {
    Insert { at: usize, text: String },
    Delete { at: usize, text: String },
}

impl EditOp {
    pub fn inverse(&self) -> EditOp {
        match self {
            EditOp::Insert { at, text } => EditOp::Delete {
                at: *at,
                text: text.clone(),
            },
            EditOp::Delete { at, text } => EditOp::Insert {
                at: *at,
                text: text.clone(),
            },
        }
    }
}

/// A selection (caret when `anchor == head`) in char offsets. Multi-cursor is
/// `Vec<Selection>` from day one (plan §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn caret(at: usize) -> Selection {
        Selection {
            anchor: at,
            head: at,
        }
    }

    pub fn new(anchor: usize, head: usize) -> Selection {
        Selection { anchor, head }
    }

    pub fn is_caret(&self) -> bool {
        self.anchor == self.head
    }

    /// `(start, end)` with `start <= end`.
    pub fn range(&self) -> (usize, usize) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

/// One undoable unit: the ops in the order they were applied, plus the
/// selection sets to restore on undo (`before`) and redo (`after`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    pub ops: Vec<EditOp>,
    pub before: Vec<Selection>,
    pub after: Vec<Selection>,
}

#[derive(Debug)]
struct Node {
    parent: Option<usize>,
    children: Vec<usize>,
    /// Which child `redo` follows; re-aimed by `commit` and `select_branch`.
    active_child: Option<usize>,
    /// `None` only for the root.
    txn: Option<Transaction>,
}

/// Error from [`UndoTree::select_branch`] / [`UndoTree::from_snapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UndoTreeError {
    #[error("node {0} is not a child of the current node")]
    NotAChild(usize),
    #[error("snapshot is malformed at node {0}")]
    MalformedSnapshot(usize),
}

#[derive(Debug)]
pub struct UndoTree {
    nodes: Vec<Node>,
    current: usize,
}

impl Default for UndoTree {
    fn default() -> Self {
        Self::new()
    }
}

impl UndoTree {
    pub fn new() -> UndoTree {
        UndoTree {
            nodes: vec![Node {
                parent: None,
                children: Vec::new(),
                active_child: None,
                txn: None,
            }],
            current: 0,
        }
    }

    /// Identity of the current node (the root is `0`). Stable across the
    /// tree's lifetime — nodes are never removed — so callers may use it as
    /// a bookmark (e.g. the buffer's "saved here" marker).
    pub fn current(&self) -> usize {
        self.current
    }

    pub fn can_undo(&self) -> bool {
        self.nodes[self.current].parent.is_some()
    }

    pub fn can_redo(&self) -> bool {
        self.nodes[self.current].active_child.is_some()
    }

    /// Children of the current node, oldest first — the redo branches.
    pub fn branches(&self) -> &[usize] {
        &self.nodes[self.current].children
    }

    /// Record a committed transaction as a new branch off the current node
    /// and move to it. Returns the new node's id.
    pub fn commit(&mut self, txn: Transaction) -> usize {
        let id = self.nodes.len();
        self.nodes.push(Node {
            parent: Some(self.current),
            children: Vec::new(),
            active_child: None,
            txn: Some(txn),
        });
        self.nodes[self.current].children.push(id);
        self.nodes[self.current].active_child = Some(id);
        self.current = id;
        id
    }

    /// Append an op to the transaction at `node` (insert-run coalescing).
    /// Only legal while `node` is current and a leaf — i.e. nothing has been
    /// committed or undone since. Returns false otherwise.
    pub fn amend(&mut self, node: usize, op: EditOp, after: Vec<Selection>) -> bool {
        if node != self.current || !self.nodes[node].children.is_empty() {
            return false;
        }
        match self.nodes[node].txn.as_mut() {
            Some(txn) => {
                txn.ops.push(op);
                txn.after = after;
                true
            }
            None => false,
        }
    }

    /// Step to the parent, returning the transaction whose ops must be
    /// applied **inverted, in reverse order** by the caller.
    pub fn undo(&mut self) -> Option<&Transaction> {
        let parent = self.nodes[self.current].parent?;
        let undone = self.current;
        self.current = parent;
        // Keep the path we came down as the redo path.
        self.nodes[parent].active_child = Some(undone);
        self.nodes[undone].txn.as_ref()
    }

    /// Step into the active child, returning the transaction whose ops must
    /// be re-applied in order by the caller.
    pub fn redo(&mut self) -> Option<&Transaction> {
        let child = self.nodes[self.current].active_child?;
        self.current = child;
        self.nodes[child].txn.as_ref()
    }

    /// Re-aim redo at a different branch of the current node.
    pub fn select_branch(&mut self, child: usize) -> Result<(), UndoTreeError> {
        if !self.nodes[self.current].children.contains(&child) {
            return Err(UndoTreeError::NotAChild(child));
        }
        self.nodes[self.current].active_child = Some(child);
        Ok(())
    }

    pub fn snapshot(&self) -> UndoTreeSnapshot {
        UndoTreeSnapshot {
            nodes: self
                .nodes
                .iter()
                .map(|n| SnapshotNode {
                    parent: n.parent,
                    active_child: n.active_child,
                    txn: n.txn.clone(),
                })
                .collect(),
            current: self.current,
        }
    }

    /// Rebuild from a snapshot. Children lists are derived from parents;
    /// the snapshot is validated (root-only at 0, parents precede children,
    /// active children consistent) so a corrupt sidecar cannot panic us.
    pub fn from_snapshot(snap: UndoTreeSnapshot) -> Result<UndoTree, UndoTreeError> {
        if snap.nodes.is_empty() || snap.current >= snap.nodes.len() {
            return Err(UndoTreeError::MalformedSnapshot(0));
        }
        let mut nodes: Vec<Node> = Vec::with_capacity(snap.nodes.len());
        for (i, n) in snap.nodes.iter().enumerate() {
            let root_shaped = n.parent.is_none() && n.txn.is_none();
            if (i == 0) != root_shaped {
                return Err(UndoTreeError::MalformedSnapshot(i));
            }
            if let Some(p) = n.parent
                && p >= i
            {
                return Err(UndoTreeError::MalformedSnapshot(i));
            }
            nodes.push(Node {
                parent: n.parent,
                children: Vec::new(),
                active_child: n.active_child,
                txn: n.txn.clone(),
            });
        }
        for i in 1..snap.nodes.len() {
            if let Some(p) = nodes[i].parent {
                nodes[p].children.push(i);
            }
        }
        for (i, n) in nodes.iter().enumerate() {
            if let Some(c) = n.active_child
                && !n.children.contains(&c)
            {
                return Err(UndoTreeError::MalformedSnapshot(i));
            }
        }
        Ok(UndoTree {
            nodes,
            current: snap.current,
        })
    }
}

/// Flat, index-based form of the tree for sidecar persistence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoTreeSnapshot {
    pub nodes: Vec<SnapshotNode>,
    pub current: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotNode {
    pub parent: Option<usize>,
    pub active_child: Option<usize>,
    pub txn: Option<Transaction>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn txn(tag: &str) -> Transaction {
        Transaction {
            ops: vec![EditOp::Insert {
                at: 0,
                text: tag.to_string(),
            }],
            before: vec![Selection::caret(0)],
            after: vec![Selection::caret(tag.chars().count())],
        }
    }

    #[test]
    fn linear_undo_redo_round_trips() {
        let mut tree = UndoTree::new();
        let a = tree.commit(txn("a"));
        let b = tree.commit(txn("b"));
        assert_eq!(tree.current(), b);
        assert!(tree.undo().is_some());
        assert_eq!(tree.current(), a);
        assert!(tree.redo().is_some());
        assert_eq!(tree.current(), b);
        assert!(!tree.can_redo());
    }

    #[test]
    fn new_edit_after_undo_keeps_the_old_branch() {
        let mut tree = UndoTree::new();
        let a = tree.commit(txn("a"));
        tree.undo();
        let b = tree.commit(txn("b"));
        // Both branches live under the root.
        tree.undo();
        assert_eq!(tree.branches(), &[a, b]);
        // Redo follows the most recent branch…
        assert!(tree.redo().is_some());
        assert_eq!(tree.current(), b);
        // …but the abandoned one is selectable again.
        tree.undo();
        match tree.select_branch(a) {
            Ok(()) => {}
            Err(e) => panic!("select_branch: {e}"),
        }
        assert!(tree.redo().is_some());
        assert_eq!(tree.current(), a);
    }

    #[test]
    fn select_branch_rejects_non_children() {
        let mut tree = UndoTree::new();
        let a = tree.commit(txn("a"));
        assert_eq!(tree.select_branch(a), Err(UndoTreeError::NotAChild(a)));
    }

    #[test]
    fn amend_only_extends_the_current_leaf() {
        let mut tree = UndoTree::new();
        let a = tree.commit(txn("a"));
        assert!(tree.amend(
            a,
            EditOp::Insert {
                at: 1,
                text: "b".into()
            },
            vec![Selection::caret(2)],
        ));
        tree.undo();
        assert!(!tree.amend(
            a,
            EditOp::Insert {
                at: 2,
                text: "c".into()
            },
            vec![Selection::caret(3)],
        ));
        tree.redo();
        let snap = tree.snapshot();
        match &snap.nodes[a].txn {
            Some(t) => assert_eq!(t.ops.len(), 2),
            None => panic!("node {a} lost its transaction"),
        }
    }

    #[test]
    fn snapshot_round_trips() {
        let mut tree = UndoTree::new();
        tree.commit(txn("a"));
        tree.undo();
        tree.commit(txn("b"));
        let snap = tree.snapshot();
        let rebuilt = match UndoTree::from_snapshot(snap.clone()) {
            Ok(t) => t,
            Err(e) => panic!("from_snapshot: {e}"),
        };
        assert_eq!(rebuilt.snapshot(), snap);
        assert_eq!(rebuilt.current(), tree.current());
        assert_eq!(rebuilt.branches(), tree.branches());
    }

    #[test]
    fn malformed_snapshots_are_rejected_not_panicked() {
        // Empty.
        let empty = UndoTreeSnapshot {
            nodes: vec![],
            current: 0,
        };
        assert!(UndoTree::from_snapshot(empty).is_err());
        // Forward parent reference.
        let forward = UndoTreeSnapshot {
            nodes: vec![
                SnapshotNode {
                    parent: None,
                    active_child: None,
                    txn: None,
                },
                SnapshotNode {
                    parent: Some(2),
                    active_child: None,
                    txn: Some(txn("x")),
                },
            ],
            current: 0,
        };
        assert!(UndoTree::from_snapshot(forward).is_err());
    }
}

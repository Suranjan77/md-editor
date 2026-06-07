use super::document::DocBuffer;
use serde::{Deserialize, Serialize};

const MAX_UNDO_TRANSACTIONS: usize = 1000;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub anchor: usize,
    pub focus: usize,
}

impl Selection {
    pub fn new(anchor: usize, focus: usize) -> Option<Self> {
        if anchor == focus {
            None
        } else {
            Some(Self { anchor, focus })
        }
    }

    pub fn range(self) -> (usize, usize) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EditOp {
    Insert { char_offset: usize, text: String },
    Delete { char_offset: usize, text: String },
}

impl EditOp {
    pub(crate) fn inverse(&self) -> Self {
        match self {
            EditOp::Insert { char_offset, text } => EditOp::Delete {
                char_offset: *char_offset,
                text: text.clone(),
            },
            EditOp::Delete { char_offset, text } => EditOp::Insert {
                char_offset: *char_offset,
                text: text.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditTransaction {
    ops: Vec<EditOp>,
    before_cursor: usize,
    after_cursor: usize,
    before_selection: Option<Selection>,
    after_selection: Option<Selection>,
}

impl DocBuffer {
    pub fn undo(&mut self) -> bool {
        let Some(transaction) = self.undo_stack.pop() else {
            return false;
        };
        for op in transaction.ops.iter().rev().map(EditOp::inverse) {
            self.apply_op(&op);
        }
        self.cursor_offset = transaction.before_cursor.min(self.rope.len_chars());
        self.selection_offsets = transaction.before_selection;
        self.redo_stack.push(transaction);
        self.dirty = true;
        self.desired_col = None;
        self.sync_public_state();
        true
    }
    pub fn redo(&mut self) -> bool {
        let Some(transaction) = self.redo_stack.pop() else {
            return false;
        };
        for op in &transaction.ops {
            self.apply_op(op);
        }
        self.cursor_offset = transaction.after_cursor.min(self.rope.len_chars());
        self.selection_offsets = transaction.after_selection;
        self.undo_stack.push(transaction);
        self.dirty = true;
        self.desired_col = None;
        self.sync_public_state();
        true
    }
    pub(crate) fn commit_transaction(
        &mut self,
        ops: Vec<EditOp>,
        before_cursor: usize,
        before_selection: Option<Selection>,
    ) {
        self.dirty = true;
        self.desired_col = None;
        self.undo_stack.push(EditTransaction {
            ops,
            before_cursor,
            after_cursor: self.cursor_offset,
            before_selection,
            after_selection: self.selection_offsets,
        });
        if self.undo_stack.len() > MAX_UNDO_TRANSACTIONS {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        self.sync_public_state();
    }
    pub(crate) fn apply_op(&mut self, op: &EditOp) {
        match op {
            EditOp::Insert { char_offset, text } => {
                let offset = (*char_offset).min(self.rope.len_chars());
                self.rope.insert(offset, text);
            }
            EditOp::Delete { char_offset, text } => {
                let start = (*char_offset).min(self.rope.len_chars());
                let end = (start + text.chars().count()).min(self.rope.len_chars());
                self.rope.remove(start..end);
            }
        }
    }
}

//! Undo/redo transactions and typing-run coalescing on `DocBuffer`.

use std::time::{Duration, Instant};

use super::{DocBuffer, EditOp, EditTransaction, Selection};

/// Consecutive single-character edits committed within this window merge into
/// one undo step.
const UNDO_COALESCE_WINDOW: Duration = Duration::from_millis(300);

fn can_coalesce(prev: &[EditOp], next: &[EditOp]) -> bool {
    let (Some(last), 1) = (prev.last(), next.len()) else {
        return false;
    };

    match (last, &next[0]) {
        (
            EditOp::Insert {
                char_offset: prev_at,
                text: prev_text,
            },
            EditOp::Insert {
                char_offset: next_at,
                text: next_text,
            },
        ) => {
            next_text.chars().count() == 1
                && !next_text.contains('\n')
                && !prev_text.contains('\n')
                && *next_at == prev_at + prev_text.chars().count()
        }
        (
            EditOp::Delete {
                char_offset: prev_at,
                text: prev_text,
            },
            EditOp::Delete {
                char_offset: next_at,
                text: next_text,
            },
        ) => {
            next_text.chars().count() == 1
                && !next_text.contains('\n')
                && !prev_text.contains('\n')
                && (*next_at + next_text.chars().count() == *prev_at || next_at == prev_at)
        }
        _ => false,
    }
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
        self.revision = self
            .redo_stack
            .last()
            .map(|transaction| transaction.before_revision)
            .unwrap_or(0);
        self.update_dirty();
        self.desired_col = None;
        self.last_commit_at = None;
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
        self.revision = transaction.after_revision;
        self.undo_stack.push(transaction);
        self.update_dirty();
        self.desired_col = None;
        self.last_commit_at = None;
        self.sync_public_state();
        true
    }

    pub(super) fn commit_transaction(
        &mut self,
        ops: Vec<EditOp>,
        before_cursor: usize,
        before_selection: Option<Selection>,
    ) {
        let before_revision = self.revision;
        let after_revision = self.next_revision;
        self.next_revision = self.next_revision.wrapping_add(1);
        self.revision = after_revision;
        self.update_dirty();
        self.desired_col = None;

        let now = Instant::now();
        let recent = self
            .last_commit_at
            .is_some_and(|at| now.duration_since(at) <= UNDO_COALESCE_WINDOW);

        if recent
            && before_selection.is_none()
            && let Some(previous) = self.undo_stack.last_mut()
            && previous.after_selection.is_none()
            && previous.after_cursor == before_cursor
            && can_coalesce(&previous.ops, &ops)
        {
            previous.ops.extend(ops);
            previous.after_cursor = self.cursor_offset;
            previous.after_selection = self.selection_offsets;
            self.last_commit_at = Some(now);
            self.redo_stack.clear();
            self.sync_public_state();
            return;
        }

        self.undo_stack.push(EditTransaction {
            ops,
            before_cursor,
            after_cursor: self.cursor_offset,
            before_selection,
            after_selection: self.selection_offsets,
            before_revision,
            after_revision,
        });
        self.last_commit_at = Some(now);
        self.redo_stack.clear();
        self.sync_public_state();
    }

    pub(super) fn apply_op(&mut self, op: &EditOp) {
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

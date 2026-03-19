/// Undo operation types.
#[derive(Debug, Clone)]
pub enum UndoOp {
    Insert {
        byte_offset: usize,
        text: String,
    },
    Delete {
        byte_offset: usize,
        length: usize,
        deleted_text: String,
    },
}

/// A single entry in the undo log.
#[derive(Debug, Clone)]
pub struct UndoEntry {
    pub op: UndoOp,
    pub inverse: UndoOp,
    pub cursor_before: usize,
    pub cursor_after: usize,
    pub timestamp_ms: u64,
}

/// Undo/redo history with coalescing of rapid keystrokes.
pub struct UndoHistory {
    undo_stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    /// Coalescing window in milliseconds.
    coalesce_window_ms: u64,
}

impl UndoHistory {
    pub fn new() -> Self {
        UndoHistory {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            coalesce_window_ms: 300,
        }
    }

    /// Record a new operation. Clears the redo stack.
    pub fn push(&mut self, entry: UndoEntry) {
        // Try to coalesce with the last entry
        if let Some(last) = self.undo_stack.last_mut() {
            if entry.timestamp_ms.saturating_sub(last.timestamp_ms) < self.coalesce_window_ms {
                if let Some(coalesced) = try_coalesce(last, &entry) {
                    *last = coalesced;
                    self.redo_stack.clear();
                    return;
                }
            }
        }

        self.undo_stack.push(entry);
        self.redo_stack.clear();
    }

    /// Pop the most recent undo entry. Returns the inverse operation to apply.
    pub fn undo(&mut self) -> Option<UndoEntry> {
        let entry = self.undo_stack.pop()?;
        self.redo_stack.push(entry.clone());
        Some(entry)
    }

    /// Pop the most recent redo entry. Returns the forward operation to re-apply.
    pub fn redo(&mut self) -> Option<UndoEntry> {
        let entry = self.redo_stack.pop()?;
        self.undo_stack.push(entry.clone());
        Some(entry)
    }

    /// Clear all history.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

/// Create an UndoEntry for an insert operation.
pub fn make_insert_entry(
    byte_offset: usize,
    text: &str,
    cursor_before: usize,
    cursor_after: usize,
    timestamp_ms: u64,
) -> UndoEntry {
    UndoEntry {
        op: UndoOp::Insert {
            byte_offset,
            text: text.to_string(),
        },
        inverse: UndoOp::Delete {
            byte_offset,
            length: text.len(),
            deleted_text: text.to_string(),
        },
        cursor_before,
        cursor_after,
        timestamp_ms,
    }
}

/// Create an UndoEntry for a delete operation.
pub fn make_delete_entry(
    byte_offset: usize,
    deleted_text: &str,
    cursor_before: usize,
    cursor_after: usize,
    timestamp_ms: u64,
) -> UndoEntry {
    UndoEntry {
        op: UndoOp::Delete {
            byte_offset,
            length: deleted_text.len(),
            deleted_text: deleted_text.to_string(),
        },
        inverse: UndoOp::Insert {
            byte_offset,
            text: deleted_text.to_string(),
        },
        cursor_before,
        cursor_after,
        timestamp_ms,
    }
}

/// Try to coalesce two consecutive entries (both must be inserts at adjacent positions,
/// or both deletes at adjacent positions).
fn try_coalesce(last: &UndoEntry, new: &UndoEntry) -> Option<UndoEntry> {
    match (&last.op, &new.op) {
        (
            UndoOp::Insert {
                byte_offset: o1,
                text: t1,
            },
            UndoOp::Insert {
                byte_offset: o2,
                text: t2,
            },
        ) => {
            // Adjacent insert: new insert starts right after last insert
            if *o2 == o1 + t1.len() {
                let merged_text = format!("{}{}", t1, t2);
                Some(UndoEntry {
                    op: UndoOp::Insert {
                        byte_offset: *o1,
                        text: merged_text.clone(),
                    },
                    inverse: UndoOp::Delete {
                        byte_offset: *o1,
                        length: merged_text.len(),
                        deleted_text: merged_text,
                    },
                    cursor_before: last.cursor_before,
                    cursor_after: new.cursor_after,
                    timestamp_ms: new.timestamp_ms,
                })
            } else {
                None
            }
        }
        (
            UndoOp::Delete {
                byte_offset: o1,
                length: _l1,
                deleted_text: t1,
            },
            UndoOp::Delete {
                byte_offset: o2,
                length: _l2,
                deleted_text: t2,
            },
        ) => {
            // Backspace coalescing: new delete at position just before last delete
            if *o2 + t2.len() == *o1 {
                let merged_text = format!("{}{}", t2, t1);
                Some(UndoEntry {
                    op: UndoOp::Delete {
                        byte_offset: *o2,
                        length: merged_text.len(),
                        deleted_text: merged_text.clone(),
                    },
                    inverse: UndoOp::Insert {
                        byte_offset: *o2,
                        text: merged_text,
                    },
                    cursor_before: last.cursor_before,
                    cursor_after: new.cursor_after,
                    timestamp_ms: new.timestamp_ms,
                })
            } else if *o1 == *o2 {
                // Forward-delete coalescing
                let merged_text = format!("{}{}", t1, t2);
                Some(UndoEntry {
                    op: UndoOp::Delete {
                        byte_offset: *o1,
                        length: merged_text.len(),
                        deleted_text: merged_text.clone(),
                    },
                    inverse: UndoOp::Insert {
                        byte_offset: *o1,
                        text: merged_text,
                    },
                    cursor_before: last.cursor_before,
                    cursor_after: new.cursor_after,
                    timestamp_ms: new.timestamp_ms,
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_undo_redo_basic() {
        let mut history = UndoHistory::new();
        let entry = make_insert_entry(0, "hello", 0, 5, 1000);
        history.push(entry);

        assert!(history.can_undo());
        let undone = history.undo().unwrap();
        assert!(matches!(undone.inverse, UndoOp::Delete { .. }));
        assert!(history.can_redo());

        let redone = history.redo().unwrap();
        assert!(matches!(redone.op, UndoOp::Insert { .. }));
    }

    #[test]
    fn test_coalescing() {
        let mut history = UndoHistory::new();
        history.push(make_insert_entry(0, "h", 0, 1, 1000));
        history.push(make_insert_entry(1, "e", 1, 2, 1100));
        history.push(make_insert_entry(2, "l", 2, 3, 1200));

        // All within 300ms window, should coalesce to one entry
        assert_eq!(history.undo_stack.len(), 1);
        if let UndoOp::Insert { text, .. } = &history.undo_stack[0].op {
            assert_eq!(text, "hel");
        }
    }

    #[test]
    fn test_no_coalescing_across_window() {
        let mut history = UndoHistory::new();
        history.push(make_insert_entry(0, "h", 0, 1, 1000));
        history.push(make_insert_entry(1, "e", 1, 2, 1500)); // 500ms gap

        assert_eq!(history.undo_stack.len(), 2);
    }
}

//! Text insertion, block insertion, replace-all and deletion operations on `DocBuffer`.

use super::lists::parse_list_item;
use super::{CommandResult, DocBuffer, EditOp};

impl DocBuffer {
    pub fn insert_at_cursor(&mut self, text: &str) {
        self.insert_text(text);
    }

    pub fn backspace(&mut self) {
        self.delete_backward();
    }

    pub fn delete(&mut self) {
        self.delete_forward();
    }

    /// Rewrite the entire document as one undoable edit.
    pub fn replace_all_text(&mut self, text: &str) -> CommandResult {
        let old_text = self.rope.to_string();
        if old_text == text {
            return CommandResult::default();
        }

        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;

        let ops = vec![
            EditOp::Delete {
                char_offset: 0,
                text: old_text,
            },
            EditOp::Insert {
                char_offset: 0,
                text: text.to_string(),
            },
        ];
        for op in &ops {
            self.apply_op(op);
        }

        self.cursor_offset = before_cursor.min(self.rope.len_chars());
        self.selection_offsets = None;
        self.last_commit_at = None;
        self.commit_transaction(ops, before_cursor, before_selection);
        self.last_commit_at = None;
        CommandResult::changed()
    }

    pub(super) fn insert_block(&mut self, block: &str) -> CommandResult {
        let at = self
            .selection_offsets
            .map_or(self.cursor_offset, |s| s.range().0);
        let len = self.rope.len_chars();
        let mut text = String::new();
        if at > 0 && self.rope.char(at - 1) != '\n' {
            text.push_str(self.eol);
        }
        text.push_str(block);
        if at >= len || !matches!(self.rope.char(at), '\n' | '\r') {
            text.push_str(self.eol);
        }
        self.insert_text(&text)
    }

    pub(super) fn insert_text(&mut self, text: &str) -> CommandResult {
        if text.is_empty() {
            return CommandResult::default();
        }

        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        let mut ops = Vec::new();
        let insert_at = if let Some(selection) = self.selection_offsets {
            let (start, end) = selection.range();
            let removed = self.rope.slice(start..end).to_string();
            self.rope.remove(start..end);
            ops.push(EditOp::Delete {
                char_offset: start,
                text: removed,
            });
            start
        } else {
            self.cursor_offset
        };

        let mut text_to_insert = text.to_string();

        if text == "\n" {
            text_to_insert = self.eol.to_string();
            let line_idx = self.rope.char_to_line(insert_at);
            let line_text = self.line_text(line_idx);
            if let Some(list_item) = parse_list_item(&line_text) {
                let line_start = self.rope.line_to_char(line_idx);
                let marker_end = line_start
                    + list_item.indent.chars().count()
                    + list_item.marker.chars().count();
                if insert_at >= marker_end {
                    if list_item.is_empty {
                        let marker_start = line_start + list_item.indent.chars().count();
                        let marker_len = list_item.marker.chars().count();
                        let marker_text = self
                            .rope
                            .slice(marker_start..marker_start + marker_len)
                            .to_string();
                        self.rope.remove(marker_start..marker_start + marker_len);
                        ops.push(EditOp::Delete {
                            char_offset: marker_start,
                            text: marker_text,
                        });

                        self.cursor_offset = marker_start;
                        self.selection_offsets = None;
                        self.commit_transaction(ops, before_cursor, before_selection);
                        return CommandResult::changed();
                    }
                    let next_prefix = format!("{}{}", list_item.indent, list_item.next_marker);
                    text_to_insert = format!("{}{}", self.eol, next_prefix);
                }
            }
        }

        self.rope.insert(insert_at, &text_to_insert);
        ops.push(EditOp::Insert {
            char_offset: insert_at,
            text: text_to_insert.clone(),
        });

        self.cursor_offset = insert_at + text_to_insert.chars().count();
        self.selection_offsets = None;
        self.commit_transaction(ops, before_cursor, before_selection);
        CommandResult::changed()
    }

    pub(super) fn delete_selection(&mut self) -> CommandResult {
        let Some(selection) = self.selection_offsets else {
            return CommandResult::default();
        };
        let (start, end) = selection.range();
        if start == end {
            self.selection_offsets = None;
            self.sync_public_state();
            return CommandResult::default();
        }

        let removed = self.rope.slice(start..end).to_string();
        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        self.rope.remove(start..end);
        self.cursor_offset = start;
        self.selection_offsets = None;
        self.commit_transaction(
            vec![EditOp::Delete {
                char_offset: start,
                text: removed,
            }],
            before_cursor,
            before_selection,
        );
        CommandResult::changed()
    }

    pub(super) fn delete_backward(&mut self) -> CommandResult {
        if self.selection_offsets.is_some() {
            return self.delete_selection();
        }
        let start = self.grapheme_before(self.cursor_offset);
        if start == self.cursor_offset {
            return CommandResult::default();
        }
        let previous = self.char_at(self.cursor_offset - 1);
        let next = self.char_at(self.cursor_offset);
        if matches!(
            (previous, next),
            (Some('('), Some(')'))
                | (Some('['), Some(']'))
                | (Some('{'), Some('}'))
                | (Some('"'), Some('"'))
                | (Some('\''), Some('\''))
                | (Some('`'), Some('`'))
        ) {
            return self.delete_range(self.cursor_offset - 1, self.cursor_offset + 1);
        }
        self.delete_range(start, self.cursor_offset)
    }

    pub(super) fn delete_forward(&mut self) -> CommandResult {
        if self.selection_offsets.is_some() {
            return self.delete_selection();
        }
        let end = self.grapheme_after(self.cursor_offset);
        if end == self.cursor_offset {
            return CommandResult::default();
        }
        self.delete_range(self.cursor_offset, end)
    }

    pub(super) fn delete_range(&mut self, start: usize, end: usize) -> CommandResult {
        let removed = self.rope.slice(start..end).to_string();
        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        self.rope.remove(start..end);
        self.cursor_offset = start;
        self.selection_offsets = None;
        self.commit_transaction(
            vec![EditOp::Delete {
                char_offset: start,
                text: removed,
            }],
            before_cursor,
            before_selection,
        );
        CommandResult::changed()
    }
}

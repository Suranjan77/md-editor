//! Markdown inline formatting wrappers and bracket/quote auto-pairing on `DocBuffer`.

use super::{CommandResult, DocBuffer, EditOp, Movement, Selection};

impl DocBuffer {
    pub(super) fn wrap_selection_or_insert(
        &mut self,
        prefix: &str,
        suffix: &str,
        placeholder: &str,
    ) -> CommandResult {
        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        let mut ops = Vec::new();

        if let Some(selection) = self.selection_offsets {
            let (start, end) = selection.range();
            self.apply_selection_wrap(start, end, prefix, suffix, &mut ops);
        } else {
            let text = format!("{prefix}{placeholder}{suffix}");
            self.rope.insert(self.cursor_offset, &text);
            ops.push(EditOp::Insert {
                char_offset: self.cursor_offset,
                text,
            });
            let start = self.cursor_offset + prefix.chars().count();
            let end = start + placeholder.chars().count();
            self.cursor_offset = end;
            self.selection_offsets = Selection::new(start, end);
        }

        self.commit_transaction(ops, before_cursor, before_selection);
        CommandResult::changed()
    }

    fn apply_selection_wrap(
        &mut self,
        start: usize,
        end: usize,
        prefix: &str,
        suffix: &str,
        ops: &mut Vec<EditOp>,
    ) {
        let prefix_len = prefix.chars().count();
        let suffix_len = suffix.chars().count();

        let has_inner = (end - start >= prefix_len + suffix_len) && {
            let text = self.rope.slice(start..end).to_string();
            text.starts_with(prefix) && text.ends_with(suffix)
        };
        let has_outer = start >= prefix_len && end + suffix_len <= self.rope.len_chars() && {
            let before = self.rope.slice(start - prefix_len..start).to_string();
            let after = self.rope.slice(end..end + suffix_len).to_string();
            before == prefix && after == suffix
        };

        if has_inner {
            let suffix_start = end - suffix_len;
            let suffix_text = self.rope.slice(suffix_start..end).to_string();
            self.rope.remove(suffix_start..end);
            ops.push(EditOp::Delete {
                char_offset: suffix_start,
                text: suffix_text,
            });

            let prefix_end = start + prefix_len;
            let prefix_text = self.rope.slice(start..prefix_end).to_string();
            self.rope.remove(start..prefix_end);
            ops.push(EditOp::Delete {
                char_offset: start,
                text: prefix_text,
            });

            self.cursor_offset = suffix_start - prefix_len;
            self.selection_offsets = Selection::new(start, suffix_start - prefix_len);
        } else if has_outer {
            let suffix_end = end + suffix_len;
            let suffix_text = self.rope.slice(end..suffix_end).to_string();
            self.rope.remove(end..suffix_end);
            ops.push(EditOp::Delete {
                char_offset: end,
                text: suffix_text,
            });

            let prefix_start = start - prefix_len;
            let prefix_text = self.rope.slice(prefix_start..start).to_string();
            self.rope.remove(prefix_start..start);
            ops.push(EditOp::Delete {
                char_offset: prefix_start,
                text: prefix_text,
            });

            self.cursor_offset = end - prefix_len;
            self.selection_offsets = Selection::new(start - prefix_len, end - prefix_len);
        } else {
            self.rope.insert(end, suffix);
            ops.push(EditOp::Insert {
                char_offset: end,
                text: suffix.to_string(),
            });
            self.rope.insert(start, prefix);
            ops.push(EditOp::Insert {
                char_offset: start,
                text: prefix.to_string(),
            });
            self.cursor_offset = end + prefix_len + suffix_len;
            self.selection_offsets = Selection::new(start + prefix_len, end + prefix_len);
        }
    }

    pub(super) fn type_paired(&mut self, c: char) -> CommandResult {
        const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];

        if let Some(&(open, close)) = PAIRS.iter().find(|&&(o, _)| o == c) {
            if self.selection_offsets.is_some() {
                return self.wrap_selection_or_insert(&open.to_string(), &close.to_string(), "");
            }
            return self.insert_pair_empty(open, close);
        }

        if let Some(&(_, close)) = PAIRS.iter().find(|&&(_, cl)| cl == c) {
            if self.selection_offsets.is_none() && self.char_at(self.cursor_offset) == Some(close) {
                self.move_cursor(Movement::Right, false);
                return CommandResult::default();
            }
            return self.insert_text(&c.to_string());
        }

        if matches!(c, '"' | '\'' | '`') {
            if self.selection_offsets.is_some() {
                return self.wrap_selection_or_insert(&c.to_string(), &c.to_string(), "");
            }
            if self.char_at(self.cursor_offset) == Some(c) {
                self.move_cursor(Movement::Right, false);
                return CommandResult::default();
            }
            let prev = self
                .cursor_offset
                .checked_sub(1)
                .and_then(|i| self.char_at(i));
            if prev.is_some_and(|p| p.is_alphanumeric()) {
                return self.insert_text(&c.to_string());
            }
            return self.insert_pair_empty(c, c);
        }

        self.insert_text(&c.to_string())
    }

    fn insert_pair_empty(&mut self, open: char, close: char) -> CommandResult {
        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        let insert_at = self.cursor_offset;
        let text = format!("{open}{close}");
        self.rope.insert(insert_at, &text);
        let ops = vec![EditOp::Insert {
            char_offset: insert_at,
            text,
        }];
        self.cursor_offset = insert_at + 1;
        self.selection_offsets = None;
        self.commit_transaction(ops, before_cursor, before_selection);
        CommandResult::changed()
    }
}

//! Word-wise motion and deletion, word/line selection, and block indent/outdent on `DocBuffer`.

use super::{CommandResult, DocBuffer, EditOp, Selection};

/// Spaces one indent level inserts (Tab) or removes (Shift+Tab).
const INDENT: &str = "    ";

#[cfg(test)]
mod tests;

#[derive(PartialEq, Eq, Clone, Copy)]
enum Class {
    Space,
    Word,
    Punct,
}

fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Punct
    }
}

impl DocBuffer {
    /// Start of the word before `offset`: skip whitespace (line breaks
    /// included), then one run of word characters or of punctuation.
    pub(super) fn word_start_before(&self, offset: usize) -> usize {
        let mut i = offset.min(self.rope.len_chars());
        while i > 0 && class(self.rope.char(i - 1)) == Class::Space {
            i -= 1;
        }
        if i > 0 {
            let run = class(self.rope.char(i - 1));
            while i > 0 && class(self.rope.char(i - 1)) == run {
                i -= 1;
            }
        }
        self.snap(i)
    }

    /// End of the word after `offset`, mirroring [`Self::word_start_before`].
    pub(super) fn word_end_after(&self, offset: usize) -> usize {
        let len = self.rope.len_chars();
        let mut i = offset.min(len);
        while i < len && class(self.rope.char(i)) == Class::Space {
            i += 1;
        }
        if i < len {
            let run = class(self.rope.char(i));
            while i < len && class(self.rope.char(i)) == run {
                i += 1;
            }
        }
        self.snap(i)
    }

    /// Nearest caret stop at or before `offset` (never inside a grapheme).
    fn snap(&self, offset: usize) -> usize {
        let (line, col) = self.offset_to_line_col(offset);
        self.caret_stop(line, col)
    }

    /// Ctrl+Backspace / Ctrl+Delete: delete the selection, or to the word
    /// boundary. Always its own undo step.
    pub(super) fn delete_word(&mut self, forward: bool) -> CommandResult {
        if self.selection_offsets.is_some() {
            return self.delete_selection();
        }
        let (start, end) = if forward {
            (self.cursor_offset, self.word_end_after(self.cursor_offset))
        } else {
            (
                self.word_start_before(self.cursor_offset),
                self.cursor_offset,
            )
        };
        if start == end {
            return CommandResult::default();
        }
        self.last_commit_at = None;
        let result = self.delete_range(start, end);
        self.last_commit_at = None;
        self.sync_public_state();
        result
    }

    /// Double-click: select the word (or run of punctuation or spaces) at
    /// the click.
    pub(super) fn select_word_at(&mut self, line: usize, col: usize) {
        let at = self.caret_stop(line, col);
        let line_start = self.rope.line_to_char(self.rope.char_to_line(at));
        let line_end = line_start + self.line_text(self.rope.char_to_line(at)).chars().count();
        let probe = if at < line_end {
            at
        } else {
            at.saturating_sub(1)
        };
        if probe < line_start || probe >= line_end {
            self.set_cursor_offset(at);
            return;
        }
        let run = class(self.rope.char(probe));
        let mut start = probe;
        while start > line_start && class(self.rope.char(start - 1)) == run {
            start -= 1;
        }
        let mut end = probe + 1;
        while end < line_end && class(self.rope.char(end)) == run {
            end += 1;
        }
        self.select_range(self.snap(start), self.snap(end));
    }

    /// Triple-click: select the whole line, its line break included.
    pub(super) fn select_line(&mut self, line: usize) {
        let line = line.min(self.line_count().saturating_sub(1));
        let start = self.rope.line_to_char(line);
        let end = if line + 1 < self.line_count() {
            self.rope.line_to_char(line + 1)
        } else {
            self.rope.len_chars()
        };
        self.select_range(start, end);
    }

    fn select_range(&mut self, anchor: usize, focus: usize) {
        self.cursor_offset = focus;
        self.selection_offsets = Selection::new(anchor, focus);
        self.desired_col = None;
        self.sync_public_state();
    }

    /// Lines the selection (or the caret) touches. A selection ending at the
    /// very start of a line does not take that line along.
    fn touched_lines(&self) -> (usize, usize) {
        let (start, end) = self
            .selection_offsets
            .map_or((self.cursor_offset, self.cursor_offset), Selection::range);
        let first = self.rope.char_to_line(start);
        let mut last = self.rope.char_to_line(end);
        if last > first && self.rope.line_to_char(last) == end {
            last -= 1;
        }
        (first, last)
    }

    /// Tab with a selection across lines: indent every non-blank line.
    pub(super) fn indent_lines(&mut self) -> CommandResult {
        let (first, last) = self.touched_lines();
        let ops = (first..=last)
            .rev()
            .filter(|&line| !self.line_text(line).trim().is_empty())
            .map(|line| EditOp::Insert {
                char_offset: self.rope.line_to_char(line),
                text: INDENT.to_string(),
            })
            .collect();
        self.apply_line_ops(ops)
    }

    /// Shift+Tab: remove one indent level (up to four spaces, or a tab) from
    /// every touched line.
    pub(super) fn outdent_lines(&mut self) -> CommandResult {
        let (first, last) = self.touched_lines();
        let ops = (first..=last)
            .rev()
            .filter_map(|line| {
                let text = self.line_text(line);
                let width = if text.starts_with('\t') {
                    1
                } else {
                    text.chars()
                        .take(INDENT.len())
                        .take_while(|c| *c == ' ')
                        .count()
                };
                (width > 0).then(|| EditOp::Delete {
                    char_offset: self.rope.line_to_char(line),
                    text: text.chars().take(width).collect(),
                })
            })
            .collect();
        self.apply_line_ops(ops)
    }

    /// Apply per-line edits (given bottom line first, so earlier offsets stay
    /// valid) as one undo step, carrying the caret and selection along.
    fn apply_line_ops(&mut self, ops: Vec<EditOp>) -> CommandResult {
        if ops.is_empty() {
            return CommandResult::default();
        }
        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        let shift = |offset: usize| -> usize {
            ops.iter().fold(offset as isize, |acc, op| match op {
                EditOp::Insert { char_offset, text } if *char_offset <= offset => {
                    acc + text.chars().count() as isize
                }
                EditOp::Delete { char_offset, text } if *char_offset < offset => {
                    let gone = text.chars().count().min(offset - char_offset);
                    acc - gone as isize
                }
                _ => acc,
            }) as usize
        };
        let cursor = shift(before_cursor);
        let selection =
            before_selection.and_then(|s| Selection::new(shift(s.anchor), shift(s.focus)));
        for op in &ops {
            self.apply_op(op);
        }
        self.cursor_offset = cursor.min(self.rope.len_chars());
        self.selection_offsets = selection;
        self.last_commit_at = None;
        self.commit_transaction(ops, before_cursor, before_selection);
        self.last_commit_at = None;
        CommandResult::changed()
    }
}

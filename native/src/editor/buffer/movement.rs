//! Caret placement, selection bounds and grapheme-aware cursor movement on `DocBuffer`.

use super::{DocBuffer, Movement, Selection};

impl DocBuffer {
    pub fn move_cursor_left(&mut self) {
        self.move_cursor(Movement::Left, false);
    }

    pub fn move_cursor_right(&mut self) {
        self.move_cursor(Movement::Right, false);
    }

    pub fn move_cursor_up(&mut self) {
        self.move_cursor(Movement::Up, false);
    }

    pub fn move_cursor_down(&mut self) {
        self.move_cursor(Movement::Down, false);
    }

    pub fn move_cursor_home(&mut self) {
        self.move_cursor(Movement::Home, false);
    }

    pub fn move_cursor_end(&mut self) {
        self.move_cursor(Movement::End, false);
    }

    pub fn set_cursor(&mut self, line: usize, col: usize) {
        self.cursor_offset = self.caret_stop(line, col);
        self.selection_offsets = None;
        self.desired_col = None;
        self.sync_public_state();
    }

    /// Place the cursor at a raw character offset, clamped to the document.
    pub fn set_cursor_offset(&mut self, offset: usize) {
        self.cursor_offset = offset.min(self.rope.len_chars());
        self.selection_offsets = None;
        self.desired_col = None;
        self.sync_public_state();
    }

    pub fn set_selection(
        &mut self,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) {
        let anchor = self.caret_stop(start_line, start_col);
        let focus = self.caret_stop(end_line, end_col);
        self.cursor_offset = focus;
        self.selection_offsets = Selection::new(anchor, focus);
        self.desired_col = None;
        self.sync_public_state();
    }

    pub fn select_all(&mut self) {
        self.cursor_offset = self.rope.len_chars();
        self.selection_offsets = Selection::new(0, self.cursor_offset);
        self.desired_col = None;
        self.sync_public_state();
    }

    pub(super) fn grapheme_before(&self, offset: usize) -> usize {
        let (line, col) = self.offset_to_line_col(offset);
        if col == 0 {
            return match line.checked_sub(1) {
                Some(prev) => self.line_col_to_offset(prev, usize::MAX),
                None => 0,
            };
        }
        self.rope.line_to_char(line) + grapheme_boundary_before(&self.line_text(line), col)
    }

    pub(super) fn grapheme_after(&self, offset: usize) -> usize {
        let (line, col) = self.offset_to_line_col(offset);
        let text = self.line_text(line);
        if col >= text.chars().count() {
            return if line + 1 < self.line_count() {
                self.rope.line_to_char(line + 1)
            } else {
                self.rope.len_chars()
            };
        }
        self.rope.line_to_char(line) + grapheme_boundary_after(&text, col)
    }

    pub(super) fn caret_stop(&self, line: usize, col: usize) -> usize {
        let offset = self.line_col_to_offset(line, col);
        let (line, col) = self.offset_to_line_col(offset);
        self.rope.line_to_char(line) + grapheme_boundary_at_or_before(&self.line_text(line), col)
    }

    pub(super) fn move_cursor(&mut self, movement: Movement, extend: bool) {
        let anchor = if extend {
            self.selection_offsets
                .map(|selection| selection.anchor)
                .unwrap_or(self.cursor_offset)
        } else {
            self.selection_offsets = None;
            self.cursor_offset
        };

        self.cursor_offset = match movement {
            Movement::Left => self.grapheme_before(self.cursor_offset),
            Movement::Right => self.grapheme_after(self.cursor_offset),
            Movement::Home => {
                self.desired_col = None;
                let line = self
                    .rope
                    .char_to_line(self.cursor_offset.min(self.rope.len_chars()));
                self.rope.line_to_char(line)
            }
            Movement::End => {
                self.desired_col = None;
                let line = self
                    .rope
                    .char_to_line(self.cursor_offset.min(self.rope.len_chars()));
                self.line_col_to_offset(line, self.line_text(line).chars().count())
            }
            Movement::Up => self.vertical_offset(-1),
            Movement::Down => self.vertical_offset(1),
        };

        if extend {
            self.selection_offsets = Selection::new(anchor, self.cursor_offset);
        } else {
            self.selection_offsets = None;
        }

        if !matches!(movement, Movement::Up | Movement::Down) {
            self.desired_col = None;
        }
        self.sync_public_state();
    }

    fn vertical_offset(&mut self, delta: isize) -> usize {
        let (line, col) = self.offset_to_line_col(self.cursor_offset);
        let desired_col = *self.desired_col.get_or_insert(col);
        let max_line = self.line_count().saturating_sub(1);
        let target_line = if delta < 0 {
            line.saturating_sub(delta.unsigned_abs())
        } else {
            (line + delta as usize).min(max_line)
        };
        self.caret_stop(target_line, desired_col)
    }
}

fn grapheme_boundaries(text: &str) -> impl Iterator<Item = usize> + '_ {
    use unicode_segmentation::UnicodeSegmentation;
    std::iter::once(0).chain(text.graphemes(true).scan(0, |chars, cluster| {
        *chars += cluster.chars().count();
        Some(*chars)
    }))
}

fn grapheme_boundary_before(text: &str, col: usize) -> usize {
    grapheme_boundaries(text)
        .take_while(|&boundary| boundary < col)
        .last()
        .unwrap_or(0)
}

fn grapheme_boundary_after(text: &str, col: usize) -> usize {
    grapheme_boundaries(text)
        .find(|&boundary| boundary > col)
        .unwrap_or_else(|| text.chars().count())
}

fn grapheme_boundary_at_or_before(text: &str, col: usize) -> usize {
    grapheme_boundaries(text)
        .take_while(|&boundary| boundary <= col)
        .last()
        .unwrap_or(0)
}

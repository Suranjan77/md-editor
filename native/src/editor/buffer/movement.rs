use super::command::Movement;
use super::document::DocBuffer;
use super::transaction::Selection;
use unicode_segmentation::UnicodeSegmentation;

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
        self.cursor_offset = self.line_col_to_offset(line, col);
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
        let anchor = self.line_col_to_offset(start_line, start_col);
        let focus = self.line_col_to_offset(end_line, end_col);
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
    pub(crate) fn move_cursor(&mut self, movement: Movement, extend: bool) {
        let anchor = if extend {
            self.selection_offsets
                .map(|selection| selection.anchor)
                .unwrap_or(self.cursor_offset)
        } else if let Some(selection) = self.selection_offsets {
            let (start, end) = selection.range();
            match movement {
                Movement::Left => self.cursor_offset = start,
                Movement::Right => self.cursor_offset = end,
                _ => {}
            }
            self.selection_offsets = None;
            if matches!(movement, Movement::Left | Movement::Right) {
                self.desired_col = None;
                self.sync_public_state();
                return;
            }
            self.cursor_offset
        } else {
            self.selection_offsets = None;
            self.cursor_offset
        };

        self.cursor_offset = match movement {
            Movement::Left => self.previous_grapheme_offset(self.cursor_offset),
            Movement::Right => self.next_grapheme_offset(self.cursor_offset),
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
    pub(crate) fn previous_grapheme_offset(&self, offset: usize) -> usize {
        let offset = offset.min(self.rope.len_chars());
        if offset == 0 {
            return 0;
        }

        self.rope
            .slice(..offset)
            .to_string()
            .graphemes(true)
            .last()
            .map(|grapheme| offset - grapheme.chars().count())
            .unwrap_or(0)
    }
    pub(crate) fn next_grapheme_offset(&self, offset: usize) -> usize {
        let offset = offset.min(self.rope.len_chars());
        if offset >= self.rope.len_chars() {
            return self.rope.len_chars();
        }

        self.rope
            .slice(offset..)
            .to_string()
            .graphemes(true)
            .next()
            .map(|grapheme| offset + grapheme.chars().count())
            .unwrap_or(self.rope.len_chars())
            .min(self.rope.len_chars())
    }
    pub(crate) fn vertical_offset(&mut self, delta: isize) -> usize {
        let (line, col) = self.offset_to_line_col(self.cursor_offset);
        let desired_col = *self.desired_col.get_or_insert(col);
        let max_line = self.line_count().saturating_sub(1);
        let target_line = if delta < 0 {
            line.saturating_sub(delta.unsigned_abs())
        } else {
            (line + delta as usize).min(max_line)
        };
        self.line_col_to_offset(target_line, desired_col)
    }
}

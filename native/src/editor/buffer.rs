use ropey::Rope;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EditOp {
    Insert { byte_offset: usize, text: String },
    Delete { byte_offset: usize, text: String },
}

impl EditOp {
    pub fn inverse(&self) -> Self {
        match self {
            EditOp::Insert { byte_offset, text } => EditOp::Delete {
                byte_offset: *byte_offset,
                text: text.clone(),
            },
            EditOp::Delete { byte_offset, text } => EditOp::Insert {
                byte_offset: *byte_offset,
                text: text.clone(),
            },
        }
    }
}

pub struct DocBuffer {
    rope: Rope,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub selection: Option<(usize, usize, usize, usize)>, // (start_line, start_col, end_line, end_col)
    pub dirty: bool,
    undo_stack: Vec<EditOp>,
    redo_stack: Vec<EditOp>,
}

impl DocBuffer {
    pub fn new() -> Self {
        Self {
            rope: Rope::new(),
            cursor_line: 0,
            cursor_col: 0,
            selection: None,
            dirty: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn from_text(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
            cursor_line: 0,
            cursor_col: 0,
            selection: None,
            dirty: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn set_text(&mut self, text: &str) {
        self.rope = Rope::from_str(text);
        self.dirty = true;
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn line_text(&self, line: usize) -> String {
        if line < self.line_count() {
            self.rope
                .line(line)
                .to_string()
                .trim_end_matches(['\n', '\r'])
                .to_string()
        } else {
            String::new()
        }
    }

    pub fn insert_at_cursor(&mut self, text: &str) {
        let char_off = self.cursor_to_offset();
        let byte_off = self.rope.char_to_byte(char_off);

        self.rope.insert(char_off, text);
        self.undo_stack.push(EditOp::Insert {
            byte_offset: byte_off,
            text: text.to_string(),
        });
        self.redo_stack.clear();

        // Update cursor for single-character input and multi-line paste.
        let mut lines = text.split('\n');
        let first = lines.next().unwrap_or_default();
        let mut line_count = 0usize;
        let mut last_len = first.chars().count();
        for line in lines {
            line_count += 1;
            last_len = line.chars().count();
        }

        if line_count > 0 {
            self.cursor_line += line_count;
            self.cursor_col = last_len;
        } else {
            self.cursor_col += text.chars().count();
        }
        self.dirty = true;
    }

    pub fn backspace(&mut self) {
        let char_off = self.cursor_to_offset();
        if char_off > 0 {
            let start = char_off - 1;
            let text = self.rope.slice(start..char_off).to_string();
            let byte_off = self.rope.char_to_byte(start);

            self.rope.remove(start..char_off);
            self.undo_stack.push(EditOp::Delete {
                byte_offset: byte_off,
                text,
            });
            self.redo_stack.clear();

            if self.cursor_col > 0 {
                self.cursor_col -= 1;
            } else if self.cursor_line > 0 {
                self.cursor_line -= 1;
                self.cursor_col = self.line_text(self.cursor_line).chars().count();
            }
            self.dirty = true;
        }
    }

    pub fn delete(&mut self) {
        let char_off = self.cursor_to_offset();
        if char_off < self.rope.len_chars() {
            let end = char_off + 1;
            let text = self.rope.slice(char_off..end).to_string();
            let byte_off = self.rope.char_to_byte(char_off);

            self.rope.remove(char_off..end);
            self.undo_stack.push(EditOp::Delete {
                byte_offset: byte_off,
                text,
            });
            self.redo_stack.clear();
            self.dirty = true;
        }
    }

    fn cursor_to_offset(&self) -> usize {
        let line_start = self.rope.line_to_char(self.cursor_line);
        let line_len = self.rope.line(self.cursor_line).len_chars();
        line_start + self.cursor_col.min(line_len)
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = self.line_text(self.cursor_line).chars().count();
        }
    }

    pub fn move_cursor_right(&mut self) {
        let line_len = self.line_text(self.cursor_line).chars().count();
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        } else if self.cursor_line + 1 < self.line_count() {
            self.cursor_line += 1;
            self.cursor_col = 0;
        }
    }

    pub fn move_cursor_up(&mut self) {
        if self.cursor_line > 0 {
            self.cursor_line -= 1;
            let line_len = self.line_text(self.cursor_line).chars().count();
            self.cursor_col = self.cursor_col.min(line_len);
        }
    }

    pub fn move_cursor_down(&mut self) {
        if self.cursor_line + 1 < self.line_count() {
            self.cursor_line += 1;
            let line_len = self.line_text(self.cursor_line).chars().count();
            self.cursor_col = self.cursor_col.min(line_len);
        }
    }

    pub fn move_cursor_home(&mut self) {
        self.cursor_col = 0;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_col = self.line_text(self.cursor_line).chars().count();
    }

    pub fn undo(&mut self) {
        if let Some(op) = self.undo_stack.pop() {
            let inverse = op.inverse();
            self.apply_op(&inverse);
            self.redo_stack.push(op);
        }
    }

    pub fn redo(&mut self) {
        if let Some(op) = self.redo_stack.pop() {
            self.apply_op(&op);
            self.undo_stack.push(op);
        }
    }

    fn apply_op(&mut self, op: &EditOp) {
        match op {
            EditOp::Insert { byte_offset, text } => {
                let char_off = self.rope.byte_to_char(*byte_offset);
                self.rope.insert(char_off, text);
                self.set_cursor_from_char_offset(char_off + text.chars().count());
            }
            EditOp::Delete { byte_offset, text } => {
                let char_start = self.rope.byte_to_char(*byte_offset);
                let char_end = char_start + text.chars().count();
                self.rope.remove(char_start..char_end);
                self.set_cursor_from_char_offset(char_start);
            }
        }
        self.dirty = true;
    }

    fn set_cursor_from_char_offset(&mut self, char_offset: usize) {
        let clamped = char_offset.min(self.rope.len_chars());
        let line = self.rope.char_to_line(clamped);
        let line_start = self.rope.line_to_char(line);
        self.cursor_line = line;
        self.cursor_col = clamped.saturating_sub(line_start);
    }

    pub fn set_cursor(&mut self, line: usize, col: usize) {
        self.cursor_line = line.min(self.line_count().saturating_sub(1));
        let line_len = self.line_text(self.cursor_line).chars().count();
        self.cursor_col = col.min(line_len);
        self.selection = None;
    }

    pub fn set_selection(
        &mut self,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) {
        self.selection = Some((start_line, start_col, end_line, end_col));
    }
}

#[cfg(test)]
mod tests {
    use crate::editor::buffer::DocBuffer;

    #[test]
    fn test_insert_at_cursor() {
        let mut buffer = DocBuffer::from_text("Hello");
        buffer.cursor_line = 0;
        buffer.cursor_col = 5;
        buffer.insert_at_cursor(" world");
        assert_eq!(buffer.text(), "Hello world");
        assert_eq!(buffer.cursor_col, 11);
    }

    #[test]
    fn test_backspace() {
        let mut buffer = DocBuffer::from_text("Hello");
        buffer.cursor_line = 0;
        buffer.cursor_col = 5;
        buffer.backspace();
        assert_eq!(buffer.text(), "Hell");
        assert_eq!(buffer.cursor_col, 4);
    }

    #[test]
    fn test_newline() {
        let mut buffer = DocBuffer::from_text("Line 1");
        buffer.cursor_line = 0;
        buffer.cursor_col = 6;
        buffer.insert_at_cursor("\n");
        assert_eq!(buffer.text(), "Line 1\n");
        assert_eq!(buffer.cursor_line, 1);
        assert_eq!(buffer.cursor_col, 0);
    }

    #[test]
    fn test_multiline_paste_cursor_position() {
        let mut buffer = DocBuffer::from_text("Hello");
        buffer.cursor_line = 0;
        buffer.cursor_col = 5;
        buffer.insert_at_cursor("\nworld\nagain");
        assert_eq!(buffer.text(), "Hello\nworld\nagain");
        assert_eq!(buffer.cursor_line, 2);
        assert_eq!(buffer.cursor_col, 5);
    }

    #[test]
    fn test_undo_redo_updates_cursor_position() {
        let mut buffer = DocBuffer::from_text("Hello");
        buffer.cursor_line = 0;
        buffer.cursor_col = 5;
        buffer.insert_at_cursor(" world");
        buffer.undo();
        assert_eq!(buffer.text(), "Hello");
        assert_eq!(buffer.cursor_line, 0);
        assert_eq!(buffer.cursor_col, 5);
        buffer.redo();
        assert_eq!(buffer.text(), "Hello world");
        assert_eq!(buffer.cursor_line, 0);
        assert_eq!(buffer.cursor_col, 11);
    }

    #[test]
    fn test_cursor_movement() {
        let mut buffer = DocBuffer::from_text("Line 1\nLine 2");
        buffer.cursor_line = 0;
        buffer.cursor_col = 6;
        buffer.move_cursor_down();
        assert_eq!(buffer.cursor_line, 1);
        assert_eq!(buffer.cursor_col, 6);
        buffer.move_cursor_left();
        assert_eq!(buffer.cursor_col, 5);
        buffer.move_cursor_up();
        assert_eq!(buffer.cursor_line, 0);
        assert_eq!(buffer.cursor_col, 5);
    }

    #[test]
    fn test_buffer_state_machine_fuzzing() {
        let mut buffer = DocBuffer::new();
        let mut seed = 12345u64;
        let mut next_rng = |modulus: usize| -> usize {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            (seed as usize) % modulus
        };

        // We run 1000 sequential actions and verify buffer invariants at every single step!
        for _ in 0..1000 {
            let action = next_rng(10);
            match action {
                0 => {
                    // Insert standard text
                    let words = vec!["hello", "world", "rust", "iced", "buffer", "rope", "🦀", "\n", "\nnew line\n"];
                    let word = words[next_rng(words.len())];
                    buffer.insert_at_cursor(word);
                }
                1 => {
                    // Backspace
                    buffer.backspace();
                }
                2 => {
                    // Delete
                    buffer.delete();
                }
                3 => {
                    // Move cursor left
                    buffer.move_cursor_left();
                }
                4 => {
                    // Move cursor right
                    buffer.move_cursor_right();
                }
                5 => {
                    // Move cursor up
                    buffer.move_cursor_up();
                }
                6 => {
                    // Move cursor down
                    buffer.move_cursor_down();
                }
                7 => {
                    // Move cursor home/end
                    if next_rng(2) == 0 {
                        buffer.move_cursor_home();
                    } else {
                        buffer.move_cursor_end();
                    }
                }
                8 => {
                    // Undo
                    buffer.undo();
                }
                9 => {
                    // Redo
                    buffer.redo();
                }
                _ => unreachable!(),
            }

            // Assert Invariants at each step:
            let line_count = buffer.line_count();
            assert!(line_count > 0, "Buffer line count must always be at least 1");
            
            // Cursor line must always be in-bounds
            assert!(
                buffer.cursor_line < line_count,
                "Cursor line {} out of bounds for line count {}",
                buffer.cursor_line,
                line_count
            );

            // Cursor col must always be in-bounds for the current line
            let current_line_len = buffer.line_text(buffer.cursor_line).chars().count();
            assert!(
                buffer.cursor_col <= current_line_len,
                "Cursor col {} out of bounds for current line len {} (line {})",
                buffer.cursor_col,
                current_line_len,
                buffer.cursor_line
            );
        }
    }

    #[test]
    fn test_bug_finder_utf8_char_boundaries() {
        // Test strings with complex multi-byte characters and ZWJ emojis
        let complex_texts = vec![
            "👨‍👩‍👧‍👦", // ZWJ complex family emoji
            "こんにちは", // Japanese Hiragana
            "𠮷野家", // CJK Extension B
            "Hello 👩‍💻 World", // Mixed ASCII and emoji
            "🏳️‍🌈", // Rainbow flag (ZWJ sequence)
            "\u{0000}\u{0001}\u{0007}", // Control characters / Null bytes
        ];

        for text in complex_texts {
            let mut buffer = DocBuffer::from_text(text);
            
            // Perform deletions from the end to the start
            let initial_char_count = text.chars().count();
            for _ in 0..=initial_char_count + 5 {
                buffer.backspace();
                let current_text = buffer.line_text(0);
                let _char_count = current_text.chars().count();
            }
            
            // Reset buffer
            let mut buffer = DocBuffer::from_text(text);
            buffer.move_cursor_home();
            // Perform delete forward
            for _ in 0..=initial_char_count + 5 {
                buffer.delete();
                let _text = buffer.line_text(0);
            }
        }
    }

    #[test]
    fn test_bug_finder_empty_and_out_of_bounds_states() {
        let mut buffer = DocBuffer::from_text("");
        
        // Assert empty state boundary actions do not panic
        buffer.backspace();
        buffer.delete();
        buffer.move_cursor_left();
        buffer.move_cursor_right();
        buffer.move_cursor_up();
        buffer.move_cursor_down();
        buffer.move_cursor_home();
        buffer.move_cursor_end();
        buffer.undo();
        buffer.redo();
        
        // Assert that the cursor remains strictly at (0, 0)
        assert_eq!(buffer.cursor_line, 0);
        assert_eq!(buffer.cursor_col, 0);
        
        // Assert out of bounds cursor values don't crash
        buffer.cursor_line = 9999;
        buffer.cursor_col = 9999;
        let text = buffer.line_text(buffer.cursor_line);
        assert!(text.is_empty());
    }
}

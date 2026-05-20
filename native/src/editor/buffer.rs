use ropey::Rope;
use serde::{Deserialize, Serialize};

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
    fn inverse(&self) -> Self {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Movement {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

#[derive(Debug, Clone)]
pub enum EditorCommand {
    InsertText(String),
    DeleteSelection,
    DeleteBackward,
    DeleteForward,
    MoveCursor {
        movement: Movement,
        extend: bool,
    },
    SetCursor {
        line: usize,
        col: usize,
    },
    SetSelection {
        anchor_line: usize,
        anchor_col: usize,
        focus_line: usize,
        focus_col: usize,
    },
    SelectAll,
    ToggleCheckbox {
        line: usize,
    },
    FormatBold,
    FormatItalic,
    FormatInlineCode,
    InsertLink,
    Undo,
    Redo,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommandResult {
    pub text_changed: bool,
    pub projection_changed: bool,
    pub media_changed: bool,
}

impl CommandResult {
    fn changed() -> Self {
        Self {
            text_changed: true,
            projection_changed: true,
            media_changed: true,
        }
    }
}

pub struct DocBuffer {
    rope: Rope,
    cursor_offset: usize,
    selection_offsets: Option<Selection>,
    desired_col: Option<usize>,

    pub cursor_line: usize,
    pub cursor_col: usize,
    pub selection: Option<(usize, usize, usize, usize)>,
    pub dirty: bool,

    undo_stack: Vec<EditTransaction>,
    redo_stack: Vec<EditTransaction>,
}

impl DocBuffer {
    pub fn new() -> Self {
        Self::from_text("")
    }

    pub fn from_text(text: &str) -> Self {
        let mut buffer = Self {
            rope: Rope::from_str(text),
            cursor_offset: 0,
            selection_offsets: None,
            desired_col: None,
            cursor_line: 0,
            cursor_col: 0,
            selection: None,
            dirty: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        };
        buffer.sync_public_state();
        buffer
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn set_text(&mut self, text: &str) {
        self.rope = Rope::from_str(text);
        self.cursor_offset = self.cursor_offset.min(self.rope.len_chars());
        self.selection_offsets = None;
        self.desired_col = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.dirty = true;
        self.sync_public_state();
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

    pub fn cursor_offset(&self) -> usize {
        self.cursor_offset
    }

    pub fn selection_offsets(&self) -> Option<Selection> {
        self.selection_offsets
    }

    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection_offsets?.range();
        Some(self.rope.slice(start..end).to_string())
    }

    pub fn execute(&mut self, command: EditorCommand) -> CommandResult {
        match command {
            EditorCommand::InsertText(text) => self.insert_text(&text),
            EditorCommand::DeleteSelection => self.delete_selection(),
            EditorCommand::DeleteBackward => self.delete_backward(),
            EditorCommand::DeleteForward => self.delete_forward(),
            EditorCommand::MoveCursor { movement, extend } => {
                self.move_cursor(movement, extend);
                CommandResult::default()
            }
            EditorCommand::SetCursor { line, col } => {
                self.set_cursor(line, col);
                CommandResult::default()
            }
            EditorCommand::SetSelection {
                anchor_line,
                anchor_col,
                focus_line,
                focus_col,
            } => {
                self.set_selection(anchor_line, anchor_col, focus_line, focus_col);
                CommandResult::default()
            }
            EditorCommand::SelectAll => {
                self.select_all();
                CommandResult::default()
            }
            EditorCommand::ToggleCheckbox { line } => self.toggle_checkbox(line),
            EditorCommand::FormatBold => self.wrap_selection_or_insert("**", "**", "bold"),
            EditorCommand::FormatItalic => self.wrap_selection_or_insert("*", "*", "italic"),
            EditorCommand::FormatInlineCode => self.wrap_selection_or_insert("`", "`", "code"),
            EditorCommand::InsertLink => self.wrap_selection_or_insert("[", "](url)", "link"),
            EditorCommand::Undo => {
                if self.undo() {
                    CommandResult::changed()
                } else {
                    CommandResult::default()
                }
            }
            EditorCommand::Redo => {
                if self.redo() {
                    CommandResult::changed()
                } else {
                    CommandResult::default()
                }
            }
        }
    }

    pub fn insert_at_cursor(&mut self, text: &str) {
        self.insert_text(text);
    }

    pub fn backspace(&mut self) {
        self.delete_backward();
    }

    pub fn delete(&mut self) {
        self.delete_forward();
    }

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

    fn insert_text(&mut self, text: &str) -> CommandResult {
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
            let line_idx = self.rope.char_to_line(insert_at);
            let line_text = self.line_text(line_idx);
            if let Some(list_item) = parse_list_item(&line_text) {
                let line_start = self.rope.line_to_char(line_idx);
                let marker_end = line_start + list_item.indent.chars().count() + list_item.marker.chars().count();
                if insert_at >= marker_end {
                    if list_item.is_empty {
                        // Empty list item: clear the prefix on the current line and do NOT insert newline!
                        let marker_start = line_start + list_item.indent.chars().count();
                        let marker_len = list_item.marker.chars().count();
                        let marker_text = self.rope.slice(marker_start..marker_start + marker_len).to_string();
                        self.rope.remove(marker_start..marker_start + marker_len);
                        ops.push(EditOp::Delete {
                            char_offset: marker_start,
                            text: marker_text,
                        });
                        
                        self.cursor_offset = marker_start;
                        self.selection_offsets = None;
                        self.commit_transaction(ops, before_cursor, before_selection);
                        return CommandResult::changed();
                    } else {
                        // Non-empty list item: auto-continue on next line!
                        let next_prefix = format!("{}{}", list_item.indent, list_item.next_marker);
                        text_to_insert = format!("\n{}", next_prefix);
                    }
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

    fn delete_selection(&mut self) -> CommandResult {
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

    fn delete_backward(&mut self) -> CommandResult {
        if self.selection_offsets.is_some() {
            return self.delete_selection();
        }
        if self.cursor_offset == 0 {
            return CommandResult::default();
        }
        self.delete_range(self.cursor_offset - 1, self.cursor_offset)
    }

    fn delete_forward(&mut self) -> CommandResult {
        if self.selection_offsets.is_some() {
            return self.delete_selection();
        }
        if self.cursor_offset >= self.rope.len_chars() {
            return CommandResult::default();
        }
        self.delete_range(self.cursor_offset, self.cursor_offset + 1)
    }

    fn delete_range(&mut self, start: usize, end: usize) -> CommandResult {
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

    fn move_cursor(&mut self, movement: Movement, extend: bool) {
        let anchor = if extend {
            self.selection_offsets
                .map(|selection| selection.anchor)
                .unwrap_or(self.cursor_offset)
        } else {
            self.selection_offsets = None;
            self.cursor_offset
        };

        self.cursor_offset = match movement {
            Movement::Left => self.cursor_offset.saturating_sub(1),
            Movement::Right => (self.cursor_offset + 1).min(self.rope.len_chars()),
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
        self.line_col_to_offset(target_line, desired_col)
    }

    fn toggle_checkbox(&mut self, line: usize) -> CommandResult {
        let line_text = self.line_text(line);
        let indent_chars = line_text
            .chars()
            .take_while(|ch| ch.is_whitespace())
            .count();
        let marker_start = self.line_col_to_offset(line, indent_chars);
        let marker = line_text
            .chars()
            .skip(indent_chars)
            .take(5)
            .collect::<String>();
        let replacement = match marker.as_str() {
            "- [ ]" => "- [x]",
            "- [x]" | "- [X]" => "- [ ]",
            _ => return CommandResult::default(),
        };

        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        self.rope.remove(marker_start..marker_start + 5);
        self.rope.insert(marker_start, replacement);
        self.commit_transaction(
            vec![
                EditOp::Delete {
                    char_offset: marker_start,
                    text: marker,
                },
                EditOp::Insert {
                    char_offset: marker_start,
                    text: replacement.to_string(),
                },
            ],
            before_cursor,
            before_selection,
        );
        CommandResult::changed()
    }

    fn wrap_selection_or_insert(
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
            
            let has_inner_formatting = (end - start >= prefix.chars().count() + suffix.chars().count())
                && {
                    let text = self.rope.slice(start..end).to_string();
                    text.starts_with(prefix) && text.ends_with(suffix)
                };

            let has_outer_formatting = start >= prefix.chars().count()
                && end + suffix.chars().count() <= self.rope.len_chars()
                && {
                    let before = self.rope.slice(start - prefix.chars().count()..start).to_string();
                    let after = self.rope.slice(end..end + suffix.chars().count()).to_string();
                    before == prefix && after == suffix
                };

            if has_inner_formatting {
                // Delete suffix first (at end - suffix.chars().count()..end)
                let suffix_start = end - suffix.chars().count();
                let suffix_text = self.rope.slice(suffix_start..end).to_string();
                self.rope.remove(suffix_start..end);
                ops.push(EditOp::Delete {
                    char_offset: suffix_start,
                    text: suffix_text,
                });
                
                // Delete prefix (at start..start + prefix.chars().count())
                let prefix_end = start + prefix.chars().count();
                let prefix_text = self.rope.slice(start..prefix_end).to_string();
                self.rope.remove(start..prefix_end);
                ops.push(EditOp::Delete {
                    char_offset: start,
                    text: prefix_text,
                });
                
                self.cursor_offset = suffix_start - prefix.chars().count();
                self.selection_offsets = Selection::new(start, suffix_start - prefix.chars().count());
            } else if has_outer_formatting {
                // Delete suffix (at end..end + suffix.chars().count())
                let suffix_end = end + suffix.chars().count();
                let suffix_text = self.rope.slice(end..suffix_end).to_string();
                self.rope.remove(end..suffix_end);
                ops.push(EditOp::Delete {
                    char_offset: end,
                    text: suffix_text,
                });
                
                // Delete prefix (at start - prefix.chars().count()..start)
                let prefix_start = start - prefix.chars().count();
                let prefix_text = self.rope.slice(prefix_start..start).to_string();
                self.rope.remove(prefix_start..start);
                ops.push(EditOp::Delete {
                    char_offset: prefix_start,
                    text: prefix_text,
                });
                
                self.cursor_offset = end - prefix.chars().count();
                self.selection_offsets = Selection::new(start - prefix.chars().count(), end - prefix.chars().count());
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
                self.cursor_offset = end + prefix.chars().count() + suffix.chars().count();
                self.selection_offsets =
                    Selection::new(start + prefix.chars().count(), end + prefix.chars().count());
            }
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

    fn commit_transaction(
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
        self.redo_stack.clear();
        self.sync_public_state();
    }

    fn apply_op(&mut self, op: &EditOp) {
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

    fn line_col_to_offset(&self, line: usize, col: usize) -> usize {
        let max_line = self.line_count().saturating_sub(1);
        let line = line.min(max_line);
        let line_start = self.rope.line_to_char(line);
        let line_len = self.line_text(line).chars().count();
        line_start + col.min(line_len)
    }

    fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        let clamped = offset.min(self.rope.len_chars());
        let line = self.rope.char_to_line(clamped);
        let line_start = self.rope.line_to_char(line);
        let col = clamped.saturating_sub(line_start);
        (line, col.min(self.line_text(line).chars().count()))
    }

    fn sync_public_state(&mut self) {
        self.cursor_offset = self.cursor_offset.min(self.rope.len_chars());
        let (line, col) = self.offset_to_line_col(self.cursor_offset);
        self.cursor_line = line;
        self.cursor_col = col;
        self.selection = self.selection_offsets.map(|selection| {
            let (start, end) = selection.range();
            let (start_line, start_col) = self.offset_to_line_col(start);
            let (end_line, end_col) = self.offset_to_line_col(end);
            (start_line, start_col, end_line, end_col)
        });
    }
}

struct ListItem {
    indent: String,
    marker: String,
    next_marker: String,
    is_empty: bool,
}

fn parse_list_item(line_text: &str) -> Option<ListItem> {
    // Find leading whitespace (indentation)
    let mut indent_len = 0;
    for c in line_text.chars() {
        if c.is_whitespace() && c != '\n' && c != '\r' {
            indent_len += c.len_utf8();
        } else {
            break;
        }
    }
    let indent = line_text[..indent_len].to_string();
    let rest = line_text[indent_len..].trim_end_matches(&['\r', '\n'][..]);
    
    // Check Checklist: e.g. "- [ ] ", "* [ ] ", "- [x] ", etc.
    if (rest.starts_with("- [") || rest.starts_with("* [") || rest.starts_with("+ ["))
        && rest.len() >= 5
    {
        let has_space_after = rest.len() >= 6 && &rest[5..6] == " ";
        let bracket_end = rest.find(']');
        if bracket_end == Some(4) {
            let box_char = &rest[3..4];
            if box_char == " " || box_char == "x" || box_char == "X" {
                let marker_len = if has_space_after { 6 } else { 5 };
                let marker = rest[..marker_len].to_string();
                let content = &rest[marker_len..];
                let is_empty = content.trim().is_empty();
                let bullet = &rest[..1];
                let next_marker = format!("{} [ ] ", bullet);
                return Some(ListItem {
                    indent,
                    marker,
                    next_marker,
                    is_empty,
                });
            }
        }
    }
    
    // Check Unordered List: e.g. "- ", "* ", "+ " (or just "-", "*", "+" at the end of line)
    if rest == "-" || rest == "*" || rest == "+" {
        return Some(ListItem {
            indent,
            marker: rest.to_string(),
            next_marker: format!("{} ", rest),
            is_empty: true,
        });
    }
    if rest.starts_with("- ") || rest.starts_with("* ") || rest.starts_with("+ ") {
        let marker = rest[..2].to_string();
        let content = &rest[2..];
        let is_empty = content.trim().is_empty();
        return Some(ListItem {
            indent,
            marker: marker.clone(),
            next_marker: marker,
            is_empty,
        });
    }
    
    // Check ordered list: e.g. "1. ", "123. " or just "1.", "123." at the end of line
    let mut dot_idx = None;
    for (idx, c) in rest.char_indices() {
        if c.is_ascii_digit() {
            continue;
        } else if c == '.' {
            dot_idx = Some(idx);
            break;
        } else {
            break;
        }
    }
    if let Some(dot) = dot_idx {
        if dot > 0 {
            let is_at_end = rest.len() == dot + 1;
            let has_space_after = rest.len() >= dot + 2 && &rest[dot + 1..dot + 2] == " ";
            if is_at_end || has_space_after {
                let marker_len = if has_space_after { dot + 2 } else { dot + 1 };
                let marker = rest[..marker_len].to_string();
                let content = &rest[marker_len..];
                let is_empty = content.trim().is_empty();
                if let Ok(num) = rest[..dot].parse::<usize>() {
                    let next_marker = format!("{}. ", num + 1);
                    return Some(ListItem {
                        indent,
                        marker,
                        next_marker,
                        is_empty,
                    });
                }
            }
        }
    }
    
    None
}

#[cfg(test)]
mod tests {
    use crate::editor::buffer::{DocBuffer, EditorCommand, Movement};

    #[test]
    fn inserts_at_cursor_and_updates_offset_cursor() {
        let mut buffer = DocBuffer::from_text("Hello");
        buffer.set_cursor(0, 5);
        buffer.insert_at_cursor(" world");
        assert_eq!(buffer.text(), "Hello world");
        assert_eq!(buffer.cursor_offset(), 11);
        assert_eq!((buffer.cursor_line, buffer.cursor_col), (0, 11));
    }

    #[test]
    fn handles_multiline_insert_cursor_position() {
        let mut buffer = DocBuffer::from_text("Hello");
        buffer.set_cursor(0, 5);
        buffer.insert_at_cursor("\nworld\nagain");
        assert_eq!(buffer.text(), "Hello\nworld\nagain");
        assert_eq!((buffer.cursor_line, buffer.cursor_col), (2, 5));
    }

    #[test]
    fn replaces_multiline_selection() {
        let mut buffer = DocBuffer::from_text("one\ntwo\nthree");
        buffer.set_selection(0, 2, 2, 2);
        buffer.insert_at_cursor("X");
        assert_eq!(buffer.text(), "onXree");
        assert_eq!((buffer.cursor_line, buffer.cursor_col), (0, 3));
        assert_eq!(buffer.selection_offsets(), None);
    }

    #[test]
    fn deletes_selection_with_backspace_and_delete() {
        let mut buffer = DocBuffer::from_text("abc\ndef");
        buffer.set_selection(0, 1, 1, 2);
        buffer.backspace();
        assert_eq!(buffer.text(), "af");
        assert_eq!((buffer.cursor_line, buffer.cursor_col), (0, 1));

        buffer.set_selection(0, 0, 0, 2);
        buffer.delete();
        assert_eq!(buffer.text(), "");
        assert_eq!((buffer.cursor_line, buffer.cursor_col), (0, 0));
    }

    #[test]
    fn undo_redo_restores_replacement_transaction() {
        let mut buffer = DocBuffer::from_text("Hello brave world");
        buffer.set_selection(0, 6, 0, 11);
        buffer.insert_at_cursor("small");
        assert_eq!(buffer.text(), "Hello small world");

        assert!(buffer.undo());
        assert_eq!(buffer.text(), "Hello brave world");
        assert_eq!(buffer.selection, Some((0, 6, 0, 11)));

        assert!(buffer.redo());
        assert_eq!(buffer.text(), "Hello small world");
        assert_eq!((buffer.cursor_line, buffer.cursor_col), (0, 11));
        assert_eq!(buffer.selection, None);
    }

    #[test]
    fn vertical_movement_preserves_desired_column() {
        let mut buffer = DocBuffer::from_text("abcdef\nxy\n123456");
        buffer.set_cursor(0, 5);
        buffer.move_cursor_down();
        assert_eq!((buffer.cursor_line, buffer.cursor_col), (1, 2));
        buffer.move_cursor_down();
        assert_eq!((buffer.cursor_line, buffer.cursor_col), (2, 5));
    }

    #[test]
    fn movement_can_extend_selection() {
        let mut buffer = DocBuffer::from_text("abc");
        buffer.set_cursor(0, 1);
        buffer.execute(EditorCommand::MoveCursor {
            movement: Movement::Right,
            extend: true,
        });
        assert_eq!(buffer.selection, Some((0, 1, 0, 2)));
        assert_eq!(buffer.selected_text().as_deref(), Some("b"));
    }

    #[test]
    fn select_all_and_replace() {
        let mut buffer = DocBuffer::from_text("a\nb\nc");
        buffer.execute(EditorCommand::SelectAll);
        assert_eq!(buffer.selected_text().as_deref(), Some("a\nb\nc"));
        buffer.execute(EditorCommand::InsertText("z".to_string()));
        assert_eq!(buffer.text(), "z");
    }

    #[test]
    fn toggles_task_checkbox_as_transaction() {
        let mut buffer = DocBuffer::from_text("- [ ] item\n  - [X] nested");
        buffer.execute(EditorCommand::ToggleCheckbox { line: 0 });
        buffer.execute(EditorCommand::ToggleCheckbox { line: 1 });
        assert_eq!(buffer.text(), "- [x] item\n  - [ ] nested");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "- [x] item\n  - [X] nested");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "- [ ] item\n  - [X] nested");
    }

    #[test]
    fn formatting_wraps_selection_and_preserves_inner_selection() {
        let mut buffer = DocBuffer::from_text("make this bold");
        buffer.set_selection(0, 5, 0, 9);
        buffer.execute(EditorCommand::FormatBold);

        assert_eq!(buffer.text(), "make **this** bold");
        assert_eq!(buffer.selected_text().as_deref(), Some("this"));
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "make this bold");
        assert_eq!(buffer.selection, Some((0, 5, 0, 9)));
        assert!(buffer.redo());
        assert_eq!(buffer.text(), "make **this** bold");
    }

    #[test]
    fn formatting_without_selection_inserts_editable_placeholder() {
        let mut buffer = DocBuffer::from_text("prefix ");
        buffer.set_cursor(0, 7);

        buffer.execute(EditorCommand::FormatItalic);
        assert_eq!(buffer.text(), "prefix *italic*");
        assert_eq!(buffer.selected_text().as_deref(), Some("italic"));

        buffer.execute(EditorCommand::InsertText("emphasis".to_string()));
        assert_eq!(buffer.text(), "prefix *emphasis*");
    }

    #[test]
    fn inline_code_and_link_shortcuts_are_transactions() {
        let mut buffer = DocBuffer::from_text("x and y");
        buffer.set_selection(0, 0, 0, 1);
        buffer.execute(EditorCommand::FormatInlineCode);
        assert_eq!(buffer.text(), "`x` and y");

        buffer.set_selection(0, 8, 0, 9);
        buffer.execute(EditorCommand::InsertLink);
        assert_eq!(buffer.text(), "`x` and [y](url)");

        assert!(buffer.undo());
        assert_eq!(buffer.text(), "`x` and y");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "x and y");
    }

    #[test]
    fn selection_drag_direction_does_not_affect_replacement() {
        let mut buffer = DocBuffer::from_text("alpha beta gamma");
        buffer.set_selection(0, 10, 0, 6);
        assert_eq!(buffer.selected_text().as_deref(), Some("beta"));
        buffer.execute(EditorCommand::InsertText("B".to_string()));
        assert_eq!(buffer.text(), "alpha B gamma");
        assert_eq!((buffer.cursor_line, buffer.cursor_col), (0, 7));
    }

    #[test]
    fn multi_line_select_all_cut_undo_redo_round_trip() {
        let original = "one\n\nthree\n四";
        let mut buffer = DocBuffer::from_text(original);
        buffer.execute(EditorCommand::SelectAll);
        assert_eq!(
            buffer.execute(EditorCommand::DeleteSelection).text_changed,
            true
        );
        assert_eq!(buffer.text(), "");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), original);
        assert_eq!(buffer.selection, Some((0, 0, 3, 1)));
        assert!(buffer.redo());
        assert_eq!(buffer.text(), "");
    }

    #[test]
    fn command_result_reports_no_change_for_boundaries() {
        let mut buffer = DocBuffer::from_text("");
        assert!(!buffer.execute(EditorCommand::DeleteBackward).text_changed);
        assert!(!buffer.execute(EditorCommand::DeleteForward).text_changed);
        assert!(!buffer.execute(EditorCommand::Undo).text_changed);
        assert!(!buffer.execute(EditorCommand::Redo).text_changed);
    }

    #[test]
    fn deterministic_editing_stress_keeps_cursor_and_selection_valid() {
        let mut buffer = DocBuffer::new();
        let mut seed = 0xC0FFEE_u64;

        for _ in 0..1500 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            match (seed >> 32) % 18 {
                0 => {
                    buffer.execute(EditorCommand::InsertText("a".to_string()));
                }
                1 => {
                    buffer.execute(EditorCommand::InsertText("λ".to_string()));
                }
                2 => {
                    buffer.execute(EditorCommand::InsertText("\n".to_string()));
                }
                3 => {
                    buffer.execute(EditorCommand::DeleteBackward);
                }
                4 => {
                    buffer.execute(EditorCommand::DeleteForward);
                }
                5 => buffer.move_cursor_left(),
                6 => buffer.move_cursor_right(),
                7 => buffer.move_cursor_up(),
                8 => buffer.move_cursor_down(),
                9 => buffer.move_cursor_home(),
                10 => buffer.move_cursor_end(),
                11 => {
                    buffer.execute(EditorCommand::MoveCursor {
                        movement: Movement::Right,
                        extend: true,
                    });
                }
                12 => {
                    buffer.execute(EditorCommand::FormatBold);
                }
                13 => {
                    buffer.execute(EditorCommand::FormatInlineCode);
                }
                14 => {
                    buffer.execute(EditorCommand::SelectAll);
                }
                15 => {
                    buffer.execute(EditorCommand::Undo);
                }
                16 => {
                    buffer.execute(EditorCommand::Redo);
                }
                _ => {
                    let line = buffer.cursor_line;
                    buffer.execute(EditorCommand::ToggleCheckbox { line });
                }
            }

            assert!(buffer.cursor_offset() <= buffer.text().chars().count());
            assert!(buffer.cursor_line < buffer.line_count());
            assert!(buffer.cursor_col <= buffer.line_text(buffer.cursor_line).chars().count());
            if let Some(selection) = buffer.selection_offsets() {
                let (start, end) = selection.range();
                assert!(start < end);
                assert!(end <= buffer.text().chars().count());
            }
        }
    }

    #[test]
    fn unicode_boundaries_are_char_based() {
        let mut buffer = DocBuffer::from_text("a👩‍💻b");
        buffer.set_cursor(0, 4);
        buffer.backspace();
        assert_eq!(buffer.text(), "a👩‍b");
        buffer.undo();
        assert_eq!(buffer.text(), "a👩‍💻b");
    }

    #[test]
    fn empty_boundary_actions_do_not_panic() {
        let mut buffer = DocBuffer::new();
        buffer.backspace();
        buffer.delete();
        buffer.move_cursor_left();
        buffer.move_cursor_right();
        buffer.move_cursor_up();
        buffer.move_cursor_down();
        buffer.move_cursor_home();
        buffer.move_cursor_end();
        assert!(!buffer.undo());
        assert!(!buffer.redo());
        assert_eq!((buffer.cursor_line, buffer.cursor_col), (0, 0));
    }

    #[test]
    fn format_toggle_bold_inner_and_outer() {
        let mut buffer = DocBuffer::from_text("hello");
        buffer.set_selection(0, 0, 0, 5);
        buffer.execute(EditorCommand::FormatBold);
        assert_eq!(buffer.text(), "**hello**");
        
        // Inner toggle (selection is the formatted text itself)
        buffer.set_selection(0, 0, 0, 9);
        buffer.execute(EditorCommand::FormatBold);
        assert_eq!(buffer.text(), "hello");
        
        // Re-bold
        buffer.set_selection(0, 0, 0, 5);
        buffer.execute(EditorCommand::FormatBold);
        assert_eq!(buffer.text(), "**hello**");
        
        // Outer toggle (selection is the inner unformatted text)
        buffer.set_selection(0, 2, 0, 7);
        buffer.execute(EditorCommand::FormatBold);
        assert_eq!(buffer.text(), "hello");
    }

    #[test]
    fn list_auto_continuation_unordered_and_checklist() {
        // Unordered list continuation
        let mut buffer = DocBuffer::from_text("- Buy milk");
        buffer.set_cursor(0, 10);
        buffer.insert_at_cursor("\n");
        assert_eq!(buffer.text(), "- Buy milk\n- ");
        assert_eq!((buffer.cursor_line, buffer.cursor_col), (1, 2));

        // Unordered list empty line clearing
        buffer.insert_at_cursor("\n");
        assert_eq!(buffer.text(), "- Buy milk\n");
        assert_eq!((buffer.cursor_line, buffer.cursor_col), (1, 0));

        // Checklist continuation
        let mut buffer2 = DocBuffer::from_text("* [ ] Code task");
        buffer2.set_cursor(0, 15);
        buffer2.insert_at_cursor("\n");
        assert_eq!(buffer2.text(), "* [ ] Code task\n* [ ] ");
        assert_eq!((buffer2.cursor_line, buffer2.cursor_col), (1, 6));

        // Checklist empty line clearing
        buffer2.insert_at_cursor("\n");
        assert_eq!(buffer2.text(), "* [ ] Code task\n");
        assert_eq!((buffer2.cursor_line, buffer2.cursor_col), (1, 0));
    }

    #[test]
    fn list_auto_continuation_ordered() {
        let mut buffer = DocBuffer::from_text("1. Step one");
        buffer.set_cursor(0, 11);
        buffer.insert_at_cursor("\n");
        assert_eq!(buffer.text(), "1. Step one\n2. ");
        assert_eq!((buffer.cursor_line, buffer.cursor_col), (1, 3));

        // Ordered list empty line clearing
        buffer.insert_at_cursor("\n");
        assert_eq!(buffer.text(), "1. Step one\n");
        assert_eq!((buffer.cursor_line, buffer.cursor_col), (1, 0));
    }
}

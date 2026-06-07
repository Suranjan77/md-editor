#![allow(dead_code)]
use ropey::Rope;

use super::command::{CommandResult, EditorCommand};
use super::table::get_table_row_columns_count;
use super::transaction::EditTransaction;
pub(crate) use super::transaction::Selection;

pub(crate) struct DocBuffer {
    pub(crate) rope: Rope,
    pub(crate) cursor_offset: usize,
    pub(crate) selection_offsets: Option<Selection>,
    pub(crate) desired_col: Option<usize>,

    pub cursor_line: usize,
    pub cursor_col: usize,
    pub selection: Option<(usize, usize, usize, usize)>,
    pub dirty: bool,

    pub(crate) undo_stack: Vec<EditTransaction>,
    pub(crate) redo_stack: Vec<EditTransaction>,
}

impl DocBuffer {
    pub(crate) fn new() -> Self {
        Self::from_text("")
    }
    pub(crate) fn from_text(text: &str) -> Self {
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
    pub(crate) fn text(&self) -> String {
        self.rope.to_string()
    }
    pub(crate) fn set_text(&mut self, text: &str) {
        self.rope = Rope::from_str(text);
        self.cursor_offset = self.cursor_offset.min(self.rope.len_chars());
        self.selection_offsets = None;
        self.desired_col = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.dirty = true;
        self.sync_public_state();
    }
    pub(crate) fn line_count(&self) -> usize {
        self.rope.len_lines()
    }
    pub(crate) fn line_text(&self, line: usize) -> String {
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
    pub(crate) fn cursor_offset(&self) -> usize {
        self.cursor_offset
    }
    pub(crate) fn selection_offsets(&self) -> Option<Selection> {
        self.selection_offsets
    }
    pub(crate) fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection_offsets?.range();
        Some(self.rope.slice(start..end).to_string())
    }
    pub(crate) fn execute(&mut self, command: EditorCommand) -> CommandResult {
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
            EditorCommand::ReplaceTextRange {
                line,
                start_col,
                end_col,
                replacement,
            } => {
                let line_start = self.rope.line_to_char(line);
                let start_offset = line_start + start_col;
                let end_offset = line_start + end_col;
                self.replace_range(start_offset, end_offset, &replacement)
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
            EditorCommand::ToggleHeading => self.toggle_heading(),
            EditorCommand::ToggleBlockquote => self.toggle_line_prefix("> "),
            EditorCommand::ToggleUnorderedList => self.toggle_line_prefix("- "),
            EditorCommand::ToggleOrderedList => self.toggle_ordered_list(),
            EditorCommand::InsertCodeBlock => self.wrap_lines_or_insert_block("```", "```", "code"),
            EditorCommand::InsertMathBlock => self.wrap_lines_or_insert_block("$$", "$$", "x = y"),
            EditorCommand::InsertTable => {
                self.insert_text("| Column 1 | Column 2 |\n|---|---|\n| value | value |")
            }
            EditorCommand::InsertPdfQuoteLink {
                selected_text: _,
                page_number: _,
                link,
            } => self.insert_pdf_quote_link(&link),
            EditorCommand::InsertPdfAnnotationLink {
                selected_text: _,
                page_number: _,
                link,
            } => self.insert_pdf_annotation_link(&link),
            EditorCommand::DuplicateLine => self.duplicate_line(),
            EditorCommand::MoveLineUp => self.move_line(-1),
            EditorCommand::MoveLineDown => self.move_line(1),
            EditorCommand::ReplaceAll {
                query,
                replacement,
                regex,
                match_case,
            } => self.replace_all(&query, &replacement, regex, match_case),
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
            EditorCommand::ConvertToH1 { line } => {
                let text = self.line_text(line);
                let trimmed = text.trim_start();
                let indent_len = text.len() - trimmed.len();
                let level = trimmed.chars().take_while(|ch| *ch == '#').count();
                let has_heading = level > 0 && trimmed.chars().nth(level) == Some(' ');
                let delete_len = if has_heading { level + 1 } else { 0 };
                self.replace_line_prefix(line, indent_len, delete_len, "# ")
            }
            EditorCommand::ConvertToH2 { line } => {
                let text = self.line_text(line);
                let trimmed = text.trim_start();
                let indent_len = text.len() - trimmed.len();
                let level = trimmed.chars().take_while(|ch| *ch == '#').count();
                let has_heading = level > 0 && trimmed.chars().nth(level) == Some(' ');
                let delete_len = if has_heading { level + 1 } else { 0 };
                self.replace_line_prefix(line, indent_len, delete_len, "## ")
            }
            EditorCommand::ConvertToH3 { line } => {
                let text = self.line_text(line);
                let trimmed = text.trim_start();
                let indent_len = text.len() - trimmed.len();
                let level = trimmed.chars().take_while(|ch| *ch == '#').count();
                let has_heading = level > 0 && trimmed.chars().nth(level) == Some(' ');
                let delete_len = if has_heading { level + 1 } else { 0 };
                self.replace_line_prefix(line, indent_len, delete_len, "### ")
            }
            EditorCommand::ConvertToParagraph { line } => {
                let text = self.line_text(line);
                let trimmed = text.trim_start();
                let indent_len = text.len() - trimmed.len();
                let level = trimmed.chars().take_while(|ch| *ch == '#').count();
                let has_heading = level > 0 && trimmed.chars().nth(level) == Some(' ');
                let delete_len = if has_heading { level + 1 } else { 0 };
                self.replace_line_prefix(line, indent_len, delete_len, "")
            }
            EditorCommand::RemoveCheckbox { line } => {
                let line_text = self.line_text(line);
                let trimmed = line_text.trim_start();
                let indent_len = line_text.len() - trimmed.len();
                if trimmed.starts_with("- [ ]")
                    || trimmed.starts_with("- [x]")
                    || trimmed.starts_with("- [X]")
                {
                    self.replace_line_prefix(line, indent_len, 5, "-")
                } else if trimmed.starts_with("* [ ]")
                    || trimmed.starts_with("* [x]")
                    || trimmed.starts_with("* [X]")
                {
                    self.replace_line_prefix(line, indent_len, 5, "*")
                } else {
                    CommandResult::default()
                }
            }
            EditorCommand::InsertRowAbove { line } => {
                let row_text = self.line_text(line);
                let columns_count = get_table_row_columns_count(&row_text);
                let start_offset = self.rope.line_to_char(line);
                let new_row = "|".to_string() + &"  |".repeat(columns_count) + "\n";
                self.insert_text_at(start_offset, &new_row)
            }
            EditorCommand::InsertRowBelow { line } => {
                let row_text = self.line_text(line);
                let columns_count = get_table_row_columns_count(&row_text);
                let start_offset = self.rope.line_to_char(line + 1);
                let last_char_is_newline =
                    start_offset > 0 && self.rope.char(start_offset - 1) == '\n';
                let mut new_row = "|".to_string() + &"  |".repeat(columns_count) + "\n";
                if start_offset == self.rope.len_chars() && !last_char_is_newline {
                    new_row = "\n".to_string() + &new_row;
                }
                self.insert_text_at(start_offset, &new_row)
            }
            EditorCommand::DeleteRow { line } => {
                let start_offset = self.rope.line_to_char(line);
                let end_offset = self.rope.line_to_char(line + 1);
                if end_offset > start_offset {
                    let last_char = self.rope.char(end_offset - 1);
                    if last_char != '\n' && start_offset > 0 {
                        let prev_newline = self.rope.line_to_char(line) - 1;
                        self.delete_range(prev_newline, end_offset)
                    } else {
                        self.delete_range(start_offset, end_offset)
                    }
                } else {
                    self.delete_range(start_offset, end_offset)
                }
            }
            EditorCommand::InsertColumnLeft { line } => {
                let (table_start, table_end) = self.get_table_bounds(line);
                let cell_idx = self.get_current_table_column_idx(line);
                self.insert_column_at(table_start, table_end, cell_idx)
            }
            EditorCommand::InsertColumnRight { line } => {
                let (table_start, table_end) = self.get_table_bounds(line);
                let cell_idx = self.get_current_table_column_idx(line);
                self.insert_column_at(table_start, table_end, cell_idx + 1)
            }
            EditorCommand::DeleteColumn { line } => {
                let (table_start, table_end) = self.get_table_bounds(line);
                let cell_idx = self.get_current_table_column_idx(line);
                self.delete_column_at(table_start, table_end, cell_idx)
            }
            EditorCommand::SetCodeLanguage { line, language } => {
                let text = self.line_text(line);
                let trimmed = text.trim_start();
                let indent_len = text.len() - trimmed.len();
                if trimmed.starts_with("```") {
                    let start_offset = self.rope.line_to_char(line);
                    let end_offset = self.rope.line_to_char(line + 1);
                    let new_line = text[..indent_len].to_string() + "```" + &language + "\n";
                    self.replace_range(start_offset, end_offset, &new_line)
                } else {
                    CommandResult::default()
                }
            }
            EditorCommand::ConvertQuoteToParagraph { line } => {
                let text = self.line_text(line);
                let trimmed = text.trim_start();
                let indent_len = text.len() - trimmed.len();
                let delete_len = if trimmed.starts_with("> ") {
                    2
                } else if trimmed.starts_with(">") {
                    1
                } else {
                    0
                };
                self.replace_line_prefix(line, indent_len, delete_len, "")
            }
        }
    }
    pub(crate) fn sync_public_state(&mut self) {
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
    pub(crate) fn line_col_to_offset(&self, line: usize, col: usize) -> usize {
        let max_line = self.line_count().saturating_sub(1);
        let line = line.min(max_line);
        let line_start = self.rope.line_to_char(line);
        let line_len = self.line_text(line).chars().count();
        line_start + col.min(line_len)
    }
    pub(crate) fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        let clamped = offset.min(self.rope.len_chars());
        let line = self.rope.char_to_line(clamped);
        let line_start = self.rope.line_to_char(line);
        let col = clamped.saturating_sub(line_start);
        (line, col.min(self.line_text(line).chars().count()))
    }
}

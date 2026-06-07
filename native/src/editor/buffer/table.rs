use super::command::CommandResult;
use super::document::DocBuffer;

impl DocBuffer {
    pub(crate) fn get_table_bounds(&self, line: usize) -> (usize, usize) {
        let mut table_start = line;
        while table_start > 0 && is_table_row_line(&self.line_text(table_start - 1)) {
            table_start -= 1;
        }
        let mut table_end = line;
        while table_end + 1 < self.rope.len_lines()
            && is_table_row_line(&self.line_text(table_end + 1))
        {
            table_end += 1;
        }
        (table_start, table_end)
    }
    pub(crate) fn get_current_table_column_idx(&self, line: usize) -> usize {
        let cursor_col = if self.cursor_line == line {
            self.cursor_col
        } else {
            0
        };
        let line_text = self.line_text(line);
        let mut col_idx: usize = 0;
        let mut char_count = 0;
        let mut chars = line_text.chars().peekable();
        while let Some(ch) = chars.next() {
            if char_count >= cursor_col {
                break;
            }
            if ch == '\\' {
                let _ = chars.next();
                char_count += 2;
            } else {
                if ch == '|' {
                    col_idx += 1;
                }
                char_count += 1;
            }
        }
        col_idx.saturating_sub(1)
    }
    pub(crate) fn insert_column_at(
        &mut self,
        table_start: usize,
        table_end: usize,
        cell_idx: usize,
    ) -> CommandResult {
        let table_start_offset = self.rope.line_to_char(table_start);
        let table_end_offset = self.rope.line_to_char(table_end + 1);
        let mut new_table_text = String::new();
        for r in table_start..=table_end {
            let row_text = self.line_text(r);
            let clean_row = row_text.trim_end_matches(&['\r', '\n'][..]);
            let mut cells = parse_table_row_cells(clean_row);
            let is_separator = cells.iter().all(|c| {
                let t = c.trim();
                !t.is_empty() && t.chars().all(|ch| ch == '-' || ch == ':')
            });
            let new_cell = if is_separator {
                " --- ".to_string()
            } else {
                "  ".to_string()
            };
            if cell_idx < cells.len() {
                cells.insert(cell_idx, new_cell);
            } else {
                cells.push(new_cell);
            }
            new_table_text.push('|');
            for cell in cells {
                new_table_text.push_str(&cell);
                new_table_text.push('|');
            }
            new_table_text.push('\n');
        }
        self.replace_range(table_start_offset, table_end_offset, &new_table_text)
    }
    pub(crate) fn delete_column_at(
        &mut self,
        table_start: usize,
        table_end: usize,
        cell_idx: usize,
    ) -> CommandResult {
        let table_start_offset = self.rope.line_to_char(table_start);
        let table_end_offset = self.rope.line_to_char(table_end + 1);
        let mut new_table_text = String::new();
        for r in table_start..=table_end {
            let row_text = self.line_text(r);
            let clean_row = row_text.trim_end_matches(&['\r', '\n'][..]);
            let mut cells = parse_table_row_cells(clean_row);
            if cell_idx < cells.len() {
                cells.remove(cell_idx);
            }
            new_table_text.push('|');
            for cell in cells {
                new_table_text.push_str(&cell);
                new_table_text.push('|');
            }
            new_table_text.push('\n');
        }
        self.replace_range(table_start_offset, table_end_offset, &new_table_text)
    }
}

pub(crate) fn is_table_row_line(row_text: &str) -> bool {
    let trimmed = row_text.trim();
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() > 1
}

pub(crate) fn get_table_row_columns_count(row_text: &str) -> usize {
    let mut count: usize = 0;
    let mut chars = row_text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let _ = chars.next();
        } else if ch == '|' {
            count += 1;
        }
    }
    count.saturating_sub(1)
}

pub(crate) fn parse_table_row_cells(row_text: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut current_cell = String::new();
    let mut chars = row_text.chars().peekable();
    if chars.peek() == Some(&'|') {
        chars.next();
    }
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            current_cell.push(ch);
            if let Some(next_ch) = chars.next() {
                current_cell.push(next_ch);
            }
        } else if ch == '|' {
            cells.push(current_cell.clone());
            current_cell.clear();
        } else {
            current_cell.push(ch);
        }
    }
    cells
}

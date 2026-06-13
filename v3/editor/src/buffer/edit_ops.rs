use super::typing::{ListKind, is_closing_char, matching_pair, parse_list_prefix};
use super::*;

fn is_separator_row(row_cells: &[String]) -> bool {
    row_cells.iter().all(|cell| {
        let trimmed = cell.trim();
        !trimmed.is_empty() && trimmed.chars().all(|c| c == '-' || c == ':' || c == ' ')
    })
}

fn is_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() > 1
}

impl Buffer {
    pub(super) fn insert(&mut self, text: &str) -> ApplyResult {
        if text.is_empty() {
            return ApplyResult::default();
        }
        let old_total = self.line_count();
        let before = self.selections.clone();
        let mut ops: Vec<EditOp> = Vec::new();
        let mut acc = SpanAcc::default();
        let mut new_sels = Vec::with_capacity(before.len());
        let mut delta: isize = 0;
        let mut replaced_range = false;

        let is_newline = text == "\n" || text == "\r\n";

        if is_newline {
            for sel in &before {
                let (start, end) = sel.range();
                let start = shift(start, delta);
                let end = shift(end, delta);
                let new_caret = self.insert_newline(start, end, &mut acc, &mut ops, &mut delta);
                new_sels.push(Selection::caret(new_caret));
            }
            self.replace_selections(new_sels);
            let after = self.selections.clone();
            let _node = self.undo.commit(Transaction { ops, before, after });
            self.coalesce = None;
            return ApplyResult {
                text_changed: true,
                selection_changed: true,
                changed: acc.span(old_total, self.line_count()),
            };
        }

        let is_single_char = text.chars().count() == 1;
        let typed_char = is_single_char.then(|| text.chars().next()).flatten();
        let is_url = text.starts_with("http://") || text.starts_with("https://");

        for sel in &before {
            let (start, end) = sel.range();
            let start = shift(start, delta);
            let end = shift(end, delta);

            if end > start {
                replaced_range = true;
                if is_url {
                    let removed = self.rope_delete(start, end, &mut acc);
                    ops.push(EditOp::Delete {
                        at: start,
                        text: removed.clone(),
                    });
                    let replacement = format!("[{}]({})", removed, text);
                    self.rope_insert(start, &replacement, &mut acc);
                    ops.push(EditOp::Insert {
                        at: start,
                        text: replacement.clone(),
                    });

                    let new_caret = start + replacement.chars().count();
                    new_sels.push(Selection::caret(new_caret));
                    delta += replacement.chars().count() as isize - (end - start) as isize;
                } else if let Some(c) = typed_char
                    && let Some(close_c) = matching_pair(c)
                {
                    let removed = self.rope_delete(start, end, &mut acc);
                    ops.push(EditOp::Delete {
                        at: start,
                        text: removed.clone(),
                    });
                    let replacement = format!("{}{}{}", c, removed, close_c);
                    self.rope_insert(start, &replacement, &mut acc);
                    ops.push(EditOp::Insert {
                        at: start,
                        text: replacement.clone(),
                    });

                    new_sels.push(Selection::new(
                        start + 1,
                        start + 1 + removed.chars().count(),
                    ));
                    delta += 2;
                } else {
                    let removed = self.rope_delete(start, end, &mut acc);
                    ops.push(EditOp::Delete {
                        at: start,
                        text: removed,
                    });
                    self.rope_insert(start, text, &mut acc);
                    ops.push(EditOp::Insert {
                        at: start,
                        text: text.to_string(),
                    });
                    let text_chars = text.chars().count();
                    new_sels.push(Selection::caret(start + text_chars));
                    delta += text_chars as isize - (end - start) as isize;
                }
            } else {
                if let Some(c) = typed_char {
                    let next_char = if start < self.rope.len_chars() {
                        Some(self.rope.char(start))
                    } else {
                        None
                    };

                    if is_closing_char(c) && next_char == Some(c) {
                        new_sels.push(Selection::caret(start + 1));
                    } else if let Some(close_c) = matching_pair(c) {
                        let should_pair = next_char.is_none_or(|nc| !nc.is_alphanumeric());
                        if should_pair {
                            let pair_str = format!("{}{}", c, close_c);
                            self.rope_insert(start, &pair_str, &mut acc);
                            ops.push(EditOp::Insert {
                                at: start,
                                text: pair_str,
                            });
                            new_sels.push(Selection::caret(start + 1));
                            delta += 2;
                        } else {
                            self.rope_insert(start, text, &mut acc);
                            ops.push(EditOp::Insert {
                                at: start,
                                text: text.to_string(),
                            });
                            new_sels.push(Selection::caret(start + 1));
                            delta += 1;
                        }
                    } else {
                        self.rope_insert(start, text, &mut acc);
                        ops.push(EditOp::Insert {
                            at: start,
                            text: text.to_string(),
                        });
                        new_sels.push(Selection::caret(start + 1));
                        delta += 1;
                    }
                } else {
                    self.rope_insert(start, text, &mut acc);
                    ops.push(EditOp::Insert {
                        at: start,
                        text: text.to_string(),
                    });
                    let text_chars = text.chars().count();
                    new_sels.push(Selection::caret(start + text_chars));
                    delta += text_chars as isize;
                }
            }
        }
        self.replace_selections(new_sels);
        self.record(ops, before, replaced_range, text);
        ApplyResult {
            text_changed: true,
            selection_changed: true,
            changed: acc.span(old_total, self.line_count()),
        }
    }

    pub(super) fn delete(&mut self, direction: Direction) -> ApplyResult {
        let old_total = self.line_count();
        let before = self.selections.clone();
        let mut ops: Vec<EditOp> = Vec::new();
        let mut acc = SpanAcc::default();
        let mut new_sels = Vec::with_capacity(before.len());
        let mut delta: isize = 0;
        for sel in &before {
            let (start, end) = sel.range();
            let (start, end) = (shift(start, delta), shift(end, delta));
            let (start, end) = if end > start {
                (start, end)
            } else {
                match direction {
                    Direction::Backward => {
                        if start > 0 && start < self.rope.len_chars() {
                            let prev_c = self.rope.char(start - 1);
                            let next_c = self.rope.char(start);
                            if let Some(expected_next) = matching_pair(prev_c) {
                                if next_c == expected_next {
                                    (start - 1, start + 1)
                                } else {
                                    (prev_grapheme(&self.rope, start), start)
                                }
                            } else {
                                (prev_grapheme(&self.rope, start), start)
                            }
                        } else {
                            (prev_grapheme(&self.rope, start), start)
                        }
                    }
                    Direction::Forward => (start, next_grapheme(&self.rope, start)),
                }
            };
            if end > start {
                let removed = self.rope_delete(start, end, &mut acc);
                ops.push(EditOp::Delete {
                    at: start,
                    text: removed,
                });
                delta -= (end - start) as isize;
            }
            new_sels.push(Selection::caret(start));
        }
        let text_changed = !ops.is_empty();
        self.replace_selections(new_sels);
        if text_changed {
            let after = self.selections.clone();
            self.undo.commit(Transaction { ops, before, after });
        }
        ApplyResult {
            text_changed,
            selection_changed: true,
            changed: acc.span(old_total, self.line_count()),
        }
    }

    fn insert_newline(
        &mut self,
        start: usize,
        end: usize,
        acc: &mut SpanAcc,
        ops: &mut Vec<EditOp>,
        delta: &mut isize,
    ) -> usize {
        let start = if end > start {
            let removed = self.rope_delete(start, end, acc);
            ops.push(EditOp::Delete {
                at: start,
                text: removed,
            });
            *delta -= (end - start) as isize;
            start
        } else {
            start
        };

        let line_idx = self.rope.char_to_line(start);
        let line_start = self.rope.line_to_char(line_idx);
        let line_text: String = self.rope.slice(line_start..start).to_string();

        if let Some(prefix) = parse_list_prefix(&line_text) {
            let rest_of_line = &line_text[prefix.indent.len() + prefix.raw.len()..];
            if rest_of_line.trim().is_empty() {
                let delete_start = line_start;
                let delete_end = line_start + prefix.indent.len() + prefix.raw.len();
                let removed = self.rope_delete(delete_start, delete_end, acc);
                ops.push(EditOp::Delete {
                    at: delete_start,
                    text: removed,
                });
                *delta -= (delete_end - delete_start) as isize;
                return delete_start;
            } else {
                let continued_raw = match &prefix.kind {
                    ListKind::Unordered => prefix.raw.clone(),
                    ListKind::Checkbox => {
                        if prefix.raw.starts_with("- [") {
                            "- [ ] ".to_string()
                        } else {
                            "* [ ] ".to_string()
                        }
                    }
                    ListKind::Ordered(num, delimiter) => {
                        format!("{}{}{} ", "", num + 1, delimiter)
                    }
                };
                let continued = format!("{}{}", prefix.indent, continued_raw);
                let text_to_insert = format!("\n{}", continued);
                self.rope_insert(start, &text_to_insert, acc);
                ops.push(EditOp::Insert {
                    at: start,
                    text: text_to_insert.clone(),
                });
                *delta += text_to_insert.chars().count() as isize;

                if let ListKind::Ordered(_, delimiter) = prefix.kind {
                    let mut next_line_idx = line_idx + 2;
                    let mut current_delta = *delta;
                    while next_line_idx < self.rope.len_lines() {
                        let next_line_start = self.rope.line_to_char(next_line_idx);
                        let next_line_end = self
                            .rope
                            .line_to_char(next_line_idx + 1)
                            .min(self.rope.len_chars());
                        let next_line_text: String =
                            self.rope.slice(next_line_start..next_line_end).to_string();

                        if let Some(next_prefix) = parse_list_prefix(&next_line_text) {
                            if let ListKind::Ordered(next_num, next_delim) = next_prefix.kind {
                                if next_delim == delimiter && next_prefix.indent == prefix.indent {
                                    let new_num = next_num + 1;
                                    let old_prefix_len =
                                        next_prefix.indent.len() + next_prefix.raw.len();

                                    let del_start = next_line_start;
                                    let del_end = next_line_start + old_prefix_len;
                                    let removed = self.rope_delete(del_start, del_end, acc);
                                    ops.push(EditOp::Delete {
                                        at: del_start,
                                        text: removed,
                                    });
                                    current_delta -= old_prefix_len as isize;

                                    let new_prefix_raw =
                                        format!("{}{}{} ", next_prefix.indent, new_num, next_delim);
                                    self.rope_insert(del_start, &new_prefix_raw, acc);
                                    ops.push(EditOp::Insert {
                                        at: del_start,
                                        text: new_prefix_raw.clone(),
                                    });
                                    current_delta += new_prefix_raw.chars().count() as isize;
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                        next_line_idx += 1;
                    }
                    *delta = current_delta;
                }

                return start + text_to_insert.chars().count();
            }
        }

        self.rope_insert(start, "\n", acc);
        ops.push(EditOp::Insert {
            at: start,
            text: "\n".to_string(),
        });
        *delta += 1;
        start + 1
    }

    pub(super) fn table_cell_nav(&mut self, backward: bool) -> Option<ApplyResult> {
        let before = self.selections.clone();
        if before.len() != 1 {
            return None;
        }
        let sel = before[0];
        let (start, end) = sel.range();
        if start != end {
            return None;
        }

        let line_idx = self.rope.char_to_line(start);
        let line_start = self.rope.line_to_char(line_idx);
        let line_text = self.rope.line(line_idx).to_string();

        if !is_table_row(&line_text) {
            return None;
        }

        let mut start_line = line_idx;
        while start_line > 0 {
            let prev = self.rope.line(start_line - 1).to_string();
            if is_table_row(&prev) {
                start_line -= 1;
            } else {
                break;
            }
        }
        let mut end_line = line_idx;
        while end_line + 1 < self.rope.len_lines() {
            let next = self.rope.line(end_line + 1).to_string();
            if is_table_row(&next) {
                end_line += 1;
            } else {
                break;
            }
        }

        let mut rows: Vec<Vec<String>> = Vec::new();
        for l in start_line..=end_line {
            let row_text = self.rope.line(l).to_string();
            let trimmed_row = row_text.trim();
            let parts: Vec<&str> = trimmed_row.split('|').collect();
            let mut row_cells = Vec::new();
            let start_p = if parts.first() == Some(&"") { 1 } else { 0 };
            let end_p = if parts.last() == Some(&"") {
                parts.len().saturating_sub(1)
            } else {
                parts.len()
            };
            for part in parts.iter().take(end_p).skip(start_p) {
                row_cells.push(part.trim().to_string());
            }
            rows.push(row_cells);
        }

        let mut col_count = 0;
        for r in &rows {
            col_count = col_count.max(r.len());
        }
        if col_count == 0 {
            return None;
        }

        for r in &mut rows {
            while r.len() < col_count {
                r.push(String::new());
            }
        }

        let mut alignments = vec![Alignment::Left; col_count];
        let mut sep_row_idx = None;
        for (i, r) in rows.iter().enumerate() {
            if is_separator_row(r) {
                sep_row_idx = Some(i);
                for (c, cell) in r.iter().enumerate() {
                    let trimmed = cell.trim();
                    let left = trimmed.starts_with(':');
                    let right = trimmed.ends_with(':');
                    alignments[c] = if left && right {
                        Alignment::Center
                    } else if right {
                        Alignment::Right
                    } else {
                        Alignment::Left
                    };
                }
                break;
            }
        }

        let mut col_widths = vec![3; col_count];
        for (i, r) in rows.iter().enumerate() {
            if Some(i) == sep_row_idx {
                continue;
            }
            for (c, cell) in r.iter().enumerate() {
                col_widths[c] = col_widths[c].max(cell.chars().count());
            }
        }

        let line_text_before_caret = self.rope.slice(line_start..start).to_string();
        let pipe_count = line_text_before_caret.chars().filter(|c| *c == '|').count();
        let current_cell_col = pipe_count.saturating_sub(1).min(col_count - 1);
        let current_cell_row = line_idx - start_line;

        let mut target_row = current_cell_row;
        let mut target_col = current_cell_col;
        let mut append_row = false;

        if backward {
            if target_col > 0 {
                target_col -= 1;
            } else if target_row > 0 {
                target_row -= 1;
                if Some(target_row) == sep_row_idx {
                    if target_row > 0 {
                        target_row -= 1;
                    } else {
                        target_row = current_cell_row;
                    }
                }
                target_col = col_count - 1;
            }
        } else {
            if target_col + 1 < col_count {
                target_col += 1;
            } else {
                target_row += 1;
                if Some(target_row) == sep_row_idx {
                    target_row += 1;
                }
                target_col = 0;
                if target_row >= rows.len() {
                    append_row = true;
                }
            }
        }

        if append_row {
            let new_row = vec![String::new(); col_count];
            rows.push(new_row);
            target_row = rows.len() - 1;
            target_col = 0;
        }

        let mut formatted_lines = Vec::new();
        let mut target_caret_offset = None;
        let mut current_char_offset = self.rope.line_to_char(start_line);

        for (r_idx, r) in rows.iter().enumerate() {
            let mut line_str = "|".to_string();

            for (c_idx, cell) in r.iter().enumerate() {
                let width = col_widths[c_idx];
                if Some(r_idx) == sep_row_idx {
                    let indicator = match alignments[c_idx] {
                        Alignment::Left => {
                            let mut s = ":".to_string();
                            s.push_str(&"-".repeat(width - 1));
                            s
                        }
                        Alignment::Center => {
                            let mut s = ":".to_string();
                            s.push_str(&"-".repeat(width - 2));
                            s.push(':');
                            s
                        }
                        Alignment::Right => {
                            let mut s = "-".to_string();
                            s.push_str(&"-".repeat(width - 2));
                            s.push(':');
                            s
                        }
                    };
                    line_str.push_str(&format!(" {} |", indicator));
                } else {
                    let cell_len = cell.chars().count();
                    let pad_needed = width.saturating_sub(cell_len);
                    let formatted_cell = match alignments[c_idx] {
                        Alignment::Left => {
                            format!("{}{}", cell, " ".repeat(pad_needed))
                        }
                        Alignment::Right => {
                            format!("{}{}", " ".repeat(pad_needed), cell)
                        }
                        Alignment::Center => {
                            let left_pad = pad_needed / 2;
                            let right_pad = pad_needed - left_pad;
                            format!("{}{}{}", " ".repeat(left_pad), cell, " ".repeat(right_pad))
                        }
                    };

                    if r_idx == target_row && c_idx == target_col {
                        let caret_offset_in_cell = match alignments[c_idx] {
                            Alignment::Left => 1,
                            Alignment::Right => 1 + pad_needed,
                            Alignment::Center => 1 + pad_needed / 2,
                        };
                        target_caret_offset = Some(
                            current_char_offset + line_str.chars().count() + caret_offset_in_cell,
                        );
                    }

                    line_str.push_str(&format!(" {} |", formatted_cell));
                }
            }

            line_str.push('\n');
            current_char_offset += line_str.chars().count();
            formatted_lines.push(line_str);
        }

        let table_start_char = self.rope.line_to_char(start_line);
        let table_end_char = self
            .rope
            .line_to_char(end_line + 1)
            .min(self.rope.len_chars());
        let old_total = self.line_count();

        let mut ops = Vec::new();
        let mut acc = SpanAcc::default();

        let removed = self.rope_delete(table_start_char, table_end_char, &mut acc);
        ops.push(EditOp::Delete {
            at: table_start_char,
            text: removed,
        });

        let replacement = formatted_lines.concat();
        self.rope_insert(table_start_char, &replacement, &mut acc);
        ops.push(EditOp::Insert {
            at: table_start_char,
            text: replacement,
        });

        let final_caret = target_caret_offset.unwrap_or(table_start_char);
        self.replace_selections(vec![Selection::caret(final_caret)]);

        let after = self.selections.clone();
        self.undo.commit(Transaction { ops, before, after });

        Some(ApplyResult {
            text_changed: true,
            selection_changed: true,
            changed: acc.span(old_total, self.line_count()),
        })
    }

    pub(super) fn normal_tab(&mut self, backward: bool) -> ApplyResult {
        if backward {
            self.mutate_lines(|line| {
                if line.starts_with("  ") {
                    (0, 2, String::new())
                } else if line.starts_with(" ") {
                    (0, 1, String::new())
                } else {
                    (0, 0, String::new())
                }
            })
        } else {
            self.insert("  ")
        }
    }

    pub(super) fn set_heading(&mut self, level: usize) -> ApplyResult {
        let level = level.clamp(1, 6);
        self.mutate_lines(|line| {
            let current_level = if line.starts_with("###### ") {
                6
            } else if line.starts_with("##### ") {
                5
            } else if line.starts_with("#### ") {
                4
            } else if line.starts_with("### ") {
                3
            } else if line.starts_with("## ") {
                2
            } else if line.starts_with("# ") {
                1
            } else {
                0
            };

            if current_level == level {
                let prefix_len = level + 1;
                (0, prefix_len, String::new())
            } else {
                let prefix_len = if current_level > 0 {
                    current_level + 1
                } else {
                    0
                };
                let new_prefix = format!("{} ", "#".repeat(level));
                (0, prefix_len, new_prefix)
            }
        })
    }
}

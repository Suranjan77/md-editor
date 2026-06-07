use super::command::CommandResult;
use super::document::DocBuffer;
use super::transaction::EditOp;
use super::transaction::Selection;

pub(crate) struct ListItem {
    indent: String,
    marker: String,
    next_marker: String,
    is_empty: bool,
}
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
    pub(crate) fn insert_text(&mut self, text: &str) -> CommandResult {
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
                let marker_end = line_start
                    + list_item.indent.chars().count()
                    + list_item.marker.chars().count();
                if insert_at >= marker_end {
                    if list_item.is_empty {
                        // Empty list item: clear the prefix on the current line and do NOT insert newline!
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
    pub(crate) fn insert_pdf_quote_link(&mut self, link: &str) -> CommandResult {
        self.wrap_selection_or_insert("[", &format!("]({link})"), "label")
    }
    pub(crate) fn insert_pdf_annotation_link(&mut self, link: &str) -> CommandResult {
        self.wrap_selection_or_insert("[", &format!("]({link})"), "label")
    }
    pub(crate) fn delete_selection(&mut self) -> CommandResult {
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
    pub(crate) fn delete_backward(&mut self) -> CommandResult {
        if self.selection_offsets.is_some() {
            return self.delete_selection();
        }
        if self.cursor_offset == 0 {
            return CommandResult::default();
        }
        self.delete_range(
            self.previous_grapheme_offset(self.cursor_offset),
            self.cursor_offset,
        )
    }
    pub(crate) fn delete_forward(&mut self) -> CommandResult {
        if self.selection_offsets.is_some() {
            return self.delete_selection();
        }
        if self.cursor_offset >= self.rope.len_chars() {
            return CommandResult::default();
        }
        self.delete_range(
            self.cursor_offset,
            self.next_grapheme_offset(self.cursor_offset),
        )
    }
    pub(crate) fn delete_range(&mut self, start: usize, end: usize) -> CommandResult {
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
    pub(crate) fn toggle_checkbox(&mut self, line: usize) -> CommandResult {
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
    pub(crate) fn wrap_selection_or_insert(
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

            let has_inner_formatting =
                (end - start >= prefix.chars().count() + suffix.chars().count()) && {
                    let text = self.rope.slice(start..end).to_string();
                    text.starts_with(prefix) && text.ends_with(suffix)
                };

            let has_outer_formatting = start >= prefix.chars().count()
                && end + suffix.chars().count() <= self.rope.len_chars()
                && {
                    let before = self
                        .rope
                        .slice(start - prefix.chars().count()..start)
                        .to_string();
                    let after = self
                        .rope
                        .slice(end..end + suffix.chars().count())
                        .to_string();
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
                self.selection_offsets =
                    Selection::new(start, suffix_start - prefix.chars().count());
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
                self.selection_offsets =
                    Selection::new(start - prefix.chars().count(), end - prefix.chars().count());
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
    pub(crate) fn replace_all(
        &mut self,
        query: &str,
        replacement: &str,
        regex: bool,
        match_case: bool,
    ) -> CommandResult {
        if query.is_empty() {
            return CommandResult::default();
        }

        let text = self.text();
        let new_text = if regex {
            let Ok(re) = regex::RegexBuilder::new(query)
                .case_insensitive(!match_case)
                .build()
            else {
                return CommandResult::default();
            };
            re.replace_all(&text, replacement).to_string()
        } else if match_case {
            text.replace(query, replacement)
        } else {
            let Ok(re) = regex::RegexBuilder::new(&regex::escape(query))
                .case_insensitive(true)
                .build()
            else {
                return CommandResult::default();
            };
            re.replace_all(&text, replacement).to_string()
        };

        if new_text == text {
            return CommandResult::default();
        }

        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        let old_len = self.rope.len_chars();
        self.rope.remove(0..old_len);
        self.rope.insert(0, &new_text);
        self.cursor_offset = self.cursor_offset.min(self.rope.len_chars());
        self.selection_offsets = None;
        self.commit_transaction(
            vec![
                EditOp::Delete {
                    char_offset: 0,
                    text,
                },
                EditOp::Insert {
                    char_offset: 0,
                    text: new_text,
                },
            ],
            before_cursor,
            before_selection,
        );
        CommandResult::changed()
    }
    pub(crate) fn toggle_heading(&mut self) -> CommandResult {
        let line = self.cursor_line;
        let text = self.line_text(line);
        let trimmed = text.trim_start();
        let indent_len = text.len() - trimmed.len();
        let (delete_len, insert) = if trimmed.starts_with("###### ") {
            (7, "")
        } else {
            let level = trimmed.chars().take_while(|ch| *ch == '#').count();
            if (1..=5).contains(&level) && trimmed.chars().nth(level) == Some(' ') {
                (level + 1, &"#".repeat(level + 1)[..])
            } else {
                (0, "# ")
            }
        };
        self.replace_line_prefix(line, indent_len, delete_len, insert)
    }
    pub(crate) fn toggle_line_prefix(&mut self, prefix: &str) -> CommandResult {
        let line = self.cursor_line;
        let text = self.line_text(line);
        let trimmed = text.trim_start();
        let indent_len = text.len() - trimmed.len();
        let delete_len = if trimmed.starts_with(prefix) {
            prefix.len()
        } else {
            0
        };
        let insert = if delete_len == 0 { prefix } else { "" };
        self.replace_line_prefix(line, indent_len, delete_len, insert)
    }
    pub(crate) fn toggle_ordered_list(&mut self) -> CommandResult {
        let line = self.cursor_line;
        let text = self.line_text(line);
        let trimmed = text.trim_start();
        let indent_len = text.len() - trimmed.len();
        let delete_len = detect_numbered_list_marker(trimmed).unwrap_or(0);
        let insert = if delete_len == 0 { "1. " } else { "" };
        self.replace_line_prefix(line, indent_len, delete_len, insert)
    }
    pub(crate) fn replace_line_prefix(
        &mut self,
        line: usize,
        indent_bytes: usize,
        delete_bytes: usize,
        insert: &str,
    ) -> CommandResult {
        let line_start = self
            .rope
            .line_to_char(line.min(self.line_count().saturating_sub(1)));
        let indent_chars = self.line_text(line)[..indent_bytes].chars().count();
        let delete_chars = self.line_text(line)[indent_bytes..indent_bytes + delete_bytes]
            .chars()
            .count();
        let start = line_start + indent_chars;
        let end = start + delete_chars;
        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        let mut ops = Vec::new();
        if start < end {
            let old = self.rope.slice(start..end).to_string();
            self.rope.remove(start..end);
            ops.push(EditOp::Delete {
                char_offset: start,
                text: old,
            });
        }
        if !insert.is_empty() {
            self.rope.insert(start, insert);
            ops.push(EditOp::Insert {
                char_offset: start,
                text: insert.to_string(),
            });
        }
        if ops.is_empty() {
            return CommandResult::default();
        }
        self.cursor_offset = if insert.is_empty() {
            self.cursor_offset.saturating_sub(delete_chars)
        } else {
            self.cursor_offset + insert.chars().count()
        }
        .min(self.rope.len_chars());
        self.selection_offsets = None;
        self.commit_transaction(ops, before_cursor, before_selection);
        CommandResult::changed()
    }
    pub(crate) fn wrap_lines_or_insert_block(
        &mut self,
        open: &str,
        close: &str,
        placeholder: &str,
    ) -> CommandResult {
        if let Some(selection) = self.selection_offsets {
            let (start, end) = selection.range();
            let start_line = self.rope.char_to_line(start);
            let end_line = self.rope.char_to_line(end);
            let line_start = self.rope.line_to_char(start_line);
            let line_end =
                self.line_col_to_offset(end_line, self.line_text(end_line).chars().count());
            let selected = self.rope.slice(line_start..line_end).to_string();
            let wrapped = format!("{open}\n{selected}\n{close}");
            let before_cursor = self.cursor_offset;
            let before_selection = self.selection_offsets;
            self.rope.remove(line_start..line_end);
            self.rope.insert(line_start, &wrapped);
            self.cursor_offset = line_start + wrapped.chars().count();
            self.selection_offsets = None;
            self.commit_transaction(
                vec![
                    EditOp::Delete {
                        char_offset: line_start,
                        text: selected,
                    },
                    EditOp::Insert {
                        char_offset: line_start,
                        text: wrapped,
                    },
                ],
                before_cursor,
                before_selection,
            );
            CommandResult::changed()
        } else {
            self.insert_text(&format!("{open}\n{placeholder}\n{close}"))
        }
    }
    pub(crate) fn duplicate_line(&mut self) -> CommandResult {
        let line = self.cursor_line;
        let line_text = self.line_text(line);
        let insert_at = self.line_col_to_offset(line, line_text.chars().count());
        let text = format!("\n{line_text}");
        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        self.rope.insert(insert_at, &text);
        self.cursor_offset = insert_at + text.chars().count();
        self.selection_offsets = None;
        self.commit_transaction(
            vec![EditOp::Insert {
                char_offset: insert_at,
                text,
            }],
            before_cursor,
            before_selection,
        );
        CommandResult::changed()
    }
    pub(crate) fn move_line(&mut self, delta: isize) -> CommandResult {
        let line = self.cursor_line;
        let max_line = self.line_count().saturating_sub(1);
        if (delta < 0 && line == 0) || (delta > 0 && line >= max_line) {
            return CommandResult::default();
        }
        let target = if delta < 0 { line - 1 } else { line + 1 };
        let mut lines: Vec<String> = self.text().split('\n').map(ToString::to_string).collect();
        lines.swap(line, target);
        let new_text = lines.join("\n");
        let old_text = self.text();
        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        let old_len = self.rope.len_chars();
        self.rope.remove(0..old_len);
        self.rope.insert(0, &new_text);
        self.cursor_offset = self.line_col_to_offset(target, self.cursor_col);
        self.selection_offsets = None;
        self.commit_transaction(
            vec![
                EditOp::Delete {
                    char_offset: 0,
                    text: old_text,
                },
                EditOp::Insert {
                    char_offset: 0,
                    text: new_text,
                },
            ],
            before_cursor,
            before_selection,
        );
        CommandResult::changed()
    }
    pub(crate) fn replace_range(&mut self, start: usize, end: usize, text: &str) -> CommandResult {
        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        let mut ops = Vec::new();
        if start < end {
            let old = self.rope.slice(start..end).to_string();
            self.rope.remove(start..end);
            ops.push(EditOp::Delete {
                char_offset: start,
                text: old,
            });
        }
        if !text.is_empty() {
            self.rope.insert(start, text);
            ops.push(EditOp::Insert {
                char_offset: start,
                text: text.to_string(),
            });
        }
        if ops.is_empty() {
            return CommandResult::default();
        }
        self.commit_transaction(ops, before_cursor, before_selection);
        CommandResult::changed()
    }
    pub(crate) fn insert_text_at(&mut self, char_offset: usize, text: &str) -> CommandResult {
        self.replace_range(char_offset, char_offset, text)
    }
}

pub(crate) fn parse_list_item(line_text: &str) -> Option<ListItem> {
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

pub(crate) fn detect_numbered_list_marker(trimmed: &str) -> Option<usize> {
    let mut digits = 0;
    for ch in trimmed.chars() {
        if ch.is_ascii_digit() {
            digits += ch.len_utf8();
        } else {
            break;
        }
    }
    if digits > 0 && trimmed[digits..].starts_with(". ") {
        Some(digits + 2)
    } else {
        None
    }
}

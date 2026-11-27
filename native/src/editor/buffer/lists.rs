//! Markdown list continuation and checkbox toggling on `DocBuffer`.

use super::{CommandResult, DocBuffer, EditOp};

pub(super) struct ListItem {
    pub(super) indent: String,
    pub(super) marker: String,
    pub(super) next_marker: String,
    pub(super) is_empty: bool,
}

impl DocBuffer {
    pub(super) fn toggle_checkbox(&mut self, line: usize) -> CommandResult {
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

    /// Insert or toggle a task-list `- [ ] ` prefix on the cursor's current line.
    pub(super) fn insert_checkbox_at_cursor(&mut self) -> CommandResult {
        let line = self.cursor_line;
        let line_text = self.line_text(line);
        let indent_chars = line_text
            .chars()
            .take_while(|ch| ch.is_whitespace())
            .count();
        let marker = line_text
            .chars()
            .skip(indent_chars)
            .take(5)
            .collect::<String>();
        if matches!(marker.as_str(), "- [ ]" | "- [x]" | "- [X]") {
            return self.toggle_checkbox(line);
        }
        let insert_at = self.line_col_to_offset(line, indent_chars);
        let prefix = "- [ ] ";
        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        self.rope.insert(insert_at, prefix);
        if self.cursor_offset >= insert_at {
            self.cursor_offset += prefix.chars().count();
        }
        self.selection_offsets = None;
        self.commit_transaction(
            vec![EditOp::Insert {
                char_offset: insert_at,
                text: prefix.to_string(),
            }],
            before_cursor,
            before_selection,
        );
        CommandResult::changed()
    }
}

pub(super) fn parse_list_item(line_text: &str) -> Option<ListItem> {
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

    if rest.starts_with("- [") || rest.starts_with("* [") || rest.starts_with("+ [") {
        let head: Vec<char> = rest.chars().take(6).collect();
        if head.len() >= 5 && head[4] == ']' && matches!(head[3], ' ' | 'x' | 'X') {
            let has_space_after = head.len() >= 6 && head[5] == ' ';
            let marker_len = if has_space_after { 6 } else { 5 };
            let marker = rest[..marker_len].to_string();
            let content = &rest[marker_len..];
            let is_empty = content.trim().is_empty();
            let bullet = &rest[..1];
            let next_marker = format!("{bullet} [ ] ");
            return Some(ListItem {
                indent,
                marker,
                next_marker,
                is_empty,
            });
        }
    }

    if rest == "-" || rest == "*" || rest == "+" {
        return Some(ListItem {
            indent,
            marker: rest.to_string(),
            next_marker: format!("{rest} "),
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
    if let Some(dot) = dot_idx
        && dot > 0
    {
        let after_dot = rest[dot + 1..].chars().next();
        let is_at_end = after_dot.is_none();
        let has_space_after = after_dot == Some(' ');
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

    None
}

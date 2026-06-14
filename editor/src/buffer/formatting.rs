use super::*;

impl Buffer {
    pub(super) fn toggle_wrap(&mut self, marker: &str) -> ApplyResult {
        self.toggle_wrap_pair(marker, marker)
    }

    pub(super) fn toggle_wrap_double(
        &mut self,
        start_marker: &str,
        end_marker: &str,
    ) -> ApplyResult {
        self.toggle_wrap_pair(start_marker, end_marker)
    }

    fn toggle_wrap_pair(&mut self, start_marker: &str, end_marker: &str) -> ApplyResult {
        let old_total = self.line_count();
        let before = self.selections.clone();
        let mut ops = Vec::new();
        let mut acc = SpanAcc::default();
        let mut new_selections = Vec::with_capacity(before.len());
        let mut delta = 0;
        let start_len = start_marker.chars().count();
        let end_len = end_marker.chars().count();

        for selection in &before {
            let (start, end) = selection.range();
            let start = shift(start, delta);
            let end = shift(end, delta);
            let empty = start == end;
            let inside = start >= start_len
                && end + end_len <= self.rope.len_chars()
                && self.rope.slice(start - start_len..start) == start_marker
                && self.rope.slice(end..end + end_len) == end_marker;
            let outside = !empty
                && end - start >= start_len + end_len
                && self.rope.slice(start..start + start_len) == start_marker
                && self.rope.slice(end - end_len..end) == end_marker;

            if outside {
                let removed_end = self.rope_delete(end - end_len, end, &mut acc);
                ops.push(EditOp::Delete {
                    at: end - end_len,
                    text: removed_end,
                });
                let removed_start = self.rope_delete(start, start + start_len, &mut acc);
                ops.push(EditOp::Delete {
                    at: start,
                    text: removed_start,
                });
                new_selections.push(Selection::new(start, end - start_len - end_len));
                delta -= (start_len + end_len) as isize;
            } else if inside {
                let removed_end = self.rope_delete(end, end + end_len, &mut acc);
                ops.push(EditOp::Delete {
                    at: end,
                    text: removed_end,
                });
                let removed_start = self.rope_delete(start - start_len, start, &mut acc);
                ops.push(EditOp::Delete {
                    at: start - start_len,
                    text: removed_start,
                });
                if empty {
                    new_selections.push(Selection::caret(start - start_len));
                } else {
                    new_selections.push(Selection::new(start - start_len, end - start_len));
                }
                delta -= (start_len + end_len) as isize;
            } else {
                self.rope_insert(end, end_marker, &mut acc);
                ops.push(EditOp::Insert {
                    at: end,
                    text: end_marker.to_string(),
                });
                self.rope_insert(start, start_marker, &mut acc);
                ops.push(EditOp::Insert {
                    at: start,
                    text: start_marker.to_string(),
                });
                if empty {
                    new_selections.push(Selection::caret(start + start_len));
                } else {
                    new_selections.push(Selection::new(start + start_len, end + start_len));
                }
                delta += (start_len + end_len) as isize;
            }
        }

        let text_changed = !ops.is_empty();
        self.replace_selections(new_selections);
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

    pub(super) fn mutate_lines<F>(&mut self, mut mutate: F) -> ApplyResult
    where
        F: FnMut(&str) -> (usize, usize, String),
    {
        let old_total = self.line_count();
        let before = self.selections.clone();
        let mut ops = Vec::new();
        let mut acc = SpanAcc::default();
        let mut lines = std::collections::BTreeSet::new();
        for selection in &before {
            let (start, end) = selection.range();
            for line in self.rope.char_to_line(start)..=self.rope.char_to_line(end) {
                lines.insert(line);
            }
        }
        let mut new_selections = before.clone();
        for line in lines.into_iter().rev() {
            if line >= self.rope.len_lines() {
                continue;
            }
            let line_start = self.rope.line_to_char(line);
            let line_end = self.rope.line_to_char(line + 1).min(self.rope.len_chars());
            let line_text = self.rope.slice(line_start..line_end).to_string();
            let (local_offset, delete_len, insert_text) = mutate(&line_text);
            let edit_at = line_start + local_offset;
            let delta = insert_text.chars().count() as isize - delete_len as isize;
            if delete_len > 0 {
                let removed = self.rope_delete(edit_at, edit_at + delete_len, &mut acc);
                ops.push(EditOp::Delete {
                    at: edit_at,
                    text: removed,
                });
            }
            if !insert_text.is_empty() {
                self.rope_insert(edit_at, &insert_text, &mut acc);
                ops.push(EditOp::Insert {
                    at: edit_at,
                    text: insert_text,
                });
            }
            for selection in &mut new_selections {
                if selection.anchor >= edit_at {
                    selection.anchor = shift(selection.anchor, delta);
                }
                if selection.head >= edit_at {
                    selection.head = shift(selection.head, delta);
                }
            }
        }
        let text_changed = !ops.is_empty();
        self.replace_selections(new_selections);
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

    pub(super) fn heading_cycle(&mut self) -> ApplyResult {
        self.mutate_lines(|line| {
            for level in (1..=6).rev() {
                let marker = format!("{} ", "#".repeat(level));
                if line.starts_with(&marker) {
                    return if level == 6 {
                        (0, marker.len(), String::new())
                    } else {
                        (0, marker.len(), format!("{} ", "#".repeat(level + 1)))
                    };
                }
            }
            (0, 0, "# ".to_string())
        })
    }

    pub(super) fn toggle_bullet(&mut self) -> ApplyResult {
        self.mutate_lines(|line| {
            if line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ") {
                (0, 2, String::new())
            } else {
                (0, 0, "- ".to_string())
            }
        })
    }

    pub(super) fn toggle_checkbox(&mut self) -> ApplyResult {
        self.mutate_lines(|line| {
            let marker = ["- ", "* ", "+ "]
                .into_iter()
                .find(|marker| line.starts_with(marker));
            let Some(marker) = marker else {
                return (0, 0, "- [ ] ".to_string());
            };
            let rest = &line[marker.len()..];
            if rest.starts_with("[ ] ") {
                (marker.len(), 4, "[x] ".to_string())
            } else if rest.starts_with("[x] ") {
                (marker.len(), 4, "[ ] ".to_string())
            } else {
                (marker.len(), 0, "[ ] ".to_string())
            }
        })
    }
}

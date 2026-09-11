//! Selections, as ordered (start, end) pairs of (line, col).

use super::{Editor, State};

pub(super) type TextRange = ((usize, usize), (usize, usize));

/// Order an anchor and focus into (start, end). Empty selections are `None`.
pub(super) fn normalized_selection(
    anchor: Option<(usize, usize)>,
    focus: Option<(usize, usize)>,
) -> Option<TextRange> {
    let anchor = anchor?;
    let focus = focus?;
    if anchor == focus {
        return None;
    }
    if anchor <= focus {
        Some((anchor, focus))
    } else {
        Some((focus, anchor))
    }
}

/// Columns of `range` that fall on line `line_idx`, clamped to `line_len`.
pub(super) fn cols_on_line(range: TextRange, line_idx: usize, line_len: usize) -> (usize, usize) {
    let ((start_line, start_col), (end_line, end_col)) = range;
    let from = if line_idx == start_line {
        start_col.min(line_len)
    } else {
        0
    };
    let to = if line_idx == end_line {
        end_col.min(line_len)
    } else {
        line_len
    };
    (from, to)
}

impl<Message> Editor<'_, Message> {
    /// The selection to paint: the in-progress pointer selection if there is
    /// one, otherwise the buffer's.
    pub(super) fn painted_selection(&self, state: &State) -> Option<TextRange> {
        normalized_selection(state.selection_anchor, state.selection_focus).or_else(|| {
            self.buffer.selection.map(|(sl, sc, el, ec)| {
                if (sl, sc) <= (el, ec) {
                    ((sl, sc), (el, ec))
                } else {
                    ((el, ec), (sl, sc))
                }
            })
        })
    }

    /// Text covered by the pointer selection.
    pub(super) fn selected_text(&self, state: &State) -> Option<String> {
        let range = normalized_selection(state.selection_anchor, state.selection_focus)?;
        let ((start_line, _), (end_line, _)) = range;

        let mut out = String::new();
        for line_idx in start_line..=end_line {
            let line = self.buffer.line_text(line_idx);
            let (from, to) = cols_on_line(range, line_idx, line.chars().count());
            if from < to {
                out.extend(line.chars().skip(from).take(to - from));
            }
            if line_idx != end_line {
                out.push('\n');
            }
        }

        if out.is_empty() { None } else { Some(out) }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::super::testing::make_line;
    use super::*;
    use crate::editor::buffer::DocBuffer;
    use crate::editor::highlight::{StyledLine, StyledSpan};

    #[test]
    fn test_normalized_selection_combinatorics() {
        // Run thousands of combinations of boundary cases for selections
        for anchor_line in 0..15 {
            for anchor_col in 0..10 {
                for focus_line in 0..15 {
                    for focus_col in 0..10 {
                        let norm = normalized_selection(
                            Some((anchor_line, anchor_col)),
                            Some((focus_line, focus_col)),
                        );
                        if (anchor_line, anchor_col) == (focus_line, focus_col) {
                            assert!(norm.is_none());
                        } else {
                            let (start, end) = norm.unwrap();
                            assert!(start <= end);
                            if anchor_line < focus_line {
                                assert_eq!(start, (anchor_line, anchor_col));
                                assert_eq!(end, (focus_line, focus_col));
                            } else if anchor_line > focus_line {
                                assert_eq!(start, (focus_line, focus_col));
                                assert_eq!(end, (anchor_line, anchor_col));
                            } else {
                                assert_eq!(start, (anchor_line, anchor_col.min(focus_col)));
                                assert_eq!(end, (anchor_line, anchor_col.max(focus_col)));
                            }
                        }
                    }
                }
            }
        }

        assert!(normalized_selection(None, None).is_none());
        assert!(normalized_selection(Some((1, 1)), None).is_none());
        assert!(normalized_selection(None, Some((2, 2))).is_none());
    }

    #[test]
    fn test_editor_selected_text_extraction() {
        let buffer = DocBuffer::from_text("line one\nline two\nline three\nline four");
        let lines: Vec<StyledLine> = vec![
            make_line(1, vec![StyledSpan::plain("line one")]),
            make_line(2, vec![StyledSpan::plain("line two")]),
            make_line(3, vec![StyledSpan::plain("line three")]),
            make_line(4, vec![StyledSpan::plain("line four")]),
        ];

        let image_cache = HashMap::new();
        let math_cache = HashMap::new();
        let editor = super::super::testing::editor_for(&buffer, &lines, &image_cache, &math_cache);

        // Perform combinatorial selections over the entire document
        for start_line in 0..4 {
            for start_col in 0..10 {
                for end_line in 0..4 {
                    for end_col in 0..10 {
                        let state = State {
                            is_focused: true,
                            selection_anchor: Some((start_line, start_col)),
                            selection_focus: Some((end_line, end_col)),
                            ..Default::default()
                        };

                        let sel = editor.selected_text(&state);
                        if let Some(((s_l, s_c), (e_l, e_c))) = normalized_selection(
                            Some((start_line, start_col)),
                            Some((end_line, end_col)),
                        ) {
                            let mut manual = String::new();
                            for l in s_l..=e_l {
                                let content = buffer.line_text(l);
                                let from = if l == s_l {
                                    s_c.min(content.chars().count())
                                } else {
                                    0
                                };
                                let to = if l == e_l {
                                    e_c.min(content.chars().count())
                                } else {
                                    content.chars().count()
                                };
                                if from < to {
                                    manual.push_str(
                                        &content
                                            .chars()
                                            .skip(from)
                                            .take(to - from)
                                            .collect::<String>(),
                                    );
                                }
                                if l != e_l {
                                    manual.push('\n');
                                }
                            }
                            if manual.is_empty() {
                                assert!(sel.is_none());
                            } else {
                                assert_eq!(sel.unwrap(), manual);
                            }
                        } else {
                            assert!(sel.is_none());
                        }
                    }
                }
            }
        }
    }
}

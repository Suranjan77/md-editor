//! What a span shows: its rendered form or its markdown source.
//!
//! Editing is Typora-style. Code, math and table blocks switch wholesale to
//! source while the caret is inside them. Elsewhere, only the inline element
//! under the caret — a bold run, a link, inline math — reveals its source,
//! together with the syntax markers on either side of it.

use crate::editor::highlight::{StyledLine, StyledSpan};

/// Whether `line` belongs to the code, math or table block holding the caret.
pub(super) fn is_block_editing_line(
    line: &StyledLine,
    active_block_id: Option<usize>,
    focused: bool,
) -> bool {
    focused
        && Some(line.block_id) == active_block_id
        && (line.is_code_block || line.is_math_block || line.is_table_row)
}

/// Whether span `span_idx` shows its source: its whole block is being edited,
/// or the caret at `active_col` reveals it inline.
pub(super) fn span_is_editing(
    line: &StyledLine,
    span_idx: usize,
    block_editing: bool,
    active_col: Option<usize>,
) -> bool {
    block_editing || active_col.is_some_and(|col| span_is_inline_edit_target(line, span_idx, col))
}

/// TeX source of a math span, with `$` delimiters stripped.
pub(super) fn math_source(span: &StyledSpan) -> &str {
    span.visible_text(false).trim_matches('$').trim()
}

/// Source column just past `span`, given the column it starts at.
pub(super) fn source_col_after_span(span: &StyledSpan, start_col: usize) -> usize {
    start_col + span.text.chars().count()
}

/// Source columns `[start, end]` covered by span `span_idx`.
fn span_source_range(line: &StyledLine, span_idx: usize) -> Option<(usize, usize)> {
    let mut start = 0usize;
    for (idx, span) in line.spans.iter().enumerate() {
        let end = source_col_after_span(span, start);
        if idx == span_idx {
            return Some((start, end));
        }
        start = end;
    }
    None
}

/// Whether a caret at `active_col` reveals the source of span `span_idx`:
/// the caret touches the span itself, or the styled content it belongs to
/// (a content span and the syntax markers around it reveal together).
fn span_is_inline_edit_target(line: &StyledLine, span_idx: usize, active_col: usize) -> bool {
    let Some(span) = line.spans.get(span_idx) else {
        return false;
    };

    let touches = |idx: usize| -> bool {
        span_source_range(line, idx)
            .is_some_and(|(start, end)| active_col >= start && active_col <= end)
    };

    if touches(span_idx) {
        return true;
    }

    let is_content = |idx: usize| {
        line.spans.get(idx).is_some_and(|s| {
            !s.is_syntax
                && (s.bold
                    || s.italic
                    || s.is_code
                    || s.is_link
                    || s.is_math
                    || s.is_heading
                    || line.is_blockquote)
        })
    };

    let is_syntax = |idx: usize| line.spans.get(idx).is_some_and(|s| s.is_syntax);

    if span.is_syntax {
        // A marker reveals when the content next to it is touched, or when
        // the marker on the far side of that content is.
        let check_side = |content_idx: usize| -> bool {
            if is_content(content_idx) {
                if touches(content_idx) {
                    return true;
                }
                let other_syntax_idx = if content_idx > span_idx {
                    content_idx + 1
                } else {
                    content_idx.saturating_sub(1)
                };
                if is_syntax(other_syntax_idx) && touches(other_syntax_idx) {
                    return true;
                }
            }
            false
        };

        if span_idx > 0 && check_side(span_idx - 1) {
            return true;
        }
        if check_side(span_idx + 1) {
            return true;
        }
    } else if is_content(span_idx) {
        if span_idx > 0 && is_syntax(span_idx - 1) && touches(span_idx - 1) {
            return true;
        }
        if is_syntax(span_idx + 1) && touches(span_idx + 1) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::highlight::highlight_markdown;

    #[test]
    fn typora_inline_editing_reveals_only_active_span_and_markers() {
        let lines = highlight_markdown("alpha **bold** omega");
        let line = &lines[0];
        let shown = |active_col: Option<usize>| {
            (0..line.spans.len())
                .map(|idx| {
                    line.spans[idx].visible_text(span_is_editing(line, idx, false, active_col))
                })
                .collect::<Vec<_>>()
        };

        assert_eq!(shown(None), vec!["alpha ", "", "bold", "", " omega"]);

        let active_inside_bold = "alpha **bo".chars().count();
        assert_eq!(
            shown(Some(active_inside_bold)),
            vec!["alpha ", "**", "bold", "**", " omega"]
        );

        let active_inside_plain = "al".chars().count();
        assert_eq!(
            shown(Some(active_inside_plain)),
            vec!["alpha ", "", "bold", "", " omega"]
        );
    }
}

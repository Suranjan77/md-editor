//! Mapping between source columns and visual positions.
//!
//! Positions are relative to the top-left of a line's text column. Both
//! directions re-run the line's word wrapping, so they must wrap exactly the
//! way painting does for the caret to land on the text.

use iced::Point;

use super::measure::{measure_char_width, measure_width, span_font};
use super::metrics::*;
use super::spans::{math_source, source_col_after_span, span_is_editing, span_visible_text};
use super::{Editor, Measure, State};
use crate::editor::highlight::StyledLine;

/// Pen for walking a wrapped line towards a source column.
struct Pen {
    x: f32,
    y: f32,
    max_w: f32,
}

impl Pen {
    /// Move to the next row if `width` doesn't fit on this one.
    fn fit(&mut self, width: f32, step: f32) {
        if self.x > 0.0 && self.x + width > self.max_w {
            self.y += step;
            self.x = 0.0;
        }
    }

    /// Advance over a token of (char, source column) pairs, stopping at the
    /// first character at or past `col`.
    fn advance_token<R: Measure>(
        &mut self,
        token: &[(char, usize)],
        col: usize,
        font_size: f32,
        font: iced::Font,
        step: f32,
    ) -> Option<(f32, f32)> {
        if token.is_empty() {
            return None;
        }

        let token_width = token
            .iter()
            .map(|(ch, _)| measure_char_width::<R>(*ch, font_size, font))
            .sum::<f32>();
        self.fit(token_width, step);

        let breaks_inside = token_width > self.max_w;
        for (ch, ch_col) in token {
            let ch_w = measure_char_width::<R>(*ch, font_size, font);
            if breaks_inside {
                self.fit(ch_w, step);
            }
            if *ch_col >= col {
                return Some((self.x, self.y));
            }
            self.x += ch_w;
        }
        None
    }
}

/// Scan state for mapping a point back to a source column.
struct RowScan {
    x: f32,
    row_y: f32,
    row_start_col: usize,
    /// Column just past the last character placed on the current row.
    row_end_col: usize,
    row_step: f32,
    /// Y being hit, relative to the line top.
    target_y: f32,
    max_w: f32,
}

impl RowScan {
    fn on_target_row(&self) -> bool {
        self.target_y < self.row_y + self.row_step
    }

    /// Start a new row at `col` if `width` doesn't fit. If the row being left
    /// is the one hit, the answer is its end, returned as `Some`.
    fn fit(&mut self, width: f32, col: usize, step: f32) -> Option<usize> {
        if self.x > 0.0 && self.x + width > self.max_w {
            if self.on_target_row() {
                return Some(self.row_end_col);
            }
            self.row_y += self.row_step;
            self.x = 0.0;
            self.row_start_col = col;
            self.row_end_col = col;
            self.row_step = step;
        }
        None
    }

    /// Scan a token of (char, source column) pairs for `click_x`. A character
    /// is hit when the click lands left of 60% of its width.
    fn scan_token<R: Measure>(
        &mut self,
        token: &[(char, usize)],
        click_x: f32,
        font_size: f32,
        font: iced::Font,
        step: f32,
    ) -> Option<usize> {
        let (&(_, first_col), &(_, last_col)) = (token.first()?, token.last()?);

        let token_width = token
            .iter()
            .map(|(ch, _)| measure_char_width::<R>(*ch, font_size, font))
            .sum::<f32>();
        if let Some(col) = self.fit(token_width, first_col, step) {
            return Some(col);
        }

        if token_width <= self.max_w {
            if !self.on_target_row() {
                self.x += token_width;
                self.row_end_col = last_col + 1;
                return None;
            }
            for (ch, ch_col) in token {
                let cw = measure_char_width::<R>(*ch, font_size, font);
                if click_x < self.x + cw * 0.6 {
                    return Some(*ch_col);
                }
                self.row_end_col = *ch_col + 1;
                self.x += cw;
            }
        } else {
            for (ch, ch_col) in token {
                let cw = measure_char_width::<R>(*ch, font_size, font);
                if let Some(col) = self.fit(cw, *ch_col, step) {
                    return Some(col);
                }
                if self.on_target_row() {
                    if click_x < self.x + cw * 0.6 {
                        return Some(*ch_col);
                    }
                    self.row_end_col = *ch_col + 1;
                }
                self.x += cw;
            }
        }
        None
    }
}

impl<Message> Editor<'_, Message> {
    /// Visual (x, y) of source column `col` on line `line_idx`.
    pub(super) fn position_for_col<R: Measure>(
        &self,
        line_idx: usize,
        col: usize,
        available_width: f32,
        is_editing: bool,
        active_col: Option<usize>,
    ) -> (f32, f32) {
        let Some(line) = self.lines.get(line_idx) else {
            return (0.0, 0.0);
        };
        if line.is_code_block {
            return (code_x_for_col::<R>(line, col, is_editing), 0.0);
        }

        let mut pen = Pen {
            x: 0.0,
            y: 0.0,
            max_w: wrap_width(available_width),
        };
        let mut source_col = 0usize;

        for (span_idx, span) in line.spans.iter().enumerate() {
            let font = span_font(span, line);
            let span_editing = span_is_editing(line, span_idx, is_editing, active_col);
            let display = span_visible_text(line, span_idx, is_editing, active_col);
            let span_end_col = source_col_after_span(span, source_col);
            if display.is_empty() {
                if col <= span_end_col {
                    return (pen.x, pen.y);
                }
                source_col = span_end_col;
                continue;
            }

            let step = visual_line_step(span.font_size);
            let mut token = Vec::new();

            for ch in display.chars() {
                if span.is_checkbox && !is_editing {
                    if source_col >= col {
                        return (pen.x, pen.y);
                    }
                    pen.x += CHECKBOX_ADVANCE;
                    source_col += 1;
                    continue;
                }

                if span.is_math && !span_editing {
                    let tex = math_source(span);
                    if !tex.is_empty() && !span.is_syntax {
                        // Rendered math is one atomic box.
                        let width = self
                            .math_cache
                            .get(tex)
                            .map(|m| m.width)
                            .unwrap_or_else(|| measure_width::<R>(tex, span.font_size, font));
                        pen.fit(width, step);
                        if source_col >= col {
                            return (pen.x, pen.y);
                        }
                        pen.x += width + INLINE_MATH_GAP;
                        if col <= span_end_col {
                            return (pen.x, pen.y);
                        }
                        break;
                    }
                }

                token.push((ch, source_col));
                source_col += 1;
                if ch.is_whitespace() {
                    if let Some(pos) =
                        pen.advance_token::<R>(&token, col, span.font_size, font, step)
                    {
                        return pos;
                    }
                    token.clear();
                }
            }
            if let Some(pos) = pen.advance_token::<R>(&token, col, span.font_size, font, step) {
                return pos;
            }
            if col <= span_end_col {
                return (pen.x, pen.y);
            }
            source_col = span_end_col;
        }
        (pen.x, pen.y)
    }

    /// Source column at `click_x` on the visual row containing `line_y`, both
    /// relative to the line's text origin.
    pub(super) fn col_for_visual_point<R: Measure>(
        &self,
        line: &StyledLine,
        click_x: f32,
        line_y: f32,
        available_width: f32,
        is_editing: bool,
        active_col: Option<usize>,
    ) -> usize {
        if click_x <= 0.0 {
            return 0;
        }

        let mut scan = RowScan {
            x: 0.0,
            row_y: 0.0,
            row_start_col: 0,
            row_end_col: 0,
            row_step: BASE_LINE_HEIGHT,
            target_y: line_y,
            max_w: wrap_width(available_width),
        };
        let mut source_col = 0usize;

        for (span_idx, span) in line.spans.iter().enumerate() {
            let font = span_font(span, line);
            let span_editing = span_is_editing(line, span_idx, is_editing, active_col);
            let display = span_visible_text(line, span_idx, is_editing, active_col);
            let span_end_col = source_col_after_span(span, source_col);

            if display.is_empty() {
                source_col = span_end_col;
                scan.row_end_col = source_col;
                continue;
            }

            let step = visual_line_step(span.font_size);
            scan.row_step = scan.row_step.max(step);
            let mut token = Vec::new();

            for ch in display.chars() {
                if span.is_checkbox && !is_editing {
                    let cw = CHECKBOX_ADVANCE;
                    if let Some(col) = scan.fit(cw, source_col, step) {
                        return col;
                    }
                    if scan.on_target_row() {
                        if click_x < scan.x + cw * 0.6 {
                            return source_col;
                        }
                        scan.row_end_col = source_col + 1;
                    }
                    scan.x += cw;
                    source_col += 1;
                    continue;
                }

                if span.is_math && !span_editing {
                    let tex = math_source(span);
                    if !tex.is_empty() && !span.is_syntax {
                        let (width, height) = self
                            .math_cache
                            .get(tex)
                            .map(|m| (m.width, m.height))
                            .unwrap_or_else(|| {
                                (
                                    measure_width::<R>(tex, span.font_size, font),
                                    BASE_LINE_HEIGHT,
                                )
                            });

                        let extra_h = (height - BASE_LINE_HEIGHT).max(0.0);
                        scan.row_step = scan.row_step.max(BASE_LINE_HEIGHT + extra_h);

                        if let Some(col) = scan.fit(width, source_col, step) {
                            return col;
                        }
                        if scan.on_target_row() {
                            if click_x < scan.x + width {
                                return source_col;
                            }
                            scan.row_end_col = span_end_col;
                        }

                        scan.x += width + INLINE_MATH_GAP;
                        break;
                    }
                }

                token.push((ch, source_col));
                source_col += 1;
                if ch.is_whitespace() {
                    if let Some(col) =
                        scan.scan_token::<R>(&token, click_x, span.font_size, font, step)
                    {
                        return col;
                    }
                    token.clear();
                }
            }
            if let Some(col) = scan.scan_token::<R>(&token, click_x, span.font_size, font, step) {
                return col;
            }

            source_col = span_end_col;
            if scan.on_target_row() {
                scan.row_end_col = source_col;
            }
        }

        if scan.on_target_row() {
            scan.row_end_col.max(scan.row_start_col)
        } else {
            source_col
        }
    }

    /// Visual position of the caret within its line.
    pub(super) fn cursor_position<R: Measure>(
        &self,
        line_idx: usize,
        available_width: f32,
    ) -> (f32, f32) {
        let Some(line) = self.lines.get(line_idx) else {
            return (0.0, 0.0);
        };
        self.position_for_col::<R>(
            line_idx,
            self.buffer.cursor_col,
            available_width,
            self.is_block_editing(line, true),
            self.active_col(line_idx, true),
        )
    }

    /// Convert a position relative to the content bounds into (line, col).
    pub(super) fn hit_test<R: Measure>(
        &self,
        pos: Point,
        available_width: f32,
        focused: bool,
        state: &State,
    ) -> (usize, usize) {
        let line_idx = self.line_at_widget_y(pos.y, state).unwrap_or(0);
        let line_top = self.widget_y_for_line(line_idx, state);

        let Some(line) = self.lines.get(line_idx) else {
            return (line_idx, 0);
        };
        let click_x = pos.x - TEXT_X_OFFSET;
        if click_x <= 0.0 {
            return (line_idx, 0);
        }

        let col = self.col_for_visual_point::<R>(
            line,
            click_x,
            pos.y - line_top,
            available_width,
            self.is_block_editing(line, focused),
            self.active_col(line_idx, focused),
        );
        (line_idx, col)
    }

    /// Target of moving the caret one visual row up (`delta_lines < 0`) or
    /// down, keeping the remembered x.
    pub(super) fn move_visual<R: Measure>(
        &self,
        state: &mut State,
        delta_lines: f32,
        available_width: f32,
    ) -> (usize, usize) {
        if state.layout_tree.len() != self.lines.len() {
            self.rebuild_layout_tree::<R>(state, available_width);
        }
        let (cur_x, cur_y_in_line) =
            self.cursor_position::<R>(self.buffer.cursor_line, available_width);
        let cur_y_base = self.widget_y_for_line(self.buffer.cursor_line, state);

        let visual_x = *state.desired_visual_x.get_or_insert(cur_x);
        let line = &self.lines[self.buffer.cursor_line];
        let max_font = line
            .spans
            .iter()
            .map(|s| s.font_size)
            .fold(DEFAULT_FONT_SIZE, f32::max);
        let step = visual_line_step(max_font);

        let target_y = cur_y_base + cur_y_in_line + delta_lines * step + step / 2.0;

        let mut target = self.hit_test::<R>(
            Point::new(visual_x + TEXT_X_OFFSET, target_y),
            available_width,
            state.is_focused,
            state,
        );

        // Wrapping estimates can put the target on the wrong side of the
        // caret; never let a move go backwards.
        let current = (self.buffer.cursor_line, self.buffer.cursor_col);
        if delta_lines > 0.0 && target <= current {
            target = self.adjacent_line_target::<R>(visual_x, 1, available_width);
        } else if delta_lines < 0.0 && target >= current {
            target = self.adjacent_line_target::<R>(visual_x, -1, available_width);
        }

        target
    }

    /// Column at `visual_x` on the first row of the neighbouring source line.
    fn adjacent_line_target<R: Measure>(
        &self,
        visual_x: f32,
        delta_lines: isize,
        available_width: f32,
    ) -> (usize, usize) {
        let current_line = self.buffer.cursor_line;
        let target_line = if delta_lines < 0 {
            current_line.saturating_sub(1)
        } else {
            (current_line + 1).min(self.lines.len().saturating_sub(1))
        };

        if target_line == current_line {
            return (self.buffer.cursor_line, self.buffer.cursor_col);
        }

        let Some(line) = self.lines.get(target_line) else {
            return (self.buffer.cursor_line, self.buffer.cursor_col);
        };
        let col = self.col_for_visual_point::<R>(
            line,
            visual_x,
            BASE_LINE_HEIGHT / 2.0,
            available_width,
            self.is_block_editing(line, true),
            None,
        );
        (target_line, col)
    }
}

/// X of `col` in an unwrapped code line.
pub(super) fn code_x_for_col<R: Measure>(line: &StyledLine, col: usize, is_editing: bool) -> f32 {
    let mut x = 0.0_f32;
    let mut source_col = 0usize;
    for span in &line.spans {
        for ch in span.visible_text(is_editing).chars() {
            if source_col >= col {
                return x;
            }
            x += measure_char_width::<R>(ch, CODE_FONT_SIZE, iced::Font::MONOSPACE);
            source_col += 1;
        }
    }
    x
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::super::layout::line_height_for;
    use super::super::test_support::{editor_for, focused_state};
    use super::*;
    use crate::editor::buffer::{DocBuffer, EditorCommand};
    use crate::editor::highlight::highlight_markdown;

    #[test]
    fn visual_down_movement_is_monotonic_through_wrapped_markdown_lines() {
        let text = concat!(
            "alpha **bold text with enough words to wrap around the editor width** omega\n",
            "second line with `inline code` and more words to move through\n",
            "third line ends here"
        );
        let mut buffer = DocBuffer::from_text(text);
        buffer.execute(EditorCommand::SetCursor { line: 0, col: 0 });
        let image_cache = HashMap::new();
        let math_cache = HashMap::new();
        let mut previous = (buffer.cursor_line, buffer.cursor_col);

        for _ in 0..12 {
            let lines = highlight_markdown(&buffer.text());
            let editor = editor_for(&buffer, &lines, &image_cache, &math_cache);
            let mut state = focused_state();
            let next = editor.move_visual::<iced::Renderer>(&mut state, 1.0, 260.0);

            if next == previous {
                assert_eq!(next.0, lines.len().saturating_sub(1));
                break;
            }
            assert!(
                next > previous,
                "visual down must move forward, previous={previous:?}, next={next:?}"
            );
            drop(editor);
            buffer.execute(EditorCommand::SetCursor {
                line: next.0,
                col: next.1,
            });
            previous = next;
        }

        assert_eq!(previous.0, 2);
    }

    #[test]
    fn visual_down_moves_through_empty_lines_without_vanishing() {
        let text = "first\n\nthird\n\nfifth";
        let mut buffer = DocBuffer::from_text(text);
        buffer.execute(EditorCommand::SetCursor { line: 0, col: 2 });
        let image_cache = HashMap::new();
        let math_cache = HashMap::new();

        let mut visited = Vec::new();
        for _ in 0..8 {
            let lines = highlight_markdown(&buffer.text());
            let editor = editor_for(&buffer, &lines, &image_cache, &math_cache);
            let mut state = focused_state();
            let next = editor.move_visual::<iced::Renderer>(&mut state, 1.0, 900.0);
            visited.push(next);
            drop(editor);
            buffer.execute(EditorCommand::SetCursor {
                line: next.0,
                col: next.1,
            });
            if next.0 == lines.len().saturating_sub(1) {
                break;
            }
        }

        assert!(
            visited.iter().any(|(line, col)| *line == 1 && *col == 0),
            "down should visit first empty line, visited={visited:?}"
        );
        assert!(
            visited.iter().any(|(line, col)| *line == 3 && *col == 0),
            "down should visit second empty line, visited={visited:?}"
        );
        assert_eq!(buffer.cursor_line, 4);
    }

    #[test]
    fn trailing_empty_line_after_enter_has_visible_cursor_geometry() {
        let mut buffer = DocBuffer::from_text("first");
        buffer.execute(EditorCommand::SetCursor { line: 0, col: 5 });
        buffer.execute(EditorCommand::InsertText("\n".to_string()));

        let lines = highlight_markdown(&buffer.text());
        let image_cache = HashMap::new();
        let math_cache = HashMap::new();
        let editor = editor_for(&buffer, &lines, &image_cache, &math_cache);
        let mut seen_math_blocks = HashSet::new();

        assert_eq!((buffer.cursor_line, buffer.cursor_col), (1, 0));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].spans.len(), 1);
        assert_eq!(lines[1].spans[0].visible_text(false), "");

        let height = line_height_for::<iced::Renderer>(
            &lines[1],
            &image_cache,
            &math_cache,
            900.0,
            false,
            Some(0),
            &mut seen_math_blocks,
        );
        let cursor = editor.cursor_position::<iced::Renderer>(1, 900.0);

        assert_eq!(height, BASE_LINE_HEIGHT);
        assert_eq!(cursor, (0.0, 0.0));
        assert!(height.min(20.0) > 0.0);
    }
}

//! Mapping between source columns and visual positions.
//!
//! Positions are relative to a line's text origin: the left edge of the text
//! column and the top of the line's body. Inline lines read their [`Flow`];
//! code lines never wrap and are measured directly.

use iced::Point;

use super::flow::Flow;
use super::measure::measure_char_width;
use super::metrics::*;
use super::{Editor, Measure, State};
use crate::editor::highlight::StyledLine;

/// The caret's rectangle within its line.
pub(super) struct CaretBox {
    pub x: f32,
    pub y: f32,
    pub height: f32,
}

/// Caret rectangle in a code line, whose rows are all one height.
const CODE_CARET_TOP: f32 = 12.0;
const CODE_CARET_HEIGHT: f32 = 22.0;

impl<Message> Editor<'_, Message> {
    /// Where the caret for column `col` of line `line_idx` is drawn. Code
    /// lines are positioned as if unscrolled.
    pub(super) fn caret_box<R: Measure>(
        &self,
        line_idx: usize,
        col: usize,
        available_width: f32,
        is_editing: bool,
        active_col: Option<usize>,
    ) -> CaretBox {
        let Some(line) = self.lines.get(line_idx) else {
            return CaretBox {
                x: 0.0,
                y: 0.0,
                height: 0.0,
            };
        };
        if line.is_code_block {
            return CaretBox {
                x: code_x_for_col::<R>(line, col, is_editing),
                y: CODE_CARET_TOP,
                height: CODE_CARET_HEIGHT,
            };
        }

        let flow = self.flow::<R>(line_idx, available_width, is_editing, active_col);
        caret_in_flow::<R>(&flow, col)
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
        let Some(line) = self.lines.get(line_idx) else {
            return (line_idx, 0);
        };
        let is_editing = self.is_block_editing(line, focused);
        let x = pos.x - TEXT_X_OFFSET;

        if line.is_code_block {
            let scroll = self
                .scroll_extent::<R>(state, line.block_id, available_width, focused)
                .map_or(0.0, |extent| state.block_scroll(line.block_id, &extent));
            return (line_idx, code_col_at::<R>(line, x + scroll, is_editing));
        }

        let flow = self.flow::<R>(
            line_idx,
            available_width,
            is_editing,
            self.active_col(line_idx, focused),
        );
        let y = pos.y - self.line_body_top(line_idx, state, focused);
        (line_idx, flow.col_at::<R>(x, y))
    }

    /// Target of moving the caret one visual row up (`delta_lines < 0`) or
    /// down, keeping the remembered x.
    pub(super) fn move_visual<R: Measure>(
        &self,
        state: &mut State,
        delta_lines: f32,
        available_width: f32,
    ) -> (usize, usize) {
        let (line_idx, col) = (self.buffer.cursor_line, self.buffer.cursor_col);
        let Some(line) = self.lines.get(line_idx) else {
            return (line_idx, col);
        };
        let is_editing = self.is_block_editing(line, true);
        let down = delta_lines > 0.0;

        if line.is_code_block {
            let x = code_x_for_col::<R>(line, col, is_editing);
            let visual_x = *state.desired_visual_x.get_or_insert(x);
            return self.adjacent_line_target::<R>(visual_x, down, available_width);
        }

        let flow = self.flow::<R>(line_idx, available_width, is_editing, Some(col));
        let spot = flow.caret::<R>(col);
        let visual_x = *state.desired_visual_x.get_or_insert(spot.x);

        let target_row = if down {
            Some(spot.row + 1).filter(|row| *row < flow.rows.len())
        } else {
            spot.row.checked_sub(1)
        };
        match target_row {
            Some(row) => {
                let row = &flow.rows[row];
                let target_col = flow.col_at::<R>(visual_x, row.top + row.height / 2.0);
                (line_idx, target_col)
            }
            None => self.adjacent_line_target::<R>(visual_x, down, available_width),
        }
    }

    /// Column at `visual_x` on the nearest row of the next (`down`) or
    /// previous source line. Stays put at either end of the document.
    fn adjacent_line_target<R: Measure>(
        &self,
        visual_x: f32,
        down: bool,
        available_width: f32,
    ) -> (usize, usize) {
        let current = (self.buffer.cursor_line, self.buffer.cursor_col);
        let target_line = if down {
            current.0 + 1
        } else {
            match current.0.checked_sub(1) {
                Some(line) => line,
                None => return current,
            }
        };
        let Some(line) = self.lines.get(target_line) else {
            return current;
        };

        let is_editing = self.is_block_editing(line, true);
        if line.is_code_block {
            return (target_line, code_col_at::<R>(line, visual_x, is_editing));
        }
        let flow = self.flow::<R>(target_line, available_width, is_editing, None);
        let row = if down {
            &flow.rows[0]
        } else {
            &flow.rows[flow.rows.len() - 1]
        };
        (
            target_line,
            flow.col_at::<R>(visual_x, row.top + row.height / 2.0),
        )
    }
}

/// Caret rectangle for `col`, centred on the text line box it sits in.
pub(super) fn caret_in_flow<R: Measure>(flow: &Flow<'_>, col: usize) -> CaretBox {
    let spot = flow.caret::<R>(col);
    let font_size = spot.font_size;
    let height = font_size + 2.0;
    CaretBox {
        x: spot.x,
        y: flow.text_top(spot.row, font_size) + (font_size * LINE_BOX_FACTOR - height) / 2.0,
        height,
    }
}

/// Visible characters of a code line with their source columns.
fn code_chars(line: &StyledLine, is_editing: bool) -> impl Iterator<Item = (usize, char)> + '_ {
    line.spans
        .iter()
        .flat_map(move |span| span.visible_text(is_editing).chars())
        .enumerate()
}

fn code_char_width<R: Measure>(ch: char) -> f32 {
    measure_char_width::<R>(ch, CODE_FONT_SIZE, iced::Font::MONOSPACE)
}

/// X of `col` in an unwrapped code line.
pub(super) fn code_x_for_col<R: Measure>(line: &StyledLine, col: usize, is_editing: bool) -> f32 {
    code_chars(line, is_editing)
        .take_while(|(char_col, _)| *char_col < col)
        .map(|(_, ch)| code_char_width::<R>(ch))
        .sum()
}

/// Column nearest `x` in an unwrapped code line.
fn code_col_at<R: Measure>(line: &StyledLine, x: f32, is_editing: bool) -> usize {
    let mut cx = 0.0;
    let mut count = 0;
    for (col, ch) in code_chars(line, is_editing) {
        let cw = code_char_width::<R>(ch);
        if x < cx + cw / 2.0 {
            return col;
        }
        cx += cw;
        count = col + 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::super::layout::line_height_for;
    use super::super::testing::{editor_for, focused_state};
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
        let caret = editor.caret_box::<iced::Renderer>(1, 0, 900.0, false, Some(0));

        assert_eq!(height, BASE_LINE_HEIGHT);
        assert_eq!(caret.x, 0.0);
        assert!(caret.y > 0.0 && caret.y + caret.height < height);
        assert!(height.min(20.0) > 0.0);
    }
}

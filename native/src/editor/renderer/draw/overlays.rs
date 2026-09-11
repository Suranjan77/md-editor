//! Highlights and the caret, painted over a line's background.

use iced::{Color, Rectangle};

use super::super::metrics::*;
use super::super::selection::cols_on_line;
use super::super::{Editor, Measure};
use super::primitives::{fill, rounded};
use super::{Frame, LineBox};
use crate::{search, theme};

const CARET_WIDTH: f32 = 2.0;

impl<Message> Editor<'_, Message> {
    pub(super) fn paint_selection<R: Measure>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
    ) {
        let Some(range) = frame.selection else {
            return;
        };
        let ((start_line, _), (end_line, _)) = range;
        if row.idx < start_line || row.idx > end_line {
            return;
        }

        let line_len = self.buffer.line_text(row.idx).chars().count();
        let (from_col, to_col) = cols_on_line(range, row.idx, line_len);
        if from_col >= to_col {
            return;
        }
        let rect = self.highlight_rect::<R>(frame, row, from_col, to_col, 3.0, 4.0);
        fill(
            renderer,
            rect,
            rounded(3.0),
            Color::from_rgba(0.69, 0.80, 0.78, 0.24),
        );
    }

    pub(super) fn paint_search_matches<R: Measure>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
    ) {
        if self.search_query.is_empty() {
            return;
        }
        let line_text = self.buffer.line_text(row.idx);
        for line_match in search::line_matches(
            &line_text,
            self.search_query,
            self.search_regex,
            self.search_match_case,
        ) {
            let from_col = line_match.start_col;
            let rect = self.highlight_rect::<R>(frame, row, from_col, line_match.end_col, 4.0, 5.0);
            let alpha = if self.active_search_match == Some((row.idx, from_col)) {
                0.45
            } else {
                0.24
            };
            fill(
                renderer,
                rect,
                rounded(3.0),
                Color::from_rgba(0.92, 0.70, 0.30, alpha),
            );
        }
    }

    /// Highlight box for columns `from_col..to_col` of a line. A range that
    /// wraps is highlighted from its start to the end of the text column.
    ///
    /// NOTE: only the first visual row of a wrapped range is highlighted.
    fn highlight_rect<R: Measure>(
        &self,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
        from_col: usize,
        to_col: usize,
        min_width: f32,
        inset: f32,
    ) -> Rectangle {
        let width = frame.bounds.width;
        let (from_x, from_y) =
            self.position_for_col::<R>(row.idx, from_col, width, row.is_editing, row.active_col);
        let (to_x, to_y) =
            self.position_for_col::<R>(row.idx, to_col, width, row.is_editing, row.active_col);
        let highlight_w = if (to_y - from_y).abs() < 1.0 {
            (to_x - from_x).max(min_width)
        } else {
            (text_column_width(width) - from_x).max(min_width)
        };
        Rectangle {
            x: frame.bounds.x + TEXT_X_OFFSET + from_x,
            y: row.y + from_y + inset,
            width: highlight_w,
            height: (BASE_LINE_HEIGHT - 2.0 * inset).max(16.0),
        }
    }

    /// The caret, if it is on this line. Code lines paint their own.
    pub(super) fn paint_caret<R: Measure>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
    ) {
        if !frame.focused || row.idx != self.buffer.cursor_line {
            return;
        }

        let font_size = self.caret_font_size(row);
        let caret_h = font_size + 2.0;
        if caret_h <= 0.0 {
            return;
        }

        let (cx, cy) = self.cursor_position::<R>(row.idx, frame.bounds.width);
        // Center the caret in the row, matching how text is centered.
        let centering_offset = (visual_line_step(font_size) - caret_h) / 2.0;

        fill(
            renderer,
            Rectangle {
                x: frame.bounds.x + TEXT_X_OFFSET + cx,
                y: row.y + cy + centering_offset,
                width: CARET_WIDTH,
                height: caret_h,
            },
            rounded(1.0),
            theme::ACCENT,
        );
    }

    /// Font size of the span holding the caret, measured in source columns.
    fn caret_font_size(&self, row: &LineBox<'_>) -> f32 {
        let col = self.buffer.cursor_col;
        let mut span_start = 0;
        for span in &row.line.spans {
            let span_end = span_start + span.visible_text(true).chars().count();
            if col >= span_start && col <= span_end {
                return span.font_size;
            }
            span_start = span_end;
        }
        row.line
            .spans
            .first()
            .map_or(DEFAULT_FONT_SIZE, |span| span.font_size)
    }
}

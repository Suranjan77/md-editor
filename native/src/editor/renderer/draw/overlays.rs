//! Highlights and the caret, painted over a line's background.

use iced::{Color, Rectangle};

use super::super::caret::caret_in_flow;
use super::super::flow::Flow;
use super::super::metrics::*;
use super::super::selection::cols_on_line;
use super::super::{Editor, Measure};
use super::primitives::{fill, rounded};
use super::{Frame, LineBox, LineKind};
use crate::{search, theme};

const CARET_WIDTH: f32 = 2.0;
const MIN_HIGHLIGHT_HEIGHT: f32 = 16.0;

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
        let color = Color::from_rgba(0.69, 0.80, 0.78, 0.24);
        for rect in self.highlight_rects::<R>(frame, row, from_col, to_col, 3.0, 4.0) {
            fill(renderer, rect, rounded(3.0), color);
        }
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
            let alpha = if self.active_search_match == Some((row.idx, from_col)) {
                0.45
            } else {
                0.24
            };
            let color = Color::from_rgba(0.92, 0.70, 0.30, alpha);
            for rect in
                self.highlight_rects::<R>(frame, row, from_col, line_match.end_col, 4.0, 5.0)
            {
                fill(renderer, rect, rounded(3.0), color);
            }
        }
    }

    /// Boxes highlighting source columns `from_col..to_col` of a line: one per
    /// visual row the range touches. Lines drawn as a whole — rendered tables,
    /// equations and rules — are highlighted across the column.
    fn highlight_rects<R: Measure>(
        &self,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
        from_col: usize,
        to_col: usize,
        min_width: f32,
        inset: f32,
    ) -> Vec<Rectangle> {
        let left = frame.bounds.x + TEXT_X_OFFSET;
        let rect = |x0: f32, x1: f32, top: f32, height: f32| Rectangle {
            x: left + x0,
            y: row.y + top + inset,
            width: (x1 - x0).max(min_width),
            height: (height - 2.0 * inset).max(MIN_HIGHLIGHT_HEIGHT),
        };

        match &row.kind {
            LineKind::Flow(flow) => flow
                .range_extents::<R>(from_col, to_col)
                .into_iter()
                .map(|(r, x0, x1)| rect(x0, x1, flow.rows[r].top, flow.rows[r].height))
                .collect(),
            LineKind::Code => self
                .code_range_x::<R>(frame, row, from_col, to_col)
                .map(|(x0, x1)| rect(x0, x1, 0.0, row.height))
                .into_iter()
                .collect(),
            LineKind::Table | LineKind::BlockMath | LineKind::Rule if row.height > 0.0 => {
                vec![rect(
                    0.0,
                    text_column_width(frame.bounds.width),
                    0.0,
                    row.height,
                )]
            }
            _ => Vec::new(),
        }
    }

    /// The caret, if it is on this line. Code lines paint their own.
    pub(super) fn paint_caret<R: Measure>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
        flow: &Flow<'_>,
    ) {
        if !frame.focused || row.idx != self.buffer.cursor_line {
            return;
        }
        let caret = caret_in_flow::<R>(flow, self.buffer.cursor_col);
        fill(
            renderer,
            Rectangle {
                x: frame.bounds.x + TEXT_X_OFFSET + caret.x,
                y: row.y + caret.y,
                width: CARET_WIDTH,
                height: caret.height,
            },
            rounded(1.0),
            theme::ACCENT,
        );
    }
}

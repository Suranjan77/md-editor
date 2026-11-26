//! Line numbers in the left page margin (Settings → Editor → Line numbers).

use iced::alignment::{Horizontal, Vertical};
use iced::{Point, Size};

use super::super::metrics::{body_row, text_left};
use super::primitives::TextRun;
use super::{Frame, LineBox, LineKind};
use crate::design::TextRole;
use crate::design::metrics::SPACE_3;
use crate::editor::renderer::Measure;

/// Below this much margin the numbers would crowd the text: none are drawn.
const MIN_GUTTER_WIDTH: f32 = 3.0 * SPACE_3;

/// Number every painted line by its source line, centred on its first row.
/// Lines that collapse (the tail of rendered block math) were never placed in
/// `rows`, so they get no number.
pub(super) fn paint_line_numbers<R: Measure>(renderer: &mut R, frame: &Frame, rows: &[LineBox]) {
    let width = text_left(frame.bounds.width) - SPACE_3;
    if width < MIN_GUTTER_WIDTH {
        return;
    }
    let t = crate::design::tokens();
    let role = TextRole::MonoCaption;
    for row in rows {
        let (top, height) = match &row.kind {
            LineKind::Flow(flow) => flow
                .rows
                .first()
                .map_or((0.0, row.height), |first| (first.top, first.height)),
            LineKind::BlockMath => (0.0, row.height.min(body_row())),
            LineKind::Rule | LineKind::Code | LineKind::Table => (0.0, row.height),
        };
        let color = if row.active_col.is_some() {
            t.fg_secondary
        } else {
            t.fg_tertiary
        };
        TextRun::new(
            (row.idx + 1).to_string(),
            role.size(),
            Size::new(width, height),
        )
        .font(role.font())
        .align(Horizontal::Right, Vertical::Center)
        .draw(
            renderer,
            // The point is the alignment anchor: right edge, vertical centre.
            Point::new(frame.bounds.x + width, row.y + top + height / 2.0),
            color,
            frame.viewport,
        );
    }
}

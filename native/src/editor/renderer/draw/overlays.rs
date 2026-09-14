//! Highlights and the caret, painted over a line's background.

use iced::border::Radius;
use iced::{Border, Color, Rectangle};

use super::super::caret::{caret_in_flow, code_x_for_col};
use super::super::flow::{Affinity, Flow};
use super::super::metrics::*;
use super::super::selection::{TextRange, cols_on_line};
use super::super::spans::span_is_editing;
use super::super::{Editor, Measure};
use super::primitives::{fill, rounded};
use super::{Frame, LineBox, LineKind};
use crate::{search, theme};

const CARET_WIDTH: f32 = 2.0;
const MIN_HIGHLIGHT_HEIGHT: f32 = 16.0;
const SELECTION_COLOR: Color = Color::from_rgba(0.69, 0.80, 0.78, 0.24);
const SELECTION_RADIUS: f32 = 4.0;
/// Width of the band that marks a selected line break.
const NEWLINE_TAIL: f32 = 8.0;

/// One piece of the selection's shape.
pub(super) struct SelectionPiece {
    bounds: Rectangle,
    border: Border,
    /// Painted over the line's content rather than under it.
    over_content: bool,
}

/// Paint the pieces of a selection `shape` that go on one side of the content.
///
/// Renderers draw each layer's quads before its images and text, whatever
/// order they were drawn in, so pieces over the content get a layer of their
/// own, clipped to `viewport`, which renders after the content's.
pub(super) fn paint_selection<R: Measure>(
    renderer: &mut R,
    shape: &[SelectionPiece],
    over_content: bool,
    viewport: Rectangle,
) {
    let mut pieces = shape
        .iter()
        .filter(|piece| piece.over_content == over_content)
        .peekable();
    if pieces.peek().is_none() {
        return;
    }
    let paint = |renderer: &mut R| {
        for piece in pieces {
            fill(renderer, piece.bounds, piece.border, SELECTION_COLOR);
        }
    };
    if over_content {
        renderer.with_layer(viewport, paint);
    } else {
        paint(renderer);
    }
}

impl<Message> Editor<'_, Message> {
    /// The selection over the visible `lines`, as one continuous shape: a
    /// full-height band per visual row it touches, bridged across the space
    /// between lines, and rounded only at the corners that stick out.
    ///
    /// Bands over blocks drawn whole — tables, images, equations, rules — are
    /// painted over them, since an opaque table header or picture would hide
    /// them from below. Everything else goes under the text, which a
    /// translucent band on top would dim.
    pub(super) fn selection_shape<R: Measure>(
        &self,
        frame: &Frame<'_>,
        lines: &[LineBox<'_>],
    ) -> Vec<SelectionPiece> {
        let Some(range) = frame.selection else {
            return Vec::new();
        };
        let bands: Vec<(Rectangle, bool)> = lines
            .iter()
            .flat_map(|row| self.selection_bands::<R>(frame, row, range))
            .collect();

        let right = |r: &Rectangle| r.x + r.width;
        let touching = |a: &Rectangle, b: &Rectangle| right(a).min(right(b)) > a.x.max(b.x);
        let mut shape = Vec::with_capacity(bands.len() * 2);
        for (i, &(band, over_content)) in bands.iter().enumerate() {
            let above = i
                .checked_sub(1)
                .map(|j| &bands[j].0)
                .filter(|a| touching(a, &band));
            let below = bands
                .get(i + 1)
                .map(|(b, _)| b)
                .filter(|b| touching(&band, b));
            let r = SELECTION_RADIUS
                .min(band.width / 2.0)
                .min(band.height / 2.0);
            let corner = |sticks_out: bool| if sticks_out { r } else { 0.0 };
            let radius = Radius {
                top_left: corner(above.is_none_or(|a| band.x < a.x)),
                top_right: corner(above.is_none_or(|a| right(&band) > right(a))),
                bottom_right: corner(below.is_none_or(|b| right(&band) > right(b))),
                bottom_left: corner(below.is_none_or(|b| band.x < b.x)),
            };
            shape.push(SelectionPiece {
                bounds: band,
                border: Border {
                    radius,
                    ..Border::default()
                },
                over_content,
            });

            if let Some(below) = below {
                let gap_top = band.y + band.height;
                if below.y > gap_top {
                    let x = band.x.max(below.x);
                    shape.push(SelectionPiece {
                        bounds: Rectangle {
                            x,
                            y: gap_top,
                            width: right(&band).min(right(below)) - x,
                            height: below.y - gap_top,
                        },
                        border: Border::default(),
                        over_content: false,
                    });
                }
            }
        }
        shape
    }

    /// The selection's bands on one line, top to bottom, in window
    /// coordinates, each marked with whether it goes over the line's content.
    /// A selection that continues past the line's end gets a short tail there,
    /// standing for the selected line break.
    fn selection_bands<R: Measure>(
        &self,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
        range: TextRange,
    ) -> Vec<(Rectangle, bool)> {
        let ((start_line, _), (end_line, _)) = range;
        if row.idx < start_line || row.idx > end_line || row.height <= 0.0 {
            return Vec::new();
        }
        let line_len = self.buffer.line_text(row.idx).chars().count();
        let (from, to) = cols_on_line(range, row.idx, line_len);
        let continues = row.idx < end_line;
        if from >= to && !continues {
            return Vec::new();
        }
        let tail = if continues { NEWLINE_TAIL } else { 0.0 };
        let left = frame.bounds.x + text_left(frame.bounds.width);
        let band = |x0: f32, x1: f32, top: f32, height: f32| Rectangle {
            x: left + x0,
            y: row.y + top,
            width: x1 - x0,
            height,
        };

        let rendered_image = row.line.spans.iter().enumerate().any(|(idx, span)| {
            span.is_image && !span_is_editing(row.line, idx, row.is_editing, row.active_col)
        });
        let whole_line = || {
            vec![(
                band(0.0, text_column_width(frame.bounds.width), 0.0, row.height),
                true,
            )]
        };
        match &row.kind {
            // A rendered image is selected whole, like a table or an equation.
            LineKind::Flow(_) if rendered_image => whole_line(),
            LineKind::Flow(flow) => {
                let mut extents = flow.range_extents::<R>(from, to);
                match extents.last_mut() {
                    Some(last) => last.2 += tail,
                    None => {
                        let spot = flow.caret::<R>(to, Affinity::Downstream);
                        extents.push((spot.row, spot.x, spot.x + tail));
                    }
                }
                extents
                    .into_iter()
                    .filter(|(_, x0, x1)| x1 > x0)
                    .filter_map(|(r, x0, x1)| {
                        let visual = flow.rows.get(r)?;
                        Some((band(x0, x1, visual.top, visual.height), false))
                    })
                    .collect()
            }
            LineKind::Code => {
                let (extent, scroll_x) = self.row_scroll::<R>(frame, row);
                let x_of = |col| code_x_for_col::<R>(row.line, col, row.is_editing) - scroll_x;
                let x0 = x_of(from).max(0.0);
                let x1 = (x_of(to) + tail).min(extent.viewport_w);
                if x1 > x0 {
                    vec![(band(x0, x1, 0.0, row.height), false)]
                } else {
                    Vec::new()
                }
            }
            LineKind::Table | LineKind::BlockMath | LineKind::Rule => whole_line(),
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
        let left = frame.bounds.x + text_left(frame.bounds.width);
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
        let caret = caret_in_flow::<R>(flow, self.buffer.cursor_col, self.buffer.cursor_affinity);
        let motion = &frame.state.caret_motion;
        let alpha = motion.alpha();
        if alpha <= 0.0 {
            return;
        }
        let offset = motion.offset();
        fill(
            renderer,
            Rectangle {
                x: frame.bounds.x + text_left(frame.bounds.width) + caret.x + offset.x,
                y: self.snap_px(row.y + caret.y + offset.y),
                width: CARET_WIDTH,
                height: caret.height,
            },
            rounded(1.0),
            Color {
                a: theme::ACCENT.a * alpha,
                ..theme::ACCENT
            },
        );
    }
}

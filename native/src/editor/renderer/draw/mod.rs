//! Painting.
//!
//! A frame paints in layers: the page background; the chrome of every block
//! that intersects the viewport (cards, captions, badges); then each visible
//! line — selection and search highlights first, then its content, then the
//! caret.
//!
//! Lines are painted by kind:
//!
//! | line                         | painter             |
//! |------------------------------|---------------------|
//! | horizontal rule              | `paint_rule`        |
//! | unstyled paragraph           | `paint_plain_line`  |
//! | code block line              | `paint_code_line`   |
//! | table row (not being edited) | `paint_table_row`   |
//! | anything else                | `paint_spans`       |

mod blocks;
mod captions;
mod inline;
mod overlays;
mod primitives;

use std::collections::HashMap;

use iced::advanced::renderer::Quad;
use iced::{Border, Rectangle};

use super::layout::{caption_band, is_first_block_line, table_block_gutter_after};
use super::metrics::{TOP_PAD, content_bounds};
use super::selection::TextRange;
use super::{Editor, Paint, State};
use crate::editor::highlight::StyledLine;
use crate::theme;
use blocks::BlockMeta;

/// What every painter needs to know about the frame being drawn.
struct Frame<'a> {
    state: &'a State,
    /// The content column, in window coordinates.
    bounds: Rectangle,
    viewport: Rectangle,
    focused: bool,
    /// Blocks with at least one visible line, keyed by block id.
    blocks: HashMap<usize, BlockMeta>,
    selection: Option<TextRange>,
}

/// A line placed in the frame.
struct LineBox<'a> {
    idx: usize,
    line: &'a StyledLine,
    /// Top of the line's body, below any caption band.
    y: f32,
    /// Height of the body, excluding caption band and scrollbar gutter.
    height: f32,
    is_editing: bool,
    active_col: Option<usize>,
}

/// Running figure and equation counts, for items the highlighter did not
/// number.
#[derive(Default)]
struct Counters {
    images: usize,
    equations: usize,
}

impl<Message> Editor<'_, Message> {
    pub(super) fn draw_document<R: Paint>(
        &self,
        state: &State,
        renderer: &mut R,
        layout_bounds: Rectangle,
        viewport: Rectangle,
    ) {
        let bounds = content_bounds(layout_bounds);
        let focused = state.is_focused;

        renderer.fill_quad(
            Quad {
                bounds,
                border: Border::default(),
                ..Default::default()
            },
            theme::BG_PRIMARY,
        );

        let (visible_start, visible_end) = self.visible_lines(state, bounds, viewport);
        let frame = Frame {
            state,
            bounds,
            viewport,
            focused,
            blocks: self.measure_blocks::<R>(state, bounds, focused, visible_start, visible_end),
            selection: self.painted_selection(state),
        };

        for (&block_id, meta) in &frame.blocks {
            self.paint_block_chrome(renderer, &frame, block_id, meta);
        }

        let mut counters = Counters::default();
        let mut y = bounds.y + TOP_PAD + state.layout_tree.prefix_sum(visible_start);
        for (idx, line) in self.lines.iter().enumerate().skip(visible_start) {
            let is_editing = self.is_block_editing(line, focused);
            let opens_block =
                (line.is_code_block || line.is_table_row) && is_first_block_line(self.lines, idx);
            let caption = caption_band(opens_block, is_editing);
            let gutter = table_block_gutter_after(self.lines, idx, is_editing);
            let height = (state.layout_tree.get_height(idx) - caption - gutter).max(0.0);
            y += caption;

            if y + height + gutter < viewport.y {
                y += height + gutter;
                continue;
            }
            if y > viewport.y + viewport.height {
                break;
            }
            // Block math paints as a unit on its first line; the rest collapse.
            if line.is_math_block && !is_editing && height == 0.0 {
                continue;
            }

            let row = LineBox {
                idx,
                line,
                y,
                height,
                is_editing,
                active_col: self.active_col(idx, focused),
            };
            self.paint_line(renderer, &frame, &row, &mut counters);
            y += height + gutter;
        }
    }

    /// Range of line indices intersecting the viewport.
    fn visible_lines(
        &self,
        state: &State,
        bounds: Rectangle,
        viewport: Rectangle,
    ) -> (usize, usize) {
        let start = state
            .layout_tree
            .find_line_at_y((viewport.y - bounds.y - TOP_PAD).max(0.0));
        let end = state
            .layout_tree
            .find_line_at_y((viewport.y + viewport.height - bounds.y - TOP_PAD).max(0.0))
            .saturating_add(1)
            .min(self.lines.len());
        (start, end)
    }

    fn paint_line<R: Paint>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
        counters: &mut Counters,
    ) {
        self.paint_selection(renderer, frame, row);
        self.paint_search_matches(renderer, frame, row);

        let line = row.line;
        if line.spans.iter().any(|s| s.is_rule) {
            self.paint_rule(renderer, frame, row);
            self.paint_caret(renderer, frame, row);
        } else if inline::is_plain_line(line, row.is_editing) {
            self.paint_plain_line(renderer, frame, row);
            self.paint_caret(renderer, frame, row);
        } else if line.is_code_block && !line.is_math_block {
            // Code lines draw their own caret, offset by the block's scroll.
            self.paint_code_line(renderer, frame, row);
        } else if line.is_table_row && !row.is_editing {
            self.paint_table_row(renderer, frame, row);
        } else {
            self.paint_spans(renderer, frame, row, counters);
            self.paint_caret(renderer, frame, row);
        }
    }
}

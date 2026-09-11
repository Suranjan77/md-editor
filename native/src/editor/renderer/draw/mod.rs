//! Painting.
//!
//! A frame paints in layers: the page background; the chrome of every block
//! that intersects the viewport (cards, captions, badges); each visible line —
//! selection and search highlights first, then its content, then the caret;
//! and finally the scrollbars of blocks wider than the column.
//!
//! Lines are painted by kind (see [`LineKind`]):
//!
//! | line                                   | painter            |
//! |----------------------------------------|--------------------|
//! | horizontal rule, source hidden         | `paint_rule`       |
//! | code block line                        | `paint_code_line`  |
//! | table row, not being edited            | `paint_table_row`  |
//! | block math, not being edited           | `paint_block_math` |
//! | everything else, laid out by [`Flow`]  | `paint_flow`       |

mod blocks;
mod captions;
mod inline;
mod overlays;
mod primitives;

use std::collections::HashMap;

use iced::advanced::renderer::Quad;
use iced::{Border, Rectangle};

use super::flow::Flow;
use super::layout::{caption_band, is_first_block_line, table_block_gutter_after};
use super::metrics::{TEXT_X_OFFSET, TOP_PAD, content_bounds};
use super::scroll::scrollbar_y;
use super::selection::TextRange;
use super::spans::span_is_editing;
use super::{Editor, Paint, State};
use crate::editor::highlight::StyledLine;
use crate::theme;
use blocks::BlockMeta;
use primitives::paint_scrollbar;

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

/// How a line is painted.
enum LineKind<'a> {
    /// A horizontal rule whose `---` source is hidden.
    Rule,
    Code,
    /// A table row shown as cells.
    Table,
    /// The first line of block math shown rendered; the rest have no height.
    BlockMath,
    /// Inline content laid out in rows.
    Flow(Flow<'a>),
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
    kind: LineKind<'a>,
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

            let active_col = self.active_col(idx, focused);
            let row = LineBox {
                idx,
                line,
                y,
                height,
                is_editing,
                active_col,
                kind: self.line_kind::<R>(idx, bounds.width, is_editing, active_col),
            };
            self.paint_line(renderer, &frame, &row, &mut counters);
            y += height + gutter;
        }

        for (&block_id, meta) in &frame.blocks {
            if let Some(extent) = meta.scroll.filter(|extent| extent.overflows()) {
                paint_scrollbar(
                    renderer,
                    state,
                    block_id,
                    bounds.x + TEXT_X_OFFSET,
                    &extent,
                    scrollbar_y(&self.lines[meta.start], meta.y, meta.height),
                );
            }
        }
    }

    fn line_kind<R: Paint>(
        &self,
        idx: usize,
        available_width: f32,
        is_editing: bool,
        active_col: Option<usize>,
    ) -> LineKind<'_> {
        let line = &self.lines[idx];
        let rule_hidden = line.spans.first().is_some_and(|span| span.is_rule)
            && !span_is_editing(line, 0, is_editing, active_col);
        if line.is_code_block {
            LineKind::Code
        } else if line.is_table_row && !is_editing {
            LineKind::Table
        } else if line.is_math_block && !is_editing {
            LineKind::BlockMath
        } else if rule_hidden {
            LineKind::Rule
        } else {
            LineKind::Flow(self.flow::<R>(idx, available_width, is_editing, active_col))
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

        match &row.kind {
            LineKind::Rule => self.paint_rule(renderer, frame, row),
            LineKind::Code => self.paint_code_line(renderer, frame, row),
            LineKind::Table => self.paint_table_row(renderer, frame, row),
            LineKind::BlockMath => self.paint_block_math(renderer, frame, row, counters),
            LineKind::Flow(flow) => {
                self.paint_flow(renderer, frame, row, flow, counters);
                self.paint_caret(renderer, frame, row, flow);
            }
        }
    }
}

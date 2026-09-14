//! Horizontal scrolling of blocks wider than the text column: code blocks,
//! tables and block math. Each block keeps its own offset in [`State`], moved
//! by shift+wheel, horizontal wheel, or dragging the block's scrollbar.
//!
//! [`Editor::scroll_extent`] is the single answer to "how wide is this block
//! and how much of it shows"; painting, the wheel, the scrollbar and hit
//! testing all ask it, so the thumb, the drag range and the content agree.

use iced::{Point, mouse};

use super::measure::{measure_width, span_font};
use super::metrics::*;
use super::spans::math_source;
use super::{Editor, MATH_BLOCK_SCALE, Measure, State};
use crate::editor::highlight::StyledLine;

const MIN_THUMB_WIDTH: f32 = 32.0;
/// Pixels scrolled per wheel "line".
const WHEEL_LINE_PX: f32 = 48.0;
/// How far above and below a scrollbar track a press still grabs it.
const GRAB_SLOP_ABOVE: f32 = 8.0;
const GRAB_SLOP_BELOW: f32 = 10.0;
/// Narrowest a table column is drawn.
pub(super) const MIN_COLUMN_WIDTH: f32 = 42.0;

/// How much of a block shows, and how wide all of it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ScrollExtent {
    pub viewport_w: f32,
    pub content_w: f32,
}

impl ScrollExtent {
    pub fn max_scroll(&self) -> f32 {
        (self.content_w - self.viewport_w).max(0.0)
    }

    /// Whether the content overflows enough to scroll.
    pub fn overflows(&self) -> bool {
        self.content_w > self.viewport_w + 1.0
    }
}

/// An in-progress drag of a block's scrollbar thumb.
#[derive(Debug, Clone, Copy)]
pub(super) struct HorizontalScrollDrag {
    block_id: usize,
    viewport_x: f32,
    extent: ScrollExtent,
    /// Where on the thumb the pointer grabbed it.
    grab_offset: f32,
}

impl HorizontalScrollDrag {
    pub fn block_id(&self) -> usize {
        self.block_id
    }

    /// Scroll offset that keeps the grabbed point of the thumb under
    /// `pointer_x`.
    pub fn scroll_for_pointer(&self, pointer_x: f32) -> f32 {
        let bar = scrollbar_geometry(self.viewport_x, &self.extent, 0.0);
        let track_range = (bar.track_w - bar.thumb_w).max(1.0);
        let thumb_x = (pointer_x - self.viewport_x - self.grab_offset).clamp(0.0, track_range);
        (thumb_x / track_range) * self.extent.max_scroll()
    }
}

/// A scrollbar's track and thumb along the x axis.
pub(super) struct ScrollbarGeometry {
    pub track_w: f32,
    pub thumb_x: f32,
    pub thumb_w: f32,
}

/// Scrollbar geometry for a viewport at `viewport_x` scrolled by `scroll`.
pub(super) fn scrollbar_geometry(
    viewport_x: f32,
    extent: &ScrollExtent,
    scroll: f32,
) -> ScrollbarGeometry {
    let track_w = extent.viewport_w.max(1.0);
    let thumb_w = (track_w * (extent.viewport_w / extent.content_w))
        .clamp(MIN_THUMB_WIDTH.min(track_w), track_w);
    let max_scroll = extent.max_scroll();
    let progress = if max_scroll > 0.0 {
        scroll / max_scroll
    } else {
        0.0
    };
    ScrollbarGeometry {
        track_w,
        thumb_x: viewport_x + (track_w - thumb_w) * progress,
        thumb_w,
    }
}

/// Y of the scrollbar of a block spanning `top..top + height`.
pub(super) fn scrollbar_y(first_line: &StyledLine, top: f32, height: f32) -> f32 {
    let bottom = top + height;
    if first_line.is_table_row {
        // Tables keep a gutter under their last row for it.
        bottom - HORIZONTAL_SCROLLBAR_GUTTER + 5.0
    } else {
        bottom - SCROLLBAR_BOTTOM_OFFSET
    }
}

/// Width of each table column, padding included, across `rows`.
pub(super) fn table_columns<R: Measure>(rows: &[StyledLine]) -> Vec<f32> {
    let mut columns: Vec<f32> = Vec::new();
    for row in rows.iter().filter(|row| row.is_table_row) {
        for (idx, cell) in row.table_cells.iter().enumerate() {
            let text_w: f32 = cell
                .iter()
                .map(|span| {
                    measure_width::<R>(
                        span.visible_text(false),
                        span.font_size,
                        span_font(span, row),
                    )
                })
                .sum();
            let width = (text_w + TABLE_CELL_PADDING).max(MIN_COLUMN_WIDTH);
            match columns.get_mut(idx) {
                Some(column) => *column = column.max(width),
                None => columns.push(width),
            }
        }
    }
    columns
}

impl State {
    /// Scroll offset of `block_id`, clamped to what its extent allows.
    pub(super) fn block_scroll(&self, block_id: usize, extent: &ScrollExtent) -> f32 {
        self.block_scroll_x
            .get(&block_id)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, extent.max_scroll())
    }
}

impl<Message> Editor<'_, Message> {
    /// The horizontal extent of a block, or `None` if it doesn't scroll:
    /// quotes, and tables and math while they are edited as source.
    pub(super) fn scroll_extent<R: Measure>(
        &self,
        state: &State,
        block_id: usize,
        available_width: f32,
        focused: bool,
    ) -> Option<ScrollExtent> {
        let &(start, end) = state.block_ranges.get(&block_id)?;
        let lines = self.lines.get(start..=end)?;
        let first = lines.first()?;
        let is_editing = self.is_block_editing(first, focused);

        if first.is_code_block {
            let widest = lines
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|span| {
                            measure_width::<R>(
                                span.visible_text(is_editing),
                                CODE_FONT_SIZE,
                                iced::Font::MONOSPACE,
                            )
                        })
                        .sum::<f32>()
                })
                .fold(0.0, f32::max);
            Some(ScrollExtent {
                viewport_w: code_viewport_width(available_width),
                content_w: widest + CODE_CONTENT_PADDING,
            })
        } else if first.is_table_row && !is_editing {
            Some(ScrollExtent {
                viewport_w: wrap_width(available_width),
                content_w: table_columns::<R>(lines).iter().sum(),
            })
        } else if first.is_math_block && !is_editing {
            let tex = lines
                .iter()
                .flat_map(|line| &line.spans)
                .map(math_source)
                .find(|tex| !tex.is_empty())?;
            Some(ScrollExtent {
                viewport_w: math_viewport_width(available_width),
                content_w: math_block_width::<R>(self.math_cache.get(tex).map(|m| m.width), tex),
            })
        } else {
            None
        }
    }

    /// The scrollable block under a widget-relative y.
    pub(super) fn block_at_y(&self, pos_y: f32, state: &State) -> Option<usize> {
        let line_idx = self.line_at_widget_y(pos_y, state)?;
        let line = self.lines.get(line_idx)?;
        if (line.is_code_block || line.is_table_row || line.is_math_block)
            && state.block_ranges.contains_key(&line.block_id)
        {
            Some(line.block_id)
        } else {
            None
        }
    }

    /// The scrollbar drag a press at `pos` would start, if it lands on one.
    pub(super) fn horizontal_scrollbar_hit<R: Measure>(
        &self,
        pos: Point,
        available_width: f32,
        state: &State,
    ) -> Option<HorizontalScrollDrag> {
        let block_id = self.block_at_y(pos.y, state)?;
        let extent = self.scroll_extent::<R>(state, block_id, available_width, state.is_focused)?;
        if !extent.overflows() {
            return None;
        }

        let &(start, end) = state.block_ranges.get(&block_id)?;
        let (block_y, block_h) = super::layout::lines_extent(state, start, end);
        let bar_y = scrollbar_y(&self.lines[start], block_y, block_h);
        let viewport_x = text_left(available_width);
        if pos.x < viewport_x
            || pos.x > viewport_x + extent.viewport_w
            || pos.y < bar_y - GRAB_SLOP_ABOVE
            || pos.y > bar_y + GRAB_SLOP_BELOW
        {
            return None;
        }

        let bar = scrollbar_geometry(viewport_x, &extent, state.block_scroll(block_id, &extent));
        let grab_offset = if pos.x >= bar.thumb_x && pos.x <= bar.thumb_x + bar.thumb_w {
            pos.x - bar.thumb_x
        } else {
            bar.thumb_w / 2.0
        };

        Some(HorizontalScrollDrag {
            block_id,
            viewport_x,
            extent,
            grab_offset,
        })
    }

    /// Apply a wheel event at `pos` to the scrollable block under it.
    pub(super) fn scroll_block_with_wheel<R: Measure>(
        &self,
        state: &mut State,
        pos: Point,
        available_width: f32,
        delta: &mouse::ScrollDelta,
    ) {
        let Some(block_id) = self.block_at_y(pos.y, state) else {
            return;
        };
        let Some(extent) =
            self.scroll_extent::<R>(state, block_id, available_width, state.is_focused)
        else {
            return;
        };
        let max_scroll = extent.max_scroll();
        if max_scroll <= 0.0 {
            return;
        }

        let (dx, dy) = match delta {
            mouse::ScrollDelta::Lines { x, y } => (*x * WHEEL_LINE_PX, *y * WHEEL_LINE_PX),
            mouse::ScrollDelta::Pixels { x, y } => (*x, *y),
        };
        // Shift turns a vertical wheel into horizontal scrolling.
        let horizontal_delta = if dx.abs() > 0.0 {
            dx
        } else if state.modifiers.shift() {
            -dy
        } else {
            0.0
        };
        if horizontal_delta.abs() > 0.0 {
            // Start from the offset actually shown, so a stale offset beyond
            // the current extent can't swallow the first steps back.
            let current = state.block_scroll(block_id, &extent);
            state.block_scroll_x.insert(
                block_id,
                (current + horizontal_delta).clamp(0.0, max_scroll),
            );
        }
    }
}

/// Width of a block equation: its rendered bitmap, or its widest source line
/// while it hasn't rendered.
pub(super) fn math_block_width<R: Measure>(rendered_width: Option<f32>, tex: &str) -> f32 {
    match rendered_width {
        Some(width) => width * MATH_BLOCK_SCALE,
        None => tex
            .lines()
            .map(|line| measure_width::<R>(line, MATH_SOURCE_FONT_SIZE, iced::Font::MONOSPACE))
            .fold(0.0, f32::max),
    }
}

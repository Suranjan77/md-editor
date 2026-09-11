//! Horizontal scrolling of blocks wider than the text column: code blocks,
//! tables and block math. Each block keeps its own offset in [`State`], moved
//! by shift+wheel, horizontal wheel, or dragging the block's scrollbar.

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

/// An in-progress drag of a block's scrollbar thumb.
#[derive(Debug, Clone, Copy)]
pub(super) struct HorizontalScrollDrag {
    block_id: usize,
    viewport_x: f32,
    viewport_w: f32,
    content_w: f32,
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
        let track_w = self.viewport_w.max(1.0);
        let thumb_w = thumb_width(track_w, self.viewport_w, self.content_w);
        let max_scroll = (self.content_w - self.viewport_w).max(0.0);
        let track_range = (track_w - thumb_w).max(1.0);
        let thumb_x = (pointer_x - self.viewport_x - self.grab_offset).clamp(0.0, track_range);
        (thumb_x / track_range) * max_scroll
    }
}

/// A scrollbar's track and thumb along the x axis.
pub(super) struct ScrollbarGeometry {
    pub track_w: f32,
    pub thumb_x: f32,
    pub thumb_w: f32,
}

fn thumb_width(track_w: f32, viewport_w: f32, content_w: f32) -> f32 {
    (track_w * (viewport_w / content_w)).clamp(MIN_THUMB_WIDTH, track_w)
}

/// Scrollbar geometry for a viewport showing `content_w` scrolled by `scroll`.
pub(super) fn scrollbar_geometry(
    viewport_x: f32,
    viewport_w: f32,
    content_w: f32,
    scroll: f32,
) -> ScrollbarGeometry {
    let track_w = viewport_w.max(1.0);
    let thumb_w = thumb_width(track_w, viewport_w, content_w);
    let thumb_x = viewport_x + ((track_w - thumb_w) * (scroll / (content_w - viewport_w)));
    ScrollbarGeometry {
        track_w,
        thumb_x,
        thumb_w,
    }
}

/// Whether content this wide overflows its viewport enough to scroll.
pub(super) fn overflows(content_w: f32, viewport_w: f32) -> bool {
    content_w > viewport_w + 1.0
}

/// Width of the scrollable viewport of the block `line` belongs to, as used
/// by input handling.
fn block_viewport_width(line: &StyledLine, content_width: f32) -> f32 {
    let column = text_column_width(content_width);
    if line.is_table_row {
        column
    } else if line.is_math_block {
        column - MATH_BLOCK_PADDING
    } else {
        column - CODE_VIEWPORT_INSET
    }
    .max(MIN_TEXT_WIDTH)
}

impl State {
    /// Scroll offset of `block_id`, clamped to what its content allows.
    pub(super) fn block_scroll(&self, block_id: usize, content_w: f32, viewport_w: f32) -> f32 {
        self.block_scroll_x
            .get(&block_id)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, (content_w - viewport_w).max(0.0))
    }
}

impl<Message> Editor<'_, Message> {
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

    /// Scrollable width of a block's content, never less than the text column.
    fn block_content_width<R: Measure>(
        &self,
        block_id: usize,
        available_width: f32,
        focused: bool,
    ) -> f32 {
        let mut max_width = 0.0_f32;
        let mut table_widths: Vec<f32> = Vec::new();
        let Some(start) = self.lines.iter().position(|line| line.block_id == block_id) else {
            return wrap_width(available_width);
        };
        let end = self.lines[start..]
            .iter()
            .position(|line| line.block_id != block_id)
            .map(|offset| start + offset.saturating_sub(1))
            .unwrap_or_else(|| self.lines.len().saturating_sub(1));

        for line in &self.lines[start..=end] {
            let is_editing = self.is_block_editing(line, focused);
            if line.is_code_block {
                let width = line
                    .spans
                    .iter()
                    .map(|span| {
                        measure_width::<R>(
                            span.visible_text(is_editing),
                            CODE_FONT_SIZE,
                            iced::Font::MONOSPACE,
                        )
                    })
                    .sum::<f32>();
                max_width = max_width.max(width + CODE_CONTENT_PADDING);
            } else if line.is_table_row && !is_editing {
                for (idx, cell) in line.table_cells.iter().enumerate() {
                    let width = cell
                        .iter()
                        .map(|span| {
                            measure_width::<R>(
                                span.visible_text(false),
                                span.font_size,
                                span_font(span, line),
                            )
                        })
                        .sum::<f32>()
                        + TABLE_CELL_PADDING;
                    if idx >= table_widths.len() {
                        table_widths.push(width);
                    } else {
                        table_widths[idx] = table_widths[idx].max(width);
                    }
                }
            } else if line.is_math_block {
                for span in &line.spans {
                    let tex = math_source(span);
                    // NOTE: painting sizes this scrollbar from the unpadded
                    // equation width, so the drag range and the drawn thumb
                    // disagree.
                    let width = self
                        .math_cache
                        .get(tex)
                        .map(|m| m.width * MATH_BLOCK_SCALE + 72.0)
                        .unwrap_or_else(|| {
                            measure_width::<R>(tex, MATH_SOURCE_FONT_SIZE, iced::Font::MONOSPACE)
                        });
                    max_width = max_width.max(width);
                }
            }
        }
        max_width
            .max(table_widths.iter().sum::<f32>() + TABLE_EDGE_PADDING)
            .max(wrap_width(available_width))
    }

    /// The scrollbar drag a press at `pos` would start, if it lands on one.
    pub(super) fn horizontal_scrollbar_hit<R: Measure>(
        &self,
        pos: Point,
        available_width: f32,
        state: &State,
    ) -> Option<HorizontalScrollDrag> {
        let line_idx = self.line_at_widget_y(pos.y, state)?;
        let line = self.lines.get(line_idx)?;
        if !(line.is_code_block || line.is_table_row || line.is_math_block) {
            return None;
        }
        let &(start, end) = state.block_ranges.get(&line.block_id)?;
        let (block_y, block_h) = super::layout::lines_extent(state, start, end);
        let viewport_x = TEXT_X_OFFSET;
        let viewport_w = block_viewport_width(line, available_width);

        let content_w =
            self.block_content_width::<R>(line.block_id, available_width, state.is_focused);
        if !overflows(content_w, viewport_w) {
            return None;
        }

        let scrollbar_y = block_y + block_h - SCROLLBAR_BOTTOM_OFFSET;
        if pos.x < viewport_x
            || pos.x > viewport_x + viewport_w
            || pos.y < scrollbar_y - GRAB_SLOP_ABOVE
            || pos.y > scrollbar_y + GRAB_SLOP_BELOW
        {
            return None;
        }

        let scroll = state.block_scroll(line.block_id, content_w, viewport_w);
        let bar = scrollbar_geometry(viewport_x, viewport_w, content_w, scroll);
        let grab_offset = if pos.x >= bar.thumb_x && pos.x <= bar.thumb_x + bar.thumb_w {
            pos.x - bar.thumb_x
        } else {
            bar.thumb_w / 2.0
        };

        Some(HorizontalScrollDrag {
            block_id: line.block_id,
            viewport_x,
            viewport_w,
            content_w,
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
        let Some(first_line) = self.lines.iter().find(|l| l.block_id == block_id) else {
            return;
        };
        let viewport_w = block_viewport_width(first_line, available_width);
        let content_w = self.block_content_width::<R>(block_id, available_width, state.is_focused);
        let max_scroll = (content_w - viewport_w).max(0.0);
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
            let entry = state.block_scroll_x.entry(block_id).or_insert(0.0);
            *entry = (*entry + horizontal_delta).clamp(0.0, max_scroll);
        }
    }
}

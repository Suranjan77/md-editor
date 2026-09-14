//! Multi-line blocks: blockquotes, code blocks, tables and block math.
//!
//! Blocks are measured once per frame, then painted in passes: their chrome
//! (card, caption, badge) behind everything, their rows as part of the line
//! pass, and their scrollbars last.

use std::collections::HashMap;

use iced::border::Radius;
use iced::{Border, Color, Point, Rectangle, Size};

use super::super::caret::code_x_for_col;
use super::super::layout::lines_extent;
use super::super::measure::{centered_text_top, measure_width, span_font};
use super::super::metrics::*;
use super::super::scroll::{ScrollExtent, table_columns};
use super::super::{Editor, Measure, Paint, State};
use super::captions::{block_ordinal, paint_block_caption, paint_language_badge};
use super::primitives::{TextRun, clip_viewport, draw_nowrap_text, fill, rounded};
use super::{Frame, LineBox};
use crate::theme;

const CARD_RADIUS: f32 = 8.0;
/// Cards start this far left of the text column.
const CARD_OUTSET: f32 = 16.0;
const QUOTE_BAR_WIDTH: f32 = 4.0;
/// Cell text starts this far right of its column's left edge.
const CELL_TEXT_INSET: f32 = 7.0;

#[derive(PartialEq)]
pub(super) enum BlockKind {
    Quote,
    Table,
    Code,
    Math,
}

/// Per-frame measurements of a block.
pub(super) struct BlockMeta {
    /// Index of the block's first line.
    pub start: usize,
    /// Top of the block's box, below its margin and including any caption
    /// band, in window coordinates.
    pub y: f32,
    pub height: f32,
    pub kind: BlockKind,
    /// Width of each table column, padding included.
    pub columns: Vec<f32>,
    /// Horizontal extent, for blocks that scroll.
    pub scroll: Option<ScrollExtent>,
    pub code_lang: Option<String>,
}

impl<Message> Editor<'_, Message> {
    /// Measure every block with a line in `visible_start..visible_end`.
    pub(super) fn measure_blocks<R: Measure>(
        &self,
        state: &State,
        bounds: Rectangle,
        focused: bool,
        visible_start: usize,
        visible_end: usize,
    ) -> HashMap<usize, BlockMeta> {
        // Blocks keyed by their id, except runs of quoted lines, which frame
        // together and are keyed by their first line's id.
        let mut visible: HashMap<usize, (usize, usize)> = HashMap::new();
        for idx in visible_start..visible_end {
            let line = &self.lines[idx];
            if line.is_blockquote {
                let start = (0..idx)
                    .rev()
                    .take_while(|&i| self.lines[i].is_blockquote)
                    .last()
                    .unwrap_or(idx);
                let end = (idx + 1..self.lines.len())
                    .take_while(|&i| self.lines[i].is_blockquote)
                    .last()
                    .unwrap_or(idx);
                visible.insert(self.lines[start].block_id, (start, end));
            } else if (line.is_code_block || line.is_math_block || line.is_table_row)
                && let Some(&range) = state.block_ranges.get(&line.block_id)
            {
                visible.insert(line.block_id, range);
            }
        }

        let mut blocks = HashMap::new();
        for (block_id, (start, end)) in visible {
            let Some(first_line) = self.lines.get(start) else {
                continue;
            };
            let (top, block_height) = lines_extent(state, start, end);
            let block_y = bounds.y + top;
            if block_height <= 0.0 {
                continue;
            }

            let kind = if first_line.is_blockquote {
                BlockKind::Quote
            } else if first_line.is_table_row {
                BlockKind::Table
            } else if first_line.is_code_block {
                BlockKind::Code
            } else {
                BlockKind::Math
            };
            let is_editing = self.is_block_editing(first_line, focused);
            let lines = &self.lines[start..=end];
            let meta = BlockMeta {
                start,
                y: block_y,
                height: block_height,
                columns: if kind == BlockKind::Table && !is_editing {
                    table_columns::<R>(lines)
                } else {
                    Vec::new()
                },
                kind,
                scroll: self.scroll_extent::<R>(state, block_id, bounds.width, focused),
                code_lang: lines.iter().find_map(|line| line.code_block_lang.clone()),
            };
            blocks.insert(block_id, meta);
        }
        blocks
    }

    /// Card, caption and badge behind a block.
    pub(super) fn paint_block_chrome<R: Measure>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        block_id: usize,
        meta: &BlockMeta,
    ) {
        let (bounds, viewport) = (frame.bounds, frame.viewport);
        if meta.y + meta.height < viewport.y || meta.y > viewport.y + viewport.height {
            return;
        }

        let scale = page_scale(bounds.width);
        let card_x = bounds.x + text_left(bounds.width) - CARD_OUTSET * scale;
        let card_w = bounds.width - text_left(bounds.width);
        let card = Rectangle {
            x: card_x,
            y: meta.y,
            width: card_w,
            height: meta.height,
        };

        match meta.kind {
            BlockKind::Quote => {
                fill(
                    renderer,
                    card,
                    rounded(CARD_RADIUS),
                    Color::from_rgba(1.0, 1.0, 1.0, 0.012),
                );
                fill(
                    renderer,
                    Rectangle {
                        width: QUOTE_BAR_WIDTH,
                        ..card
                    },
                    Border {
                        radius: Radius {
                            top_left: CARD_RADIUS,
                            bottom_left: CARD_RADIUS,
                            top_right: 0.0,
                            bottom_right: 0.0,
                        },
                        ..Default::default()
                    },
                    theme::ACCENT,
                );
            }
            BlockKind::Table => {
                let table_x = bounds.x + text_left(bounds.width);
                let table_width = meta
                    .scroll
                    .map_or(text_column_width(bounds.width), |extent| extent.viewport_w);
                fill(
                    renderer,
                    Rectangle {
                        x: table_x - TABLE_EDGE_PADDING / 2.0,
                        y: meta.y,
                        width: table_width + TABLE_EDGE_PADDING,
                        height: meta.height,
                    },
                    Border {
                        color: theme::BORDER,
                        width: 1.0,
                        radius: CARD_RADIUS.into(),
                    },
                    theme::BG_SECONDARY,
                );
                let number = block_ordinal(self.lines, block_id, "table-").unwrap_or(1);
                paint_block_caption(
                    renderer,
                    format!("Table {}", number),
                    table_x,
                    table_width,
                    meta.y,
                    viewport,
                );
            }
            BlockKind::Code | BlockKind::Math => {
                fill(
                    renderer,
                    card,
                    Border {
                        color: theme::BORDER_SUBTLE,
                        width: 1.0,
                        radius: CARD_RADIUS.into(),
                    },
                    theme::BG_SECONDARY,
                );

                if meta.kind == BlockKind::Code {
                    let number = block_ordinal(self.lines, block_id, "code-").unwrap_or(1);
                    paint_block_caption(
                        renderer,
                        format!("Listing {}", number),
                        card_x,
                        card_w,
                        meta.y,
                        viewport,
                    );
                    if let Some(lang) = &meta.code_lang {
                        paint_language_badge(renderer, lang, card_x, card_w, meta.y, viewport);
                    }
                }
            }
        }
    }

    /// Horizontal extent and current offset of the block `row` belongs to.
    pub(super) fn row_scroll<R: Measure>(
        &self,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
    ) -> (ScrollExtent, f32) {
        let block_id = row.line.block_id;
        let extent = frame
            .blocks
            .get(&block_id)
            .and_then(|meta| meta.scroll)
            .or_else(|| {
                self.scroll_extent::<R>(frame.state, block_id, frame.bounds.width, frame.focused)
            })
            .unwrap_or(ScrollExtent {
                viewport_w: wrap_width(frame.bounds.width),
                content_w: 0.0,
            });
        (extent, frame.state.block_scroll(block_id, &extent))
    }

    /// Where the horizontally scrolled content of `row`'s block may show: its
    /// scroll viewport, `viewport_w` wide, over the block's full height — so
    /// every line of a block shares one clip — within the frame's viewport.
    pub(super) fn block_clip(
        &self,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
        viewport_w: f32,
    ) -> Rectangle {
        let (y, height) = frame
            .blocks
            .get(&row.line.block_id)
            .map_or((row.y, row.height), |meta| (meta.y, meta.height));
        clip_viewport(
            frame.viewport,
            Rectangle {
                x: frame.bounds.x + text_left(frame.bounds.width),
                y,
                width: viewport_w,
                height,
            },
        )
    }

    /// Visible x range, relative to the text column, of source columns
    /// `from..to` in a code line, after scrolling and clipping.
    pub(super) fn code_range_x<R: Measure>(
        &self,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
        from: usize,
        to: usize,
    ) -> Option<(f32, f32)> {
        let (extent, scroll_x) = self.row_scroll::<R>(frame, row);
        let x0 = (code_x_for_col::<R>(row.line, from, row.is_editing) - scroll_x).max(0.0);
        let x1 =
            (code_x_for_col::<R>(row.line, to, row.is_editing) - scroll_x).min(extent.viewport_w);
        (x1 > x0).then_some((x0, x1))
    }

    /// One line of a code block: unwrapped, clipped to the block's viewport
    /// and shifted by its horizontal scroll. Draws its own caret.
    pub(super) fn paint_code_line<R: Paint>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
    ) {
        let (bounds, line) = (frame.bounds, row.line);
        let (extent, scroll_x) = self.row_scroll::<R>(frame, row);
        let code_left = bounds.x + text_left(bounds.width);
        let code_right = code_left + extent.viewport_w;
        let clip = self.block_clip(frame, row, extent.viewport_w);
        let text_top = self
            .snap_px(row.y + centered_text_top(row.height, CODE_FONT_SIZE, iced::Font::MONOSPACE));
        let spans = || {
            line.spans
                .iter()
                .map(|span| (span, span.visible_text(row.is_editing)))
                .filter(|(_, text)| !text.is_empty())
                .map(|(span, text)| {
                    let width = measure_width::<R>(text, CODE_FONT_SIZE, iced::Font::MONOSPACE);
                    (span, text, width)
                })
        };
        let paint_text = |renderer: &mut R| {
            let mut code_x = code_left - scroll_x;
            for (span, text, width) in spans() {
                if code_x + width >= code_left && code_x <= code_right {
                    draw_nowrap_text(
                        renderer,
                        text,
                        code_x,
                        text_top,
                        width,
                        CODE_FONT_SIZE,
                        iced::Font::MONOSPACE,
                        span.color,
                        // The layer clips. A clip rectangle inside it would
                        // switch tiny-skia's clipping off.
                        frame.viewport,
                    );
                }
                code_x += width;
            }
        };
        // Only a line that doesn't fit needs clipping — and only a layer clips
        // text in every renderer.
        let line_width: f32 = spans().map(|(_, _, width)| width).sum();
        if scroll_x > 0.0 || line_width > extent.viewport_w {
            renderer.with_layer(clip, paint_text);
        } else {
            paint_text(renderer);
        }

        let motion = &frame.state.caret_motion;
        if frame.focused && row.idx == self.buffer.cursor_line && motion.alpha() > 0.0 {
            let caret = self.caret_box::<R>(
                row.idx,
                self.buffer.cursor_col,
                self.buffer.cursor_affinity,
                bounds.width,
                row.is_editing,
                row.active_col,
            );
            let offset = motion.offset();
            fill(
                renderer,
                Rectangle {
                    x: code_left + caret.x - scroll_x + offset.x,
                    y: self.snap_px(row.y + caret.y + offset.y),
                    width: 2.0,
                    height: caret.height,
                },
                rounded(1.0),
                Color {
                    a: theme::ACCENT_SECONDARY.a * motion.alpha(),
                    ..theme::ACCENT_SECONDARY
                },
            );
        }
    }

    /// One rendered table row.
    pub(super) fn paint_table_row<R: Paint>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
    ) {
        let (bounds, line) = (frame.bounds, row.line);
        let Some(meta) = frame.blocks.get(&line.block_id) else {
            return;
        };

        let (extent, scroll_x) = self.row_scroll::<R>(frame, row);
        let table_width = extent.viewport_w;
        let table_x = bounds.x + text_left(bounds.width);
        let (row_y, row_h) = (row.y, row.height);
        let full_row = Rectangle {
            x: table_x - TABLE_EDGE_PADDING / 2.0,
            y: row_y,
            width: table_width + TABLE_EDGE_PADDING,
            height: row_h,
        };

        // Separator row (`|---|`): a rule under the header.
        if line.table_cells.is_empty() {
            fill(
                renderer,
                Rectangle {
                    height: 1.0,
                    ..full_row
                },
                Border::default(),
                theme::BORDER,
            );
            return;
        }

        // Rows before this one that have cells: 0 for the header.
        let ordinal = self.lines[meta.start..row.idx]
            .iter()
            .filter(|l| l.is_table_row && !l.table_cells.is_empty())
            .count();
        let is_header = ordinal == 0;
        let row_bg = if is_header {
            Some(theme::BG_TERTIARY)
        } else if ordinal % 2 == 0 {
            // Every second body row.
            Some(Color::from_rgba(1.0, 1.0, 1.0, 0.025))
        } else {
            None
        };
        if let Some(bg) = row_bg {
            // Rows sit between the caption band and the scrollbar gutter, clear
            // of the card's rounded corners, so they are square.
            fill(renderer, full_row, Border::default(), bg);
        }

        let table_right = table_x + table_width;
        let cell_clip = self.block_clip(frame, row, table_width);
        let paint_cells = |renderer: &mut R| {
            let mut cx = table_x - scroll_x;
            for (c_idx, cell) in line.table_cells.iter().enumerate() {
                let Some(&col_width) = meta.columns.get(c_idx) else {
                    break;
                };

                if c_idx > 0 && cx >= table_x && cx <= table_right {
                    fill(
                        renderer,
                        Rectangle {
                            x: cx - 3.0,
                            y: row_y,
                            width: 1.0,
                            height: row_h,
                        },
                        Border::default(),
                        theme::BORDER_SUBTLE,
                    );
                }

                let mut px = cx + CELL_TEXT_INSET;
                for span in cell {
                    let text = span.visible_text(false);
                    if text.is_empty() {
                        continue;
                    }

                    let font = span_font(span, line);
                    let fs = span.font_size;
                    let width = measure_width::<R>(text, fs, font);
                    if px + width < table_x || px > table_right {
                        px += width;
                        continue;
                    }

                    TextRun::new(
                        text,
                        fs,
                        Size::new(width.min((table_right - px).max(1.0)).max(1.0), row_h),
                    )
                    .font(font)
                    .draw(
                        renderer,
                        Point::new(px, self.snap_px(row_y + centered_text_top(row_h, fs, font))),
                        if is_header {
                            theme::TEXT_PRIMARY
                        } else {
                            span.color
                        },
                        frame.viewport,
                    );
                    px += width;
                }
                cx += col_width;
            }
        };
        // Only a table that doesn't fit needs clipping — and only a layer
        // clips text in every renderer.
        if scroll_x > 0.0 || extent.overflows() {
            renderer.with_layer(cell_clip, paint_cells);
        } else {
            paint_cells(renderer);
        }
    }
}

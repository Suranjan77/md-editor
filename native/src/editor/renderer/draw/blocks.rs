//! Multi-line blocks: blockquotes, code blocks, tables and block math.
//!
//! Blocks are measured once per frame, then painted in passes: their chrome
//! (card, caption, badge) behind everything, their rows as part of the line
//! pass, and their scrollbars last.

use std::collections::{HashMap, HashSet};

use iced::border::Radius;
use iced::{Border, Color, Point, Rectangle, Size};

use super::super::caret::code_x_for_col;
use super::super::measure::{measure_width, span_font};
use super::super::metrics::*;
use super::super::scroll::{ScrollExtent, table_columns};
use super::super::{Editor, Measure, Paint, State};
use super::captions::{block_ordinal, paint_block_caption, paint_language_badge};
use super::primitives::{TextRun, draw_nowrap_text, fill, rounded};
use super::{Frame, LineBox};
use crate::theme;

const CARD_RADIUS: f32 = 8.0;
/// Cards start this far left of the text column.
const CARD_OUTSET: f32 = 16.0;
const QUOTE_BAR_WIDTH: f32 = 4.0;
/// Code text sits this far below the top of its row.
const CODE_TEXT_TOP: f32 = 10.0;
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
    /// Top of the block, including any caption band, in window coordinates.
    pub y: f32,
    pub height: f32,
    pub kind: BlockKind,
    pub is_editing: bool,
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
        let visible_block_ids: HashSet<usize> = self.lines[visible_start..visible_end]
            .iter()
            .filter(|l| l.is_code_block || l.is_math_block || l.is_blockquote || l.is_table_row)
            .map(|l| l.block_id)
            .collect();

        let mut blocks = HashMap::new();
        for block_id in visible_block_ids {
            let Some(&(start, end)) = state.block_ranges.get(&block_id) else {
                continue;
            };
            let Some(first_line) = self.lines.get(start) else {
                continue;
            };
            let block_y = bounds.y + TOP_PAD + state.layout_tree.prefix_sum(start);
            let block_height = state.layout_tree.prefix_sum(end.saturating_add(1))
                - state.layout_tree.prefix_sum(start);
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
                is_editing,
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

        let card_x = bounds.x + TEXT_X_OFFSET - CARD_OUTSET;
        let card_w = bounds.width - TEXT_X_OFFSET;
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
            BlockKind::Table if !meta.is_editing => {
                let table_x = bounds.x + TEXT_X_OFFSET;
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
            BlockKind::Table => {}
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

                if meta.kind == BlockKind::Code && !meta.is_editing {
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
                viewport_w: text_column_width(frame.bounds.width).max(MIN_TEXT_WIDTH),
                content_w: 0.0,
            });
        (extent, frame.state.block_scroll(block_id, &extent))
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
        let code_left = bounds.x + TEXT_X_OFFSET;
        let code_right = code_left + extent.viewport_w;

        let mut code_x = code_left - scroll_x;
        for span in &line.spans {
            let text = span.visible_text(row.is_editing);
            if text.is_empty() {
                continue;
            }
            let width = measure_width::<R>(text, CODE_FONT_SIZE, iced::Font::MONOSPACE);
            if code_x + width >= code_left && code_x <= code_right {
                draw_nowrap_text(
                    renderer,
                    text,
                    code_x,
                    row.y + CODE_TEXT_TOP,
                    width,
                    CODE_FONT_SIZE,
                    iced::Font::MONOSPACE,
                    span.color,
                    frame.viewport,
                );
            }
            code_x += width;
        }

        if frame.focused && row.idx == self.buffer.cursor_line {
            let caret = self.caret_box::<R>(
                row.idx,
                self.buffer.cursor_col,
                bounds.width,
                row.is_editing,
                row.active_col,
            );
            fill(
                renderer,
                Rectangle {
                    x: code_left + caret.x - scroll_x,
                    y: row.y + caret.y,
                    width: 2.0,
                    height: caret.height,
                },
                rounded(1.0),
                theme::ACCENT_SECONDARY,
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
        let table_x = bounds.x + TEXT_X_OFFSET;
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
        let cell_clip = Rectangle {
            x: table_x,
            y: row_y,
            width: table_width,
            height: row_h,
        };
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
                    Point::new(px, row_y + (row_h - fs) / 2.0),
                    if is_header {
                        theme::TEXT_PRIMARY
                    } else {
                        span.color
                    },
                    cell_clip,
                );
                px += width;
            }
            cx += col_width;
        }
    }
}

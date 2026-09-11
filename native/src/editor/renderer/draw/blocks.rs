//! Multi-line blocks: blockquotes, code blocks, tables and block math.
//!
//! Blocks are measured once per frame, then painted in two passes: their
//! chrome (card, caption, badge) behind everything, and their rows as part of
//! the line pass.

use std::collections::{HashMap, HashSet};

use iced::border::Radius;
use iced::{Border, Color, Point, Rectangle, Size};

use super::super::measure::{measure_width, span_font};
use super::super::metrics::*;
use super::super::spans::math_source;
use super::super::{Editor, MATH_BLOCK_SCALE, Measure, Paint, State};
use super::captions::{block_ordinal, paint_block_caption, paint_language_badge};
use super::primitives::{TextRun, draw_nowrap_text, fill, paint_scrollbar, rounded};
use super::{Frame, LineBox};
use crate::theme;

const CARD_RADIUS: f32 = 8.0;
/// Cards start this far left of the text column.
const CARD_OUTSET: f32 = 16.0;
const QUOTE_BAR_WIDTH: f32 = 4.0;
/// Code text sits this far below the top of its row.
const CODE_TEXT_TOP: f32 = 10.0;
const MIN_COLUMN_WIDTH: f32 = 42.0;
/// Cell text starts this far right of its column's left edge.
const CELL_TEXT_INSET: f32 = 7.0;

pub(super) enum BlockKind {
    Quote,
    Table,
    Code,
    Math,
}

/// Per-frame measurements of a block.
pub(super) struct BlockMeta {
    /// Top of the block, including any caption band, in window coordinates.
    pub y: f32,
    pub height: f32,
    pub kind: BlockKind,
    pub is_editing: bool,
    /// Width of each table column, padding included.
    pub col_widths: Vec<f32>,
    /// Scrollable width of code, table or math content.
    pub content_width: f32,
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
            let mut meta = BlockMeta {
                y: block_y,
                height: block_height,
                kind,
                is_editing: self.is_block_editing(first_line, focused),
                col_widths: Vec::new(),
                content_width: 0.0,
                code_lang: first_line.code_block_lang.clone(),
            };

            for line in &self.lines[start..=end] {
                if meta.code_lang.is_none() && line.code_block_lang.is_some() {
                    meta.code_lang = line.code_block_lang.clone();
                }
                if line.is_code_block {
                    let width = line
                        .spans
                        .iter()
                        .map(|span| {
                            measure_width::<R>(
                                span.visible_text(meta.is_editing),
                                CODE_FONT_SIZE,
                                iced::Font::MONOSPACE,
                            )
                        })
                        .sum::<f32>();
                    meta.content_width = meta.content_width.max(width + CODE_CONTENT_PADDING);
                } else if line.is_math_block {
                    let width = line
                        .spans
                        .iter()
                        .map(|span| {
                            let tex = math_source(span);
                            self.math_cache
                                .get(tex)
                                .map(|m| m.width * MATH_BLOCK_SCALE + MATH_BLOCK_PADDING)
                                .unwrap_or_else(|| {
                                    measure_width::<R>(
                                        tex,
                                        MATH_SOURCE_FONT_SIZE,
                                        iced::Font::MONOSPACE,
                                    ) + MATH_BLOCK_PADDING
                                })
                        })
                        .fold(0.0_f32, f32::max);
                    meta.content_width = meta.content_width.max(width);
                } else if line.is_table_row && !meta.is_editing {
                    for (c_idx, cell) in line.table_cells.iter().enumerate() {
                        let w = cell
                            .iter()
                            .map(|span| {
                                measure_width::<R>(
                                    span.visible_text(false),
                                    span.font_size,
                                    span_font(span, line),
                                )
                            })
                            .fold(0.0, |acc, w| acc + w);
                        let padded = w + TABLE_CELL_PADDING;
                        if c_idx >= meta.col_widths.len() {
                            meta.col_widths.push(padded);
                        } else if padded > meta.col_widths[c_idx] {
                            meta.col_widths[c_idx] = padded;
                        }
                    }
                    meta.content_width = meta.col_widths.iter().sum::<f32>() + TABLE_EDGE_PADDING;
                }
            }
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
                let table_width = text_column_width(bounds.width);
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

                if matches!(meta.kind, BlockKind::Code) && !meta.is_editing {
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

    /// One line of a code block: unwrapped, clipped to the block's viewport
    /// and shifted by its horizontal scroll.
    pub(super) fn paint_code_line<R: Paint>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
    ) {
        let (bounds, line) = (frame.bounds, row.line);
        let viewport_w =
            (text_column_width(bounds.width) - CODE_VIEWPORT_INSET).max(MIN_TEXT_WIDTH);
        let meta = frame.blocks.get(&line.block_id);
        let content_w = meta.map(|meta| meta.content_width).unwrap_or(viewport_w);
        let scroll_x = frame
            .state
            .block_scroll(line.block_id, content_w, viewport_w);
        let code_left = bounds.x + TEXT_X_OFFSET;
        let code_right = code_left + viewport_w;

        let mut code_x = bounds.x + TEXT_X_OFFSET - scroll_x;
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
            let (cx, _) = self.cursor_position::<R>(row.idx, bounds.width);
            fill(
                renderer,
                Rectangle {
                    x: bounds.x + TEXT_X_OFFSET + cx - scroll_x,
                    y: row.y + 12.0,
                    width: 2.0,
                    height: 22.0,
                },
                rounded(1.0),
                theme::ACCENT_SECONDARY,
            );
        }

        // NOTE: repainted by every visible line of the block.
        if let Some(meta) = meta {
            paint_scrollbar(
                renderer,
                frame.state,
                line.block_id,
                code_left,
                viewport_w,
                meta.y + meta.height - SCROLLBAR_BOTTOM_OFFSET,
                content_w,
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

        let table_width = text_column_width(bounds.width);
        let raw_table_width: f32 = meta.col_widths.iter().sum();
        let scroll_content_width = raw_table_width.max(table_width);
        let scroll_x = frame
            .state
            .block_scroll(line.block_id, scroll_content_width, table_width);
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

        // NOTE: `meta.y` includes the caption band and `row_y` does not, so
        // this never holds and headers render as body rows.
        let is_header = meta.y == row_y;
        let is_last_row = !self.lines[row.idx + 1..].iter().any(|next| {
            next.is_table_row && next.block_id == line.block_id && !next.table_cells.is_empty()
        });

        let row_bg = if is_header {
            Some(theme::BG_TERTIARY)
        } else if ((row_y - meta.y) / row_h).round() as usize % 2 == 1 {
            Some(Color::from_rgba(1.0, 1.0, 1.0, 0.025))
        } else {
            None
        };
        if let Some(bg) = row_bg {
            let top = if is_header { CARD_RADIUS } else { 0.0 };
            let bottom = if is_last_row { CARD_RADIUS } else { 0.0 };
            fill(
                renderer,
                full_row,
                Border {
                    radius: Radius {
                        top_left: top,
                        top_right: top,
                        bottom_left: bottom,
                        bottom_right: bottom,
                    },
                    ..Default::default()
                },
                bg,
            );
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
            let Some(&col_width) = meta.col_widths.get(c_idx) else {
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
            cx += col_width.max(MIN_COLUMN_WIDTH);
        }

        paint_scrollbar(
            renderer,
            frame.state,
            line.block_id,
            table_x,
            table_width,
            meta.y + meta.height - HORIZONTAL_SCROLLBAR_GUTTER + 5.0,
            scroll_content_width,
        );
    }
}

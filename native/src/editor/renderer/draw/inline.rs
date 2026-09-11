//! Lines of inline content: paragraphs, headings, list items, quotes, and the
//! images, equations and checkboxes that can appear among their spans.

use iced::alignment::{Horizontal, Vertical};
use iced::{Border, Color, Point, Rectangle, Size};

use super::super::measure::{bold_font, measure_width, span_font};
use super::super::metrics::*;
use super::super::spans::{math_source, span_is_editing, span_visible_text};
use super::super::{Editor, MATH_BLOCK_SCALE, MathRender, Measure, Paint};
use super::captions::{block_ordinal, figure_number, paint_equation_number};
use super::primitives::{TextRun, WrapPen, clip_viewport, fill, paint_scrollbar, rounded};
use super::{Counters, Frame, LineBox};
use crate::editor::highlight::{StyledLine, StyledSpan};
use crate::theme;

const IMAGE_TOP: f32 = 5.0;
const IMAGE_GAP: f32 = 10.0;
const FIGURE_CAPTION_SIZE: f32 = 13.0;
const MATH_SOURCE_TOP: f32 = 18.0;

/// Whether a line has nothing but unstyled text, so it can be painted as a
/// single wrapped run instead of span by span.
pub(super) fn is_plain_line(line: &StyledLine, is_editing: bool) -> bool {
    !is_editing
        && !line.is_code_block
        && !line.is_math_block
        && !line.is_table_row
        && !line.is_blockquote
        && line.spans.iter().all(|span| {
            !span.bold
                && !span.italic
                && !span.is_code
                && !span.is_link
                && !span.is_syntax
                && span.display_text.is_none()
                && !span.is_image
                && !span.is_math
                && !span.is_checkbox
        })
}

impl<Message> Editor<'_, Message> {
    pub(super) fn paint_rule<R: Measure>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
    ) {
        let bounds = frame.bounds;
        fill(
            renderer,
            Rectangle {
                x: bounds.x + TEXT_X_OFFSET,
                y: row.y + row.height / 2.0,
                width: bounds.width - TEXT_X_OFFSET - 20.0,
                height: 2.0,
            },
            rounded(1.0),
            theme::ACCENT_GLOW,
        );
    }

    /// See [`is_plain_line`].
    ///
    /// NOTE: wraps with the text shaper rather than the word-wrapping used by
    /// layout and the caret, so the two can disagree on where rows break.
    pub(super) fn paint_plain_line<R: Measure>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
    ) {
        let bounds = frame.bounds;
        let spans = &row.line.spans;
        let content = spans
            .iter()
            .map(|span| span.visible_text(false))
            .collect::<String>();
        if content.trim().is_empty() {
            return;
        }

        let max_font = spans
            .iter()
            .map(|s| s.font_size)
            .fold(DEFAULT_FONT_SIZE, f32::max);
        let color = spans
            .iter()
            .find(|span| !span.visible_text(false).is_empty())
            .map(|span| span.color)
            .unwrap_or(theme::TEXT_PRIMARY);
        TextRun::new(
            content,
            max_font,
            Size::new(text_column_width(bounds.width), row.height),
        )
        .wrapping(iced::advanced::text::Wrapping::WordOrGlyph)
        .draw(
            renderer,
            Point::new(bounds.x + TEXT_X_OFFSET, row.y + 2.0),
            color,
            frame.viewport,
        );
    }

    /// Paint a line span by span.
    pub(super) fn paint_spans<R: Paint>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
        counters: &mut Counters,
    ) {
        let (bounds, line) = (frame.bounds, row.line);
        let left = bounds.x + TEXT_X_OFFSET;
        let mut pen = WrapPen {
            x: left,
            y: row.y,
            left,
            right: bounds.x + bounds.width - MARGIN_RIGHT,
        };

        for (span_idx, span) in line.spans.iter().enumerate() {
            let span_editing = span_is_editing(line, span_idx, row.is_editing, row.active_col);

            if span.is_image && !span_editing {
                counters.images += 1;
                if let Some(width) = self.paint_image(renderer, frame, row, span, counters.images) {
                    pen.x += width + IMAGE_GAP;
                    continue;
                }
            }

            if (span.is_math || line.is_math_block)
                && self.paint_math_span(
                    renderer,
                    frame,
                    row,
                    span,
                    span_editing,
                    &mut pen,
                    counters,
                )
            {
                continue;
            }

            let display_text = span_visible_text(line, span_idx, row.is_editing, row.active_col);
            if display_text.is_empty() {
                continue;
            }

            if span.is_checkbox && !span_editing {
                paint_checkbox(renderer, frame, span.is_checked, pen.x, pen.y);
                pen.x += CHECKBOX_ADVANCE;
                continue;
            }

            pen.draw_wrapped(
                renderer,
                display_text,
                span.font_size,
                span_font(span, line),
                span.color,
                frame.viewport,
            );
        }
    }

    /// A loaded image, centered in the text column, with its figure caption.
    /// Returns the drawn width, or `None` if the image isn't loaded.
    fn paint_image<R: Paint>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
        span: &StyledSpan,
        running_number: usize,
    ) -> Option<f32> {
        let path = span.image_path.as_ref()?;
        let (handle, w, h) = self.image_cache.get(path)?;
        let bounds = frame.bounds;

        let available_w = text_column_width(bounds.width);
        let scale = if *w > available_w {
            available_w / w
        } else {
            1.0
        };
        let draw_w = w * scale;
        let draw_h = h * scale;
        let draw_x = bounds.x + TEXT_X_OFFSET + (available_w - draw_w) / 2.0;

        renderer.draw_image(
            iced::advanced::image::Image::new(handle.clone()),
            Rectangle {
                x: draw_x,
                y: row.y + IMAGE_TOP,
                width: draw_w,
                height: draw_h,
            },
            frame.viewport,
        );

        let caption = format!(
            "Figure {}: {}",
            figure_number(span).unwrap_or(running_number),
            span.image_alt.as_deref().unwrap_or("")
        );
        TextRun::new(caption, FIGURE_CAPTION_SIZE, Size::new(draw_w, 20.0))
            .align(Horizontal::Center, Vertical::Top)
            .wrapping(iced::advanced::text::Wrapping::WordOrGlyph)
            .draw(
                renderer,
                Point::new(draw_x + draw_w / 2.0, row.y + draw_h + 12.0),
                theme::TEXT_MUTED,
                frame.viewport,
            );

        Some(draw_w)
    }

    /// Paint a math span as a rendered equation, or as TeX source for block
    /// math that hasn't rendered. Returns `false` when the span should be
    /// painted as ordinary text instead.
    #[allow(clippy::too_many_arguments)]
    fn paint_math_span<R: Paint>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
        span: &StyledSpan,
        span_editing: bool,
        pen: &mut WrapPen,
        counters: &mut Counters,
    ) -> bool {
        let line = row.line;
        if !span_editing
            && ((line.is_block_fence && span.visible_text(false).trim().is_empty())
                || span.is_syntax)
        {
            // Fences and `$` markers are hidden outside editing.
            return true;
        }

        let tex = math_source(span);
        if tex.is_empty() {
            return false;
        }
        if !span_editing && let Some(math) = self.math_cache.get(tex) {
            let drawn_w = self.paint_rendered_math(renderer, frame, row, math, pen, counters);
            pen.x += drawn_w + INLINE_MATH_GAP;
            return true;
        }
        if line.is_math_block && !row.is_editing {
            self.paint_math_source(renderer, frame, row, tex, pen.y, counters);
            return true;
        }
        false
    }

    /// Returns the drawn width.
    fn paint_rendered_math<R: Paint>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
        math: &MathRender,
        pen: &mut WrapPen,
        counters: &mut Counters,
    ) -> f32 {
        let (bounds, line, viewport) = (frame.bounds, row.line, frame.viewport);
        let is_block = line.is_math_block;
        // Each context has a bitmap rasterized for exactly its display scale
        // (see MathRender).
        let (handle, scale) = if is_block {
            (&math.block_handle, MATH_BLOCK_SCALE)
        } else {
            (&math.inline_handle, 1.0)
        };
        let draw_w = math.width * scale;
        let draw_h = math.height * scale;
        let available_w = text_column_width(bounds.width);
        let block_max_w = (available_w - MATH_BLOCK_PADDING).max(MIN_TEXT_WIDTH);

        let mut draw_x = pen.x;
        if is_block {
            counters.equations += 1;
            let scroll_x = frame.state.block_scroll(line.block_id, draw_w, block_max_w);
            draw_x = bounds.x
                + TEXT_X_OFFSET
                + if draw_w <= block_max_w {
                    (block_max_w - draw_w) / 2.0
                } else {
                    -scroll_x
                };

            let number =
                block_ordinal(self.lines, line.block_id, "equation-").unwrap_or(counters.equations);
            let eq_y = pen.y + (row.height - draw_h) / 2.0;
            paint_equation_number(
                renderer,
                number,
                bounds.x + TEXT_X_OFFSET + available_w,
                eq_y + draw_h / 2.0,
                draw_h,
                viewport,
            );
        } else if draw_x > pen.left && draw_x + draw_w > pen.right {
            pen.y += BASE_LINE_HEIGHT;
            draw_x = pen.left;
            pen.x = pen.left;
        }

        let (clip, draw_y) = if is_block {
            let clip = clip_viewport(
                viewport,
                Rectangle {
                    x: bounds.x + TEXT_X_OFFSET,
                    y: pen.y,
                    width: block_max_w,
                    height: row.height,
                },
            );
            (clip, pen.y + (row.height - draw_h) / 2.0)
        } else {
            let margin_top = (BASE_LINE_HEIGHT - draw_h).max(0.0) / 2.0;
            (viewport, pen.y + margin_top)
        };

        // Snap the rect to the device-pixel grid. The centering math above
        // yields fractional positions with a different sub-pixel phase per
        // axis, which makes the horizontal and vertical strokes of the same
        // glyph sample differently (one crisp, one split across two dim
        // pixels).
        let sf = self.scale_factor;
        let snap = |v: f32| (v * sf).round() / sf;
        renderer.draw_image(
            iced::advanced::image::Image::new(handle.clone()),
            Rectangle {
                x: snap(draw_x),
                y: snap(draw_y),
                width: snap(draw_w),
                height: snap(draw_h),
            },
            clip,
        );

        if is_block {
            paint_scrollbar(
                renderer,
                frame.state,
                line.block_id,
                bounds.x + TEXT_X_OFFSET,
                block_max_w,
                row.y + row.height - SCROLLBAR_BOTTOM_OFFSET,
                draw_w,
            );
        }
        draw_w
    }

    /// TeX source of block math that has no rendered bitmap (yet).
    fn paint_math_source<R: Paint>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
        tex: &str,
        top: f32,
        counters: &mut Counters,
    ) {
        let (bounds, line, viewport) = (frame.bounds, row.line, frame.viewport);
        counters.equations += 1;
        let available_w = text_column_width(bounds.width);
        let viewport_w = (available_w - MATH_BLOCK_PADDING).max(MIN_TEXT_WIDTH);
        let content_w = tex
            .lines()
            .map(|source_line| {
                measure_width::<R>(source_line, MATH_SOURCE_FONT_SIZE, iced::Font::MONOSPACE)
            })
            .fold(0.0_f32, f32::max);
        let scroll_x = frame
            .state
            .block_scroll(line.block_id, content_w, viewport_w);

        let clip = clip_viewport(
            viewport,
            Rectangle {
                x: bounds.x + TEXT_X_OFFSET,
                y: top,
                width: viewport_w,
                height: row.height,
            },
        );

        let mut text_y = top + MATH_SOURCE_TOP;
        for source_line in tex.lines() {
            TextRun::new(
                source_line,
                MATH_SOURCE_FONT_SIZE,
                Size::new(content_w.max(1.0), BASE_LINE_HEIGHT),
            )
            .font(iced::Font::MONOSPACE)
            .draw(
                renderer,
                Point::new(bounds.x + TEXT_X_OFFSET - scroll_x, text_y),
                theme::TEXT_SECONDARY,
                clip,
            );
            text_y += BASE_LINE_HEIGHT;
        }

        paint_scrollbar(
            renderer,
            frame.state,
            line.block_id,
            bounds.x + TEXT_X_OFFSET,
            viewport_w,
            row.y + row.height - SCROLLBAR_BOTTOM_OFFSET,
            content_w,
        );
        let number =
            block_ordinal(self.lines, line.block_id, "equation-").unwrap_or(counters.equations);
        paint_equation_number(
            renderer,
            number,
            bounds.x + TEXT_X_OFFSET + available_w,
            row.y + row.height / 2.0,
            BASE_LINE_HEIGHT,
            viewport,
        );
    }
}

/// A task-list checkbox with its top-left at (`x`, row top `line_y`).
fn paint_checkbox<R: Measure>(
    renderer: &mut R,
    frame: &Frame<'_>,
    checked: bool,
    x: f32,
    line_y: f32,
) {
    let box_y = line_y + (BASE_LINE_HEIGHT - CHECKBOX_SIZE) / 2.0;
    let box_rect = Rectangle {
        x,
        y: box_y,
        width: CHECKBOX_SIZE,
        height: CHECKBOX_SIZE,
    };

    if !checked {
        fill(
            renderer,
            box_rect,
            Border {
                color: theme::BORDER,
                width: 1.5,
                radius: 4.0.into(),
            },
            Color::TRANSPARENT,
        );
        return;
    }

    fill(renderer, box_rect, rounded(4.0), theme::ACCENT);
    let check_size = 13.0;
    let check_w = measure_width::<R>("✓", check_size, bold_font());
    TextRun::new("✓", check_size, Size::new(check_w, CHECKBOX_SIZE))
        .font(bold_font())
        .draw(
            renderer,
            Point::new(
                x + (CHECKBOX_SIZE - check_w) / 2.0,
                box_y + (CHECKBOX_SIZE - check_size) / 2.0 - 0.5,
            ),
            theme::BG_PRIMARY,
            frame.viewport,
        );
}

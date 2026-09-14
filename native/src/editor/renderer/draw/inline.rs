//! Inline content — paragraphs, headings, list items, quotes, and the
//! images, equations and checkboxes among their spans — plus block math and
//! horizontal rules.

use iced::alignment::{Horizontal, Vertical};
use iced::{Border, Color, Point, Rectangle, Size};

use super::super::flow::{Flow, ItemKind};
use super::super::measure::{bold_font, centered_text_top, measure_width};
use super::super::metrics::*;
use super::super::spans::{math_source, span_is_editing};
use super::super::{Editor, MATH_BLOCK_SCALE, MathRender, Measure, Paint};
use super::captions::{block_ordinal, figure_number, paint_equation_number};
use super::primitives::{TextRun, clip_viewport, fill, rounded};
use super::{Counters, Frame, LineBox};
use crate::editor::highlight::StyledSpan;
use crate::theme;

const IMAGE_TOP: f32 = 5.0;
const FIGURE_CAPTION_SIZE: f32 = 13.0;

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
                x: bounds.x + text_left(bounds.width),
                y: row.y + row.height / 2.0,
                width: (bounds.width - text_left(bounds.width) - 20.0 * page_scale(bounds.width))
                    .max(0.0),
                height: 2.0,
            },
            rounded(1.0),
            theme::ACCENT_GLOW,
        );
    }

    /// Paint a line laid out by its [`Flow`].
    pub(super) fn paint_flow<R: Paint>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
        flow: &Flow<'_>,
        counters: &mut Counters,
    ) {
        let (bounds, line) = (frame.bounds, row.line);
        let left = bounds.x + text_left(bounds.width);
        let right = left + text_column_width(bounds.width);

        for (span_idx, span) in line.spans.iter().enumerate() {
            if span.is_image && !span_is_editing(line, span_idx, row.is_editing, row.active_col) {
                counters.images += 1;
                self.paint_image(renderer, frame, row, span, counters.images);
            }
        }

        for item in &flow.items {
            let x = left + item.x;
            match &item.kind {
                ItemKind::Text(part) => {
                    // Each piece is drawn where layout put it: shaped runs
                    // don't add up across pieces once kerning is involved.
                    let span = &line.spans[item.span_idx];
                    let text = &span.visible_text(part.revealed)[part.bytes.clone()];
                    let size = item.font_size;
                    TextRun::new(
                        text,
                        size,
                        Size::new((right - x).max(1.0), size * LINE_BOX_FACTOR),
                    )
                    .font(part.font)
                    .draw(
                        renderer,
                        Point::new(
                            x,
                            self.snap_px(row.y + flow.text_top(item.row, size, part.font)),
                        ),
                        span.color,
                        frame.viewport,
                    );
                }
                ItemKind::Checkbox { checked } => {
                    paint_checkbox(renderer, frame, *checked, x, row.y + flow.axis(item.row));
                }
                ItemKind::Math { width, height } => {
                    let tex = math_source(&line.spans[item.span_idx]);
                    if let Some(math) = self.math_cache.get(tex) {
                        let top = row.y + flow.axis(item.row) - height / 2.0;
                        self.paint_math_bitmap(
                            renderer,
                            &math.inline_handle,
                            Rectangle {
                                x,
                                y: top,
                                width: *width,
                                height: *height,
                            },
                            frame.viewport,
                        );
                    }
                }
                ItemKind::Hidden => {}
            }
        }
    }

    /// A loaded image, centered in the text column, with its figure caption.
    fn paint_image<R: Paint>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
        span: &StyledSpan,
        running_number: usize,
    ) {
        let Some((handle, w, h)) = span
            .image_path
            .as_ref()
            .and_then(|p| self.image_cache.get(p))
        else {
            return;
        };
        let bounds = frame.bounds;

        let available_w = text_column_width(bounds.width);
        let scale = if *w > available_w && *w > 0.0 {
            available_w / w
        } else {
            1.0
        };
        let draw_w = w * scale;
        let draw_h = h * scale;
        let draw_x = bounds.x + text_left(bounds.width) + (available_w - draw_w) / 2.0;

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
    }

    /// Block math that isn't being edited: the rendered equation, or its TeX
    /// source until it has rendered.
    pub(super) fn paint_block_math<R: Paint>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
        counters: &mut Counters,
    ) {
        for span in &row.line.spans {
            let tex = math_source(span);
            if tex.is_empty() || span.is_syntax {
                continue;
            }
            counters.equations += 1;
            match self.math_cache.get(tex) {
                Some(math) => self.paint_rendered_block_math(renderer, frame, row, math, counters),
                None => self.paint_math_source(renderer, frame, row, tex, counters),
            }
        }
    }

    /// `y` snapped to the device-pixel grid, so text baselines and edges land
    /// on whole pixels instead of smearing across two.
    pub(super) fn snap_px(&self, y: f32) -> f32 {
        let sf = self.scale_factor;
        (y * sf).round() / sf
    }

    /// Draw an equation bitmap snapped to the device-pixel grid. Centering
    /// yields fractional positions with a different sub-pixel phase per axis,
    /// which makes horizontal and vertical strokes of the same glyph sample
    /// differently (one crisp, one split across two dim pixels).
    fn paint_math_bitmap<R: Paint>(
        &self,
        renderer: &mut R,
        handle: &iced::widget::image::Handle,
        bounds: Rectangle,
        clip: Rectangle,
    ) {
        let sf = self.scale_factor;
        let snap = |v: f32| (v * sf).round() / sf;
        renderer.draw_image(
            iced::advanced::image::Image::new(handle.clone()),
            Rectangle {
                x: snap(bounds.x),
                y: snap(bounds.y),
                width: snap(bounds.width),
                height: snap(bounds.height),
            },
            clip,
        );
    }

    fn paint_rendered_block_math<R: Paint>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
        math: &MathRender,
        counters: &Counters,
    ) {
        let (bounds, line, viewport) = (frame.bounds, row.line, frame.viewport);
        let (extent, scroll_x) = self.row_scroll::<R>(frame, row);
        // The block bitmap is rasterized for exactly this scale (see
        // MathRender).
        let draw_w = math.width * MATH_BLOCK_SCALE;
        let draw_h = math.height * MATH_BLOCK_SCALE;
        let left = bounds.x + text_left(bounds.width);
        let draw_x = left
            + if draw_w <= extent.viewport_w {
                (extent.viewport_w - draw_w) / 2.0
            } else {
                -scroll_x
            };
        let draw_y = row.y + (row.height - draw_h) / 2.0;

        let number =
            block_ordinal(self.lines, line.block_id, "equation-").unwrap_or(counters.equations);
        paint_equation_number(
            renderer,
            number,
            left + text_column_width(bounds.width),
            draw_y + draw_h / 2.0,
            draw_h,
            viewport,
        );

        let clip = clip_viewport(
            viewport,
            Rectangle {
                x: left,
                y: row.y,
                width: extent.viewport_w,
                height: row.height,
            },
        );
        let bitmap = Rectangle {
            x: draw_x,
            y: draw_y,
            width: draw_w,
            height: draw_h,
        };
        if draw_w <= extent.viewport_w {
            self.paint_math_bitmap(renderer, &math.block_handle, bitmap, clip);
        } else {
            // Renderers clip an image only to its layer, never to the clip
            // rectangle drawn with it.
            renderer.with_layer(clip, |renderer| {
                self.paint_math_bitmap(renderer, &math.block_handle, bitmap, clip);
            });
        }
    }

    /// TeX source of block math that has no rendered bitmap (yet).
    fn paint_math_source<R: Paint>(
        &self,
        renderer: &mut R,
        frame: &Frame<'_>,
        row: &LineBox<'_>,
        tex: &str,
        counters: &Counters,
    ) {
        let (bounds, line, viewport) = (frame.bounds, row.line, frame.viewport);
        let (extent, scroll_x) = self.row_scroll::<R>(frame, row);
        let left = bounds.x + text_left(bounds.width);

        let clip = clip_viewport(
            viewport,
            Rectangle {
                x: left,
                y: row.y,
                width: extent.viewport_w,
                height: row.height,
            },
        );

        let source_row = row_height(MATH_SOURCE_FONT_SIZE);
        let text_offset =
            centered_text_top(source_row, MATH_SOURCE_FONT_SIZE, iced::Font::MONOSPACE);
        let paint_source = |renderer: &mut R| {
            let mut row_top = row.y + MATH_BLOCK_PADDING / 2.0;
            for source_line in tex.lines() {
                TextRun::new(
                    source_line,
                    MATH_SOURCE_FONT_SIZE,
                    Size::new(
                        extent.content_w.max(1.0),
                        MATH_SOURCE_FONT_SIZE * LINE_BOX_FACTOR,
                    ),
                )
                .font(iced::Font::MONOSPACE)
                .draw(
                    renderer,
                    Point::new(left - scroll_x, row_top + text_offset),
                    theme::TEXT_SECONDARY,
                    viewport,
                );
                row_top += source_row;
            }
        };
        // Only source that doesn't fit needs clipping — and only a layer clips
        // text in every renderer.
        if scroll_x > 0.0 || extent.overflows() {
            renderer.with_layer(clip, paint_source);
        } else {
            paint_source(renderer);
        }

        let number =
            block_ordinal(self.lines, line.block_id, "equation-").unwrap_or(counters.equations);
        paint_equation_number(
            renderer,
            number,
            left + text_column_width(bounds.width),
            row.y + row.height / 2.0,
            body_row(),
            viewport,
        );
    }
}

/// A task-list checkbox with its left edge at `x`, centred on `center_y`.
fn paint_checkbox<R: Measure>(
    renderer: &mut R,
    frame: &Frame<'_>,
    checked: bool,
    x: f32,
    center_y: f32,
) {
    let box_y = center_y - CHECKBOX_SIZE / 2.0;
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

//! Drawing primitives shared by the painters.

use iced::advanced::renderer::Quad;
use iced::advanced::text;
use iced::alignment::{Horizontal, Vertical};
use iced::{Border, Color, Point, Rectangle, Size};

use super::super::measure::shaping_for;
use super::super::metrics::LINE_BOX_FACTOR;
use super::super::scroll::{ScrollExtent, scrollbar_geometry};
use super::super::{Measure, State};
use crate::theme;

const SCROLLBAR_THICKNESS: f32 = 4.0;

/// A border that only rounds corners.
pub(super) fn rounded(radius: f32) -> Border {
    Border {
        radius: radius.into(),
        ..Default::default()
    }
}

pub(super) fn fill<R: Measure>(renderer: &mut R, bounds: Rectangle, border: Border, color: Color) {
    renderer.fill_quad(
        Quad {
            bounds,
            border,
            ..Default::default()
        },
        color,
    );
}

/// A single run of text, shaped the way its font calls for.
pub(super) struct TextRun {
    content: String,
    bounds: Size,
    size: f32,
    font: iced::Font,
    align_x: Horizontal,
    align_y: Vertical,
    wrapping: text::Wrapping,
}

impl TextRun {
    /// Left/top aligned, non-wrapping text in the default font.
    pub fn new(content: impl Into<String>, size: f32, bounds: Size) -> Self {
        Self {
            content: content.into(),
            bounds,
            size,
            font: iced::Font::DEFAULT,
            align_x: Horizontal::Left,
            align_y: Vertical::Top,
            wrapping: text::Wrapping::None,
        }
    }

    pub fn font(mut self, font: iced::Font) -> Self {
        self.font = font;
        self
    }

    pub fn align(mut self, x: Horizontal, y: Vertical) -> Self {
        self.align_x = x;
        self.align_y = y;
        self
    }

    pub fn wrapping(mut self, wrapping: text::Wrapping) -> Self {
        self.wrapping = wrapping;
        self
    }

    pub fn draw<R: Measure>(self, renderer: &mut R, at: Point, color: Color, clip: Rectangle) {
        renderer.fill_text(
            text::Text {
                content: self.content,
                bounds: self.bounds,
                size: self.size.into(),
                line_height: text::LineHeight::default(),
                font: self.font,
                align_x: self.align_x.into(),
                align_y: self.align_y,
                shaping: shaping_for(self.font),
                wrapping: self.wrapping,
            },
            at,
            color,
            clip,
        );
    }
}

/// Unwrapped text whose top sits at `y`.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_nowrap_text<R: Measure>(
    renderer: &mut R,
    content: &str,
    x: f32,
    y: f32,
    max_width: f32,
    font_size: f32,
    font: iced::Font,
    color: Color,
    viewport: Rectangle,
) {
    if content.is_empty() {
        return;
    }
    TextRun::new(
        content,
        font_size,
        Size::new(max_width.max(1.0), font_size * LINE_BOX_FACTOR),
    )
    .font(font)
    .draw(renderer, Point::new(x, y), color, viewport);
}

/// Intersection of two rectangles (zero-sized when they don't overlap).
pub(super) fn clip_viewport(viewport: Rectangle, clip: Rectangle) -> Rectangle {
    let x1 = viewport.x.max(clip.x);
    let y1 = viewport.y.max(clip.y);
    let x2 = (viewport.x + viewport.width).min(clip.x + clip.width);
    let y2 = (viewport.y + viewport.height).min(clip.y + clip.height);
    Rectangle {
        x: x1,
        y: y1,
        width: (x2 - x1).max(0.0),
        height: (y2 - y1).max(0.0),
    }
}

/// Paint a block's horizontal scrollbar.
pub(super) fn paint_scrollbar<R: Measure>(
    renderer: &mut R,
    state: &State,
    block_id: usize,
    viewport_x: f32,
    extent: &ScrollExtent,
    y: f32,
) {
    let bar = scrollbar_geometry(viewport_x, extent, state.block_scroll(block_id, extent));
    let radius = rounded(SCROLLBAR_THICKNESS / 2.0);

    fill(
        renderer,
        Rectangle {
            x: viewport_x,
            y,
            width: bar.track_w,
            height: SCROLLBAR_THICKNESS,
        },
        radius,
        Color::from_rgba(1.0, 1.0, 1.0, 0.06),
    );
    fill(
        renderer,
        Rectangle {
            x: bar.thumb_x,
            y,
            width: bar.thumb_w,
            height: SCROLLBAR_THICKNESS,
        },
        radius,
        theme::ACCENT_DIM,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_viewport() {
        let viewport = Rectangle {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 200.0,
        };
        // Fully inside
        let clip_inside = Rectangle {
            x: 20.0,
            y: 30.0,
            width: 50.0,
            height: 50.0,
        };
        let res = clip_viewport(viewport, clip_inside);
        assert_eq!(res.x, 20.0);
        assert_eq!(res.y, 30.0);
        assert_eq!(res.width, 50.0);
        assert_eq!(res.height, 50.0);

        // No overlap
        let clip_no_overlap = Rectangle {
            x: 200.0,
            y: 300.0,
            width: 50.0,
            height: 50.0,
        };
        let res_none = clip_viewport(viewport, clip_no_overlap);
        assert_eq!(res_none.width, 0.0);
        assert_eq!(res_none.height, 0.0);

        // Partial overlap
        let clip_partial = Rectangle {
            x: 50.0,
            y: 100.0,
            width: 100.0,
            height: 200.0,
        };
        let res_partial = clip_viewport(viewport, clip_partial);
        assert_eq!(res_partial.x, 50.0);
        assert_eq!(res_partial.y, 100.0);
        assert_eq!(res_partial.width, 60.0);
        assert_eq!(res_partial.height, 120.0);
    }
}

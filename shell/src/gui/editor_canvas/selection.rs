use iced::widget::canvas;
use iced::{Point, Size};
use md_editor::layout::StyledLine;

use super::{content_left, content_width, palette};
use crate::gui::shaped_measurer::ShapedMeasurer;
use crate::gui::tokens::Tokens;

pub(super) fn paint_selection(
    frame: &mut canvas::Frame,
    measurer: &ShapedMeasurer,
    styled: &StyledLine,
    start: usize,
    end: usize,
    y: f32,
    width: f32,
    tokens: &Tokens,
) {
    for (x, top, rect_width, height) in
        measurer.selection_rects(styled, content_width(width), start, end)
    {
        frame.fill_rectangle(
            Point::new(content_left(width) + x, y + top),
            Size::new(rect_width, height),
            palette::selection(tokens),
        );
    }
}

//! `Popover`: floating elevation-1 card for selection toolbars, menus and link previews (MASTER §6).

use iced::widget::container;
use iced::{Background, Border, Element, Length, Renderer, Theme};

use crate::design;
use crate::design::metrics::{BORDER_WIDTH, Elevation, POPOVER_MAX_WIDTH, RADIUS_LG, SPACE_2};

pub fn popover<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    max_width: Option<f32>,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let bg = t.bg_raised;
    let border = t.border_default;

    container(content)
        .padding(SPACE_2)
        .max_width(max_width.unwrap_or(POPOVER_MAX_WIDTH))
        .width(Length::Shrink)
        .style(move |_| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: border,
                width: BORDER_WIDTH,
                radius: RADIUS_LG.into(),
            },
            shadow: Elevation::Popover.shadow(),
            ..Default::default()
        })
        .into()
}

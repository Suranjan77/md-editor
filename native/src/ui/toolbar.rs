//! `Toolbar` frames and group separators for the app header, viewer bars and find bar (MASTER §6).

use iced::widget::{Space, column, container, row};
use iced::{Alignment, Background, Element, Length, Renderer, Theme};

use super::focus::ring_border;
use crate::design;
use crate::design::metrics::{
    ControlSize, SPACE_0, SPACE_1, SPACE_2, SPACE_3, SPACE_PX, TOOLBAR_HEIGHT,
};

pub fn toolbar_separator<'a, Message: 'a>() -> Element<'a, Message, Theme, Renderer> {
    let col = design::tokens().border_subtle;
    container(Space::new())
        .width(Length::Fixed(SPACE_PX))
        .height(Length::Fixed(ControlSize::Sm.height() - SPACE_1))
        .style(move |_| container::Style {
            background: Some(Background::Color(col)),
            ..Default::default()
        })
        .into()
}

pub fn toolbar_frame<'a, Message: 'a>(
    children: Vec<Element<'a, Message, Theme, Renderer>>,
    focused: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    // Toolbars live inside the document sheet, so they share its ground.
    let bg = t.bg_canvas;
    let border_col = t.border_subtle;

    let bar = container(
        row(children)
            .spacing(SPACE_2)
            .align_y(Alignment::Center)
            .width(Length::Fill),
    )
    .height(Length::Fixed(TOOLBAR_HEIGHT))
    .padding([0.0, SPACE_3])
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(bg)),
        border: ring_border(focused, 0.0, None),
        ..Default::default()
    });

    let divider = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(SPACE_PX))
        .style(move |_| container::Style {
            background: Some(Background::Color(border_col)),
            ..Default::default()
        });

    column![bar, divider]
        .spacing(SPACE_0)
        .width(Length::Fill)
        .into()
}

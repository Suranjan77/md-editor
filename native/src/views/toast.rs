use iced::widget::{container, text};
use iced::{Element, Renderer, Theme};

use crate::messages::Message;
use crate::theme;

pub fn view<'a>(content: &'a str) -> Element<'a, Message, Theme, Renderer> {
    container(
        text(content)
            .size(theme::TEXT_BASE)
            .color(theme::TEXT_PRIMARY),
    )
    .padding([theme::SPACE_4, theme::SPACE_6])
    .style(|_| container::Style {
        background: Some(iced::Background::Color(theme::BG_SURFACE)),
        border: iced::Border {
            color: theme::BORDER,
            width: 1.0,
            radius: theme::RADIUS_SM.into(),
        },
        ..Default::default()
    })
    .into()
}

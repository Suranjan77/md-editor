use iced::widget::{container, text};
use iced::{Element, Length, Theme, Renderer};

use crate::messages::Message;
use crate::theme;

pub fn view<'a>(
    content: &'a str,
) -> Element<'a, Message, Theme, Renderer> {
    container(
        text(content)
            .size(14)
            .color(theme::TEXT_PRIMARY)
    )
    .padding([12, 20])
    .style(|_| container::Style {
        background: Some(iced::Background::Color(theme::BG_SURFACE)),
        border: iced::Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .into()
}

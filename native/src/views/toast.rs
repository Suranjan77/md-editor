use iced::widget::{container, text};
use iced::{Element, Renderer, Theme};

use crate::messages::Message;
use crate::theme;

pub(crate) fn view<'a>(content: &'a str) -> Element<'a, Message, Theme, Renderer> {
    container(text(content).size(14).color(theme::text_primary()))
        .padding([12, 20])
        .style(|_| container::Style {
            background: Some(iced::Background::Color(theme::bg_surface())),
            border: iced::Border {
                color: theme::border(),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .into()
}

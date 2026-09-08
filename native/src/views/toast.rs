use iced::widget::{container, text};
use iced::{Element, Renderer, Theme};

use crate::messages::Message;
use crate::theme;

/// Render the transient toast at `opacity`, so it fades in and out instead of
/// blinking on and off between frames.
pub fn view<'a>(content: &'a str, opacity: f32) -> Element<'a, Message, Theme, Renderer> {
    container(
        text(content)
            .size(theme::TEXT_BASE)
            .color(theme::fade(theme::TEXT_PRIMARY, opacity)),
    )
    .padding([theme::SPACE_4, theme::SPACE_6])
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(theme::fade(
            theme::BG_SURFACE,
            opacity,
        ))),
        border: iced::Border {
            color: theme::fade(theme::BORDER, opacity),
            width: 1.0,
            radius: theme::RADIUS_SM.into(),
        },
        ..Default::default()
    })
    .into()
}

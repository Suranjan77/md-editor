use iced::widget::{column, container, text};
use iced::{Background, Element, Length};

use super::Message;
use super::tokens::Tokens;

const BOLD: iced::Font = iced::Font {
    weight: iced::font::Weight::Bold,
    ..iced::Font::DEFAULT
};

pub(super) fn kpi_card<'a>(
    tokens: &'static Tokens,
    title: &'static str,
    value: String,
    sub: &'static str,
) -> Element<'a, Message> {
    container(
        column![
            text(title).size(9).color(tokens.text_muted).font(BOLD),
            text(value).size(16).color(tokens.accent).font(BOLD),
            text(sub).size(8).color(tokens.text_muted),
        ]
        .spacing(2),
    )
    .padding(8)
    .width(Length::FillPortion(1))
    .style(move |_| container::Style {
        background: Some(Background::Color(tokens.bg_secondary)),
        border: iced::Border {
            color: tokens.border_subtle,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    })
    .into()
}

pub(super) fn panel_style(tokens: &Tokens) -> container::Style {
    container::Style {
        background: Some(Background::Color(tokens.bg_primary)),
        border: iced::Border {
            color: tokens.border,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

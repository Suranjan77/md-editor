use iced::widget::{button, column, container, text, Space};
use iced::{Alignment, Element, Length, Theme, Renderer, Background};

use crate::messages::Message;
use crate::theme;

/// Render a premium welcome screen.
pub fn view<'a>() -> Element<'a, Message, Theme, Renderer> {
    let open_btn = button(
        container(text("Open Existing Vault").size(16))
            .padding([12, 24])
    )
    .on_press(Message::OpenVaultDialog)
    .style(button::primary);

    let create_btn = button(
        container(text("Create New Vault").size(16))
            .padding([12, 24])
    )
    .style(button::secondary);

    let content = column![
        text("󰠮").size(64).color(theme::ACCENT),
        text("Antigravity").size(42).color(theme::TEXT_PRIMARY),
        text("The ultimate markdown workspace").size(16).color(theme::TEXT_MUTED),
        Space::new().height(Length::Fixed(40.0)),
        open_btn,
        create_btn,
        Space::new().height(Length::Fixed(20.0)),
        text("Press Ctrl+O to open a folder").size(12).color(theme::TEXT_MUTED),
    ]
    .spacing(16)
    .align_x(Alignment::Center);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(theme::BG_PRIMARY)),
            ..Default::default()
        })
        .into()
}

use iced::widget::{Space, button, column, container, image, text};
use iced::{Alignment, Background, Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::theme;

/// Render a premium welcome screen.
pub fn view<'a>() -> Element<'a, Message, Theme, Renderer> {
    let open_btn =
        button(container(text("Open Existing Vault").size(theme::TEXT_MD)).padding([12, 24]))
            .on_press(Message::OpenVaultDialog)
            .style(button::primary);

    let app_icon_handle =
        iced::widget::image::Handle::from_bytes(include_bytes!("../../../md-editor.png").to_vec());
    let logo = image(app_icon_handle).width(128).height(128);

    let content = column![
        logo,
        text("Md-editor")
            .size(theme::TEXT_DISPLAY)
            .color(theme::TEXT_PRIMARY),
        text("The ultimate markdown workspace")
            .size(theme::TEXT_MD)
            .color(theme::TEXT_MUTED),
        Space::new().height(Length::Fixed(40.0)),
        open_btn,
        Space::new().height(Length::Fixed(20.0)),
        text("Press Ctrl+O to open a folder")
            .size(theme::TEXT_SM)
            .color(theme::TEXT_MUTED),
    ]
    .spacing(theme::SPACE_5)
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

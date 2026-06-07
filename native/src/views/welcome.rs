use iced::widget::{Space, button, column, container, image, row, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::theme;
use crate::views::icons::{self, Icon};

/// Render a premium welcome screen.
pub(crate) fn view<'a>(recent_vaults: &'a [String]) -> Element<'a, Message, Theme, Renderer> {
    let open_btn = button(
        row![
            icons::view(Icon::FolderOpen, Color::WHITE, 18.0),
            text("Open Vault").size(16).color(Color::WHITE)
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .on_press(Message::OpenVaultDialog)
    .padding([12, 24])
    .style(|theme: &Theme, status: button::Status| {
        let mut style = button::primary(theme, status);
        style.background = Some(Background::Color(theme::accent()));
        style.border = Border {
            radius: 8.0.into(),
            ..Default::default()
        };
        style
    });

    let secondary_btn = button(
        row![
            icons::view(Icon::FolderOpen, theme::text_primary(), 18.0),
            text("Create New Vault")
                .size(16)
                .color(theme::text_primary())
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .on_press(Message::CreateVaultDialog)
    .padding([12, 24])
    .style(|theme: &Theme, status: button::Status| {
        let mut style = button::text(theme, status);
        if status == button::Status::Hovered || status == button::Status::Pressed {
            style.background = Some(Background::Color(theme::bg_tertiary()));
        } else {
            style.background = Some(Background::Color(theme::bg_secondary()));
        }
        style.border = Border {
            radius: 8.0.into(),
            color: theme::border(),
            width: 1.0,
        };
        style
    });

    let app_icon_handle =
        iced::widget::image::Handle::from_bytes(include_bytes!("../../../md-editor.png").to_vec());
    let logo = image(app_icon_handle).width(128).height(128);

    let badge = container(text("Ctrl+O").size(11).color(theme::text_primary()))
        .padding([2, 6])
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(theme::bg_tertiary())),
            border: Border {
                radius: 4.0.into(),
                color: theme::border_subtle(),
                width: 1.0,
            },
            ..Default::default()
        });

    let kbd_hint = row![
        text("Or press").size(13).color(theme::text_muted()),
        badge,
        text("to open a folder").size(13).color(theme::text_muted())
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let mut recent_list = column![].spacing(6).align_x(Alignment::Center);
    if !recent_vaults.is_empty() {
        recent_list = recent_list.push(text("Recent Vaults").size(12).color(theme::text_muted()));
        for vault_path in recent_vaults {
            recent_list = recent_list.push(
                button(text(vault_path).size(13).color(theme::text_primary()))
                    .on_press(Message::OpenRecentVault(vault_path.clone()))
                    .padding([6, 10])
                    .style(button::text),
            );
        }
    }

    let content = column![
        logo,
        text("Md-editor").size(42).color(theme::text_primary()),
        text("Write notes, read PDFs, and keep citations connected")
            .size(16)
            .color(theme::text_muted()),
        Space::new().height(Length::Fixed(40.0)),
        row![open_btn, secondary_btn]
            .spacing(16)
            .align_y(Alignment::Center),
        Space::new().height(Length::Fixed(20.0)),
        kbd_hint,
        recent_list,
        Space::new().height(Length::Fixed(40.0)),
        text(format!("v{}", env!("CARGO_PKG_VERSION")))
            .size(11)
            .color(theme::text_muted())
    ]
    .spacing(16)
    .align_x(Alignment::Center);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(theme::bg_primary())),
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_exposes_open_create_and_package_version() {
        let mut ui = iced_test::simulator(view(&[]));

        ui.find("Open Vault").expect("open action should render");
        ui.click("Create New Vault")
            .expect("create action should render");
        ui.find(format!("v{}", env!("CARGO_PKG_VERSION")))
            .expect("package version should render");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(messages.as_slice(), [Message::CreateVaultDialog]));
    }

    #[test]
    fn welcome_recent_vault_emits_direct_open() {
        let recent = vec!["/vaults/research".to_string()];
        let mut ui = iced_test::simulator(view(&recent));

        ui.click("/vaults/research")
            .expect("recent vault row should render");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.as_slice(),
            [Message::OpenRecentVault(path)] if path == "/vaults/research"
        ));
    }
}

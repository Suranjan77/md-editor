//! Welcome view shown when no vault is open: wordmark, Open/Create actions, recent vaults and shortcut hints.

use iced::widget::{Column, Space, column, container, image, row, text};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use crate::design::metrics::{
    BORDER_WIDTH, DIALOG_WIDTH_MD, Elevation, RADIUS_XL, SPACE_1, SPACE_2, SPACE_3, SPACE_5,
    SPACE_6, SPACE_PX, WELCOME_LOGO_PX,
};
use crate::design::{TextRole, tokens};
use crate::messages::{Message, Shortcut, VaultMessage};
use crate::shortcuts;
use crate::ui::{Button, ButtonVariant, Icon, ListRowProps, kbd, list_row};

pub fn view<'a>(recent_vaults: &'a [String]) -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();

    let app_icon_handle =
        iced::widget::image::Handle::from_bytes(include_bytes!("../../../md-editor.png").to_vec());
    let logo = image(app_icon_handle)
        .width(WELCOME_LOGO_PX)
        .height(WELCOME_LOGO_PX);

    let actions = row![
        Button::new("Open folder…")
            .variant(ButtonVariant::Primary)
            .icon(Icon::FolderOpen)
            .on_press(Message::Vault(VaultMessage::OpenVaultDialog))
            .build(),
        Button::new("Create vault…")
            .variant(ButtonVariant::Secondary)
            .icon(Icon::FolderPlus)
            .on_press(Message::Vault(VaultMessage::CreateVaultDialog))
            .build(),
    ]
    .spacing(SPACE_3)
    .align_y(Alignment::Center);

    let hints = row![
        text("Open vault")
            .size(TextRole::Caption.size())
            .color(t.fg_tertiary)
            .font(TextRole::Caption.font()),
        kbd(shortcuts::chord(Shortcut::OpenVault)),
        Space::new().width(Length::Fixed(SPACE_3)),
        text("Command palette")
            .size(TextRole::Caption.size())
            .color(t.fg_tertiary)
            .font(TextRole::Caption.font()),
        kbd(shortcuts::chord(Shortcut::CommandPalette)),
    ]
    .spacing(SPACE_2)
    .align_y(Alignment::Center);

    let mut center_col = column![
        logo,
        text("MD Editor")
            .size(TextRole::Display.size())
            .line_height(TextRole::Display.line_height())
            .color(t.fg_primary)
            .font(TextRole::Display.font()),
        text("A quiet, local-first desk for notes, papers, slides and ink.")
            .size(TextRole::Body.size())
            .line_height(TextRole::Body.line_height())
            .color(t.fg_secondary)
            .font(TextRole::Body.font()),
        Space::new().height(Length::Fixed(SPACE_3)),
        actions,
        hints,
    ]
    .spacing(SPACE_3)
    .align_x(Alignment::Center);

    if !recent_vaults.is_empty() {
        let rows = recent_vaults.iter().map(|vault_path| {
            let folder_name = vault_path.rsplit('/').next().unwrap_or(vault_path);
            list_row(ListRowProps {
                title: folder_name.into(),
                subtitle: Some(vault_path.as_str().into()),
                leading_icon: Some(Icon::Folder),
                shortcut: None,
                selected: false,
                active_doc: false,
                focused: false,
                drag_over: false,
                max_title_chars: 36,
                on_press: Some(Message::Vault(VaultMessage::OpenRecentVault(
                    vault_path.clone(),
                ))),
            })
        });

        let rule_col = t.border_subtle;
        let recent = column![
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fixed(SPACE_PX))
                .style(move |_| container::Style {
                    background: Some(Background::Color(rule_col)),
                    ..Default::default()
                }),
            text("Recent vaults")
                .size(TextRole::Label.size())
                .line_height(TextRole::Label.line_height())
                .color(t.fg_tertiary)
                .font(TextRole::Label.font()),
            Column::with_children(rows).spacing(SPACE_1),
        ]
        .spacing(SPACE_3)
        .width(Length::Fill);

        center_col = center_col
            .push(Space::new().height(Length::Fixed(SPACE_2)))
            .push(recent);
    }

    // Folio: one paper sheet resting on the chrome ground.
    let sheet = container(center_col)
        .width(Length::Fixed(DIALOG_WIDTH_MD))
        .padding([SPACE_6, SPACE_6])
        .style(move |_| container::Style {
            background: Some(Background::Color(t.bg_canvas)),
            border: Border {
                color: t.border_subtle,
                width: BORDER_WIDTH,
                radius: RADIUS_XL.into(),
            },
            shadow: Elevation::Dialog.shadow(),
            ..Default::default()
        });

    container(sheet)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(SPACE_5)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(t.bg_surface)),
            ..Default::default()
        })
        .into()
}

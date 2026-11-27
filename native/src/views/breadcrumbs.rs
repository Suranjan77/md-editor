//! Path breadcrumbs for the status bar: vault name, clickable folders, and the active document's name.

use iced::widget::{Row, button, text};
use iced::{Alignment, Background, Border, Element, Renderer, Theme};

use crate::design::metrics::{IconSize, RADIUS_SM, SPACE_0, SPACE_1};
use crate::design::{TextRole, tokens};
use crate::messages::{Message, VaultMessage};
use crate::ui::{Icon, icon};

/// `vault › folder › … › file`; folders reveal themselves in the file tree when clicked.
pub fn view<'a>(
    vault: Option<&'a str>,
    path: Option<&'a str>,
) -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();
    let role = TextRole::Caption;
    let mut row = Row::new().spacing(SPACE_1).align_y(Alignment::Center);

    if let Some(vault) = vault.filter(|v| !v.is_empty()) {
        row = row
            .push(icon(Icon::Folder, IconSize::Sm.px(), t.fg_tertiary))
            .push(
                text(vault)
                    .size(role.size())
                    .line_height(role.line_height())
                    .font(role.font())
                    .color(t.fg_tertiary),
            );
    }

    let Some(path) = path else {
        return row.into();
    };

    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let mut accum = String::new();
    for (idx, part) in parts.iter().enumerate() {
        if idx > 0 || vault.is_some() {
            row = row.push(icon(Icon::ChevronRight, IconSize::Sm.px(), t.fg_tertiary));
        }
        if !accum.is_empty() {
            accum.push('/');
        }
        accum.push_str(part);
        if idx + 1 == parts.len() {
            row = row.push(
                text(*part)
                    .size(role.size())
                    .line_height(role.line_height())
                    .font(crate::design::fonts::UI_FONT_MEDIUM)
                    .color(t.fg_primary),
            );
        } else {
            let folder_path = accum.clone();
            row = row.push(
                button(
                    text(*part)
                        .size(role.size())
                        .line_height(role.line_height())
                        .font(role.font()),
                )
                .padding([SPACE_0, SPACE_1])
                .on_press(Message::Vault(VaultMessage::SidebarFolderToggled(
                    folder_path,
                )))
                .style(move |_, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: hovered.then_some(Background::Color(t.bg_raised)),
                        text_color: if hovered {
                            t.fg_primary
                        } else {
                            t.fg_secondary
                        },
                        border: Border {
                            radius: RADIUS_SM.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                }),
            );
        }
    }
    row.into()
}

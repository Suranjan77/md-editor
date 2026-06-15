//! The top command spine (docs/DESIGN-SYSTEM.md §5 "Top bar"). Replaces the old
//! menu bar: a 48px chrome strip with the vault switcher, rail toggles, the
//! centered ⌘K command bar (opens the palette), and settings — all routed
//! through `Message::RunCommand`, the same generic dispatcher the chrome uses.

use super::*;
use iced::widget::{column, row};

impl Shell {
    pub(super) fn command_spine(&self) -> Element<'_, Message> {
        let tokens = self.tokens();

        let vault_name = self
            .vault_root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.vault_root.display().to_string());
        let note_count = self.files.len();

        // Brand tile + vault switcher (opens a vault folder).
        let brand = container(
            text("M")
                .size(15)
                .font(chrome::BOLD)
                .color(tokens.text_primary),
        )
        .center(iced::Length::Fixed(26.0))
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(tokens.bg_tertiary)),
            border: iced::Border {
                color: tokens.border,
                width: 1.0,
                radius: 7.0.into(),
            },
            ..container::Style::default()
        });

        let vault_chip = button(
            row![
                brand,
                column![
                    text(vault_name).size(12).color(tokens.text_primary),
                    text(format!("vault · {note_count} notes"))
                        .size(10)
                        .color(tokens.text_muted),
                ]
                .spacing(1),
            ]
            .spacing(9)
            .align_y(iced::Alignment::Center),
        )
        .padding([4, 6])
        .style(button::text)
        .on_press(Message::RunCommand(CommandId("vault.open")));

        let divider = container(iced::widget::Space::new())
            .width(iced::Length::Fixed(1.0))
            .height(iced::Length::Fixed(20.0))
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(tokens.border)),
                ..container::Style::default()
            });

        let icon_button = |icon, command, color| {
            button(super::icons::view(icon, color, 16.0))
                .padding([4, 6])
                .style(button::text)
                .on_press(Message::RunCommand(CommandId(command)))
        };

        // Centered command bar — visually an input, opens the command palette.
        let command_bar = container(
            button(
                row![
                    super::icons::view(super::icons::Icon::Search, tokens.text_muted, 14.0),
                    text("Search files or run a command…")
                        .size(12)
                        .color(tokens.text_muted),
                    iced::widget::Space::new().width(Fill),
                    text("⌘K")
                        .size(11)
                        .font(super::fonts::MONO)
                        .color(tokens.text_muted),
                ]
                .spacing(9)
                .align_y(iced::Alignment::Center),
            )
            .width(Fill)
            .padding([6, 12])
            .style(move |theme, status| {
                let base = button::text(theme, status);
                button::Style {
                    background: Some(iced::Background::Color(tokens.bg_secondary)),
                    border: iced::Border {
                        color: tokens.border,
                        width: 1.0,
                        radius: 9.0.into(),
                    },
                    text_color: tokens.text_muted,
                    ..base
                }
            })
            .on_press(Message::RunCommand(CommandId("palette.open"))),
        )
        .max_width(560.0)
        .width(Fill);

        let tracker_color = if self.tracker_open {
            tokens.accent
        } else {
            tokens.text_secondary
        };

        let spine = row![
            vault_chip,
            divider,
            icon_button(
                super::icons::Icon::Sidebar,
                "workspace.toggle-files",
                tokens.text_secondary
            ),
            command_bar,
            icon_button(
                super::icons::Icon::Tracker,
                "workspace.toggle-tracker",
                tracker_color
            ),
            icon_button(
                super::icons::Icon::Settings,
                "app.settings",
                tokens.text_secondary
            ),
        ]
        .spacing(10)
        .padding([0, 12])
        .align_y(iced::Alignment::Center);

        container(spine)
            .width(Fill)
            .height(48)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(tokens.bg_chrome)),
                ..container::Style::default()
            })
            .into()
    }
}

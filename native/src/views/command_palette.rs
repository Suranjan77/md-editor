use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Length, Theme, Renderer};

use crate::messages::{Message, Shortcut};
use crate::theme;

pub struct Command {
    pub name: String,
    pub shortcut: Shortcut,
    pub icon: String,
}

pub fn get_commands() -> Vec<Command> {
    vec![
        Command { name: "New File".to_string(), shortcut: Shortcut::NewFile, icon: "📄".to_string() },
        Command { name: "Open Vault".to_string(), shortcut: Shortcut::OpenVault, icon: "📂".to_string() },
        Command { name: "Search Vault".to_string(), shortcut: Shortcut::Search, icon: "🔍".to_string() },
        Command { name: "Toggle Sidebar".to_string(), shortcut: Shortcut::ToggleSidebar, icon: "◀".to_string() },
        Command { name: "Toggle Backlinks".to_string(), shortcut: Shortcut::ToggleBacklinks, icon: "🔗".to_string() },
        Command { name: "Study Tracker".to_string(), shortcut: Shortcut::CommandPalette, icon: "⏱".to_string() }, // Reuse or add new
        Command { name: "Focus Mode".to_string(), shortcut: Shortcut::FocusMode, icon: "🧘".to_string() },
    ]
}

pub fn view<'a>(
    query: &str,
    commands: &'a [Command],
) -> Element<'a, Message, Theme, Renderer> {
    let input = text_input("Type a command or file name...", query)
        .on_input(Message::CommandPaletteQueryChanged)
        .padding(15)
        .size(18);

    let mut list = column![].spacing(5);

    let filtered: Vec<&Command> = if query.is_empty() {
        commands.iter().collect()
    } else {
        commands.iter()
            .filter(|c| c.name.to_lowercase().contains(&query.to_lowercase()))
            .collect()
    };

    for cmd in filtered {
        list = list.push(
            button(
                row![
                    text(&cmd.icon).size(18),
                    text(&cmd.name).size(14).color(theme::TEXT_PRIMARY),
                    Space::new().width(Length::Fill),
                    text(format!("{:?}", cmd.shortcut)).size(10).color(theme::TEXT_MUTED),
                ]
                .spacing(15)
                .align_y(Alignment::Center)
                .padding([10, 15])
            )
            .width(Length::Fill)
            .on_press(Message::CommandPaletteCommandClicked(cmd.shortcut))
            .style(button::text)
        );
    }

    container(
        column![
            container(input)
                .style(|_| container::Style {
                    border: iced::Border {
                        color: theme::BORDER,
                        width: 0.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                }),
            scrollable(list).height(Length::Fixed(300.0)),
        ]
        .spacing(0)
    )
    .width(Length::Fixed(500.0))
    .style(|_| container::Style {
        background: Some(iced::Background::Color(theme::BG_SECONDARY)),
        border: iced::Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    })
    .into()
}

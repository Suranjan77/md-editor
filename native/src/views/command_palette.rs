use iced::widget::{Space, button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length, Renderer, Theme};

use crate::messages::{Message, Shortcut};
use crate::theme;

pub struct Command {
    pub name: String,
    pub shortcut: Shortcut,
    pub icon: String,
}

pub fn get_commands() -> Vec<Command> {
    vec![
        Command {
            name: "New File".to_string(),
            shortcut: Shortcut::NewFile,
            icon: "+".to_string(),
        },
        Command {
            name: "Open Vault".to_string(),
            shortcut: Shortcut::OpenVault,
            icon: "O".to_string(),
        },
        Command {
            name: "Search Vault".to_string(),
            shortcut: Shortcut::Search,
            icon: "/".to_string(),
        },
        Command {
            name: "Toggle Sidebar".to_string(),
            shortcut: Shortcut::ToggleSidebar,
            icon: "S".to_string(),
        },
        Command {
            name: "Toggle Backlinks".to_string(),
            shortcut: Shortcut::ToggleBacklinks,
            icon: "B".to_string(),
        },
        Command {
            name: "Toggle Table of Contents".to_string(),
            shortcut: Shortcut::TableOfContents,
            icon: "T".to_string(),
        },
        Command {
            name: "Study Tracker".to_string(),
            shortcut: Shortcut::StudyTracker,
            icon: "R".to_string(),
        },
        Command {
            name: "Split View".to_string(),
            shortcut: Shortcut::SplitView,
            icon: "|".to_string(),
        },
        Command {
            name: "Focus Mode".to_string(),
            shortcut: Shortcut::FocusMode,
            icon: "F".to_string(),
        },
    ]
}

pub fn view<'a>(query: &str, commands: &'a [Command]) -> Element<'a, Message, Theme, Renderer> {
    let input = text_input("Type a command...", query)
        .on_input(Message::CommandPaletteQueryChanged)
        .padding(12)
        .size(16);

    let mut list = column![].spacing(5);

    let filtered: Vec<&Command> = if query.is_empty() {
        commands.iter().collect()
    } else {
        commands
            .iter()
            .filter(|c| c.name.to_lowercase().contains(&query.to_lowercase()))
            .collect()
    };

    for cmd in filtered {
        list = list.push(
            button(
                row![
                    container(text(&cmd.icon).size(12).color(theme::TEXT_SECONDARY))
                        .width(Length::Fixed(24.0))
                        .height(Length::Fixed(24.0))
                        .center_x(Length::Fixed(24.0))
                        .center_y(Length::Fixed(24.0))
                        .style(|_| container::Style {
                            background: Some(iced::Background::Color(theme::BG_TERTIARY)),
                            border: iced::Border {
                                color: theme::BORDER,
                                width: 1.0,
                                radius: 6.0.into(),
                            },
                            ..Default::default()
                        }),
                    text(&cmd.name).size(14).color(theme::TEXT_PRIMARY),
                    Space::new().width(Length::Fill),
                    text(shortcut_label(cmd.shortcut))
                        .size(11)
                        .color(theme::TEXT_MUTED),
                ]
                .spacing(12)
                .align_y(Alignment::Center)
                .padding([8, 12]),
            )
            .width(Length::Fill)
            .on_press(Message::CommandPaletteCommandClicked(cmd.shortcut))
            .style(button::text),
        );
    }

    container(
        column![
            container(input).style(|_| container::Style {
                border: iced::Border {
                    color: theme::BORDER,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            }),
            scrollable(list).height(Length::Fixed(320.0)),
        ]
        .spacing(0),
    )
    .width(Length::Fixed(520.0))
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

fn shortcut_label(shortcut: Shortcut) -> &'static str {
    match shortcut {
        Shortcut::Save => "Ctrl S",
        Shortcut::OpenVault => "Ctrl O",
        Shortcut::NewFile => "Ctrl N",
        Shortcut::Search => "Ctrl F",
        Shortcut::CommandPalette => "Ctrl P",
        Shortcut::ToggleSidebar => "Ctrl B",
        Shortcut::ToggleBacklinks => "Backlinks",
        Shortcut::FocusMode => "Focus",
        Shortcut::TableOfContents => "Ctrl T",
        Shortcut::StudyTracker => "Tracker",
        Shortcut::SplitView => "Split",
        Shortcut::Escape => "Esc",
        Shortcut::ZoomIn => "Ctrl +",
        Shortcut::ZoomOut => "Ctrl -",
        Shortcut::ZoomFit => "Ctrl 0",
        Shortcut::GoToPage => "Ctrl G",
        Shortcut::PdfSearch => "Ctrl R",
        Shortcut::PdfHighlight => "Ctrl H",
        Shortcut::PdfFirstPage => "Home",
        Shortcut::PdfLastPage => "End",
        Shortcut::PdfZoomInput => "Ctrl Z",
    }
}

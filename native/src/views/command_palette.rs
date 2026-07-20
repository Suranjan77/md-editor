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
            name: "Open Knowledge Graph".to_string(),
            shortcut: Shortcut::KnowledgeGraph,
            icon: "G".to_string(),
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

pub fn filtered_commands<'a>(query: &str, commands: &'a [Command]) -> Vec<&'a Command> {
    if query.is_empty() {
        commands.iter().collect()
    } else {
        let query = query.to_lowercase();
        commands
            .iter()
            .filter(|command| command.name.to_lowercase().contains(&query))
            .collect()
    }
}

pub fn selected_shortcut(query: &str, commands: &[Command], index: usize) -> Option<Shortcut> {
    filtered_commands(query, commands)
        .get(index)
        .map(|command| command.shortcut)
}

pub fn view<'a>(
    query: &str,
    commands: &'a [Command],
    selected_index: usize,
) -> Element<'a, Message, Theme, Renderer> {
    let input = text_input("Type a command...", query)
        .on_input(Message::CommandPaletteQueryChanged)
        .padding(12)
        .size(16);

    let mut list = column![].spacing(5);

    let filtered = filtered_commands(query, commands);

    for (index, cmd) in filtered.into_iter().enumerate() {
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
            .style(if index == selected_index {
                button::secondary
            } else {
                button::text
            }),
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
        Shortcut::KnowledgeGraph => "Ctrl G",
        Shortcut::FocusMode => "Focus",
        Shortcut::TableOfContents => "Ctrl T",
        Shortcut::StudyTracker => "Tracker",
        Shortcut::SplitView => "Split",
        Shortcut::Escape => "Esc",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtered_selection_runs_the_visible_command() {
        let commands = get_commands();
        let filtered = filtered_commands("toggle", &commands);
        assert!(filtered.len() >= 2);
        assert_eq!(
            selected_shortcut("toggle", &commands, 1),
            Some(filtered[1].shortcut)
        );
        assert_eq!(selected_shortcut("missing", &commands, 0), None);
    }

    #[test]
    fn knowledge_graph_is_discoverable() {
        let commands = get_commands();
        assert_eq!(
            selected_shortcut("knowledge graph", &commands, 0),
            Some(Shortcut::KnowledgeGraph)
        );
    }
}

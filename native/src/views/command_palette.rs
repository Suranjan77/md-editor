#![allow(dead_code)]

use iced::widget::{Space, button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length, Renderer, Theme};

use crate::messages::{Message, Shortcut};
use crate::theme;

pub const COMMAND_PALETTE_INPUT_ID: &str = "command_palette_input";

#[derive(Debug, Clone)]
pub struct Command {
    pub name: String,
    pub shortcut: Shortcut,
    pub icon: String,
    pub group_name: &'static str,
    pub shortcut_label: Option<String>,
    pub disabled_reason: Option<&'static str>,
}

pub fn insert_pdf_quote_command() -> Command {
    Command {
        name: "Insert PDF Quote".to_string(),
        shortcut: Shortcut::InsertPdfQuote,
        icon: "Q".to_string(),
        group_name: "Research",
        shortcut_label: Some("Quote".to_string()),
        disabled_reason: None,
    }
}

pub fn insert_pdf_highlight_command() -> Command {
    Command {
        name: "Insert PDF Highlight".to_string(),
        shortcut: Shortcut::InsertPdfHighlight,
        icon: "H".to_string(),
        group_name: "Research",
        shortcut_label: Some("Cite".to_string()),
        disabled_reason: None,
    }
}

pub fn get_commands() -> Vec<Command> {
    crate::command_registry::get_command_registry()
        .into_iter()
        .map(|meta| Command {
            name: meta.name.to_string(),
            shortcut: meta.id,
            icon: meta.icon.to_string(),
            group_name: match meta.group {
                crate::app_shell::CommandGroup::File => "File",
                crate::app_shell::CommandGroup::Edit => "Edit",
                crate::app_shell::CommandGroup::Navigation => "Navigation",
                crate::app_shell::CommandGroup::View => "View",
                crate::app_shell::CommandGroup::Research => "Research",
                crate::app_shell::CommandGroup::Annotation => "Annotation",
                crate::app_shell::CommandGroup::Search => "Search",
            },
            shortcut_label: meta.default_shortcut.map(|s| s.to_string()),
            disabled_reason: None,
        })
        .collect()
}

pub fn view<'a>(query: &str, commands: Vec<Command>) -> Element<'a, Message, Theme, Renderer> {
    let input = text_input("Type a command...", query)
        .id(iced::advanced::widget::Id::new(COMMAND_PALETTE_INPUT_ID))
        .on_input(Message::CommandPaletteQueryChanged)
        .padding(12)
        .size(16);

    let mut list = column![].spacing(5);

    let mut filtered = commands;
    if !query.is_empty() {
        let q = query.to_lowercase();
        filtered.retain(|c| c.name.to_lowercase().contains(&q));
        filtered.sort_by(|a, b| {
            let a_enabled = a.disabled_reason.is_none();
            let b_enabled = b.disabled_reason.is_none();
            if a_enabled != b_enabled {
                return b_enabled.cmp(&a_enabled);
            }
            let a_starts = a.name.to_lowercase().starts_with(&q);
            let b_starts = b.name.to_lowercase().starts_with(&q);
            if a_starts != b_starts {
                return b_starts.cmp(&a_starts);
            }
            a.name.cmp(&b.name)
        });
    } else {
        filtered.sort_by(|a, b| {
            let a_enabled = a.disabled_reason.is_none();
            let b_enabled = b.disabled_reason.is_none();
            if a_enabled != b_enabled {
                return b_enabled.cmp(&a_enabled);
            }
            let group_cmp = a.group_name.cmp(b.group_name);
            if group_cmp != std::cmp::Ordering::Equal {
                return group_cmp;
            }
            a.name.cmp(&b.name)
        });
    }

    for cmd in filtered {
        let is_disabled = cmd.disabled_reason.is_some();
        let content = if let Some(reason) = cmd.disabled_reason {
            row![
                container(text(cmd.icon.clone()).size(12).color(theme::text_muted()))
                    .width(Length::Fixed(24.0))
                    .height(Length::Fixed(24.0))
                    .center_x(Length::Fixed(24.0))
                    .center_y(Length::Fixed(24.0))
                    .style(|_| container::Style {
                        background: Some(iced::Background::Color(theme::bg_tertiary())),
                        border: iced::Border {
                            color: theme::border_subtle(),
                            width: 1.0,
                            radius: 6.0.into(),
                        },
                        ..Default::default()
                    }),
                column![
                    text(cmd.name.clone()).size(14).color(theme::text_muted()),
                    text(reason).size(11).color(theme::danger())
                ]
                .spacing(2),
                Space::new().width(Length::Fill),
                text(cmd.group_name).size(10).color(theme::text_muted()),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .padding([8, 12])
        } else {
            row![
                container(
                    text(cmd.icon.clone())
                        .size(12)
                        .color(theme::text_secondary())
                )
                .width(Length::Fixed(24.0))
                .height(Length::Fixed(24.0))
                .center_x(Length::Fixed(24.0))
                .center_y(Length::Fixed(24.0))
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(theme::bg_tertiary())),
                    border: iced::Border {
                        color: theme::border(),
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..Default::default()
                }),
                text(cmd.name.clone()).size(14).color(theme::text_primary()),
                Space::new().width(Length::Fill),
                text(
                    cmd.shortcut_label
                        .clone()
                        .unwrap_or_else(|| shortcut_label(cmd.shortcut).to_string())
                )
                .size(11)
                .color(theme::text_muted()),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .padding([8, 12])
        };

        let btn = button(content).width(Length::Fill);
        let btn = if is_disabled {
            btn.style(button::text)
        } else {
            btn.on_press(Message::CommandPaletteCommandClicked(cmd.shortcut))
                .style(button::text)
        };
        list = list.push(btn);
    }

    container(
        column![
            container(input).style(|_| container::Style {
                border: iced::Border {
                    color: theme::border(),
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
        background: Some(iced::Background::Color(theme::bg_secondary())),
        border: iced::Border {
            color: theme::border(),
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
        Shortcut::NavBack => "Alt Left",
        Shortcut::NavForward => "Alt Right",
        Shortcut::ToggleBacklinks => "Ctrl Alt B",
        Shortcut::FocusMode => "Focus",
        Shortcut::TableOfContents => "Ctrl T",
        Shortcut::StudyTracker => "Ctrl Alt S",
        Shortcut::SplitView => "Split",
        Shortcut::Escape => "Esc",
        Shortcut::ZoomIn => "Ctrl +",
        Shortcut::ZoomOut => "Ctrl -",
        Shortcut::ZoomFit => "Ctrl 0",
        Shortcut::GoToPage => "Ctrl G",
        Shortcut::PdfSearch => "Ctrl R",
        Shortcut::PdfHighlight => "Ctrl H",
        Shortcut::InsertPdfQuote => "Quote",
        Shortcut::InsertPdfHighlight => "Cite",
        Shortcut::PdfFirstPage => "Home",
        Shortcut::PdfLastPage => "End",
        Shortcut::PdfZoomInput => "Ctrl Z",
        Shortcut::FollowCitation => "Alt G",
        Shortcut::ShowUsages => "Alt U",
        Shortcut::CitationPalette => "Alt C",
        Shortcut::ExcerptModeToggle => "Alt E",
        Shortcut::ExcerptInsertBatch => "Alt I",
        Shortcut::Submit => "Enter",
        Shortcut::ThemeDark => "Dark Theme",
        Shortcut::ThemeLight => "Light Theme",
        Shortcut::ThemeHighContrast => "High Contrast",
        Shortcut::SwitchPane => "Alt P",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_palette_pdf_quote_click_emits_shortcut() {
        let commands = vec![insert_pdf_quote_command()];
        let mut ui = iced_test::simulator(view("", commands));

        ui.click("Insert PDF Quote")
            .expect("PDF quote command should render");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.as_slice(),
            [Message::CommandPaletteCommandClicked(
                Shortcut::InsertPdfQuote
            )]
        ));
    }

    #[test]
    fn command_palette_input_has_focusable_id() {
        let mut ui = iced_test::simulator(view("", get_commands()));

        ui.find(iced_test::selector::id(COMMAND_PALETTE_INPUT_ID))
            .expect("command palette input should expose deterministic focus id");
    }

    #[test]
    fn command_palette_pdf_highlight_click_emits_shortcut() {
        let commands = vec![insert_pdf_highlight_command()];
        let mut ui = iced_test::simulator(view("", commands));

        ui.click("Insert PDF Highlight")
            .expect("PDF highlight command should render");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.as_slice(),
            [Message::CommandPaletteCommandClicked(
                Shortcut::InsertPdfHighlight
            )]
        ));
    }

    #[test]
    fn command_palette_navigation_clicks_emit_cross_pane_shortcuts() {
        let commands = get_commands();
        let mut ui = iced_test::simulator(view("navigate", commands));

        ui.click("Navigate Back")
            .expect("navigation back command should render");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.as_slice(),
            [Message::CommandPaletteCommandClicked(Shortcut::NavBack)]
        ));

        let commands = get_commands();
        let mut ui = iced_test::simulator(view("forward", commands));
        ui.click("Navigate Forward")
            .expect("navigation forward command should render");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.as_slice(),
            [Message::CommandPaletteCommandClicked(Shortcut::NavForward)]
        ));
    }
}

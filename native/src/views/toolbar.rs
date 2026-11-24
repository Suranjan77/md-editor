//! The top bar.

use iced::widget::{Button, Space, button, container, row, text, tooltip};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use crate::messages::{
    EditorMessage, GraphMessage, InkMessage, Message, SearchMessage, ShellMessage, VaultMessage,
};
use crate::theme;
use crate::views::icons::{self, Icon};

pub fn view<'a>(
    active_path: Option<&'a str>,
    active_pdf_path: Option<&'a str>,
    dirty: bool,
    sidebar_visible: bool,
    backlinks_visible: bool,
    graph_visible: bool,
    tracker_visible: bool,
    toc_visible: bool,
    toc_available: bool,
    split_view_active: bool,
    split_available: bool,
    ink_visible: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let sidebar_toggle: Button<'_, Message, Theme, Renderer> = button(icons::view(
        Icon::LayoutPanelLeft,
        if sidebar_visible {
            theme::ACCENT
        } else {
            theme::TEXT_MUTED
        },
        18.0,
    ))
    .on_press(Message::Vault(VaultMessage::SidebarToggle))
    .padding(theme::SPACE_3)
    .style(button::text);

    let path_display = if let Some(path) = active_path.or(active_pdf_path) {
        row![
            text(path)
                .size(theme::TEXT_BASE)
                .color(theme::TEXT_PRIMARY)
                .font(iced::Font::default()),
            text(if dirty {
                " • Unsaved changes"
            } else {
                " • Saved"
            })
            .size(theme::TEXT_SM)
            .color(theme::TEXT_MUTED),
        ]
        .spacing(theme::SPACE_3)
        .align_y(Alignment::Center)
    } else {
        row![
            text("No file open")
                .size(theme::TEXT_BASE)
                .color(theme::TEXT_MUTED)
        ]
    };

    let split_button: Element<'_, Message, Theme, Renderer> = if split_available {
        button(icons::view(
            Icon::Split,
            if split_view_active {
                theme::ACCENT
            } else {
                theme::TEXT_MUTED
            },
            18.0,
        ))
        .on_press(Message::Shell(ShellMessage::SplitViewToggle))
        .padding(theme::SPACE_3)
        .style(button::text)
        .into()
    } else {
        Space::new().width(Length::Fixed(0.0)).into()
    };

    let toc_button: Element<'_, Message, Theme, Renderer> = if toc_available {
        button(icons::view(
            Icon::ListTree,
            if toc_visible {
                theme::ACCENT
            } else {
                theme::TEXT_MUTED
            },
            18.0,
        ))
        .on_press(Message::Editor(EditorMessage::ToggleTOC))
        .padding(theme::SPACE_3)
        .style(button::text)
        .into()
    } else {
        Space::new().width(Length::Fixed(0.0)).into()
    };

    let actions = row![
        button(icons::view(Icon::Search, theme::TEXT_MUTED, 18.0))
            .on_press(Message::Search(SearchMessage::GlobalSearchOpen))
            .padding(theme::SPACE_3)
            .style(button::text),
        button(icons::view(Icon::Command, theme::TEXT_MUTED, 18.0))
            .on_press(Message::Shell(ShellMessage::CommandPaletteOpen))
            .padding(theme::SPACE_3)
            .style(button::text),
        button(icons::view(
            Icon::Network,
            if graph_visible {
                theme::ACCENT
            } else {
                theme::TEXT_MUTED
            },
            18.0,
        ))
        .on_press(Message::Graph(GraphMessage::Toggle))
        .padding(8)
        .style(button::text),
        button(icons::view(
            Icon::Link,
            if backlinks_visible {
                theme::ACCENT
            } else {
                theme::TEXT_MUTED
            },
            18.0,
        ))
        .on_press(Message::KeyboardShortcut(
            crate::messages::Shortcut::ToggleBacklinks,
        ))
        .padding(8)
        .style(button::text),
        toc_button,
        split_button,
        tooltip(
            button(icons::view(
                Icon::Pen,
                if ink_visible {
                    theme::ACCENT
                } else {
                    theme::TEXT_MUTED
                },
                18.0,
            ))
            .on_press(Message::Ink(InkMessage::Toggle))
            .padding(8)
            .style(button::text),
            container(
                text("Handwriting  Ctrl I")
                    .size(11)
                    .color(theme::TEXT_SECONDARY)
            )
            .padding([4, 8])
            .style(|_| container::Style {
                background: Some(Background::Color(theme::BG_TERTIARY)),
                border: Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }),
            tooltip::Position::Bottom,
        ),
        button(icons::view(
            Icon::Clock,
            if tracker_visible {
                theme::ACCENT
            } else {
                theme::TEXT_MUTED
            },
            18.0
        ))
        .on_press(Message::Shell(ShellMessage::TrackerToggle))
        .padding(theme::SPACE_3)
        .style(button::text),
    ]
    .spacing(theme::SPACE_2);

    let content = row![
        sidebar_toggle,
        Space::new().width(Length::Fixed(16.0)),
        path_display,
        Space::new().width(Length::Fill),
        actions,
        Space::new().width(Length::Fixed(8.0)),
    ]
    .align_y(Alignment::Center)
    .padding([theme::SPACE_2, theme::SPACE_4]);

    container(content)
        .width(Length::Fill)
        .height(Length::Fixed(48.0))
        .style(|_| container::Style {
            background: Some(Background::Color(theme::BG_PRIMARY)),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

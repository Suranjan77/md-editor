use iced::widget::{Button, Space, button, container, row, text};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::theme;
use crate::views::icons::{self, Icon};

pub fn view<'a>(
    active_path: Option<&'a str>,
    active_pdf_path: Option<&'a str>,
    _sync_status: Option<&'a str>,
    sidebar_visible: bool,
    _backlinks_visible: bool,
    tracker_visible: bool,
    toc_visible: bool,
    toc_available: bool,
    split_view_active: bool,
    split_available: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let sidebar_toggle: Button<'_, Message, Theme, Renderer> = button(icons::view(
        Icon::LayoutPanelLeft,
        if sidebar_visible {
            theme::accent()
        } else {
            theme::text_muted()
        },
        18.0,
    ))
    .on_press(Message::SidebarToggle)
    .padding(8)
    .style(button::text);

    let path_display = if let Some(path) = active_path.or(active_pdf_path) {
        row![
            text(path)
                .size(13)
                .color(theme::text_primary())
                .font(iced::Font::default()),
            text(" • Saved").size(11).color(theme::text_muted()),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    } else {
        row![text("No file open").size(13).color(theme::text_muted())]
    };

    let split_button: Element<'_, Message, Theme, Renderer> = if split_available {
        button(icons::view(
            Icon::Split,
            if split_view_active {
                theme::accent()
            } else {
                theme::text_muted()
            },
            18.0,
        ))
        .on_press(Message::SplitViewToggle)
        .padding(8)
        .style(button::text)
        .into()
    } else {
        Space::new().width(Length::Fixed(0.0)).into()
    };

    let toc_button: Element<'_, Message, Theme, Renderer> = if toc_available {
        button(icons::view(
            Icon::ListTree,
            if toc_visible {
                theme::accent()
            } else {
                theme::text_muted()
            },
            18.0,
        ))
        .on_press(Message::ToggleTOC)
        .padding(8)
        .style(button::text)
        .into()
    } else {
        Space::new().width(Length::Fixed(0.0)).into()
    };

    let actions = row![
        button(icons::view(Icon::Search, theme::text_muted(), 18.0))
            .on_press(Message::GlobalSearchOpen)
            .padding(8)
            .style(button::text),
        button(icons::view(Icon::Command, theme::text_muted(), 18.0))
            .on_press(Message::CommandPaletteOpen)
            .padding(8)
            .style(button::text),
        toc_button,
        split_button,
        button(icons::view(
            Icon::Clock,
            if tracker_visible {
                theme::accent()
            } else {
                theme::text_muted()
            },
            18.0
        ))
        .on_press(Message::TrackerToggle)
        .padding(8)
        .style(button::text),
    ]
    .spacing(4);

    let content = row![
        sidebar_toggle,
        Space::new().width(Length::Fixed(16.0)),
        path_display,
        Space::new().width(Length::Fill),
        actions,
        Space::new().width(Length::Fixed(8.0)),
    ]
    .align_y(Alignment::Center)
    .padding([4, 12]);

    container(content)
        .width(Length::Fill)
        .height(Length::Fixed(48.0))
        .style(|_| container::Style {
            background: Some(Background::Color(theme::bg_primary())),
            border: Border {
                color: theme::border(),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

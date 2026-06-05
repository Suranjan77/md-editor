use iced::widget::tooltip::Position;
use iced::widget::{Space, button, container, row, text, tooltip};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::theme;
use crate::views::icons::{self, Icon};

fn action_button_style(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(theme::bg_tertiary())),
            border: Border {
                radius: theme::RADIUS_REGULAR.into(),
                ..Default::default()
            },
            text_color: if active {
                theme::accent()
            } else {
                theme::text_primary()
            },
            ..button::Style::default()
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(theme::bg_surface())),
            border: Border {
                radius: theme::RADIUS_REGULAR.into(),
                ..Default::default()
            },
            text_color: if active {
                theme::accent()
            } else {
                theme::text_primary()
            },
            ..button::Style::default()
        },
        button::Status::Disabled => button::Style {
            text_color: theme::text_muted(),
            ..button::Style::default()
        },
        button::Status::Active => button::Style {
            background: if active {
                Some(Background::Color(theme::accent_dim()))
            } else {
                None
            },
            border: if active {
                Border {
                    radius: theme::RADIUS_REGULAR.into(),
                    ..Default::default()
                }
            } else {
                Border::default()
            },
            text_color: if active {
                theme::accent()
            } else {
                theme::text_muted()
            },
            ..button::Style::default()
        },
    }
}

pub fn view<'a>(
    active_path: Option<&'a str>,
    active_pdf_path: Option<&'a str>,
    sidebar_visible: bool,
    tracker_visible: bool,
    toc_visible: bool,
    toc_available: bool,
    split_view_active: bool,
    split_available: bool,
    annotations_visible: bool,
    pdf_open: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let sidebar_toggle_btn = button(icons::view(
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
    .style(action_button_style(sidebar_visible));

    let sidebar_toggle = tooltip(sidebar_toggle_btn, "Toggle Sidebar", Position::Bottom);

    let path_display = if let Some(path) = active_path.or(active_pdf_path) {
        let basename = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);
        row![
            text(basename)
                .size(13)
                .color(theme::text_primary())
                .font(iced::Font::default()),
        ]
        .align_y(Alignment::Center)
    } else {
        row![text("No file open").size(13).color(theme::text_muted())]
    };

    let split_button: Element<'_, Message, Theme, Renderer> = if split_available {
        tooltip(
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
            .style(action_button_style(split_view_active)),
            "Split View",
            Position::Bottom,
        )
        .into()
    } else {
        Space::new().width(Length::Fixed(0.0)).into()
    };

    let toc_button: Element<'_, Message, Theme, Renderer> = if toc_available {
        tooltip(
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
            .style(action_button_style(toc_visible)),
            "Outline / TOC",
            Position::Bottom,
        )
        .into()
    } else {
        Space::new().width(Length::Fixed(0.0)).into()
    };

    let annotations_button: Element<'_, Message, Theme, Renderer> = if pdf_open {
        tooltip(
            button(icons::view(
                Icon::FileText,
                if annotations_visible {
                    theme::accent()
                } else {
                    theme::text_muted()
                },
                18.0,
            ))
            .on_press(Message::PdfToggleAnnotationsSidebar)
            .padding(8)
            .style(action_button_style(annotations_visible)),
            "Annotations",
            Position::Bottom,
        )
        .into()
    } else {
        Space::new().width(Length::Fixed(0.0)).into()
    };

    let search_button = tooltip(
        button(icons::view(Icon::Search, theme::text_muted(), 18.0))
            .on_press(Message::GlobalSearchOpen)
            .padding(8)
            .style(action_button_style(false)),
        "Global Search (Ctrl+F)",
        Position::Bottom,
    );

    let command_button = tooltip(
        button(icons::view(Icon::Command, theme::text_muted(), 18.0))
            .on_press(Message::CommandPaletteOpen)
            .padding(8)
            .style(action_button_style(false)),
        "Command Palette (Ctrl+P)",
        Position::Bottom,
    );

    let tracker_button = tooltip(
        button(icons::view(
            Icon::Clock,
            if tracker_visible {
                theme::accent()
            } else {
                theme::text_muted()
            },
            18.0,
        ))
        .on_press(Message::TrackerToggle)
        .padding(8)
        .style(action_button_style(tracker_visible)),
        "Study Tracker",
        Position::Bottom,
    );

    let divider = container(Space::new())
        .width(Length::Fixed(1.0))
        .height(Length::Fixed(16.0))
        .style(|_| container::Style {
            background: Some(Background::Color(theme::border_subtle())),
            ..Default::default()
        });

    let actions = row![
        search_button,
        command_button,
        toc_button,
        annotations_button,
        split_button,
        tracker_button,
    ]
    .spacing(4);

    let content = row![
        sidebar_toggle,
        Space::new().width(Length::Fixed(16.0)),
        path_display,
        Space::new().width(Length::Fixed(16.0)),
        divider,
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

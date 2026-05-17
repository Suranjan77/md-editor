use iced::widget::{button, container, row, text, Space, Button};
use iced::{Alignment, Element, Length, Theme, Renderer, Border, Background};

use crate::messages::Message;
use crate::theme;

pub fn view<'a>(
    active_path: Option<&'a str>,
    active_pdf_path: Option<&'a str>,
    _sync_status: Option<&'a str>,
    sidebar_visible: bool,
    backlinks_visible: bool,
    tracker_visible: bool,
    toc_visible: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let sidebar_toggle: Button<'_, Message, Theme, Renderer> = button(
        text("≡")
            .size(20)
            .color(if sidebar_visible { theme::ACCENT } else { theme::TEXT_MUTED })
    )
    .on_press(Message::SidebarToggle)
    .padding(8)
    .style(button::text);

    let path_display = if let Some(path) = active_path.or(active_pdf_path) {
        row![
            text(path).size(13).color(theme::TEXT_PRIMARY).font(iced::Font::default()),
            text(" • Saved").size(11).color(theme::TEXT_MUTED),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    } else {
        row![text("No file open").size(13).color(theme::TEXT_MUTED)]
    };

    let actions = row![
        button(text("🔍").size(14))
            .on_press(Message::SearchOpen)
            .padding(8)
            .style(button::text),
        button(text("☰").size(14).color(if toc_visible { theme::ACCENT } else { theme::TEXT_MUTED }))
            .on_press(Message::ToggleTOC)
            .padding(8)
            .style(button::text),
        button(text("◷").size(14).color(if tracker_visible { theme::ACCENT } else { theme::TEXT_MUTED }))
            .on_press(Message::TrackerToggle)
            .padding(8)
            .style(button::text),
        button(text("🔔").size(14).color(if backlinks_visible { theme::ACCENT } else { theme::TEXT_MUTED }))
            .on_press(Message::BacklinksToggle)
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

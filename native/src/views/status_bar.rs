use iced::widget::{Space, container, row, text};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use crate::app_shell::{AppShellPane, AppShellStatus, SaveStatus};
use crate::messages::Message;
use crate::theme;
use crate::views::icons::{self, Icon};

const BOLD_FONT: iced::Font = iced::Font {
    weight: iced::font::Weight::Bold,
    ..iced::Font::DEFAULT
};

pub fn view<'a>(status: AppShellStatus) -> Element<'a, Message, Theme, Renderer> {
    // 1. Active pane indicator
    let pane_element: Element<'a, Message, Theme, Renderer> = match status.active_pane {
        AppShellPane::Markdown => row![
            icons::view(Icon::FileText, theme::ACCENT, 14.0),
            text("EDITOR").size(11).font(BOLD_FONT).color(theme::ACCENT)
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into(),
        AppShellPane::Pdf => row![
            icons::view(Icon::File, theme::ACCENT, 14.0),
            text("PDF").size(11).font(BOLD_FONT).color(theme::ACCENT)
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into(),
        AppShellPane::Image => row![
            icons::view(Icon::Image, theme::ACCENT, 14.0),
            text("IMAGE").size(11).font(BOLD_FONT).color(theme::ACCENT)
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into(),
        AppShellPane::None => text("NO ACTIVE PANE")
            .size(11)
            .color(theme::TEXT_MUTED)
            .into(),
    };

    // 2. Save status
    let save_element: Element<'a, Message, Theme, Renderer> = match status.save_status {
        SaveStatus::Unsaved => row![
            text("●").size(11).color(theme::DANGER),
            text("Unsaved").size(11).color(theme::TEXT_SECONDARY)
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into(),
        SaveStatus::Saved => row![
            text("✓").size(11).color(theme::SUCCESS),
            text("Saved").size(11).color(theme::TEXT_MUTED)
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into(),
        SaveStatus::NoDocument => Space::new().width(Length::Fixed(0.0)).into(),
    };

    // Left group
    let left_group = row![
        pane_element,
        Space::new().width(Length::Fixed(16.0)),
        save_element,
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // 3. Center messages (toast/errors)
    let center_group = if let Some(msg) = status.message {
        let text_color = if msg.to_lowercase().contains("fail")
            || msg.to_lowercase().contains("error")
            || msg.to_lowercase().contains("corrupt")
        {
            theme::DANGER
        } else {
            theme::SUCCESS
        };
        text(msg)
            .size(11)
            .color(text_color)
            .align_x(iced::alignment::Horizontal::Center)
    } else {
        text("")
            .size(11)
            .color(theme::TEXT_MUTED)
            .align_x(iced::alignment::Horizontal::Center)
    };

    // 4. Right group: search status, pdf status
    let mut right_elements = Vec::new();

    if let Some(search) = status.search_status {
        right_elements.push(
            row![
                icons::view(Icon::Search, theme::TEXT_MUTED, 12.0),
                text(search).size(11).color(theme::TEXT_SECONDARY)
            ]
            .spacing(4)
            .align_y(Alignment::Center)
            .into(),
        );
    }

    if let Some(pdf) = status.pdf_status {
        right_elements.push(
            row![
                icons::view(Icon::File, theme::TEXT_MUTED, 12.0),
                text(pdf).size(11).color(theme::TEXT_SECONDARY)
            ]
            .spacing(4)
            .align_y(Alignment::Center)
            .into(),
        );
    }

    let right_group = row(right_elements).spacing(16).align_y(Alignment::Center);

    let content = row![
        left_group,
        Space::new().width(Length::Fill),
        center_group,
        Space::new().width(Length::Fill),
        right_group
    ]
    .width(Length::Fill)
    .align_y(Alignment::Center);

    container(content)
        .width(Length::Fill)
        .height(Length::Fixed(24.0))
        .padding([2, 12])
        .style(|_| container::Style {
            background: Some(Background::Color(theme::BG_SECONDARY)),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

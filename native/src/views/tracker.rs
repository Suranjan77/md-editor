use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length, Theme, Renderer};

use crate::messages::Message;
use crate::theme;
use md_editor_core::tracker::StudySession;

pub fn view<'a>(
    visible: bool,
    running: bool,
    sessions: &'a [StudySession],
) -> Element<'a, Message, Theme, Renderer> {
    if !visible {
        return container(text("")).width(Length::Fixed(0.0)).into();
    }

    let title = text("Study Tracker")
        .size(20)
        .color(theme::ACCENT);

    let controls = row![
        if running {
            button(text("Stop Timer").size(14))
                .on_press(Message::TrackerStop)
                .padding(10)
                .style(button::secondary)
        } else {
            button(text("Start Timer").size(14))
                .on_press(Message::TrackerStart)
                .padding(10)
                .style(button::primary)
        },
        button(text("Close").size(14))
            .on_press(Message::TrackerToggle)
            .padding(10)
            .style(button::text),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let sessions_list: Element<'a, Message, Theme, Renderer> = if sessions.is_empty() {
        text("No sessions yet. Start studying!")
            .color(theme::TEXT_MUTED)
            .size(14)
            .into()
    } else {
        let mut col = column![].spacing(10);
        for session in sessions {
            col = col.push(
                container(
                    row![
                        column![
                            text(&session.date).size(12).color(theme::TEXT_MUTED),
                            text(format!("{:.2} hours", session.hours)).size(16).color(theme::TEXT_PRIMARY),
                        ].width(Length::Fill),
                        text(&session.activity_type).size(12).color(theme::ACCENT),
                    ]
                    .align_y(Alignment::Center)
                    .padding(10)
                )
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(theme::BG_SECONDARY)),
                    border: iced::Border {
                        color: theme::BORDER,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
            );
        }
        scrollable(col).height(Length::Fill).into()
    };

    container(
        column![
            title,
            controls,
            text("Recent Sessions").size(16).color(theme::TEXT_PRIMARY),
            sessions_list,
        ]
        .spacing(20)
        .padding(20)
    )
    .width(Length::Fixed(300.0))
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(iced::Background::Color(theme::BG_PRIMARY)),
        border: iced::Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

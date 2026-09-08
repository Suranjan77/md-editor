use iced::widget::{Space, button, column, container, scrollable, text};
use iced::{Element, Length, Padding, Renderer, Theme};

use crate::messages::Message;
use crate::theme;

pub struct TocEntry {
    pub level: u8,
    pub text: String,
    pub line: usize,
}

pub fn get_toc(buffer_text: &str) -> Vec<TocEntry> {
    let mut toc = Vec::new();
    let mut in_code_block = false;
    for (i, line) in buffer_text.split('\n').enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }
        if trimmed.starts_with('#') {
            let mut level = 0;
            for c in trimmed.chars() {
                if c == '#' {
                    level += 1;
                } else {
                    break;
                }
            }
            if level > 0 && level <= 6 {
                let text = trimmed[level..].trim().to_string();
                toc.push(TocEntry {
                    level: level as u8,
                    text,
                    line: i,
                });
            }
        }
    }
    toc
}

pub fn view<'a>(toc: &'a [TocEntry], is_synthetic: bool) -> Element<'a, Message, Theme, Renderer> {
    let title = text("Table of Contents")
        .size(theme::TEXT_MD)
        .color(theme::TEXT_PRIMARY);

    // Subtle badge so the user knows a bookmark-less PDF's outline was
    // generated heuristically from page text rather than embedded bookmarks.
    let badge: Element<'a, Message, Theme, Renderer> = if is_synthetic {
        container(
            text("Generated")
                .size(theme::TEXT_SM)
                .color(theme::TEXT_MUTED),
        )
        .padding(Padding {
            top: 2.0,
            right: 6.0,
            bottom: 2.0,
            left: 6.0,
        })
        .style(|_| container::Style {
            background: Some(iced::Background::Color(theme::BG_PRIMARY)),
            border: iced::Border {
                color: theme::BORDER,
                width: 1.0,
                radius: theme::RADIUS_SM.into(),
            },
            ..Default::default()
        })
        .into()
    } else {
        Space::new().height(Length::Fixed(0.0)).into()
    };

    let items = toc.iter().map(|entry| {
        let indent = (entry.level.saturating_sub(1) as f32) * 15.0;

        container(
            button(
                text(&entry.text)
                    .size(theme::TEXT_BASE)
                    .color(theme::TEXT_SECONDARY),
            )
            .on_press(Message::TocClicked(entry.line))
            .padding([theme::SPACE_2, theme::SPACE_3])
            .style(button::text)
            .width(Length::Fill),
        )
        .padding(Padding {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: indent,
        })
        .into()
    });

    container(
        column![
            title,
            badge,
            Space::new().height(Length::Fixed(10.0)),
            scrollable(column(items).spacing(theme::SPACE_1))
        ]
        .spacing(theme::SPACE_2)
        .padding(theme::SPACE_5),
    )
    .width(Length::Fixed(250.0))
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(iced::Background::Color(theme::BG_SECONDARY)),
        border: iced::Border {
            color: theme::BORDER,
            width: 1.0,
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

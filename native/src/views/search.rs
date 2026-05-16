use iced::widget::{button, column, container, row, scrollable, text, text_input, Column};
use iced::{Alignment, Element, Length, Theme, Renderer};

use crate::messages::Message;
use crate::theme;

/// Render the vault search overlay.
pub fn view<'a>(
    query: &'a str,
    results: &'a [md_editor_core::types::SearchResult],
    visible: bool,
) -> Element<'a, Message, Theme, Renderer> {
    if !visible {
        return container(text("")).width(Length::Fixed(0.0)).height(Length::Fixed(0.0)).into();
    }

    let search_input = text_input("Search vault...", query)
        .on_input(Message::SearchQueryChanged)
        .padding([10, 14])
        .size(15)
        .width(Length::Fill);

    let close_btn = button(text("✕").size(14).color(theme::TEXT_MUTED))
        .on_press(Message::SearchClose)
        .padding([6, 10])
        .style(button::text);

    let header = row![
        text("🔍").size(16).color(theme::ACCENT),
        search_input,
        close_btn,
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .padding(16);

    let result_list: Column<'_, Message, Theme, Renderer> = results.iter().fold(
        Column::new().spacing(2),
        |col, result| {
            let path_text = text(&result.path)
                .size(13)
                .color(theme::ACCENT);
            let context_text = text(&result.context)
                .size(12)
                .color(theme::TEXT_SECONDARY);

            let item: iced::widget::Button<'_, Message, Theme, Renderer> = button(
                column![path_text, context_text].spacing(2),
            )
            .on_press(Message::SearchResultClicked(result.path.clone()))
            .padding([8, 12])
            .width(Length::Fill)
            .style(button::text);

            col.push(item)
        },
    );

    let empty_state = if results.is_empty() && !query.is_empty() {
        Some(
            text("No results found")
                .size(12)
                .color(theme::TEXT_MUTED),
        )
    } else {
        None
    };

    let mut content = column![
        header,
        scrollable(
            column![
                result_list,
            ]
            .padding([0, 16])
        )
        .height(Length::Fill),
    ];

    if let Some(empty) = empty_state {
        content = content.push(container(empty).padding([16, 16]).width(Length::Fill));
    }

    container(content)
        .width(Length::Fixed(520.0))
        .max_height(500.0)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(theme::BG_SECONDARY)),
            border: iced::Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 8.0.into(),
            },
            shadow: iced::Shadow {
                color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.5),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 20.0,
            },
            ..Default::default()
        })
        .into()
}

use iced::widget::{column, container, scrollable, text, button, Column};
use iced::{Element, Length, Theme, Renderer};

use crate::messages::Message;
use crate::theme;

/// Render the backlinks panel.
pub fn view<'a>(
    backlinks: &'a [String],
    visible: bool,
) -> Element<'a, Message, Theme, Renderer> {
    if !visible {
        return container(text("")).width(Length::Fixed(0.0)).into();
    }

    let header = text("BACKLINKS")
        .size(10)
        .color(theme::TEXT_MUTED);

    let count_text = if backlinks.is_empty() {
        text("No backlinks found")
            .size(12)
            .color(theme::TEXT_MUTED)
    } else {
        text(format!("{} links", backlinks.len()))
            .size(10)
            .color(theme::ACCENT)
    };

    let list: Column<'_, Message, Theme, Renderer> = backlinks
        .iter()
        .fold(Column::new().spacing(4), |col, link| {
            let btn: iced::widget::Button<'_, Message, Theme, Renderer> = button(
                text(link).size(12).color(theme::TEXT_SECONDARY),
            )
            .on_press(Message::SidebarFileClicked(link.clone()))
            .padding([6, 10])
            .width(Length::Fill)
            .style(button::text);

            col.push(btn)
        });

    let content = column![
        column![header, count_text].spacing(4).padding([12, 14]),
        scrollable(list.padding([0, 14])).height(Length::Fill),
    ]
    .width(Length::Fixed(220.0));

    container(content)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(theme::BG_SECONDARY)),
            border: iced::Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .height(Length::Fill)
        .into()
}

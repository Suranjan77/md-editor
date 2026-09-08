use iced::widget::{Column, button, column, container, scrollable, text};
use iced::{Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::theme;

/// Render the backlinks panel.
pub fn view<'a>(
    backlinks: &'a [md_editor_core::types::BacklinkItem],
    visible: bool,
) -> Element<'a, Message, Theme, Renderer> {
    if !visible {
        return container(text("")).width(Length::Fixed(0.0)).into();
    }

    let header = text("BACKLINKS")
        .size(theme::TEXT_XS)
        .color(theme::TEXT_MUTED);

    let count_text = if backlinks.is_empty() {
        text("No backlinks found")
            .size(theme::TEXT_SM)
            .color(theme::TEXT_MUTED)
    } else {
        text(format!("{} links", backlinks.len()))
            .size(theme::TEXT_XS)
            .color(theme::ACCENT)
    };

    let list: Column<'_, Message, Theme, Renderer> =
        backlinks
            .iter()
            .fold(Column::new().spacing(theme::SPACE_3), |col, item| {
                let msg = match &item.source {
                    md_editor_core::types::BacklinkTarget::MarkdownFile { path } => {
                        Message::SidebarFileClicked(path.clone())
                    }
                    md_editor_core::types::BacklinkTarget::PdfDocument { path } => {
                        Message::SidebarFileClicked(path.clone())
                    }
                    md_editor_core::types::BacklinkTarget::PdfAnnotation {
                        document_path,
                        annotation_id,
                        page,
                    } => Message::PdfAnnotationFocused {
                        document_path: document_path.clone(),
                        annotation_id: annotation_id.clone(),
                        page: *page,
                    },
                };

                let mut btn_content = column![
                    text(&item.label)
                        .size(theme::TEXT_SM)
                        .color(theme::TEXT_SECONDARY)
                ]
                .spacing(theme::SPACE_1);

                if let Some(ctx) = &item.context
                    && !ctx.trim().is_empty()
                {
                    btn_content =
                        btn_content.push(text(ctx).size(theme::TEXT_XS).color(theme::TEXT_MUTED));
                }

                let btn: iced::widget::Button<'_, Message, Theme, Renderer> = button(btn_content)
                    .on_press(msg)
                    .padding([theme::SPACE_3, theme::SPACE_4])
                    .width(Length::Fill)
                    .style(button::text);

                col.push(btn)
            });

    let content = column![
        column![header, count_text]
            .spacing(theme::SPACE_2)
            .padding([theme::SPACE_4, theme::SPACE_4]),
        scrollable(list.padding([theme::SPACE_0, theme::SPACE_4])).height(Length::Fill),
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

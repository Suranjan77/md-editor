use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length, Padding, Renderer, Theme, Color};

use crate::messages::Message;
use crate::theme;
use crate::views::icons::{self, Icon};

pub fn view<'a>(
    annotations: &'a std::collections::HashMap<u16, Vec<md_editor_core::pdf::PdfAnnotation>>,
    filter_color: Option<md_editor_core::pdf::PdfAnnotationColor>,
    focused_id: Option<&'a str>,
) -> Element<'a, Message, Theme, Renderer> {
    let title = text("Annotations")
        .size(16)
        .color(theme::TEXT_PRIMARY);

    // 1. Color filter row
    let mut colors_filter = row![
        button(text("All").size(11))
            .on_press(Message::PdfFilterAnnotationsByColor(None))
            .padding([4, 6])
            .style(if filter_color.is_none() { button::primary } else { button::secondary })
    ].spacing(4);

    let all_colors = [
        (md_editor_core::pdf::PdfAnnotationColor::Yellow, "Yellow"),
        (md_editor_core::pdf::PdfAnnotationColor::Green, "Green"),
        (md_editor_core::pdf::PdfAnnotationColor::Blue, "Blue"),
        (md_editor_core::pdf::PdfAnnotationColor::Pink, "Pink"),
        (md_editor_core::pdf::PdfAnnotationColor::Orange, "Orange"),
    ];

    for &(col_enum, name) in &all_colors {
        let is_selected = filter_color == Some(col_enum);
        colors_filter = colors_filter.push(
            button(text(name).size(11))
                .on_press(Message::PdfFilterAnnotationsByColor(Some(col_enum)))
                .padding([4, 6])
                .style(if is_selected { button::primary } else { button::secondary })
        );
    }

    let filter_container = container(colors_filter)
        .padding(Padding { top: 0.0, right: 0.0, bottom: 8.0, left: 0.0 });

    // 2. Build sorted annotation list
    let mut list = Vec::new();
    for (&_, page_anns) in annotations {
        for ann in page_anns {
            if let Some(fc) = filter_color {
                if ann.color != fc {
                    continue;
                }
            }
            list.push(ann);
        }
    }
    list.sort_by_key(|ann| (ann.page_index, ann.created_at));

    let total_count = list.len();
    let count_text = text(format!("Total: {}", total_count))
        .size(12)
        .color(theme::TEXT_MUTED);

    let items = list.into_iter().map(|ann| {
        let is_focused = Some(ann.id.as_str()) == focused_id;

        let col_text = match ann.color {
            md_editor_core::pdf::PdfAnnotationColor::Yellow => "Yellow",
            md_editor_core::pdf::PdfAnnotationColor::Green => "Green",
            md_editor_core::pdf::PdfAnnotationColor::Blue => "Blue",
            md_editor_core::pdf::PdfAnnotationColor::Pink => "Pink",
            md_editor_core::pdf::PdfAnnotationColor::Orange => "Orange",
        };

        let header = row![
            text(format!("p. {}", ann.page_index + 1))
                .size(12)
                .color(theme::TEXT_PRIMARY),
            Space::new().width(Length::Fixed(8.0)),
            text(col_text)
                .size(11)
                .color(match ann.color {
                    md_editor_core::pdf::PdfAnnotationColor::Yellow => Color::from_rgb(0.8, 0.7, 0.0),
                    md_editor_core::pdf::PdfAnnotationColor::Green => Color::from_rgb(0.1, 0.7, 0.1),
                    md_editor_core::pdf::PdfAnnotationColor::Blue => Color::from_rgb(0.1, 0.5, 0.9),
                    md_editor_core::pdf::PdfAnnotationColor::Pink => Color::from_rgb(0.9, 0.1, 0.5),
                    md_editor_core::pdf::PdfAnnotationColor::Orange => Color::from_rgb(0.9, 0.5, 0.0),
                }),
        ].align_y(Alignment::Center);

        let quote = container(
            text(format!("\"{}\"", ann.selected_text.trim()))
                .size(12)
                .color(theme::TEXT_SECONDARY)
        )
        .padding([4, 8])
        .style(|_| container::Style {
            background: Some(iced::Background::Color(theme::BG_PRIMARY)),
            border: iced::Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });

        let note = if let Some(ref note_str) = ann.note {
            if !note_str.is_empty() {
                container(
                    text(format!("Note: {}", note_str))
                        .size(11)
                        .color(theme::TEXT_MUTED)
                )
                .padding([2, 4])
            } else {
                container(Space::new())
            }
        } else {
            container(Space::new())
        };

        let actions = row![
            button(text("Go").size(11))
                .on_press(Message::PdfNavigateToAnnotation {
                    id: ann.id.clone(),
                    page: ann.page_index,
                })
                .padding([2, 6])
                .style(button::secondary),
            button(text("Edit").size(11))
                .on_press(Message::PdfEditAnnotationNote(
                    ann.id.clone(),
                    ann.page_index,
                ))
                .padding([2, 6])
                .style(button::secondary),
            button(icons::view(Icon::Trash, Color::from_rgb(0.8, 0.2, 0.2), 11.0))
                .on_press(Message::PdfDeleteHighlight(ann.id.clone()))
                .padding([2, 6])
                .style(button::text),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        let card_content = column![
            header,
            quote,
            note,
            actions,
        ].spacing(6);

        container(card_content)
            .padding(10)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(theme::BG_SECONDARY)),
                border: iced::Border {
                    color: if is_focused { theme::ACCENT } else { theme::BORDER },
                    width: if is_focused { 1.5 } else { 1.0 },
                    radius: 6.0.into(),
                },
                ..Default::default()
            })
            .width(Length::Fill)
            .into()
    });

    container(
        column![
            row![
                title,
                Space::new().width(Length::Fill),
                button(icons::view(Icon::File, theme::TEXT_MUTED, 14.0))
                    .on_press(Message::PdfExportAnnotations)
                    .padding(4)
                    .style(button::text),
                Space::new().width(Length::Fixed(8.0)),
                count_text
            ].align_y(Alignment::Center),
            Space::new().height(Length::Fixed(8.0)),
            filter_container,
            scrollable(column(items).spacing(8)).height(Length::Fill)
        ]
        .padding(12),
    )
    .width(Length::Fixed(270.0))
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

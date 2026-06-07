use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length, Renderer, Theme};

use crate::messages::{CitationItem, Message};
use crate::theme;

pub(crate) const CITATION_PALETTE_INPUT_ID: &str = "citation_palette_input";

fn focus_visible_input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = text_input::default(theme, status);
    if matches!(status, text_input::Status::Focused { .. }) {
        style.border.color = theme::accent();
        style.border.width = 2.0;
    }
    style
}

pub(crate) fn view<'a>(
    query: &str,
    items: Vec<CitationItem>,
) -> Element<'a, Message, Theme, Renderer> {
    let input = text_input("Type to search annotations and PDF content...", query)
        .id(iced::advanced::widget::Id::new(CITATION_PALETTE_INPUT_ID))
        .on_input(Message::CitationPaletteQueryChanged)
        .on_submit(Message::CitationPaletteSubmitFirst)
        .padding(12)
        .size(16)
        .style(focus_visible_input_style);

    let mut list = column![].spacing(5);

    for item in items {
        let (label, details, badge, message) = match item.clone() {
            CitationItem::Selection { text, page_index } => (
                format!("\"{}\"", text.trim()),
                format!("Current PDF selection, page {}", page_index + 1),
                "Selection",
                Message::CitationPaletteChoose(item),
            ),
            CitationItem::Annotation {
                id: _,
                text,
                page_index,
            } => (
                format!("\"{}\"", text.trim()),
                format!("Annotation, page {}", page_index + 1),
                "Annotation",
                Message::CitationPaletteChoose(item),
            ),
            CitationItem::SearchHit {
                path,
                page_index,
                snippet,
            } => (
                format!("\"{}\"", snippet.trim()),
                format!("PDF Text in {}, page {}", path, page_index + 1),
                "PDF Text",
                Message::CitationPaletteChoose(item),
            ),
        };

        list = list.push(
            button(
                row![
                    container(text(badge).size(10).color(theme::text_muted()))
                        .padding([2, 6])
                        .style(|_| container::Style {
                            background: Some(iced::Background::Color(theme::bg_tertiary())),
                            border: iced::Border {
                                color: theme::border(),
                                width: 1.0,
                                radius: 4.0.into(),
                            },
                            ..Default::default()
                        }),
                    column![
                        text(label).size(13).color(theme::text_primary()),
                        text(details).size(11).color(theme::text_muted()),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                ]
                .spacing(12)
                .align_y(Alignment::Center)
                .padding([8, 12]),
            )
            .width(Length::Fill)
            .on_press(message)
            .style(button::text),
        );
    }

    container(
        column![
            container(input).style(|_| container::Style {
                border: iced::Border {
                    color: theme::border(),
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            }),
            scrollable(list).height(Length::Fixed(320.0)),
        ]
        .spacing(0),
    )
    .width(Length::Fixed(520.0))
    .style(|_| container::Style {
        background: Some(iced::Background::Color(theme::bg_secondary())),
        border: iced::Border {
            color: theme::border(),
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_citation_palette_selection_renders_and_clicks() {
        let item = CitationItem::Selection {
            text: "test selection text".to_string(),
            page_index: 2,
        };
        let items = vec![item.clone()];
        let mut ui = iced_test::simulator(view("", items));

        // The text is shown with quotes
        ui.click("\"test selection text\"")
            .expect("Selection item should render and be clickable");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.as_slice(),
            [Message::CitationPaletteChoose(
                CitationItem::Selection { .. }
            )]
        ));
    }

    #[test]
    fn test_citation_palette_annotation_renders_and_clicks() {
        let item = CitationItem::Annotation {
            id: "ann-123".to_string(),
            text: "test annotation text".to_string(),
            page_index: 0,
        };
        let items = vec![item.clone()];
        let mut ui = iced_test::simulator(view("", items));

        ui.click("\"test annotation text\"")
            .expect("Annotation item should render and be clickable");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.as_slice(),
            [Message::CitationPaletteChoose(
                CitationItem::Annotation { .. }
            )]
        ));
    }

    #[test]
    fn test_citation_palette_search_hit_renders_and_clicks() {
        let item = CitationItem::SearchHit {
            path: "some_doc.pdf".to_string(),
            page_index: 10,
            snippet: "test search hit snippet".to_string(),
        };
        let items = vec![item.clone()];
        let mut ui = iced_test::simulator(view("", items));

        ui.click("\"test search hit snippet\"")
            .expect("SearchHit item should render and be clickable");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.as_slice(),
            [Message::CitationPaletteChoose(
                CitationItem::SearchHit { .. }
            )]
        ));
    }

    #[test]
    fn citation_palette_input_submits_first_item_message() {
        let source = include_str!("citation_palette.rs");

        assert!(
            source.contains(".on_submit(Message::CitationPaletteSubmitFirst)"),
            "citation palette input should submit the first result with Enter"
        );
    }
}

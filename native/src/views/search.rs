use iced::widget::{
    Column, button, checkbox, column, container, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::theme;
use crate::views::icons::{self, Icon};

pub const FILE_SEARCH_INPUT_ID: &str = "file_search_input";
pub const GLOBAL_SEARCH_INPUT_ID: &str = "global_search_input";

pub fn file_bar<'a>(
    query: &'a str,
    replace: &'a str,
    regex: bool,
    match_case: bool,
    current_match_count: usize,
    active_match_index: Option<usize>,
) -> Element<'a, Message, Theme, Renderer> {
    let search_input = text_input("Find in current file", query)
        .id(iced::advanced::widget::Id::new(FILE_SEARCH_INPUT_ID))
        .on_input(Message::SearchQueryChanged)
        .on_submit(Message::SearchNext)
        .padding([theme::SPACE_3, theme::SPACE_4])
        .size(theme::TEXT_BASE)
        .width(Length::FillPortion(3));

    let replace_input = text_input("Replace", replace)
        .on_input(Message::SearchReplaceChanged)
        .padding([theme::SPACE_3, theme::SPACE_4])
        .size(theme::TEXT_BASE)
        .width(Length::FillPortion(2));

    container(
        row![
            icons::view(Icon::Search, theme::ACCENT, 18.0),
            search_input,
            replace_input,
            button(text("Replace all").size(theme::TEXT_SM))
                .on_press(Message::SearchReplaceAll)
                .padding([theme::SPACE_3, theme::SPACE_4])
                .style(button::secondary),
            checkbox(regex)
                .label("Regex")
                .on_toggle(Message::SearchRegexToggled)
                .size(theme::TEXT_BASE),
            checkbox(match_case)
                .label("Case")
                .on_toggle(Message::SearchMatchCaseToggled)
                .size(theme::TEXT_BASE),
            button(icons::view(Icon::ChevronUp, theme::TEXT_MUTED, 16.0))
                .on_press(Message::SearchPrevious)
                .padding(theme::SPACE_3)
                .style(button::text),
            button(icons::view(Icon::ChevronDown, theme::TEXT_MUTED, 16.0))
                .on_press(Message::SearchNext)
                .padding(theme::SPACE_3)
                .style(button::text),
            text(match active_match_index {
                Some(index) if current_match_count > 0 =>
                    format!("{} of {}", index + 1, current_match_count),
                _ => format!("{} matches", current_match_count),
            })
            .size(theme::TEXT_SM)
            .color(theme::TEXT_MUTED),
            button(icons::view(Icon::X, theme::TEXT_MUTED, 16.0))
                .on_press(Message::SearchClose)
                .padding(theme::SPACE_3)
                .style(button::text),
        ]
        .spacing(theme::SPACE_4)
        .align_y(Alignment::Center)
        .padding([theme::SPACE_3, theme::SPACE_4]),
    )
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(iced::Background::Color(theme::BG_SECONDARY)),
        border: iced::Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

/// Render the vault search overlay.
pub fn view<'a>(
    query: &'a str,
    replace: &'a str,
    regex: bool,
    match_case: bool,
    current_match_count: usize,
    results: &'a [md_editor_core::types::SearchResult],
    pdf_results: &'a [md_editor_core::pdf::PdfSearchMatch],
    pdf_error: Option<&'a str>,
    visible: bool,
) -> Element<'a, Message, Theme, Renderer> {
    if !visible {
        return container(text(""))
            .width(Length::Fixed(0.0))
            .height(Length::Fixed(0.0))
            .into();
    }

    let search_input = text_input("Search document, vault, or PDF...", query)
        .id(iced::advanced::widget::Id::new(GLOBAL_SEARCH_INPUT_ID))
        .on_input(Message::SearchQueryChanged)
        .padding([theme::SPACE_4, theme::SPACE_4])
        .size(theme::TEXT_MD)
        .width(Length::Fill);

    let replace_input = text_input("Replace in current markdown document...", replace)
        .on_input(Message::SearchReplaceChanged)
        .padding([theme::SPACE_3, theme::SPACE_4])
        .size(theme::TEXT_BASE)
        .width(Length::Fill);

    let close_btn = button(icons::view(Icon::X, theme::TEXT_MUTED, 16.0))
        .on_press(Message::SearchClose)
        .padding(theme::SPACE_3)
        .style(button::text);

    let header = column![
        row![
            icons::view(Icon::Search, theme::ACCENT, 18.0),
            text("Global search")
                .size(theme::TEXT_MD)
                .color(theme::ACCENT),
            search_input,
            close_btn,
        ]
        .spacing(theme::SPACE_4)
        .align_y(Alignment::Center),
        row![
            replace_input,
            button(text("Replace all").size(theme::TEXT_SM))
                .on_press(Message::SearchReplaceAll)
                .padding([theme::SPACE_3, theme::SPACE_4])
                .style(button::secondary),
        ]
        .spacing(theme::SPACE_4)
        .align_y(Alignment::Center),
        row![
            checkbox(regex)
                .label("Regex")
                .on_toggle(Message::SearchRegexToggled)
                .size(theme::TEXT_BASE),
            checkbox(match_case)
                .label("Match case")
                .on_toggle(Message::SearchMatchCaseToggled)
                .size(theme::TEXT_BASE),
            text(format!(
                "{} matches in current document",
                current_match_count
            ))
            .size(theme::TEXT_SM)
            .color(theme::TEXT_MUTED),
        ]
        .spacing(theme::SPACE_5)
        .align_y(Alignment::Center),
    ]
    .spacing(theme::SPACE_4)
    .padding(theme::SPACE_5);

    let vault_results: Column<'_, Message, Theme, Renderer> =
        results
            .iter()
            .fold(Column::new().spacing(theme::SPACE_1), |col, result| {
                let path_text = text(&result.path)
                    .size(theme::TEXT_BASE)
                    .color(theme::ACCENT);
                let context_text = text(&result.context)
                    .size(theme::TEXT_SM)
                    .color(theme::TEXT_SECONDARY);

                let item: iced::widget::Button<'_, Message, Theme, Renderer> =
                    button(column![path_text, context_text].spacing(theme::SPACE_1))
                        .on_press(Message::SearchResultClicked(result.path.clone()))
                        .padding([theme::SPACE_3, theme::SPACE_4])
                        .width(Length::Fill)
                        .style(button::text);

                col.push(item)
            });

    let pdf_result_list: Column<'_, Message, Theme, Renderer> =
        pdf_results
            .iter()
            .fold(Column::new().spacing(theme::SPACE_1), |col, result| {
                let item: iced::widget::Button<'_, Message, Theme, Renderer> = button(
                    column![
                        text(format!("PDF page {}", result.page_index + 1))
                            .size(theme::TEXT_BASE)
                            .color(theme::ACCENT),
                        text(&result.context)
                            .size(theme::TEXT_SM)
                            .color(theme::TEXT_SECONDARY),
                    ]
                    .spacing(theme::SPACE_1),
                )
                .on_press(Message::PdfSearchResultClicked(result.page_index))
                .padding([theme::SPACE_3, theme::SPACE_4])
                .width(Length::Fill)
                .style(button::text);

                col.push(item)
            });

    let empty_state = if results.is_empty() && pdf_results.is_empty() && !query.is_empty() {
        Some(
            text("No results found")
                .size(theme::TEXT_SM)
                .color(theme::TEXT_MUTED),
        )
    } else {
        None
    };

    let mut content = column![
        header,
        scrollable(
            column![
                text("PDF results")
                    .size(theme::TEXT_SM)
                    .color(theme::TEXT_MUTED),
                pdf_result_list,
                text("Vault results")
                    .size(theme::TEXT_SM)
                    .color(theme::TEXT_MUTED),
                vault_results,
            ]
            .spacing(theme::SPACE_3)
            .padding([theme::SPACE_0, theme::SPACE_5])
        )
        .height(Length::Fill),
    ];

    if let Some(err) = pdf_error {
        content = content.push(
            container(text(err).size(theme::TEXT_SM).color(theme::TEXT_MUTED))
                .padding([theme::SPACE_0, theme::SPACE_5])
                .width(Length::Fill),
        );
    }

    if let Some(empty) = empty_state {
        content = content.push(
            container(empty)
                .padding([theme::SPACE_5, theme::SPACE_5])
                .width(Length::Fill),
        );
    }

    container(content)
        .width(Length::Fixed(620.0))
        .max_height(620.0)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(theme::BG_SECONDARY)),
            border: iced::Border {
                color: theme::BORDER,
                width: 1.0,
                radius: theme::RADIUS_MD.into(),
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

use iced::widget::{
    Column, Space, button, checkbox, column, container, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::theme;
use crate::views::icons::{self, Icon};

pub const FILE_SEARCH_INPUT_ID: &str = "file_search_input";
pub const GLOBAL_SEARCH_INPUT_ID: &str = "global_search_input";

const BOLD_FONT: iced::Font = iced::Font {
    weight: iced::font::Weight::Bold,
    ..iced::Font::DEFAULT
};

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
        .padding([8, 12])
        .size(14)
        .width(Length::FillPortion(3));

    let replace_input = text_input("Replace", replace)
        .on_input(Message::SearchReplaceChanged)
        .padding([8, 12])
        .size(14)
        .width(Length::FillPortion(2));

    container(
        row![
            icons::view(Icon::Search, theme::ACCENT, 18.0),
            search_input,
            replace_input,
            button(text("Replace all").size(12))
                .on_press(Message::SearchReplaceAll)
                .padding([8, 12])
                .style(button::secondary),
            checkbox(regex)
                .label("Regex")
                .on_toggle(Message::SearchRegexToggled)
                .size(14),
            checkbox(match_case)
                .label("Case")
                .on_toggle(Message::SearchMatchCaseToggled)
                .size(14),
            button(icons::view(Icon::ChevronUp, theme::TEXT_MUTED, 16.0))
                .on_press(Message::SearchPrevious)
                .padding(8)
                .style(button::text),
            button(icons::view(Icon::ChevronDown, theme::TEXT_MUTED, 16.0))
                .on_press(Message::SearchNext)
                .padding(8)
                .style(button::text),
            text(match active_match_index {
                Some(index) if current_match_count > 0 =>
                    format!("{} of {}", index + 1, current_match_count),
                _ => format!("{} matches", current_match_count),
            })
            .size(12)
            .color(theme::TEXT_MUTED),
            button(icons::view(Icon::X, theme::TEXT_MUTED, 16.0))
                .on_press(Message::SearchClose)
                .padding(8)
                .style(button::text),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .padding([8, 14]),
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

/// Render the vault search overlay with typed result groups.
pub fn view<'a>(
    query: &'a str,
    replace: &'a str,
    regex: bool,
    match_case: bool,
    current_match_count: usize,
    global_results: &'a [md_editor_core::types::UnifiedSearchResult],
    searching: bool,
    error: Option<&'a str>,
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
        .padding([10, 14])
        .size(15)
        .width(Length::Fill);

    let replace_input = text_input("Replace in current markdown document...", replace)
        .on_input(Message::SearchReplaceChanged)
        .padding([8, 12])
        .size(13)
        .width(Length::Fill);

    let close_btn = button(icons::view(Icon::X, theme::TEXT_MUTED, 16.0))
        .on_press(Message::SearchClose)
        .padding(8)
        .style(button::text);

    let header = column![
        row![
            icons::view(Icon::Search, theme::ACCENT, 18.0),
            text("Global search").size(15).font(BOLD_FONT).color(theme::ACCENT),
            search_input,
            close_btn,
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        row![
            replace_input,
            button(text("Replace all").size(12))
                .on_press(Message::SearchReplaceAll)
                .padding([8, 12])
                .style(button::secondary),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        row![
            checkbox(regex)
                .label("Regex")
                .on_toggle(Message::SearchRegexToggled)
                .size(14),
            checkbox(match_case)
                .label("Match case")
                .on_toggle(Message::SearchMatchCaseToggled)
                .size(14),
            text(format!(
                "{} matches in current document",
                current_match_count
            ))
            .size(11)
            .color(theme::TEXT_MUTED),
            if searching {
                text("Searching...").size(11).color(theme::ACCENT)
            } else {
                text("").size(11)
            }
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    ]
    .spacing(10)
    .padding(16);

    let md_content_results: Vec<_> = global_results
        .iter()
        .filter(|r| r.group == md_editor_core::types::SearchResultGroup::MarkdownContent)
        .collect();
    let pdf_content_results: Vec<_> = global_results
        .iter()
        .filter(|r| r.group == md_editor_core::types::SearchResultGroup::PdfContent)
        .collect();
    let filename_results: Vec<_> = global_results
        .iter()
        .filter(|r| r.group == md_editor_core::types::SearchResultGroup::Filename)
        .collect();
    let heading_results: Vec<_> = global_results
        .iter()
        .filter(|r| r.group == md_editor_core::types::SearchResultGroup::Heading)
        .collect();
    let annotation_results: Vec<_> = global_results
        .iter()
        .filter(|r| r.group == md_editor_core::types::SearchResultGroup::Annotation)
        .collect();

    let result_scroll = scrollable(
        column![
            render_group_section("Filenames", &filename_results),
            render_group_section("Headings", &heading_results),
            render_group_section("Markdown Content", &md_content_results),
            render_group_section("PDF Content", &pdf_content_results),
            render_group_section("Annotations & Notes", &annotation_results),
        ]
        .spacing(8)
        .padding([0, 16])
    )
    .height(Length::Fill);

    let empty_state = if global_results.is_empty() && !query.is_empty() && !searching {
        Some(text("No results found").size(12).color(theme::TEXT_MUTED))
    } else {
        None
    };

    let mut content = column![header, result_scroll];

    if let Some(err) = error {
        content = content.push(
            container(text(err).size(11).color(theme::TEXT_MUTED))
                .padding([0, 16])
                .width(Length::Fill),
        );
    }

    if let Some(empty) = empty_state {
        content = content.push(container(empty).padding([16, 16]).width(Length::Fill));
    }

    container(content)
        .width(Length::Fixed(620.0))
        .max_height(620.0)
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

fn render_group_section<'a>(
    title: &str,
    items: &[&'a md_editor_core::types::UnifiedSearchResult],
) -> Element<'a, Message, Theme, Renderer> {
    if items.is_empty() {
        return Column::new().into();
    }

    let group_header = text(format!("{} ({} matches)", title, items.len()))
        .size(12)
        .font(BOLD_FONT)
        .color(theme::TEXT_PRIMARY);

    let list = items.iter().fold(Column::new().spacing(4), |col, result| {
        let path_text = text(&result.path).size(13).color(theme::ACCENT);

        let label = match result.group {
            md_editor_core::types::SearchResultGroup::Heading => format!("Heading (Line {})", result.line),
            md_editor_core::types::SearchResultGroup::MarkdownContent => format!("Line {}", result.line),
            md_editor_core::types::SearchResultGroup::Filename => "Filename".to_string(),
            md_editor_core::types::SearchResultGroup::PdfContent => format!("PDF Page {}", result.line),
            md_editor_core::types::SearchResultGroup::Annotation => format!("PDF Page {} Annotation", result.line),
        };
        let label_text = text(label).size(11).color(theme::TEXT_MUTED);

        let context_text = text(&result.context).size(12).color(theme::TEXT_SECONDARY);

        let item = button(
            column![
                row![path_text, label_text].spacing(8).align_y(Alignment::Center),
                context_text
            ]
            .spacing(2),
        )
        .on_press(Message::UnifiedSearchResultClicked((*result).clone()))
        .padding([8, 12])
        .width(Length::Fill)
        .style(button::text);

        col.push(item)
    });

    column![
        group_header,
        Space::new().height(Length::Fixed(4.0)),
        list,
        Space::new().height(Length::Fixed(12.0)),
    ]
    .into()
}

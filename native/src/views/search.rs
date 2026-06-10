use iced::widget::{
    Column, Space, button, checkbox, column, container, rich_text, row, scrollable, span, text,
    text_input,
};
use iced::{Alignment, Element, Length, Renderer, Theme};

use crate::messages::{Message, SearchMessage, SearchWrapStatus};
use crate::theme;
use crate::views::icons::{self, Icon};

pub(crate) const FILE_SEARCH_INPUT_ID: &str = "file_search_input";
pub(crate) const GLOBAL_SEARCH_INPUT_ID: &str = "global_search_input";

const BOLD_FONT: iced::Font = iced::Font {
    weight: iced::font::Weight::Bold,
    ..iced::Font::DEFAULT
};
const MAX_HIGHLIGHT_MATCHES: usize = 32;

#[derive(Debug, PartialEq, Eq)]
struct MatchSegment {
    text: String,
    matched: bool,
}

fn focus_visible_input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = text_input::default(theme, status);
    if matches!(status, text_input::Status::Focused { .. }) {
        style.border.color = theme::accent();
        style.border.width = 2.0;
    }
    style
}

pub(crate) fn file_bar<'a>(
    query: &'a str,
    replace: &'a str,
    regex: bool,
    match_case: bool,
    current_match_count: usize,
    active_match_index: Option<usize>,
    wrap_status: Option<SearchWrapStatus>,
) -> Element<'a, Message, Theme, Renderer> {
    let search_input = text_input("Find in current file", query)
        .id(iced::advanced::widget::Id::new(FILE_SEARCH_INPUT_ID))
        .on_input(|query| Message::Search(SearchMessage::QueryChanged(query)))
        .on_submit(Message::Search(SearchMessage::Next))
        .padding([8, 12])
        .size(14)
        .width(Length::FillPortion(3))
        .style(focus_visible_input_style);

    let replace_input = text_input("Replace", replace)
        .on_input(|replace| Message::Search(SearchMessage::ReplaceChanged(replace)))
        .padding([8, 12])
        .size(14)
        .width(Length::FillPortion(2))
        .style(focus_visible_input_style);

    let count_color = if !query.is_empty() && current_match_count == 0 {
        theme::danger()
    } else {
        theme::text_muted()
    };

    let count_str = if !query.is_empty() && current_match_count == 0 {
        "No matches".to_string()
    } else {
        match active_match_index {
            Some(index) if current_match_count > 0 => {
                if let Some(wrap) = wrap_status {
                    match wrap {
                        SearchWrapStatus::WrappedForward => {
                            format!(
                                "{} of {} (wrapped search, first match)",
                                index + 1,
                                current_match_count
                            )
                        }
                        SearchWrapStatus::WrappedBackward => {
                            format!(
                                "{} of {} (wrapped search, last match)",
                                index + 1,
                                current_match_count
                            )
                        }
                    }
                } else {
                    format!("{} of {}", index + 1, current_match_count)
                }
            }
            _ => format!("{} matches", current_match_count),
        }
    };

    container(
        row![
            icons::view(Icon::Search, theme::accent(), 18.0),
            search_input,
            replace_input,
            button(text("Replace").size(12))
                .on_press(Message::Search(SearchMessage::Replace))
                .padding([8, 12])
                .style(button::secondary),
            button(text("Replace all").size(12))
                .on_press(Message::Search(SearchMessage::ReplaceAll))
                .padding([8, 12])
                .style(button::secondary),
            checkbox(regex)
                .label("Regex")
                .on_toggle(|value| Message::Search(SearchMessage::RegexToggled(value)))
                .size(14),
            checkbox(match_case)
                .label("Case")
                .on_toggle(|value| Message::Search(SearchMessage::MatchCaseToggled(value)))
                .size(14),
            button(icons::view(Icon::ChevronUp, theme::text_muted(), 16.0))
                .on_press(Message::Search(SearchMessage::Previous))
                .padding(8)
                .style(button::text),
            button(icons::view(Icon::ChevronDown, theme::text_muted(), 16.0))
                .on_press(Message::Search(SearchMessage::Next))
                .padding(8)
                .style(button::text),
            text(count_str).size(12).color(count_color),
            button(icons::view(Icon::X, theme::text_muted(), 16.0))
                .on_press(Message::Search(SearchMessage::Close))
                .padding(8)
                .style(button::text),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .padding([8, 14]),
    )
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(iced::Background::Color(theme::bg_secondary())),
        border: iced::Border {
            color: theme::border(),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

/// Render the vault search overlay with typed result groups.
pub(crate) fn view<'a>(
    query: &'a str,
    replace: &'a str,
    regex: bool,
    match_case: bool,
    current_match_count: usize,
    global_results: &'a [md_editor_core::domain::UnifiedSearchResult],
    searching: bool,
    error: Option<&'a str>,
    visible: bool,
    enabled_sources: &'a [md_editor_core::domain::UnifiedSearchSource],
    pdf_status: Option<&'a str>,
) -> Element<'a, Message, Theme, Renderer> {
    if !visible {
        return container(text(""))
            .width(Length::Fixed(0.0))
            .height(Length::Fixed(0.0))
            .into();
    }

    let search_input = text_input("Search document, vault, or PDF...", query)
        .id(iced::advanced::widget::Id::new(GLOBAL_SEARCH_INPUT_ID))
        .on_input(|query| Message::Search(SearchMessage::QueryChanged(query)))
        .padding([10, 14])
        .size(15)
        .width(Length::Fill)
        .style(focus_visible_input_style);

    let replace_input = text_input("Replace in current markdown document...", replace)
        .on_input(|replace| Message::Search(SearchMessage::ReplaceChanged(replace)))
        .padding([8, 12])
        .size(13)
        .width(Length::Fill)
        .style(focus_visible_input_style);

    let close_btn = button(icons::view(Icon::X, theme::text_muted(), 16.0))
        .on_press(Message::Search(SearchMessage::Close))
        .padding(8)
        .style(button::text);

    let header = column![
        row![
            icons::view(Icon::Search, theme::accent(), 18.0),
            text("Global search")
                .size(15)
                .font(BOLD_FONT)
                .color(theme::accent()),
            search_input,
            close_btn,
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        row![
            replace_input,
            button(text("Replace all").size(12))
                .on_press(Message::Search(SearchMessage::ReplaceAll))
                .padding([8, 12])
                .style(button::secondary),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        row![
            checkbox(regex)
                .label("Regex")
                .on_toggle(|value| Message::Search(SearchMessage::RegexToggled(value)))
                .size(14),
            checkbox(match_case)
                .label("Match case")
                .on_toggle(|value| Message::Search(SearchMessage::MatchCaseToggled(value)))
                .size(14),
            text(format!(
                "{} matches in current document",
                current_match_count
            ))
            .size(11)
            .color(theme::text_muted()),
            if searching {
                text("Searching...").size(11).color(theme::accent())
            } else {
                text("").size(11)
            },
            if let Some(status) = pdf_status {
                text(status).size(11).color(theme::text_muted())
            } else {
                text("").size(11)
            },
        ]
        .spacing(16)
        .align_y(Alignment::Center),
        row![
            source_checkbox(
                enabled_sources,
                md_editor_core::domain::UnifiedSearchSource::Filename,
                "Files"
            ),
            source_checkbox(
                enabled_sources,
                md_editor_core::domain::UnifiedSearchSource::Heading,
                "Headings"
            ),
            source_checkbox(
                enabled_sources,
                md_editor_core::domain::UnifiedSearchSource::MarkdownContent,
                "Markdown"
            ),
            source_checkbox(
                enabled_sources,
                md_editor_core::domain::UnifiedSearchSource::PdfContent,
                "PDF text"
            ),
            source_checkbox(
                enabled_sources,
                md_editor_core::domain::UnifiedSearchSource::Annotation,
                "Annotations"
            ),
            source_checkbox(
                enabled_sources,
                md_editor_core::domain::UnifiedSearchSource::QuickNote,
                "Notes"
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    ]
    .spacing(10)
    .padding(16);

    let md_content_results: Vec<_> = global_results
        .iter()
        .filter(|r| r.group == md_editor_core::domain::SearchResultGroup::MarkdownContent)
        .collect();
    let pdf_content_results: Vec<_> = global_results
        .iter()
        .filter(|r| r.group == md_editor_core::domain::SearchResultGroup::PdfContent)
        .collect();
    let filename_results: Vec<_> = global_results
        .iter()
        .filter(|r| r.group == md_editor_core::domain::SearchResultGroup::Filename)
        .collect();
    let heading_results: Vec<_> = global_results
        .iter()
        .filter(|r| r.group == md_editor_core::domain::SearchResultGroup::Heading)
        .collect();
    let annotation_results: Vec<_> = global_results
        .iter()
        .filter(|r| r.group == md_editor_core::domain::SearchResultGroup::Annotation)
        .collect();
    let quick_note_results: Vec<_> = global_results
        .iter()
        .filter(|r| r.group == md_editor_core::domain::SearchResultGroup::QuickNote)
        .collect();

    let result_scroll = scrollable(
        column![
            render_group_section("Filenames", &filename_results, query, regex, match_case),
            render_group_section("Headings", &heading_results, query, regex, match_case),
            render_group_section(
                "Markdown Content",
                &md_content_results,
                query,
                regex,
                match_case
            ),
            render_group_section(
                "PDF Content",
                &pdf_content_results,
                query,
                regex,
                match_case
            ),
            render_group_section(
                "Annotations & Notes",
                &annotation_results,
                query,
                regex,
                match_case
            ),
            render_group_section("Quick Notes", &quick_note_results, query, regex, match_case),
        ]
        .spacing(8)
        .padding([0, 16]),
    )
    .height(Length::Fill);

    let empty_state = if global_results.is_empty() && !query.is_empty() && !searching {
        Some(text("No results found").size(12).color(theme::text_muted()))
    } else {
        None
    };

    let mut content = column![header, result_scroll];

    if let Some(err) = error {
        content = content.push(
            container(text(err).size(11).color(theme::text_muted()))
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
            background: Some(iced::Background::Color(theme::bg_secondary())),
            border: iced::Border {
                color: theme::border(),
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

fn source_checkbox<'a>(
    enabled_sources: &'a [md_editor_core::domain::UnifiedSearchSource],
    source: md_editor_core::domain::UnifiedSearchSource,
    label: &'a str,
) -> Element<'a, Message, Theme, Renderer> {
    checkbox(enabled_sources.contains(&source))
        .label(label)
        .on_toggle(move |enabled| Message::Search(SearchMessage::SourceToggled(source, enabled)))
        .size(13)
        .into()
}

fn result_row_style() -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let mut style = button::text(theme, status);
        style.border.radius = theme::RADIUS_SMALL.into();

        if status == button::Status::Hovered || status == button::Status::Pressed {
            style.background = Some(iced::Background::Color(theme::bg_tertiary()));
        }

        style
    }
}

fn render_group_section<'a>(
    title: &str,
    items: &[&'a md_editor_core::domain::UnifiedSearchResult],
    query: &str,
    regex: bool,
    match_case: bool,
) -> Element<'a, Message, Theme, Renderer> {
    if items.is_empty() {
        return Column::new().into();
    }

    let group_header = text(format!("{} ({} matches)", title, items.len()))
        .size(11)
        .font(BOLD_FONT)
        .color(theme::text_muted());

    let list = items.iter().fold(Column::new().spacing(4), |col, result| {
        let path_text =
            highlighted_text(&result.path, query, regex, match_case, theme::accent(), 13);

        let label = match result.group {
            md_editor_core::domain::SearchResultGroup::Heading => {
                format!("Heading (Line {})", result.line)
            }
            md_editor_core::domain::SearchResultGroup::MarkdownContent => {
                format!("Line {}", result.line)
            }
            md_editor_core::domain::SearchResultGroup::Filename => "Filename".to_string(),
            md_editor_core::domain::SearchResultGroup::PdfContent => {
                format!("PDF Page {}", result.line)
            }
            md_editor_core::domain::SearchResultGroup::Annotation => {
                format!("PDF Page {} Annotation", result.line)
            }
            md_editor_core::domain::SearchResultGroup::QuickNote => {
                format!("PDF Page {} Note", result.line)
            }
        };
        let label_text = text(label).size(11).color(theme::text_muted());

        let context_text = highlighted_text(
            &result.context,
            query,
            regex,
            match_case,
            theme::text_secondary(),
            12,
        );

        let item = button(
            column![
                row![path_text, label_text]
                    .spacing(8)
                    .align_y(Alignment::Center),
                context_text
            ]
            .spacing(2),
        )
        .on_press(Message::Search(SearchMessage::UnifiedResultClicked(
            (*result).clone(),
        )))
        .padding([8, 12])
        .width(Length::Fill)
        .style(result_row_style());

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

fn match_segments(value: &str, query: &str, regex: bool, match_case: bool) -> Vec<MatchSegment> {
    let query = query.trim();
    if query.is_empty() {
        return vec![MatchSegment {
            text: value.to_string(),
            matched: false,
        }];
    }

    let pattern = if regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    let Ok(matcher) = regex::RegexBuilder::new(&pattern)
        .case_insensitive(!match_case)
        .build()
    else {
        return vec![MatchSegment {
            text: value.to_string(),
            matched: false,
        }];
    };

    let matches: Vec<_> = matcher
        .find_iter(value)
        .filter(|found| found.start() < found.end())
        .take(MAX_HIGHLIGHT_MATCHES)
        .map(|found| (found.start(), found.end()))
        .collect();
    if matches.is_empty() {
        return vec![MatchSegment {
            text: value.to_string(),
            matched: false,
        }];
    }

    let mut segments = Vec::with_capacity(matches.len() * 2 + 1);
    let mut cursor = 0;
    for (start, end) in matches {
        if cursor < start {
            segments.push(MatchSegment {
                text: value[cursor..start].to_string(),
                matched: false,
            });
        }
        segments.push(MatchSegment {
            text: value[start..end].to_string(),
            matched: true,
        });
        cursor = end;
    }
    if cursor < value.len() {
        segments.push(MatchSegment {
            text: value[cursor..].to_string(),
            matched: false,
        });
    }
    segments
}

fn highlighted_text(
    value: &str,
    query: &str,
    regex: bool,
    match_case: bool,
    base_color: iced::Color,
    size: u32,
) -> Element<'static, Message, Theme, Renderer> {
    let spans = match_segments(value, query, regex, match_case)
        .into_iter()
        .map(|segment| {
            let text_span = span(segment.text);
            if segment.matched {
                text_span
                    .color(theme::accent())
                    .background(theme::accent_dim())
                    .font(BOLD_FONT)
            } else {
                text_span.color(base_color)
            }
        })
        .collect::<Vec<_>>();

    rich_text::<(), Message, Theme, Renderer>(spans)
        .size(size)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use md_editor_core::domain::UnifiedSearchSource;

    const SOURCES: &[UnifiedSearchSource] = &[
        UnifiedSearchSource::Filename,
        UnifiedSearchSource::Heading,
        UnifiedSearchSource::MarkdownContent,
        UnifiedSearchSource::PdfContent,
        UnifiedSearchSource::Annotation,
        UnifiedSearchSource::QuickNote,
    ];

    #[test]
    fn visible_global_search_renders_empty_state_when_query_has_no_results() {
        let mut ui = iced_test::simulator(view(
            "missing",
            "",
            false,
            false,
            0,
            &[],
            false,
            None,
            true,
            SOURCES,
            None,
        ));

        ui.find("No results found")
            .expect("visible global search should explain empty query results");
    }

    #[test]
    fn visible_global_search_renders_error_and_pdf_status() {
        let mut ui = iced_test::simulator(view(
            "query",
            "",
            false,
            false,
            0,
            &[],
            true,
            Some("Index unavailable"),
            true,
            SOURCES,
            Some("Searched 3 PDFs"),
        ));

        ui.find("Index unavailable")
            .expect("global search error should render");
        ui.find("Searched 3 PDFs")
            .expect("global search PDF status should render");
    }

    #[test]
    fn global_search_input_focus_uses_visible_accent_ring() {
        let theme = Theme::Dark;
        let active = focus_visible_input_style(&theme, text_input::Status::Active);
        let focused =
            focus_visible_input_style(&theme, text_input::Status::Focused { is_hovered: false });

        assert_eq!(focused.border.color, theme::accent());
        assert_eq!(focused.border.width, 2.0);
        assert_ne!(focused.border, active.border);
    }

    #[test]
    fn file_bar_renders_match_count() {
        let mut ui =
            iced_test::simulator(file_bar("test query", "", false, false, 42, Some(3), None));

        ui.find("4 of 42")
            .expect("file bar should render active match out of total matches");
    }

    #[test]
    fn match_segments_highlight_all_case_insensitive_literal_matches() {
        let segments = match_segments("Rust rust RUST", "rust", false, false);

        assert_eq!(
            segments,
            vec![
                MatchSegment {
                    text: "Rust".to_string(),
                    matched: true,
                },
                MatchSegment {
                    text: " ".to_string(),
                    matched: false,
                },
                MatchSegment {
                    text: "rust".to_string(),
                    matched: true,
                },
                MatchSegment {
                    text: " ".to_string(),
                    matched: false,
                },
                MatchSegment {
                    text: "RUST".to_string(),
                    matched: true,
                },
            ]
        );
    }

    #[test]
    fn match_segments_support_regex_and_preserve_original_text() {
        let segments = match_segments("Pages 12 and 345", r"\d+", true, true);

        assert_eq!(
            segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<String>(),
            "Pages 12 and 345"
        );
        assert_eq!(
            segments
                .iter()
                .filter(|segment| segment.matched)
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>(),
            vec!["12", "345"]
        );
    }

    #[test]
    fn match_segments_bound_highlight_work() {
        let value = "x ".repeat(MAX_HIGHLIGHT_MATCHES + 10);
        let segments = match_segments(&value, "x", false, true);

        assert_eq!(
            segments.iter().filter(|segment| segment.matched).count(),
            MAX_HIGHLIGHT_MATCHES
        );
        assert_eq!(
            segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<String>(),
            value
        );
    }

    #[test]
    fn global_search_renders_result_row_with_highlighted_content() {
        let results = [md_editor_core::domain::UnifiedSearchResult {
            group: md_editor_core::domain::SearchResultGroup::MarkdownContent,
            path: "notes/Rust Guide.md".to_string(),
            line: 7,
            context: "Learn rust ownership".to_string(),
            score: 1.0,
            page_index: None,
            annotation_id: None,
        }];
        let mut ui = iced_test::simulator(view(
            "rust", "", false, false, 0, &results, false, None, true, SOURCES, None,
        ));

        ui.find("Line 7")
            .expect("result row should render alongside highlighted rich text");
    }
}

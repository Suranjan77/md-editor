use iced::widget::{Space, button, checkbox, column, container, row, text, text_input};
use iced::{Alignment, Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::theme;
use crate::views::icons::{self, Icon};
use crate::views::interactive_pdf::InteractivePdf;

pub fn search_bar<'a>(
    query: &'a str,
    regex: bool,
    match_case: bool,
    current_match_count: usize,
    active_match_index: Option<usize>,
) -> Element<'a, Message, Theme, Renderer> {
    let search_input = text_input("Find in PDF", query)
        .on_input(Message::SearchQueryChanged)
        .padding([8, 12])
        .size(14)
        .width(Length::Fill);

    container(
        row![
            icons::view(Icon::Search, theme::ACCENT, 18.0),
            search_input,
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

pub fn toolbar<'a>(
    current_page: u16,
    total_pages: u16,
    zoom: f32,
    toc_visible: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let page_label = if total_pages == 0 {
        "No PDF".to_string()
    } else {
        format!("{} / {}", current_page + 1, total_pages)
    };

    container(
        row![
            button(text("☰").size(14).color(if toc_visible {
                theme::ACCENT
            } else {
                theme::TEXT_MUTED
            }))
            .on_press(Message::ToggleTOC)
            .padding(8)
            .style(button::text),
            Space::new().width(Length::Fill),
            button(text("-").size(16))
                .on_press(Message::PdfZoomChanged((zoom - 0.1).max(0.5)))
                .padding([4, 10])
                .style(button::text),
            text(format!("{:.0}%", zoom * 100.0))
                .size(12)
                .color(theme::TEXT_MUTED),
            button(text("+").size(16))
                .on_press(Message::PdfZoomChanged((zoom + 0.1).min(4.0)))
                .padding([4, 10])
                .style(button::text),
            button(text("Fit").size(12))
                .on_press(Message::PdfFitToWidth)
                .padding([4, 10])
                .style(button::text),
            Space::new().width(Length::Fill),
            text(page_label).size(12).color(theme::TEXT_SECONDARY),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .padding([6, 12]),
    )
    .width(Length::Fill)
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

pub fn view_continuous<'a>(
    pages: &'a [Option<iced::widget::image::Handle>],
    zoom: f32,
    dimensions: &'a [Option<(u32, u32)>],
    search_matches: &'a [md_editor_core::pdf::PdfSearchMatch],
    active_search_index: Option<usize>,
) -> Element<'a, Message, Theme, Renderer> {
    if pages.is_empty() {
        return container(text("Loading PDF...").color(theme::TEXT_MUTED).size(14))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(theme::BG_PRIMARY)),
                ..Default::default()
            })
            .into();
    }

    let mut page_list = column![]
        .spacing(20)
        .padding(20)
        .align_x(Alignment::Center)
        .width(Length::Fill)
        .height(Length::Shrink);

    for (i, page_opt) in pages.iter().enumerate() {
        if let Some(handle) = page_opt {
            let (w, h) = dimensions[i].unwrap_or((800, 1100));
            let highlights = search_matches
                .iter()
                .enumerate()
                .filter(|(idx, result)| {
                    result.page_index == i as u16 && Some(*idx) != active_search_index
                })
                .flat_map(|(_, result)| result.rects.iter())
                .map(|rect| md_editor_core::pdf::PdfRect {
                    x: rect.x * zoom,
                    y: rect.y * zoom,
                    width: rect.width * zoom,
                    height: rect.height * zoom,
                })
                .collect::<Vec<_>>();
            let active_highlights = active_search_index
                .and_then(|idx| search_matches.get(idx))
                .filter(|result| result.page_index == i as u16)
                .map(|result| {
                    result
                        .rects
                        .iter()
                        .map(|rect| md_editor_core::pdf::PdfRect {
                            x: rect.x * zoom,
                            y: rect.y * zoom,
                            width: rect.width * zoom,
                            height: rect.height * zoom,
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            page_list = page_list.push(
                container(
                    InteractivePdf::new(
                        handle.clone(),
                        w as f32,
                        h as f32,
                        move |x, y| Message::PdfLeftClicked(i as u16, x, y),
                        move |x, y| Message::PdfRightClicked(i as u16, x, y),
                    )
                    .highlights(highlights, active_highlights),
                )
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(iced::Color::WHITE)),
                    shadow: iced::Shadow {
                        color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.3),
                        offset: iced::Vector::new(0.0, 4.0),
                        blur_radius: 10.0,
                    },
                    ..Default::default()
                }),
            );
        } else {
            page_list = page_list.push(
                container(text(format!("Loading Page {}...", i + 1)).color(theme::TEXT_MUTED))
                    .width(Length::Fixed(612.0 * zoom))
                    .height(Length::Fixed(792.0 * zoom))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            );
        }
    }

    container(page_list)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(theme::BG_PRIMARY)),
            ..Default::default()
        })
        .into()
}

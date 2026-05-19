use iced::widget::{Space, button, checkbox, column, container, row, text, text_input};
use iced::{Alignment, Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::theme;
use crate::views::icons::{self, Icon};
use crate::views::interactive_pdf::InteractivePdf;

pub(crate) const PDF_PAGE_LIST_PADDING: f32 = 20.0;
pub(crate) const PDF_PAGE_SPACING: f32 = 20.0;
pub const PDF_SEARCH_INPUT_ID: &str = "pdf_search_input";

fn search_rect_to_view_rect(
    rect: &md_editor_core::pdf::PdfRect,
    page_height: f32,
    zoom: f32,
) -> md_editor_core::pdf::PdfRect {
    md_editor_core::pdf::PdfRect {
        x: rect.x * zoom,
        y: (page_height - rect.y - rect.height) * zoom,
        width: rect.width * zoom,
        height: rect.height * zoom,
    }
}

pub fn search_bar<'a>(
    query: &'a str,
    regex: bool,
    match_case: bool,
    current_match_count: usize,
    active_match_index: Option<usize>,
) -> Element<'a, Message, Theme, Renderer> {
    let search_input = text_input("Find in PDF", query)
        .id(iced::advanced::widget::Id::new(PDF_SEARCH_INPUT_ID))
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
    page_sizes: &'a [Option<(f32, f32)>],
    placeholder_page_size: Option<(f32, f32)>,
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
        .spacing(PDF_PAGE_SPACING)
        .padding(PDF_PAGE_LIST_PADDING)
        .align_x(Alignment::Center)
        .width(Length::Fill)
        .height(Length::Shrink);

    let placeholder_display_size = placeholder_page_size
        .map(|(w, h)| (w * zoom, h * zoom))
        .or_else(|| {
            page_sizes
                .first()
                .and_then(|s| *s)
                .map(|(w, h)| (w * zoom, h * zoom))
        })
        .or_else(|| {
            dimensions
                .first()
                .and_then(|d| d.map(|(w, h)| (w as f32, h as f32)))
        })
        .unwrap_or((612.0 * zoom, 792.0 * zoom));

    for (i, page_opt) in pages.iter().enumerate() {
        let display_size = placeholder_display_size;
        let page_height = page_sizes
            .get(i)
            .and_then(|size| *size)
            .map(|(_, h)| h)
            .unwrap_or_else(|| display_size.1 / zoom.max(0.01));

        if let Some(handle) = page_opt {
            let (w, h) = display_size;
            let highlights = search_matches
                .iter()
                .enumerate()
                .filter(|(idx, result)| {
                    result.page_index == i as u16 && Some(*idx) != active_search_index
                })
                .flat_map(|(_, result)| result.rects.iter())
                .map(|rect| search_rect_to_view_rect(rect, page_height, zoom))
                .collect::<Vec<_>>();
            let active_highlights = active_search_index
                .and_then(|idx| search_matches.get(idx))
                .filter(|result| result.page_index == i as u16)
                .map(|result| {
                    result
                        .rects
                        .iter()
                        .map(|rect| search_rect_to_view_rect(rect, page_height, zoom))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            page_list = page_list.push(
                container(
                    InteractivePdf::new(
                        handle.clone(),
                        w,
                        h,
                        move |x, y| Message::PdfLeftClicked(i as u16, x, y),
                        move |x, y| Message::PdfRightClicked(i as u16, x, y),
                    )
                    .highlights(highlights, active_highlights),
                )
                .width(Length::Fixed(w))
                .height(Length::Fixed(h))
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
                    .width(Length::Fixed(display_size.0))
                    .height(Length::Fixed(display_size.1))
                    .style(|_| container::Style {
                        background: Some(iced::Background::Color(iced::Color::WHITE)),
                        border: iced::Border {
                            color: theme::BORDER,
                            width: 1.0,
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    }),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_rects_convert_from_pdf_space_to_view_space() {
        let rect = md_editor_core::pdf::PdfRect {
            x: 72.0,
            y: 700.0,
            width: 100.0,
            height: 14.0,
        };

        let converted = search_rect_to_view_rect(&rect, 792.0, 2.0);

        assert_eq!(converted.x, 144.0);
        assert_eq!(converted.y, 156.0);
        assert_eq!(converted.width, 200.0);
        assert_eq!(converted.height, 28.0);
    }
}

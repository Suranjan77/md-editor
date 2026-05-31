use iced::widget::{
    Space, button, checkbox, column, container, row, text, text_input,
};
use iced::{Alignment, Color, Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::pdf_layout::PdfLayout;
use crate::theme;
use crate::views::icons::{self, Icon};
use crate::views::interactive_pdf::{InteractivePdf, PdfSelection};

pub(crate) const PDF_PAGE_LIST_PADDING: f32 = 20.0;
pub(crate) const PDF_PAGE_SPACING: f32 = 20.0;
pub const PDF_SEARCH_INPUT_ID: &str = "pdf_search_input";

#[cfg(test)]
mod tests {
    #[test]
    fn loading_placeholder_does_not_use_plain_loading_text() {
        let source = include_str!("pdf_viewer.rs");
        let plain_loading_text = ["Loading Page ", "{}..."].concat();

        assert!(
            !source.contains(&plain_loading_text),
            "PDF loading placeholder should be a stable skeleton, not plain loading text"
        );
    }
}



pub fn search_bar<'a>(
    query: &'a str,
    regex: bool,
    match_case: bool,
    current_match_count: usize,
    active_match_index: Option<usize>,
    searching: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let search_input = text_input("Find in PDF", query)
        .id(iced::advanced::widget::Id::new(PDF_SEARCH_INPUT_ID))
        .on_input(Message::SearchQueryChanged)
        .on_submit(Message::SearchNext)
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
                    if searching {
                        format!("{} of {} (searching...)", index + 1, current_match_count)
                    } else {
                        format!("{} of {}", index + 1, current_match_count)
                    },
                _ => if searching {
                    format!("{} matches (searching...)", current_match_count)
                } else {
                    format!("{} matches", current_match_count)
                },
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
    fit_to_width: bool,
    fit_to_page: bool,
    toc_visible: bool,
    annotations_sidebar_visible: bool,
    selection_active: bool,
    focused_annotation: Option<&'a md_editor_core::pdf::PdfAnnotation>,
) -> Element<'a, Message, Theme, Renderer> {
    let page_label = if total_pages == 0 {
        "No PDF".to_string()
    } else {
        format!("{} / {}", current_page + 1, total_pages)
    };

    let study_controls = if selection_active {
        let colors = [
            (
                md_editor_core::pdf::PdfAnnotationColor::Yellow,
                Color::from_rgb8(250, 219, 92),
            ),
            (
                md_editor_core::pdf::PdfAnnotationColor::Green,
                Color::from_rgb8(105, 219, 124),
            ),
            (
                md_editor_core::pdf::PdfAnnotationColor::Blue,
                Color::from_rgb8(92, 182, 250),
            ),
            (
                md_editor_core::pdf::PdfAnnotationColor::Pink,
                Color::from_rgb8(250, 140, 190),
            ),
            (
                md_editor_core::pdf::PdfAnnotationColor::Orange,
                Color::from_rgb8(250, 160, 90),
            ),
        ];
        let mut color_row = row![].spacing(6).align_y(Alignment::Center);
        for (color_enum, display_color) in colors {
            color_row = color_row.push(
                button(
                    container(Space::new().width(18.0).height(18.0))
                        .width(Length::Fixed(22.0))
                        .height(Length::Fixed(22.0))
                        .center_x(Length::Fixed(22.0))
                        .center_y(Length::Fixed(22.0))
                        .style(move |_| container::Style {
                            background: Some(iced::Background::Color(Color {
                                a: 0.72,
                                ..display_color
                            })),
                            border: iced::Border {
                                color: display_color,
                                width: 1.0,
                                radius: 5.0.into(),
                            },
                            ..Default::default()
                        }),
                )
                .on_press(Message::PdfCreateHighlight(color_enum))
                .style(button::text)
                .padding(0),
            );
        }

        row![
            container(
                row![
                    text("Highlight").size(12).color(theme::TEXT_MUTED),
                    color_row,
                    button(icons::view(Icon::X, theme::TEXT_MUTED, 14.0))
                        .on_press(Message::PdfSelectionCleared)
                        .padding(5)
                        .style(button::text),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([4, 8])
            .style(|_| container::Style {
                background: Some(iced::Background::Color(theme::BG_PRIMARY)),
                border: iced::Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            }),
        ]
        .align_y(Alignment::Center)
    } else {
        row![]
    };

    let annotation_controls = if let Some(ann) = focused_annotation {
        let note_btn = button(
            row![
                icons::view(Icon::FileText, theme::TEXT_PRIMARY, 14.0),
                text(" Note").size(12).color(theme::TEXT_PRIMARY)
            ]
            .align_y(Alignment::Center),
        )
        .on_press(Message::PdfRightClicked { page_index: ann.page_index, x: -1.0, y: -1.0, absolute_pos: iced::Point::ORIGIN })
        .padding([4, 8])
        .style(button::text);

        let link_btn = if let Some(ref path) = ann.linked_note_path {
            if !path.is_empty() {
                button(
                    row![
                        icons::view(Icon::FolderOpen, theme::ACCENT, 14.0),
                        text(" Open Note").size(12).color(theme::ACCENT)
                    ]
                    .align_y(Alignment::Center),
                )
                .on_press(Message::PdfOpenLinkedNote(path.clone()))
                .padding([4, 8])
                .style(button::text)
            } else {
                button(
                    row![
                        icons::view(Icon::Folder, theme::TEXT_MUTED, 14.0),
                        text(" Link Note").size(12).color(theme::TEXT_MUTED)
                    ]
                    .align_y(Alignment::Center),
                )
                .on_press(Message::PdfLinkNote(ann.id.clone(), String::new()))
                .padding([4, 8])
                .style(button::text)
            }
        } else {
            button(
                row![
                    icons::view(Icon::Folder, theme::TEXT_MUTED, 14.0),
                    text(" Link Note").size(12).color(theme::TEXT_MUTED)
                ]
                .align_y(Alignment::Center),
            )
            .on_press(Message::PdfLinkNote(ann.id.clone(), String::new()))
            .padding([4, 8])
            .style(button::text)
        };

        let delete_btn = button(icons::view(
            Icon::Trash,
            Color::from_rgb8(239, 83, 80),
            14.0,
        ))
        .on_press(Message::PdfDeleteHighlight(ann.id.clone()))
        .padding([4, 8])
        .style(button::text);

        row![
            text("Highlight:").size(12).color(theme::TEXT_MUTED),
            note_btn,
            link_btn,
            delete_btn,
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    } else {
        row![]
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
            button(icons::view(Icon::FileText, if annotations_sidebar_visible {
                theme::ACCENT
            } else {
                theme::TEXT_MUTED
            }, 14.0))
            .on_press(Message::PdfToggleAnnotationsSidebar)
            .padding(8)
            .style(button::text),
            Space::new().width(Length::Fill),
            study_controls,
            annotation_controls,
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
            button(
                text("Fit W")
                    .size(12)
                    .color(if fit_to_width {
                        theme::ACCENT
                    } else {
                        theme::TEXT_MUTED
                    }),
            )
            .on_press(Message::PdfFitToWidth)
            .padding([4, 10])
            .style(button::text),
            button(
                text("Fit P")
                    .size(12)
                    .color(if fit_to_page {
                        theme::ACCENT
                    } else {
                        theme::TEXT_MUTED
                    }),
            )
            .on_press(Message::PdfFitToPage)
            .padding([4, 10])
            .style(button::text),
            button(
                text("Rotate ⟳")
                    .size(12)
                    .color(theme::TEXT_MUTED),
            )
            .on_press(Message::PdfRotateClockwise)
            .padding([4, 10])
            .style(button::text),
            Space::new().width(Length::Fill),
            button(text(page_label).size(12).color(theme::TEXT_SECONDARY))
                .on_press(Message::PdfGoToPage)
                .padding(0)
                .style(button::text),
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
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

pub fn view_continuous<'a>(
    pages: &'a [Option<iced::widget::image::Handle>],
    zoom: f32,
    rotation: u16,
    dimensions: &'a [Option<(u32, u32)>],
    page_sizes: &'a [Option<(f32, f32)>],
    placeholder_page_size: Option<(f32, f32)>,
    search_matches: &'a [md_editor_core::pdf::PdfSearchMatch],
    _search_match_indices_by_page: &'a std::collections::HashMap<u16, Vec<usize>>,
    active_search_index: Option<usize>,
    page_texts: &'a std::collections::HashMap<u16, md_editor_core::pdf::PdfPageText>,
    annotations: &'a std::collections::HashMap<u16, Vec<md_editor_core::pdf::PdfAnnotation>>,
    links_by_page: &'a std::collections::HashMap<u16, Vec<md_editor_core::pdf::LinkInfo>>,
    active_selection: Option<PdfSelection>,
    focused_annotation_id: Option<&'a str>,
    scroll_y: f32,
    viewport_height: f32,
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

    let mut effective_page_sizes = page_sizes.to_vec();
    effective_page_sizes.resize(pages.len(), None);

    // Build PdfLayout using prefix-sum for O(1) offset queries and O(log N)
    // visible-range computation.
    let layout = PdfLayout::rebuild(
        &effective_page_sizes,
        zoom,
        placeholder_page_size.unwrap_or((612.0, 792.0)),
        PDF_PAGE_SPACING,
        PDF_PAGE_LIST_PADDING,
        rotation,
    );

    // Compute visible page range + render buffer so we pre-warm pages just
    // ahead of (and behind) the viewport.  2 extra pages on each side covers
    // typical scroll bursts.
    let render_range = layout.visible_range(scroll_y, viewport_height, 2);

    let mut placeholder_display_size = placeholder_page_size
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

    if rotation == 90 || rotation == 270 {
        placeholder_display_size = (placeholder_display_size.1, placeholder_display_size.0);
    }

    let (pw, ph) = (
        placeholder_display_size.0 / zoom.max(0.01),
        placeholder_display_size.1 / zoom.max(0.01),
    );

    let mut page_list = column![]
        .align_x(Alignment::Center)
        .width(Length::Fill)
        .height(Length::Shrink);

    if render_range.start > 0 {
        page_list = page_list
            .push(Space::new().height(Length::Fixed(layout.page_offset(render_range.start))));
    } else {
        page_list = page_list.push(Space::new().height(Length::Fixed(PDF_PAGE_LIST_PADDING)));
    }

    for (i, page_opt) in pages.iter().enumerate() {
        // Skip pages outside the visible window + buffer
        if !render_range.contains(&(i as u16)) {
            continue;
        }

        let (page_width, page_height) =
            page_sizes.get(i).and_then(|size| *size).unwrap_or((pw, ph));
        let display_size = if rotation == 90 || rotation == 270 {
            (page_height * zoom, page_width * zoom)
        } else {
            (page_width * zoom, page_height * zoom)
        };

        if let Some(handle) = page_opt {
            let (w, h) = display_size;

            let page_index = i as u16;
            let page_text = page_texts.get(&page_index);
            let page_highlights = annotations
                .get(&page_index)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let search_highlights = search_matches
                .iter()
                .enumerate()
                .filter(|(idx, result)| {
                    result.page_index == page_index && Some(*idx) != active_search_index
                })
                .flat_map(|(_, result)| result.rects.iter())
                .copied()
                .collect::<Vec<_>>();
            let active_search_highlights = active_search_index
                .and_then(|idx| search_matches.get(idx))
                .filter(|result| result.page_index == page_index)
                .map(|result| result.rects.clone())
                .unwrap_or_default();
            let page_links = links_by_page
                .get(&page_index)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let interactive = InteractivePdf::new(
                handle.clone(),
                w,
                h,
                page_width,
                page_height,
                page_index,
                page_text,
                page_highlights,
                search_highlights,
                active_search_highlights,
                active_selection,
                focused_annotation_id,
                page_links,
                move |x, y, modifiers| Message::PdfLeftClicked(i as u16, x, y, modifiers),
                move |x, y, absolute_pos| Message::PdfRightClicked { page_index: i as u16, x, y, absolute_pos },
                move |page, anchor, focus| Message::PdfSelectionChanged(page, anchor, focus),
                move |page, anchor, focus| Message::PdfSelectionFinished(page, anchor, focus),
                move || Message::PdfSelectionCleared,
                move || Message::PdfCopySelection,
            );

            page_list = page_list.push(
                container(interactive)
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
                container(text(format!("{}", i + 1)).color(theme::TEXT_MUTED).size(16))
                    .width(Length::Fixed(display_size.0))
                    .height(Length::Fixed(display_size.1))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(iced::Background::Color(theme::BG_SECONDARY)),
                        border: iced::Border {
                            color: theme::BORDER,
                            width: 1.0,
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    }),
            );
        }
        page_list = page_list.push(Space::new().height(Length::Fixed(PDF_PAGE_SPACING)));
    }

    let bottom_spacer = (layout.total_height() - layout.page_offset(render_range.end)).max(0.0);
    page_list = page_list.push(Space::new().height(Length::Fixed(bottom_spacer)));

    container(page_list)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(theme::BG_PRIMARY)),
            ..Default::default()
        })
        .into()
}

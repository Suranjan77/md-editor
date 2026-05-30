use iced::widget::{
    Space, button, canvas, checkbox, column, container, row, stack, text, text_input,
};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Renderer, Theme, mouse};

use crate::messages::Message;
use crate::theme;
use crate::views::icons::{self, Icon};
use crate::views::interactive_pdf::{InteractivePdf, PdfSelection};

pub(crate) const PDF_PAGE_LIST_PADDING: f32 = 20.0;
pub(crate) const PDF_PAGE_SPACING: f32 = 20.0;
pub const PDF_SEARCH_INPUT_ID: &str = "pdf_search_input";

#[derive(Debug, Clone)]
struct OverlayRect {
    rect: md_editor_core::pdf::PdfRect,
    color: Color,
    border_color: Option<Color>,
}

#[derive(Debug, Clone)]
struct PdfOverlay {
    rects: Vec<OverlayRect>,
}

impl<Message> canvas::Program<Message> for PdfOverlay {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        for item in &self.rects {
            let rect = Rectangle {
                x: item.rect.x,
                y: item.rect.y,
                width: item.rect.width.max(3.0),
                height: item.rect.height.max(8.0),
            };
            frame.fill_rectangle(
                Point::new(rect.x, rect.y),
                iced::Size::new(rect.width, rect.height),
                item.color,
            );
            if let Some(border_color) = item.border_color {
                frame.stroke_rectangle(
                    Point::new(rect.x, rect.y),
                    iced::Size::new(rect.width, rect.height),
                    canvas::Stroke::default()
                        .with_color(border_color)
                        .with_width(1.5),
                );
            }
        }

        vec![frame.into_geometry()]
    }
}

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
        .on_press(Message::PdfRightClicked(ann.page_index, -1.0, -1.0))
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
            button(text("Fit").size(12))
                .on_press(Message::PdfFitToWidth)
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

    let (pw, ph) = (
        placeholder_display_size.0 / zoom.max(0.01),
        placeholder_display_size.1 / zoom.max(0.01),
    );

    for (i, page_opt) in pages.iter().enumerate() {
        let (page_width, page_height) =
            page_sizes.get(i).and_then(|size| *size).unwrap_or((pw, ph));
        let display_size = (page_width * zoom, page_height * zoom);

        if let Some(handle) = page_opt {
            let (w, h) = display_size;

            let page_index = i as u16;
            let page_text = page_texts.get(&page_index);
            let page_highlights = annotations
                .get(&page_index)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let mut overlay_rects = Vec::new();
            for ann in page_highlights {
                let color = match ann.color {
                    md_editor_core::pdf::PdfAnnotationColor::Yellow => {
                        Color::from_rgba(1.0, 0.92, 0.23, 0.35)
                    }
                    md_editor_core::pdf::PdfAnnotationColor::Green => {
                        Color::from_rgba(0.3, 0.85, 0.3, 0.35)
                    }
                    md_editor_core::pdf::PdfAnnotationColor::Blue => {
                        Color::from_rgba(0.12, 0.53, 0.9, 0.35)
                    }
                    md_editor_core::pdf::PdfAnnotationColor::Pink => {
                        Color::from_rgba(0.95, 0.3, 0.6, 0.35)
                    }
                    md_editor_core::pdf::PdfAnnotationColor::Orange => {
                        Color::from_rgba(1.0, 0.6, 0.1, 0.35)
                    }
                };
                let border_color = if focused_annotation_id == Some(ann.id.as_str()) {
                    Some(theme::ACCENT)
                } else {
                    None
                };
                for rect in &ann.rects {
                    overlay_rects.push(OverlayRect {
                        rect: search_rect_to_view_rect(rect, page_height, zoom),
                        color,
                        border_color,
                    });
                }
            }
            let search_highlights = search_matches
                .iter()
                .enumerate()
                .filter(|(idx, result)| {
                    result.page_index == page_index && Some(*idx) != active_search_index
                })
                .flat_map(|(_, result)| result.rects.iter())
                .map(|rect| search_rect_to_view_rect(rect, page_height, zoom))
                .collect::<Vec<_>>();
            overlay_rects.extend(search_highlights.iter().cloned().map(|rect| OverlayRect {
                rect,
                color: Color::from_rgba(1.0, 0.78, 0.18, 0.38),
                border_color: None,
            }));
            let active_search_highlights = active_search_index
                .and_then(|idx| search_matches.get(idx))
                .filter(|result| result.page_index == page_index)
                .map(|result| {
                    result
                        .rects
                        .iter()
                        .map(|rect| search_rect_to_view_rect(rect, page_height, zoom))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            overlay_rects.extend(active_search_highlights.iter().cloned().map(|rect| {
                OverlayRect {
                    rect,
                    color: Color::from_rgba(1.0, 0.62, 0.0, 0.68),
                    border_color: None,
                }
            }));
            if let (Some(sel), Some(page_text)) = (active_selection, page_text) {
                if sel.page_index == page_index {
                    let start = sel.anchor_idx.min(sel.focus_idx);
                    let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
                    let selected_chars = page_text
                        .chars
                        .iter()
                        .filter(|c| c.text_index >= start && c.text_index < end)
                        .cloned()
                        .collect::<Vec<_>>();
                    overlay_rects.extend(
                        md_editor_core::pdf::merge_char_rects(&selected_chars)
                            .into_iter()
                            .map(|rect| OverlayRect {
                                rect: search_rect_to_view_rect(&rect, page_text.page_height, zoom),
                                color: Color::from_rgba(0.12, 0.53, 0.9, 0.45),
                                border_color: None,
                            }),
                    );
                }
            }
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
                move |x, y| Message::PdfRightClicked(i as u16, x, y),
                move |page, anchor, focus| Message::PdfSelectionChanged(page, anchor, focus),
                move |page, anchor, focus| Message::PdfSelectionFinished(page, anchor, focus),
                move || Message::PdfSelectionCleared,
                move || Message::PdfCopySelection,
            );

            page_list = page_list.push(
                container(stack![
                    interactive,
                    canvas(PdfOverlay {
                        rects: overlay_rects
                    })
                    .width(Length::Fixed(w))
                    .height(Length::Fixed(h)),
                ])
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

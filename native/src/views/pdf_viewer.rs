use iced::widget::{Space, button, checkbox, column, container, row, stack, text, text_input};
use iced::{Alignment, Color, Element, Length, Renderer, Theme};

use crate::features::pdf::view_model::PdfLayout;
use crate::messages::{Message, PdfMessage, SearchMessage};
use crate::theme;
use crate::views::icons::{self, Icon};
use crate::views::interactive_pdf::{
    InteractivePdf, PdfHighlights, PdfSelection, pdf_highlight_rects,
};

pub const PDF_PAGE_LIST_PADDING: f32 = 20.0;
pub const PDF_PAGE_SPACING: f32 = 20.0;
pub(crate) const PDF_SEARCH_INPUT_ID: &str = "pdf_search_input";

#[cfg(test)]
mod tests {
    use super::*;
    use md_editor_core::application::pdf_service::PdfSearchMatch;
    use md_editor_core::domain::pdf::{
        PdfAnnotation, PdfAnnotationColor, PdfAnnotationKind, PdfRect,
    };
    use std::collections::HashMap;

    fn annotation() -> PdfAnnotation {
        PdfAnnotation {
            id: "ann-1".to_string(),
            document_id: "doc".to_string(),
            page_index: 4,
            kind: PdfAnnotationKind::Highlight,
            color: PdfAnnotationColor::Yellow,
            selected_text: "Important highlight".to_string(),
            ranges: vec![],
            rects: vec![],
            note: None,
            linked_note_path: None,
            markdown_anchor: None,
            tags: Vec::new(),
            status: md_editor_core::domain::pdf::PdfAnnotationStatus::Unresolved,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn loading_placeholder_does_not_use_plain_loading_text() {
        let source = include_str!("pdf_viewer.rs");
        let plain_loading_text = ["Loading Page ", "{}..."].concat();

        assert!(
            !source.contains(&plain_loading_text),
            "PDF loading placeholder should be a stable skeleton, not plain loading text"
        );
    }

    #[test]
    fn search_highlights_use_page_index_without_scanning_other_pages() {
        let matches = vec![
            PdfSearchMatch {
                page_index: 0,
                context: "a".into(),
                rects: vec![PdfRect {
                    x: 1.0,
                    y: 1.0,
                    width: 1.0,
                    height: 1.0,
                }],
            },
            PdfSearchMatch {
                page_index: 2,
                context: "b".into(),
                rects: vec![PdfRect {
                    x: 2.0,
                    y: 2.0,
                    width: 2.0,
                    height: 2.0,
                }],
            },
        ];
        let mut by_page = HashMap::new();
        by_page.insert(2, vec![1]);

        assert_eq!(
            search_highlights_for_page(&matches, &by_page, Some(1), 2),
            (Vec::new(), matches[1].rects.clone())
        );

        assert_eq!(
            search_highlights_for_page(&matches, &by_page, None, 2),
            (matches[1].rects.clone(), Vec::new())
        );
    }

    #[test]
    fn focused_annotation_toolbar_cite_click_emits_insert_message() {
        let ann = annotation();
        let mut ui = iced_test::simulator(toolbar_with_companion_note(
            4,
            10,
            1.0,
            true,
            false,
            false,
            Some(&ann),
            true,
            None,
        ));

        ui.click(" Cite").expect("Cite button should exist");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.as_slice(),
            [Message::Pdf(PdfMessage::InsertAnnotationLink(id))] if id == "ann-1"
        ));
    }

    #[test]
    fn selection_toolbar_cite_click_emits_quote_insert_message() {
        let mut ui = iced_test::simulator(toolbar_with_companion_note(
            4, 10, 1.0, true, false, true, None, true, None,
        ));

        ui.click(" Cite")
            .expect("selection Cite button should exist");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.as_slice(),
            [Message::Pdf(PdfMessage::InsertQuoteLink)]
        ));
    }

    #[test]
    fn focused_annotation_toolbar_cite_is_inert_without_markdown_file() {
        let ann = annotation();
        let mut ui = iced_test::simulator(toolbar_with_companion_note(
            4,
            10,
            1.0,
            true,
            false,
            false,
            Some(&ann),
            false,
            None,
        ));

        ui.click(" Cite")
            .expect("disabled-looking Cite control should still render");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(
            messages.is_empty(),
            "Cite must not insert without an active markdown note"
        );
    }

    #[test]
    fn pdf_toolbar_exposes_reading_state_groups() {
        let mut ui = iced_test::simulator(toolbar_with_companion_note(
            0, 3, 1.25, true, false, false, None, false, None,
        ));

        ui.find("PAGE")
            .expect("PDF toolbar should label page navigation group");
        ui.find("1 / 3")
            .expect("PDF toolbar should show current page status once");
        ui.find("ZOOM")
            .expect("PDF toolbar should label zoom group");
        ui.find("125%")
            .expect("PDF toolbar should show current zoom");
    }

    #[test]
    fn pdf_toolbar_exposes_mapped_companion_note() {
        let mut ui = iced_test::simulator(toolbar_with_companion_note(
            0,
            3,
            1.0,
            true,
            false,
            false,
            None,
            false,
            Some("notes/paper.md"),
        ));

        ui.click(" Companion Note")
            .expect("mapped companion note action should exist");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.as_slice(),
            [Message::Pdf(PdfMessage::OpenCompanionNote(vault_path))]
                if vault_path == "notes/paper.md"
        ));
    }

    #[test]
    fn pdf_toolbar_hides_missing_companion_note() {
        let mut ui = iced_test::simulator(toolbar_with_companion_note(
            0, 3, 1.0, true, false, false, None, false, None,
        ));

        assert!(
            ui.find(" Companion Note").is_err(),
            "companion note action must stay contextual"
        );
    }
}

fn search_highlights_for_page(
    search_matches: &[md_editor_core::application::pdf_service::PdfSearchMatch],
    search_match_indices_by_page: &std::collections::HashMap<u16, Vec<usize>>,
    active_search_index: Option<usize>,
    page_index: u16,
) -> (
    Vec<md_editor_core::domain::pdf::PdfRect>,
    Vec<md_editor_core::domain::pdf::PdfRect>,
) {
    let active_search_highlights = active_search_index
        .and_then(|idx| search_matches.get(idx))
        .filter(|result| result.page_index == page_index)
        .map(|result| result.rects.clone())
        .unwrap_or_default();

    let search_highlights = search_match_indices_by_page
        .get(&page_index)
        .into_iter()
        .flat_map(|indices| indices.iter().copied())
        .filter(|idx| Some(*idx) != active_search_index)
        .filter_map(|idx| search_matches.get(idx))
        .flat_map(|result| result.rects.iter().copied())
        .collect::<Vec<_>>();

    (search_highlights, active_search_highlights)
}

fn toolbar_button_style(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let mut style = button::text(theme, status);
        style.border.radius = theme::RADIUS_REGULAR.into();
        style.text_color = if active {
            theme::accent()
        } else {
            theme::text_secondary()
        };

        if active {
            style.background = Some(iced::Background::Color(theme::accent_dim()));
        } else if status == button::Status::Hovered || status == button::Status::Pressed {
            style.background = Some(iced::Background::Color(theme::bg_tertiary()));
        }

        style
    }
}

fn toolbar_divider<'a>() -> Element<'a, Message, Theme, Renderer> {
    container(Space::new())
        .width(Length::Fixed(1.0))
        .height(Length::Fixed(18.0))
        .style(|_| container::Style {
            background: Some(iced::Background::Color(theme::border_subtle())),
            ..Default::default()
        })
        .into()
}

fn toolbar_group<'a>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> Element<'a, Message, Theme, Renderer> {
    container(content.into())
        .padding([4, 8])
        .style(|_| container::Style {
            background: Some(iced::Background::Color(theme::bg_primary())),
            border: iced::Border {
                color: theme::border_subtle(),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        })
        .into()
}

pub(crate) fn search_bar<'a>(
    query: &'a str,
    regex: bool,
    match_case: bool,
    current_match_count: usize,
    active_match_index: Option<usize>,
    searching: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let search_input = text_input("Find in PDF", query)
        .id(iced::advanced::widget::Id::new(PDF_SEARCH_INPUT_ID))
        .on_input(|query| Message::Search(SearchMessage::QueryChanged(query)))
        .on_submit(Message::Search(SearchMessage::Next))
        .padding([8, 12])
        .size(14)
        .width(Length::Fill);

    container(
        row![
            icons::view(Icon::Search, theme::accent(), 18.0),
            search_input,
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
            text(match active_match_index {
                Some(index) if current_match_count > 0 =>
                    if searching {
                        format!("{} of {} (searching...)", index + 1, current_match_count)
                    } else {
                        format!("{} of {}", index + 1, current_match_count)
                    },
                _ =>
                    if searching {
                        format!("{} matches (searching...)", current_match_count)
                    } else {
                        format!("{} matches", current_match_count)
                    },
            })
            .size(12)
            .color(theme::text_muted()),
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn toolbar_with_companion_note<'a>(
    current_page: u16,
    total_pages: u16,
    zoom: f32,
    fit_to_width: bool,
    fit_to_page: bool,
    selection_active: bool,
    focused_annotation: Option<&'a md_editor_core::domain::pdf::PdfAnnotation>,
    can_insert_annotation_link: bool,
    companion_note_vault_path: Option<&'a str>,
) -> Element<'a, Message, Theme, Renderer> {
    let page_label = if total_pages == 0 {
        "No PDF".to_string()
    } else {
        format!("{} / {}", current_page + 1, total_pages)
    };

    let study_controls = if selection_active {
        let colors = [
            (
                md_editor_core::domain::pdf::PdfAnnotationColor::Yellow,
                Color::from_rgb8(250, 219, 92),
            ),
            (
                md_editor_core::domain::pdf::PdfAnnotationColor::Green,
                Color::from_rgb8(105, 219, 124),
            ),
            (
                md_editor_core::domain::pdf::PdfAnnotationColor::Blue,
                Color::from_rgb8(92, 182, 250),
            ),
            (
                md_editor_core::domain::pdf::PdfAnnotationColor::Pink,
                Color::from_rgb8(250, 140, 190),
            ),
            (
                md_editor_core::domain::pdf::PdfAnnotationColor::Orange,
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
                .on_press(Message::Pdf(PdfMessage::CreateHighlight(color_enum)))
                .style(button::text)
                .padding(0),
            );
        }

        let cite_btn = if can_insert_annotation_link {
            button(
                row![
                    icons::view(Icon::FileText, theme::accent(), 14.0),
                    text(" Cite").size(12).color(theme::accent())
                ]
                .align_y(Alignment::Center),
            )
            .on_press(Message::Pdf(PdfMessage::InsertQuoteLink))
            .padding([4, 8])
            .style(toolbar_button_style(true))
        } else {
            button(
                row![
                    icons::view(Icon::FileText, theme::text_muted(), 14.0),
                    text(" Cite").size(12).color(theme::text_muted())
                ]
                .align_y(Alignment::Center),
            )
            .padding([4, 8])
            .style(toolbar_button_style(false))
        };

        row![toolbar_group(
            row![
                text("Selection").size(11).color(theme::text_muted()),
                color_row,
                cite_btn,
                text("Ctrl+H").size(11).color(theme::text_muted()),
                button(icons::view(Icon::X, theme::text_muted(), 14.0))
                    .on_press(Message::Pdf(PdfMessage::SelectionCleared))
                    .padding(5)
                    .style(toolbar_button_style(false)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ),]
        .align_y(Alignment::Center)
    } else {
        row![]
    };

    let annotation_controls = if let Some(ann) = focused_annotation {
        let note_btn = button(
            row![
                icons::view(Icon::FileText, theme::text_primary(), 14.0),
                text(" Note").size(12).color(theme::text_primary())
            ]
            .align_y(Alignment::Center),
        )
        .on_press(Message::Pdf(PdfMessage::EditAnnotationNote(
            ann.id.clone(),
            ann.page_index,
        )))
        .padding([4, 8])
        .style(toolbar_button_style(false));

        let cite_btn = if can_insert_annotation_link {
            button(
                row![
                    icons::view(Icon::FileText, theme::accent(), 14.0),
                    text(" Cite").size(12).color(theme::accent())
                ]
                .align_y(Alignment::Center),
            )
            .on_press(Message::Pdf(PdfMessage::InsertAnnotationLink(
                ann.id.clone(),
            )))
            .padding([4, 8])
            .style(toolbar_button_style(true))
        } else {
            button(
                row![
                    icons::view(Icon::FileText, theme::text_muted(), 14.0),
                    text(" Cite").size(12).color(theme::text_muted())
                ]
                .align_y(Alignment::Center),
            )
            .padding([4, 8])
            .style(toolbar_button_style(false))
        };

        let link_btn = if let Some(ref path) = ann.linked_note_path {
            if !path.is_empty() {
                button(
                    row![
                        icons::view(Icon::FolderOpen, theme::accent(), 14.0),
                        text(" Open Note").size(12).color(theme::accent())
                    ]
                    .align_y(Alignment::Center),
                )
                .on_press(Message::Pdf(PdfMessage::OpenLinkedNote(path.clone())))
                .padding([4, 8])
                .style(toolbar_button_style(true))
            } else {
                button(
                    row![
                        icons::view(Icon::Folder, theme::text_muted(), 14.0),
                        text(" Link Note").size(12).color(theme::text_muted())
                    ]
                    .align_y(Alignment::Center),
                )
                .on_press(Message::Pdf(PdfMessage::LinkNote(
                    ann.id.clone(),
                    String::new(),
                )))
                .padding([4, 8])
                .style(toolbar_button_style(false))
            }
        } else {
            button(
                row![
                    icons::view(Icon::Folder, theme::text_muted(), 14.0),
                    text(" Link Note").size(12).color(theme::text_muted())
                ]
                .align_y(Alignment::Center),
            )
            .on_press(Message::Pdf(PdfMessage::LinkNote(
                ann.id.clone(),
                String::new(),
            )))
            .padding([4, 8])
            .style(toolbar_button_style(false))
        };

        let delete_btn = button(icons::view(
            Icon::Trash,
            Color::from_rgb8(239, 83, 80),
            14.0,
        ))
        .on_press(Message::Pdf(PdfMessage::DeleteHighlight(ann.id.clone())))
        .padding([4, 8])
        .style(toolbar_button_style(false));

        row![toolbar_group(
            row![
                text("Annotation").size(11).color(theme::text_muted()),
                note_btn,
                cite_btn,
                link_btn,
                delete_btn,
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ),]
        .align_y(Alignment::Center)
    } else {
        row![]
    };

    let page_group = toolbar_group(
        row![
            text("PAGE").size(11).color(theme::text_muted()),
            button(text(page_label).size(12).color(theme::text_secondary()))
                .on_press(Message::Pdf(PdfMessage::GoToPage))
                .padding([4, 8])
                .style(toolbar_button_style(false)),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );

    let zoom_group = toolbar_group(
        row![
            text("ZOOM").size(11).color(theme::text_muted()),
            button(text("-").size(16))
                .on_press(Message::Pdf(PdfMessage::ZoomChanged((zoom - 0.1).max(0.5))))
                .padding([4, 10])
                .style(toolbar_button_style(false)),
            text(format!("{:.0}%", zoom * 100.0))
                .size(12)
                .color(theme::text_secondary()),
            button(text("+").size(16))
                .on_press(Message::Pdf(PdfMessage::ZoomChanged((zoom + 0.1).min(4.0))))
                .padding([4, 10])
                .style(toolbar_button_style(false)),
            button(text("Fit W").size(12).color(if fit_to_width {
                theme::accent()
            } else {
                theme::text_muted()
            }),)
            .on_press(Message::Pdf(PdfMessage::FitToWidth))
            .padding([4, 10])
            .style(toolbar_button_style(fit_to_width)),
            button(text("Fit P").size(12).color(if fit_to_page {
                theme::accent()
            } else {
                theme::text_muted()
            }),)
            .on_press(Message::Pdf(PdfMessage::FitToPage))
            .padding([4, 10])
            .style(toolbar_button_style(fit_to_page)),
            button(text("Rotate").size(12).color(theme::text_muted()),)
                .on_press(Message::Pdf(PdfMessage::RotateClockwise))
                .padding([4, 10])
                .style(toolbar_button_style(false)),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    );

    let companion_note_control = companion_note_vault_path
        .filter(|vault_path| !vault_path.is_empty())
        .map(|vault_path| {
            button(
                row![
                    icons::view(Icon::FolderOpen, theme::accent(), 14.0),
                    text(" Companion Note").size(12).color(theme::accent())
                ]
                .align_y(Alignment::Center),
            )
            .on_press(Message::Pdf(PdfMessage::OpenCompanionNote(
                vault_path.to_string(),
            )))
            .padding([4, 8])
            .style(toolbar_button_style(true))
        });
    let companion_note_group = companion_note_control
        .map(|control| toolbar_group(row![control].align_y(Alignment::Center)))
        .unwrap_or_else(|| Space::new().width(Length::Shrink).into());

    container(
        row![
            page_group,
            toolbar_divider(),
            zoom_group,
            Space::new().width(Length::Fill),
            companion_note_group,
            study_controls,
            annotation_controls,
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .padding([6, 12]),
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

struct SpinnerProgram {
    frame: u32,
}

impl<Message> iced::widget::canvas::Program<Message> for SpinnerProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        let mut frame = iced::widget::canvas::Frame::new(renderer, bounds.size());
        let center = bounds.center();
        let radius = 16.0;
        let stroke = iced::widget::canvas::Stroke::default()
            .with_color(crate::theme::accent())
            .with_width(3.0)
            .with_line_cap(iced::widget::canvas::LineCap::Round);

        let start_angle = (self.frame as f32 * 30.0).to_radians();
        let end_angle = start_angle + 270.0f32.to_radians();

        let path = iced::widget::canvas::Path::new(|path| {
            path.arc(iced::widget::canvas::path::Arc {
                center,
                radius,
                start_angle: iced::Radians(start_angle),
                end_angle: iced::Radians(end_angle),
            });
        });

        frame.stroke(&path, stroke);
        vec![frame.into_geometry()]
    }
}

pub(crate) fn view_continuous<'a>(
    pages: &'a [Option<iced::widget::image::Handle>],
    zoom: f32,
    rotation: u16,
    dimensions: &'a [Option<(u32, u32)>],
    page_sizes: &'a [Option<(f32, f32)>],
    placeholder_page_size: Option<(f32, f32)>,
    search_matches: &'a [md_editor_core::application::pdf_service::PdfSearchMatch],
    search_match_indices_by_page: &'a std::collections::HashMap<u16, Vec<usize>>,
    active_search_index: Option<usize>,
    page_texts: &'a std::collections::HashMap<u16, md_editor_core::domain::pdf::PdfPageText>,
    annotations: &'a std::collections::HashMap<
        u16,
        Vec<md_editor_core::domain::pdf::PdfAnnotation>,
    >,
    links_by_page: &'a std::collections::HashMap<u16, Vec<md_editor_core::domain::pdf::LinkInfo>>,
    active_selection: Option<PdfSelection>,
    focused_annotation_id: Option<&'a str>,
    scroll_y: f32,
    viewport_height: f32,
    spinner_frame: u32,
) -> Element<'a, Message, Theme, Renderer> {
    if pages.is_empty() {
        let spinner = iced::widget::canvas(SpinnerProgram {
            frame: spinner_frame,
        })
        .width(Length::Fixed(40.0))
        .height(Length::Fixed(40.0));
        let content = column![
            spinner,
            Space::new().height(10.0),
            text("Loading PDF...").color(theme::text_muted()).size(14)
        ]
        .align_x(Alignment::Center);

        return container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(theme::bg_primary())),
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

    for page_index in render_range.clone() {
        let i = page_index as usize;
        let page_opt = pages.get(i).and_then(|page| page.as_ref());

        let (page_width, page_height) =
            page_sizes.get(i).and_then(|size| *size).unwrap_or((pw, ph));
        let display_size = if rotation == 90 || rotation == 270 {
            (page_height * zoom, page_width * zoom)
        } else {
            (page_width * zoom, page_height * zoom)
        };

        if let Some(handle) = page_opt {
            let (w, h) = display_size;

            let page_text = page_texts.get(&page_index);
            let page_highlights = annotations
                .get(&page_index)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let (search_highlights, active_search_highlights) = search_highlights_for_page(
                search_matches,
                search_match_indices_by_page,
                active_search_index,
                page_index,
            );
            let raw_page_height = page_text.map(|p| p.page_height).unwrap_or(page_height);
            let overlay_rects = pdf_highlight_rects(
                page_index,
                page_width,
                raw_page_height,
                page_text,
                page_highlights,
                &search_highlights,
                &active_search_highlights,
                active_selection,
                focused_annotation_id,
                zoom,
                rotation,
            );
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
                rotation,
                move |x, y, modifiers| {
                    Message::Pdf(PdfMessage::LeftClicked(page_index, x, y, modifiers))
                },
                move |x, y, absolute_pos| {
                    Message::Pdf(PdfMessage::RightClicked {
                        page_index,
                        x,
                        y,
                        absolute_pos,
                    })
                },
                move |page, anchor, focus| {
                    Message::Pdf(PdfMessage::SelectionChanged(page, anchor, focus))
                },
                move |page, anchor, focus| {
                    Message::Pdf(PdfMessage::SelectionFinished(page, anchor, focus))
                },
                move || Message::Pdf(PdfMessage::SelectionCleared),
                move || Message::Pdf(PdfMessage::CopySelection),
            );

            page_list = page_list.push(
                container(stack![interactive, PdfHighlights::new(w, h, overlay_rects)])
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
                container(
                    text(format!("{}", usize::from(page_index) + 1))
                        .color(theme::text_muted())
                        .size(16),
                )
                .width(Length::Fixed(display_size.0))
                .height(Length::Fixed(display_size.1))
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(theme::bg_secondary())),
                    border: iced::Border {
                        color: theme::border(),
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
            background: Some(iced::Background::Color(theme::bg_primary())),
            ..Default::default()
        })
        .into()
}

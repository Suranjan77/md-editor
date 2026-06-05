use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Color, Element, Length, Padding, Renderer, Theme};

use crate::messages::Message;
use crate::theme;
use crate::views::icons::{self, Icon};

const BOLD_FONT: iced::Font = iced::Font {
    weight: iced::font::Weight::Bold,
    ..iced::Font::DEFAULT
};

fn custom_action_button<'a, Message: Clone + 'a>(
    label: impl Into<String>,
    on_press: Message,
) -> Element<'a, Message, Theme, Renderer> {
    button(text(label.into()).size(10).font(BOLD_FONT))
        .on_press(on_press)
        .height(Length::Fixed(28.0))
        .padding([5, 10])
        .style(|_theme, status| {
            let (bg, fg) = if status == button::Status::Hovered {
                (theme::bg_tertiary(), theme::accent())
            } else {
                (theme::bg_primary(), theme::text_secondary())
            };
            button::Style {
                background: Some(iced::Background::Color(bg)),
                text_color: fg,
                border: iced::Border {
                    color: theme::border(),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        })
        .into()
}

fn custom_cite_button<'a, Message: Clone + 'a>(
    on_press: Option<Message>,
) -> Element<'a, Message, Theme, Renderer> {
    let content = container(text(" Cite").size(12))
        .height(Length::Fixed(16.0))
        .center_y(Length::Fixed(16.0));
    let mut btn = button(content).width(Length::Fixed(48.0)).padding([5, 10]);
    btn = btn.height(Length::Fixed(28.0));
    if let Some(msg) = on_press {
        btn = btn.on_press(msg);
    }
    btn.style(|_theme, status| {
        if status == button::Status::Disabled {
            button::Style {
                background: Some(iced::Background::Color(theme::bg_primary())),
                text_color: theme::text_muted(),
                border: iced::Border {
                    color: theme::border(),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        } else {
            let (bg, fg, border_color) = if status == button::Status::Hovered {
                (
                    theme::bg_tertiary(),
                    theme::accent_secondary(),
                    theme::accent_secondary(),
                )
            } else {
                (theme::bg_primary(), theme::accent(), theme::accent())
            };
            button::Style {
                background: Some(iced::Background::Color(bg)),
                text_color: fg,
                border: iced::Border {
                    color: border_color,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        }
    })
    .into()
}

fn pill_button<'a, Message: Clone + 'a>(
    label: impl Into<String>,
    is_active: bool,
    on_press: Message,
) -> Element<'a, Message, Theme, Renderer> {
    button(text(label.into()).size(10).font(BOLD_FONT))
        .on_press(on_press)
        .padding([4, 10])
        .style(move |_theme, status| {
            if is_active {
                button::Style {
                    background: Some(iced::Background::Color(theme::accent())),
                    text_color: theme::bg_primary(),
                    border: iced::Border {
                        color: theme::accent(),
                        width: 1.0,
                        radius: 12.0.into(),
                    },
                    ..Default::default()
                }
            } else if status == button::Status::Hovered {
                button::Style {
                    background: Some(iced::Background::Color(theme::bg_tertiary())),
                    text_color: theme::text_primary(),
                    border: iced::Border {
                        color: theme::border(),
                        width: 1.0,
                        radius: 12.0.into(),
                    },
                    ..Default::default()
                }
            } else {
                button::Style {
                    background: Some(iced::Background::Color(theme::bg_secondary())),
                    text_color: theme::text_muted(),
                    border: iced::Border {
                        color: theme::border(),
                        width: 1.0,
                        radius: 12.0.into(),
                    },
                    ..Default::default()
                }
            }
        })
        .into()
}

fn color_dot_pill<'a>(
    color_opt: Option<md_editor_core::pdf::PdfAnnotationColor>,
    active_color: Option<md_editor_core::pdf::PdfAnnotationColor>,
) -> Element<'a, Message, Theme, Renderer> {
    let is_active = active_color == color_opt;
    let label = match color_opt {
        None => "All".to_string(),
        Some(col) => match col {
            md_editor_core::pdf::PdfAnnotationColor::Yellow => "Yellow".to_string(),
            md_editor_core::pdf::PdfAnnotationColor::Green => "Green".to_string(),
            md_editor_core::pdf::PdfAnnotationColor::Blue => "Blue".to_string(),
            md_editor_core::pdf::PdfAnnotationColor::Pink => "Pink".to_string(),
            md_editor_core::pdf::PdfAnnotationColor::Orange => "Orange".to_string(),
            md_editor_core::pdf::PdfAnnotationColor::Red => "Red".to_string(),
            md_editor_core::pdf::PdfAnnotationColor::Purple => "Purple".to_string(),
        },
    };

    let color_dot = if let Some(col) = color_opt {
        let rgb = match col {
            md_editor_core::pdf::PdfAnnotationColor::Yellow => Color::from_rgb(0.95, 0.85, 0.3),
            md_editor_core::pdf::PdfAnnotationColor::Green => Color::from_rgb(0.3, 0.8, 0.4),
            md_editor_core::pdf::PdfAnnotationColor::Blue => Color::from_rgb(0.3, 0.6, 0.95),
            md_editor_core::pdf::PdfAnnotationColor::Pink => Color::from_rgb(0.95, 0.4, 0.65),
            md_editor_core::pdf::PdfAnnotationColor::Orange => Color::from_rgb(0.95, 0.6, 0.3),
            md_editor_core::pdf::PdfAnnotationColor::Red => Color::from_rgb(0.9, 0.3, 0.3),
            md_editor_core::pdf::PdfAnnotationColor::Purple => Color::from_rgb(0.7, 0.4, 0.85),
        };
        container(
            Space::new()
                .width(Length::Fixed(8.0))
                .height(Length::Fixed(8.0)),
        )
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(rgb)),
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
    } else {
        container(
            Space::new()
                .width(Length::Fixed(0.0))
                .height(Length::Fixed(0.0)),
        )
    };

    let pill_content = row![
        color_dot,
        text(label).size(10).font(BOLD_FONT).color(if is_active {
            theme::bg_primary()
        } else {
            theme::text_muted()
        })
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    button(pill_content)
        .on_press(Message::PdfFilterAnnotationsByColor(color_opt))
        .padding([4, 10])
        .style(move |_theme, status| {
            if is_active {
                button::Style {
                    background: Some(iced::Background::Color(theme::accent())),
                    border: iced::Border {
                        color: theme::accent(),
                        width: 1.0,
                        radius: 12.0.into(),
                    },
                    ..Default::default()
                }
            } else if status == button::Status::Hovered {
                button::Style {
                    background: Some(iced::Background::Color(theme::bg_tertiary())),
                    border: iced::Border {
                        color: theme::border(),
                        width: 1.0,
                        radius: 12.0.into(),
                    },
                    ..Default::default()
                }
            } else {
                button::Style {
                    background: Some(iced::Background::Color(theme::bg_secondary())),
                    border: iced::Border {
                        color: theme::border(),
                        width: 1.0,
                        radius: 12.0.into(),
                    },
                    ..Default::default()
                }
            }
        })
        .into()
}

#[derive(Clone, Copy)]
struct AnnotationFilters<'a> {
    color: Option<md_editor_core::pdf::PdfAnnotationColor>,
    page: Option<u16>,
    tag: Option<&'a str>,
    linked: Option<bool>,
    unresolved: Option<bool>,
}

struct FilteredAnnotations<'a> {
    annotations: Vec<&'a md_editor_core::pdf::PdfAnnotation>,
    inspected: usize,
}

fn filtered_annotations<'a>(
    annotations: &'a std::collections::HashMap<u16, Vec<md_editor_core::pdf::PdfAnnotation>>,
    filters: AnnotationFilters<'_>,
) -> FilteredAnnotations<'a> {
    let mut list = Vec::new();
    let mut inspected = 0;
    for page_anns in annotations.values() {
        for ann in page_anns {
            inspected += 1;
            if let Some(fc) = filters.color {
                if ann.color != fc {
                    continue;
                }
            }
            if let Some(fp) = filters.page {
                if ann.page_index != fp {
                    continue;
                }
            }
            if let Some(ft) = filters.tag {
                if !ann.tags.iter().any(|t| t == ft) {
                    continue;
                }
            }
            if let Some(fl) = filters.linked {
                let is_linked = ann
                    .linked_note_path
                    .as_deref()
                    .filter(|p| !p.is_empty())
                    .is_some();
                if is_linked != fl {
                    continue;
                }
            }
            if let Some(fu) = filters.unresolved {
                let is_unresolved =
                    ann.status == md_editor_core::pdf::PdfAnnotationStatus::Unresolved;
                if is_unresolved != fu {
                    continue;
                }
            }
            list.push(ann);
        }
    }
    list.sort_by_key(|ann| {
        let start_idx = ann.ranges.first().map(|r| r.start_text_index).unwrap_or(0);
        (ann.page_index, start_idx, ann.created_at)
    });

    FilteredAnnotations {
        annotations: list,
        inspected,
    }
}

pub fn view<'a>(
    annotations: &'a std::collections::HashMap<u16, Vec<md_editor_core::pdf::PdfAnnotation>>,
    filter_color: Option<md_editor_core::pdf::PdfAnnotationColor>,
    filter_page: Option<u16>,
    filter_tag: Option<&str>,
    filter_linked: Option<bool>,
    filter_unresolved: Option<bool>,
    focused_id: Option<&'a str>,
    can_insert_annotation_link: bool,
    width: f32,
) -> Element<'a, Message, Theme, Renderer> {
    let title = text("ANNOTATIONS")
        .size(11)
        .color(theme::text_muted())
        .font(iced::Font::default());

    // 1. Color filter row
    let mut colors_filter = row![color_dot_pill(None, filter_color)].spacing(4);

    let all_colors = [
        (md_editor_core::pdf::PdfAnnotationColor::Yellow, "Yellow"),
        (md_editor_core::pdf::PdfAnnotationColor::Green, "Green"),
        (md_editor_core::pdf::PdfAnnotationColor::Blue, "Blue"),
        (md_editor_core::pdf::PdfAnnotationColor::Pink, "Pink"),
        (md_editor_core::pdf::PdfAnnotationColor::Orange, "Orange"),
    ];

    for &(col_enum, _) in &all_colors {
        colors_filter = colors_filter.push(color_dot_pill(Some(col_enum), filter_color));
    }

    let colors_scroll = scrollable(colors_filter).direction(scrollable::Direction::Horizontal(
        scrollable::Scrollbar::default(),
    ));

    // 2. Page filter (pages with annotations)
    let mut pages_with_anns = std::collections::BTreeSet::new();
    for page_anns in annotations.values() {
        for ann in page_anns {
            pages_with_anns.insert(ann.page_index);
        }
    }

    let mut pages_filter = row![pill_button(
        "All Pages",
        filter_page.is_none(),
        Message::PdfFilterAnnotationsByPage(None)
    )]
    .spacing(4);

    for &page_idx in &pages_with_anns {
        let is_selected = filter_page == Some(page_idx);
        pages_filter = pages_filter.push(pill_button(
            format!("p. {}", page_idx + 1),
            is_selected,
            Message::PdfFilterAnnotationsByPage(Some(page_idx)),
        ));
    }

    let pages_scroll = scrollable(pages_filter).direction(scrollable::Direction::Horizontal(
        scrollable::Scrollbar::default(),
    ));

    // 3. Tag filter (unique tags)
    let mut tags_set = std::collections::BTreeSet::new();
    for page_anns in annotations.values() {
        for ann in page_anns {
            for tag in &ann.tags {
                tags_set.insert(tag.as_str());
            }
        }
    }

    let mut tags_filter = row![pill_button(
        "All Tags",
        filter_tag.is_none(),
        Message::PdfFilterAnnotationsByTag(None)
    )]
    .spacing(4);

    for tag in &tags_set {
        let is_selected = filter_tag == Some(*tag);
        tags_filter = tags_filter.push(pill_button(
            format!("#{}", tag),
            is_selected,
            Message::PdfFilterAnnotationsByTag(Some(tag.to_string())),
        ));
    }

    let tags_scroll = scrollable(tags_filter).direction(scrollable::Direction::Horizontal(
        scrollable::Scrollbar::default(),
    ));

    // 4. Linked and Unresolved filters
    let linked_filter = row![
        pill_button(
            "All Notes",
            filter_linked.is_none(),
            Message::PdfFilterAnnotationsByLinked(None)
        ),
        pill_button(
            "Linked",
            filter_linked == Some(true),
            Message::PdfFilterAnnotationsByLinked(Some(true))
        ),
        pill_button(
            "Unlinked",
            filter_linked == Some(false),
            Message::PdfFilterAnnotationsByLinked(Some(false))
        ),
    ]
    .spacing(4);

    let unresolved_filter = row![
        pill_button(
            "All Status",
            filter_unresolved.is_none(),
            Message::PdfFilterAnnotationsByUnresolved(None)
        ),
        pill_button(
            "Open",
            filter_unresolved == Some(true),
            Message::PdfFilterAnnotationsByUnresolved(Some(true))
        ),
        pill_button(
            "Resolved",
            filter_unresolved == Some(false),
            Message::PdfFilterAnnotationsByUnresolved(Some(false))
        ),
    ]
    .spacing(4);

    let meta_filters = row![
        linked_filter,
        Space::new().width(Length::Fill),
        unresolved_filter
    ]
    .align_y(Alignment::Center);

    let mut filters_col = column![colors_scroll].spacing(6);

    if !pages_with_anns.is_empty() {
        filters_col = filters_col.push(pages_scroll);
    }
    if !tags_set.is_empty() {
        filters_col = filters_col.push(tags_scroll);
    }
    filters_col = filters_col.push(meta_filters);

    let filter_container = container(filters_col).padding(Padding {
        top: 0.0,
        right: 0.0,
        bottom: 8.0,
        left: 0.0,
    });

    let filtered = filtered_annotations(
        annotations,
        AnnotationFilters {
            color: filter_color,
            page: filter_page,
            tag: filter_tag,
            linked: filter_linked,
            unresolved: filter_unresolved,
        },
    );
    let _inspected = filtered.inspected;

    let total_count = filtered.annotations.len();
    let count_text = if total_count == 0 {
        text("0").size(11).color(theme::text_muted())
    } else {
        text(format!("{}", total_count))
            .size(11)
            .color(theme::accent())
    };

    let items = filtered.annotations.into_iter().map(|ann| {
        let is_focused = Some(ann.id.as_str()) == focused_id;

        let status_badge = match ann.status {
            md_editor_core::pdf::PdfAnnotationStatus::Unresolved => container(
                text("Open")
                    .size(9)
                    .font(BOLD_FONT)
                    .color(Color::from_rgb(0.95, 0.6, 0.2)),
            )
            .padding([2, 6])
            .style(|_| container::Style {
                background: Some(iced::Background::Color(Color::from_rgba(
                    0.95, 0.6, 0.2, 0.08,
                ))),
                border: iced::Border {
                    color: Color::from_rgba(0.95, 0.6, 0.2, 0.2),
                    width: 1.0,
                    radius: 10.0.into(),
                },
                ..Default::default()
            }),
            md_editor_core::pdf::PdfAnnotationStatus::Resolved => container(
                text("Resolved")
                    .size(9)
                    .font(BOLD_FONT)
                    .color(Color::from_rgb(0.3, 0.8, 0.4)),
            )
            .padding([2, 6])
            .style(|_| container::Style {
                background: Some(iced::Background::Color(Color::from_rgba(
                    0.3, 0.8, 0.4, 0.08,
                ))),
                border: iced::Border {
                    color: Color::from_rgba(0.3, 0.8, 0.4, 0.2),
                    width: 1.0,
                    radius: 10.0.into(),
                },
                ..Default::default()
            }),
        };

        let header = row![
            text(format!("Page {}", ann.page_index + 1))
                .size(12)
                .font(BOLD_FONT)
                .color(theme::text_primary()),
            Space::new().width(Length::Fixed(8.0)),
            text(ann.kind.as_str()).size(11).color(theme::text_muted()),
            Space::new().width(Length::Fill),
            status_badge,
        ]
        .align_y(Alignment::Center);

        let indicator_color = match ann.color {
            md_editor_core::pdf::PdfAnnotationColor::Yellow => Color::from_rgb(0.95, 0.85, 0.3),
            md_editor_core::pdf::PdfAnnotationColor::Green => Color::from_rgb(0.3, 0.8, 0.4),
            md_editor_core::pdf::PdfAnnotationColor::Blue => Color::from_rgb(0.3, 0.6, 0.95),
            md_editor_core::pdf::PdfAnnotationColor::Pink => Color::from_rgb(0.95, 0.4, 0.65),
            md_editor_core::pdf::PdfAnnotationColor::Orange => Color::from_rgb(0.95, 0.6, 0.3),
            md_editor_core::pdf::PdfAnnotationColor::Red => Color::from_rgb(0.9, 0.3, 0.3),
            md_editor_core::pdf::PdfAnnotationColor::Purple => Color::from_rgb(0.7, 0.4, 0.85),
        };

        let quote = row![
            container(Space::new().width(Length::Fixed(3.0)).height(Length::Fill)).style(
                move |_| container::Style {
                    background: Some(iced::Background::Color(indicator_color)),
                    border: iced::Border {
                        radius: iced::border::Radius {
                            top_left: 4.0,
                            bottom_left: 4.0,
                            top_right: 0.0,
                            bottom_right: 0.0,
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                }
            ),
            container(
                text(format!("\"{}\"", ann.selected_text.trim()))
                    .size(12)
                    .color(theme::text_secondary())
            )
            .padding([6, 10])
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(theme::bg_primary())),
                border: iced::Border {
                    color: theme::border(),
                    width: 1.0,
                    radius: iced::border::Radius {
                        top_left: 0.0,
                        bottom_left: 0.0,
                        top_right: 4.0,
                        bottom_right: 4.0,
                    },
                },
                ..Default::default()
            })
        ]
        .width(Length::Fill);

        let note = if let Some(ref note_str) = ann.note {
            if !note_str.is_empty() {
                container(
                    row![
                        icons::view(Icon::FileText, theme::text_muted(), 12.0),
                        text(note_str).size(11).color(theme::text_secondary()),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([6, 8])
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(theme::bg_tertiary())),
                    border: iced::Border {
                        color: theme::border(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
                .width(Length::Fill)
            } else {
                container(Space::new())
            }
        } else {
            container(Space::new())
        };

        let tags_row = if !ann.tags.is_empty() {
            let mut row_el = row![].spacing(4);
            for tag in &ann.tags {
                row_el = row_el.push(
                    container(
                        text(format!("#{}", tag))
                            .size(9)
                            .font(BOLD_FONT)
                            .color(theme::accent()),
                    )
                    .padding([2, 6])
                    .style(|_| container::Style {
                        background: Some(iced::Background::Color(theme::bg_tertiary())),
                        border: iced::Border {
                            color: theme::border(),
                            width: 1.0,
                            radius: 10.0.into(),
                        },
                        ..Default::default()
                    }),
                );
            }
            container(row_el)
        } else {
            container(Space::new())
        };

        let primary_actions = row![
            custom_action_button(
                "Go",
                Message::PdfNavigateToAnnotation {
                    id: ann.id.clone(),
                    page: ann.page_index,
                }
            ),
            custom_action_button(
                "Note",
                Message::PdfEditAnnotationNote(ann.id.clone(), ann.page_index,)
            ),
            if can_insert_annotation_link {
                custom_cite_button(Some(Message::PdfInsertAnnotationLink(ann.id.clone())))
            } else {
                custom_cite_button(None)
            },
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        let secondary_actions = row![
            custom_action_button("Tags", Message::PdfEditAnnotationTags(ann.id.clone())),
            custom_action_button(
                match ann.status {
                    md_editor_core::pdf::PdfAnnotationStatus::Unresolved => "Resolve",
                    md_editor_core::pdf::PdfAnnotationStatus::Resolved => "Reopen",
                },
                Message::PdfToggleAnnotationStatus(ann.id.clone())
            ),
            button(icons::view(Icon::Trash, theme::danger(), 12.0))
                .on_press(Message::PdfDeleteHighlight(ann.id.clone()))
                .height(Length::Fixed(28.0))
                .padding(6)
                .style(|_theme, status| {
                    let bg = if status == button::Status::Hovered {
                        Color::from_rgba(
                            theme::danger().r,
                            theme::danger().g,
                            theme::danger().b,
                            0.1,
                        )
                    } else {
                        Color::TRANSPARENT
                    };
                    button::Style {
                        background: Some(iced::Background::Color(bg)),
                        border: iced::Border {
                            color: if status == button::Status::Hovered {
                                Color::from_rgba(
                                    theme::danger().r,
                                    theme::danger().g,
                                    theme::danger().b,
                                    0.3,
                                )
                            } else {
                                Color::TRANSPARENT
                            },
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }
                }),
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        let actions = column![primary_actions, secondary_actions].spacing(4);

        let card_content = column![header, quote, note, tags_row, actions].spacing(8);

        let card_body = container(card_content)
            .padding([10, 12])
            .width(Length::Fill);

        container(card_body)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(theme::bg_secondary())),
                border: iced::Border {
                    color: if is_focused {
                        theme::accent()
                    } else {
                        theme::border()
                    },
                    width: if is_focused { 1.5 } else { 1.0 },
                    radius: 6.0.into(),
                },
                ..Default::default()
            })
            .width(Length::Fill)
            .into()
    });

    let header_row = row![
        title,
        Space::new().width(Length::Fill),
        button(icons::view(Icon::FileText, theme::text_muted(), 14.0))
            .on_press(Message::PdfExportAnnotations)
            .padding(4)
            .style(button::text),
        Space::new().width(Length::Fixed(8.0)),
        count_text
    ]
    .align_y(Alignment::Center)
    .padding([12, 14]);

    let divider = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_| container::Style {
            background: Some(iced::Background::Color(theme::border_subtle())),
            ..Default::default()
        });

    let header = column![header_row, divider];

    container(column![
        header,
        scrollable(column![
            filter_container.padding(iced::Padding {
                top: 12.0,
                right: 14.0,
                bottom: 8.0,
                left: 14.0
            }),
            column(items).spacing(8).padding(iced::Padding {
                top: 0.0,
                right: 14.0,
                bottom: 14.0,
                left: 14.0
            })
        ])
        .height(Length::Fill)
    ])
    .width(Length::Fixed(width))
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(iced::Background::Color(theme::bg_secondary())),
        border: iced::Border {
            color: theme::border(),
            width: 1.0,
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use md_editor_core::pdf::{PdfAnnotation, PdfAnnotationColor, PdfAnnotationKind};
    use std::collections::HashMap;

    fn annotation() -> PdfAnnotation {
        PdfAnnotation {
            id: "ann-1".to_string(),
            document_id: "doc".to_string(),
            page_index: 2,
            kind: PdfAnnotationKind::Highlight,
            color: PdfAnnotationColor::Yellow,
            selected_text: "Important highlight".to_string(),
            ranges: vec![],
            rects: vec![],
            note: None,
            linked_note_path: None,
            markdown_anchor: None,
            tags: Vec::new(),
            status: md_editor_core::pdf::PdfAnnotationStatus::Unresolved,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn annotations() -> HashMap<u16, Vec<PdfAnnotation>> {
        HashMap::from([(2, vec![annotation()])])
    }

    #[test]
    fn annotation_sidebar_cite_click_emits_insert_message() {
        let annotations = annotations();
        let mut ui = iced_test::simulator(view(
            &annotations,
            None,
            None,
            None,
            None,
            None,
            Some("ann-1"),
            true,
            270.0,
        ));

        ui.click(" Cite").expect("Cite button should exist");

        let messages = ui.into_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.as_slice(),
            [Message::PdfInsertAnnotationLink(id)] if id == "ann-1"
        ));
    }

    #[test]
    fn annotation_sidebar_cite_is_inert_without_markdown_file() {
        let annotations = annotations();
        let mut ui = iced_test::simulator(view(
            &annotations,
            None,
            None,
            None,
            None,
            None,
            Some("ann-1"),
            false,
            270.0,
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
    fn test_annotation_filtering() {
        let mut ann1 = annotation();
        ann1.id = "ann-1".to_string();
        ann1.page_index = 0;
        ann1.color = PdfAnnotationColor::Yellow;
        ann1.selected_text = "YellowQuote".to_string();
        ann1.tags = vec!["tagA".to_string()];
        ann1.status = md_editor_core::pdf::PdfAnnotationStatus::Unresolved;
        ann1.linked_note_path = None;

        let mut ann2 = annotation();
        ann2.id = "ann-2".to_string();
        ann2.page_index = 1;
        ann2.color = PdfAnnotationColor::Green;
        ann2.selected_text = "GreenQuote".to_string();
        ann2.tags = vec!["tagB".to_string()];
        ann2.status = md_editor_core::pdf::PdfAnnotationStatus::Resolved;
        ann2.linked_note_path = Some("note.md".to_string());

        let annotations = HashMap::from([(0, vec![ann1]), (1, vec![ann2])]);

        // Filter by color: Yellow
        let mut ui_color = iced_test::simulator(view(
            &annotations,
            Some(PdfAnnotationColor::Yellow),
            None,
            None,
            None,
            None,
            None,
            false,
            270.0,
        ));
        assert!(ui_color.find("\"YellowQuote\"").is_ok());
        assert!(ui_color.find("\"GreenQuote\"").is_err());

        // Filter by page: p. 2 (index 1)
        let mut ui_page = iced_test::simulator(view(
            &annotations,
            None,
            Some(1),
            None,
            None,
            None,
            None,
            false,
            270.0,
        ));
        assert!(ui_page.find("\"GreenQuote\"").is_ok());
        assert!(ui_page.find("\"YellowQuote\"").is_err());

        // Filter by tag: tagB
        let mut ui_tag = iced_test::simulator(view(
            &annotations,
            None,
            None,
            Some("tagB"),
            None,
            None,
            None,
            false,
            270.0,
        ));
        assert!(ui_tag.find("\"GreenQuote\"").is_ok());
        assert!(ui_tag.find("\"YellowQuote\"").is_err());

        // Filter by linked state: Linked
        let mut ui_linked = iced_test::simulator(view(
            &annotations,
            None,
            None,
            None,
            Some(true),
            None,
            None,
            false,
            270.0,
        ));
        assert!(ui_linked.find("\"GreenQuote\"").is_ok());
        assert!(ui_linked.find("\"YellowQuote\"").is_err());

        // Filter by unresolved state: Resolved (unresolved = false)
        let mut ui_unresolved = iced_test::simulator(view(
            &annotations,
            None,
            None,
            None,
            None,
            Some(false),
            None,
            false,
            270.0,
        ));
        assert!(ui_unresolved.find("\"GreenQuote\"").is_ok());
        assert!(ui_unresolved.find("\"YellowQuote\"").is_err());
    }

    #[test]
    fn large_annotation_filter_reports_linear_baseline_counter() {
        let mut annotations = HashMap::new();
        let mut page = Vec::new();
        for index in 0..600 {
            let mut ann = annotation();
            ann.id = format!("ann-{index}");
            ann.page_index = (index % 12) as u16;
            ann.selected_text = format!("Quote {index}");
            ann.created_at = index;
            ann.tags = if index % 3 == 0 {
                vec!["review".to_string()]
            } else {
                Vec::new()
            };
            if index % 2 == 0 {
                ann.color = PdfAnnotationColor::Green;
            }
            page.push(ann);
        }
        annotations.insert(0, page);

        let filtered = filtered_annotations(
            &annotations,
            AnnotationFilters {
                color: Some(PdfAnnotationColor::Green),
                page: None,
                tag: Some("review"),
                linked: None,
                unresolved: Some(true),
            },
        );

        assert_eq!(filtered.inspected, 600);
        assert_eq!(filtered.annotations.len(), 100);
        assert!(filtered.annotations.windows(2).all(|pair| {
            (pair[0].page_index, pair[0].created_at) <= (pair[1].page_index, pair[1].created_at)
        }));
    }
}

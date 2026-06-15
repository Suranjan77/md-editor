use super::chrome::BOLD;
use super::*;
use iced::widget::text_input;

impl Shell {
    pub(super) fn view_pdf_toc_panel(
        &self,
        session: &PdfSession,
        tab: TabId,
    ) -> Element<'_, Message> {
        let tokens = self.tokens();
        let title = iced::widget::row![
            text("Table of Contents")
                .size(14)
                .color(tokens.text_primary)
                .font(BOLD),
            iced::widget::Space::new().width(iced::Length::Fill),
            button(text("✕").size(12).font(BOLD))
                .on_press(Message::PdfCommand {
                    tab,
                    command: CommandId("pdf.toc-panel"),
                })
                .style(button::text),
        ]
        .align_y(iced::Alignment::Center);

        let current_index = session.current_section_index();
        let mut list = iced::widget::column![].spacing(2);
        for (index, entry) in session.outline.iter().enumerate() {
            let active = Some(index) == current_index;
            let text_color = if active {
                tokens.accent
            } else {
                tokens.text_primary
            };
            let font = if active { BOLD } else { super::fonts::SANS };
            let content = iced::widget::row![
                iced::widget::Space::new().width(iced::Length::Fixed(entry.depth as f32 * 12.0)),
                text(entry.title.clone())
                    .size(12)
                    .color(text_color)
                    .font(font),
                iced::widget::Space::new().width(iced::Length::Fill),
                text(format!("{}", entry.page + 1))
                    .size(11)
                    .color(tokens.text_muted),
            ]
            .align_y(iced::Alignment::Center)
            .padding([4, 6]);

            let item = button(content)
                .width(Fill)
                .style(move |_theme, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: if active {
                            Some(iced::Background::Color(tokens.bg_tertiary))
                        } else if hovered {
                            Some(iced::Background::Color(tokens.bg_surface))
                        } else {
                            None
                        },
                        ..button::Style::default()
                    }
                })
                .on_press(Message::PdfJumpToPage {
                    tab,
                    page: entry.page as usize,
                });
            list = list.push(item);
        }

        let panel = iced::widget::column![
            title,
            iced::widget::Space::new().height(8),
            iced::widget::scrollable(list).height(iced::Length::Fill)
        ]
        .spacing(4)
        .padding(10);

        container(panel)
            .width(iced::Length::Fixed(session.toc_width))
            .height(iced::Length::Fill)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(tokens.bg_secondary)),
                border: iced::Border {
                    color: tokens.border,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    pub(super) fn view_md_outline_panel(
        &self,
        session: &MdSession,
        tab: TabId,
    ) -> Element<'_, Message> {
        let tokens = self.tokens();
        let title = iced::widget::row![
            text("Outline")
                .size(14)
                .color(tokens.text_primary)
                .font(BOLD),
            iced::widget::Space::new().width(iced::Length::Fill),
            button(text("✕").size(12).font(BOLD))
                .on_press(Message::RunCommand(CommandId("note.outline-panel")))
                .style(button::text),
        ]
        .align_y(iced::Alignment::Center);

        let head = session.doc.buffer().primary().head;
        let caret_line = session.doc.buffer().offset_to_line_col(head).0;
        let headings = session.doc.headings();
        let active_index = headings
            .iter()
            .enumerate()
            .rfind(|(_, (_, _, line_index))| *line_index <= caret_line)
            .map(|(index, _)| index);

        let mut list = iced::widget::column![].spacing(2);
        for (index, (level, title_text, line_index)) in headings.into_iter().enumerate() {
            let active = Some(index) == active_index;
            let text_color = if active {
                tokens.accent
            } else {
                tokens.text_primary
            };
            let font = if active { BOLD } else { super::fonts::SANS };
            let indent = level.saturating_sub(1) as f32 * 12.0;
            let content = iced::widget::row![
                iced::widget::Space::new().width(iced::Length::Fixed(indent)),
                text(title_text).size(12).color(text_color).font(font),
                iced::widget::Space::new().width(iced::Length::Fill),
            ]
            .align_y(iced::Alignment::Center)
            .padding([4, 6]);

            let item = button(content)
                .width(Fill)
                .style(move |_theme, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: if active {
                            Some(iced::Background::Color(tokens.bg_tertiary))
                        } else if hovered {
                            Some(iced::Background::Color(tokens.bg_surface))
                        } else {
                            None
                        },
                        ..button::Style::default()
                    }
                })
                .on_press(Message::MdJumpToLine {
                    tab,
                    line: line_index,
                });
            list = list.push(item);
        }

        let panel = iced::widget::column![
            title,
            iced::widget::Space::new().height(8),
            iced::widget::scrollable(list).height(iced::Length::Fill)
        ]
        .spacing(4)
        .padding(10);

        container(panel)
            .width(iced::Length::Fixed(session.outline_width))
            .height(iced::Length::Fill)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(tokens.bg_secondary)),
                border: iced::Border {
                    color: tokens.border,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    pub(super) fn view_md_find_replace_bar(
        &self,
        session: &MdSession,
        tab: TabId,
    ) -> Element<'_, Message> {
        let tokens = self.tokens();
        let text_value = session.doc.buffer().text();
        let matches = find_all_matches(&text_value, &session.find_query);
        let primary = session.doc.buffer().primary();
        let caret_start = primary.anchor.min(primary.head);
        let caret_end = primary.anchor.max(primary.head);
        let active_index = matches
            .iter()
            .position(|&(start, end)| start == caret_start && end == caret_end);
        let count = if session.find_query.is_empty() {
            "0 of 0".to_string()
        } else {
            active_index.map_or_else(
                || format!("0 of {}", matches.len()),
                |index| format!("{} of {}", index + 1, matches.len()),
            )
        };

        let find_input = text_input("Find...", &session.find_query)
            .on_input(move |query| Message::MdFindQueryChanged { tab, query })
            .width(180)
            .padding(4)
            .size(13);
        let replace_input = text_input("Replace with...", &session.replace_text)
            .on_input(move |text| Message::MdReplaceTextChanged { tab, text })
            .width(180)
            .padding(4)
            .size(13);
        let content = iced::widget::row![
            text("Find:").size(12).color(tokens.text_muted),
            find_input,
            text(count).size(12).color(tokens.text_muted),
            button(text("▲").size(10))
                .padding([2, 6])
                .on_press(Message::MdFindPrev { tab }),
            button(text("▼").size(10))
                .padding([2, 6])
                .on_press(Message::MdFindNext { tab }),
            iced::widget::Space::new().width(12),
            text("Replace:").size(12).color(tokens.text_muted),
            replace_input,
            button(text("Replace").size(12))
                .padding([3, 8])
                .on_press(Message::MdReplace { tab }),
            button(text("Replace All").size(12))
                .padding([3, 8])
                .on_press(Message::MdReplaceAll { tab }),
            iced::widget::Space::new().width(iced::Length::Fill),
            button(text("✕").size(12).font(BOLD))
                .style(button::text)
                .on_press(Message::MdCloseFind { tab })
        ]
        .spacing(8)
        .padding([4, 8])
        .align_y(iced::Alignment::Center);

        container(content)
            .width(Fill)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(tokens.bg_secondary)),
                border: iced::Border {
                    color: tokens.border_subtle,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    pub(super) fn view_pdf_annotations_panel(
        &self,
        session: &PdfSession,
        tab: TabId,
    ) -> Element<'_, Message> {
        let tokens = self.tokens();
        let title = iced::widget::row![
            text("Annotations")
                .size(14)
                .color(tokens.text_primary)
                .font(BOLD),
            iced::widget::Space::new().width(iced::Length::Fill),
            button(text("✕").size(12).font(BOLD))
                .on_press(Message::PdfCommand {
                    tab,
                    command: CommandId("pdf.annotations-panel"),
                })
                .style(button::text),
        ]
        .align_y(iced::Alignment::Center);

        let mut list = iced::widget::column![].spacing(6);
        for annotation in &session.annotations {
            let text_value = match session.annotation_text(annotation) {
                text if text.is_empty() => "Highlight".to_string(),
                text => text,
            };
            let text_value = if text_value.chars().count() > 60 {
                format!("{}...", text_value.chars().take(57).collect::<String>())
            } else {
                text_value
            };
            let swatch_color = pdf_view::quad_color(&annotation.color, 1.0, tokens);
            let swatch = button(
                container(iced::widget::Space::new())
                    .width(12)
                    .height(12)
                    .style(move |_| container::Style {
                        background: Some(iced::Background::Color(swatch_color)),
                        border: iced::Border {
                            color: tokens.border,
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
                    }),
            )
            .padding(0)
            .on_press(Message::PdfCycleAnnotationColor {
                tab,
                annotation_id: annotation.id,
            });
            let note_preview = if annotation.note.is_empty() {
                None
            } else {
                let note = if annotation.note.chars().count() > 40 {
                    format!(
                        "{}...",
                        annotation.note.chars().take(37).collect::<String>()
                    )
                } else {
                    annotation.note.clone()
                };
                Some(text(note).size(11).color(tokens.accent_secondary))
            };
            let delete = button(text("🗑").size(12).color(tokens.danger))
                .style(button::text)
                .on_press(Message::PdfDeleteAnnotation {
                    tab,
                    annotation_id: annotation.id,
                });
            let edit = button(text("📝").size(12).color(tokens.text_primary))
                .style(button::text)
                .on_press(Message::PdfEditAnnotationNote {
                    tab,
                    annotation_id: annotation.id,
                });
            let active = Some(annotation.id) == session.selected_annotation;
            let text_color = if active {
                tokens.accent
            } else {
                tokens.text_primary
            };
            let mut content = iced::widget::column![
                iced::widget::row![
                    swatch,
                    iced::widget::Space::new().width(4),
                    text(format!("Page {}", annotation.page + 1))
                        .size(11)
                        .color(tokens.text_muted),
                    iced::widget::Space::new().width(iced::Length::Fill),
                    edit,
                    delete,
                ]
                .align_y(iced::Alignment::Center)
                .spacing(2),
                text(text_value).size(12).color(text_color)
            ]
            .spacing(4);
            if let Some(note) = note_preview {
                content = content.push(note);
            }
            let card = container(content)
                .padding(8)
                .width(Fill)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(if active {
                        tokens.bg_tertiary
                    } else {
                        tokens.bg_surface
                    })),
                    border: iced::Border {
                        color: if active {
                            tokens.accent
                        } else {
                            tokens.border_subtle
                        },
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..Default::default()
                });
            let item = button(card).width(Fill).style(button::text).on_press(
                Message::PdfJumpToAnnotation {
                    tab,
                    annotation_id: annotation.id,
                },
            );
            list = list.push(item);
        }

        let panel = iced::widget::column![
            title,
            iced::widget::Space::new().height(8),
            iced::widget::scrollable(list).height(iced::Length::Fill)
        ]
        .spacing(4)
        .padding(10);
        container(panel)
            .width(iced::Length::Fixed(session.annotations_width))
            .height(iced::Length::Fill)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(tokens.bg_secondary)),
                border: iced::Border {
                    color: tokens.border,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }
}

pub(super) fn find_all_matches(text: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();
    let mut matches = Vec::new();
    let mut start = 0;
    while let Some(position) = text_lower[start..].find(&query_lower) {
        let position = start + position;
        matches.push((position, position + query.len()));
        start = position + query.len().max(1);
    }
    matches
}

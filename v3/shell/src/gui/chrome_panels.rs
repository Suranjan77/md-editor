use super::*;

impl Shell {
    pub(super) fn view_pdf_toc_panel(
        &self,
        session: &PdfSession,
        tab: TabId,
    ) -> Element<'_, Message> {
        let tokens = tokens::dark();
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
            let font = if active { BOLD } else { iced::Font::DEFAULT };
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
}

use super::*;

impl Shell {
    pub(super) fn view_pdf_context_menu(
        &self,
        context: &PdfContextMenuState,
    ) -> Element<'_, Message> {
        let backdrop = mouse_area(
            container(iced::widget::Space::new())
                .width(Fill)
                .height(Fill),
        )
        .on_press(Message::PdfContextMenuClosed);

        let tokens = tokens::dark();
        let items = iced::widget::column![
            button(text("Copy").size(13))
                .width(150)
                .style(button::text)
                .on_press(Message::PdfContextMenuCommand {
                    tab: context.tab,
                    command: CommandId("pdf.copy-selection"),
                }),
            button(text("Highlight").size(13))
                .width(150)
                .style(button::text)
                .on_press(Message::PdfContextMenuCommand {
                    tab: context.tab,
                    command: CommandId("pdf.highlight"),
                }),
            button(text("Highlight + Note").size(13))
                .width(150)
                .style(button::text)
                .on_press(Message::PdfContextMenuCommand {
                    tab: context.tab,
                    command: CommandId("pdf.highlight-and-note"),
                }),
        ]
        .spacing(1)
        .padding(5);

        let card = container(items).style(move |_| container::Style {
            background: Some(iced::Background::Color(tokens.bg_secondary)),
            border: iced::Border {
                color: tokens.border,
                width: 1.0,
                radius: 5.0.into(),
            },
            ..container::Style::default()
        });

        let positioned = container(card)
            .width(Fill)
            .height(Fill)
            .padding(iced::Padding {
                top: context.abs_pos.1,
                right: 0.0,
                bottom: 0.0,
                left: context.abs_pos.0,
            });

        iced::widget::stack![backdrop, positioned].into()
    }
}

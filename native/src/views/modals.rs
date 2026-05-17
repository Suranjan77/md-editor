use iced::widget::{button, column, container, row, text, text_input};
use iced::{Alignment, Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::theme;

pub enum ModalType {
    CreateFile,
    CreateFolder,
    Delete(String), // path
}

pub fn view<'a>(
    modal_type: &ModalType,
    input_value: &str,
) -> Element<'a, Message, Theme, Renderer> {
    let title = match modal_type {
        ModalType::CreateFile => "Create New File",
        ModalType::CreateFolder => "Create New Folder",
        ModalType::Delete(_) => "Delete Confirmation",
    };

    let content: Element<'a, Message, Theme, Renderer> = match modal_type {
        ModalType::Delete(path) => column![
            text(format!("Are you sure you want to delete '{}'?", path)).color(theme::TEXT_PRIMARY),
            text("This action cannot be undone.")
                .size(12)
                .color(theme::TEXT_MUTED),
            row![
                button(text("Cancel").size(14))
                    .on_press(Message::NameModalCancel)
                    .padding([8, 20])
                    .style(button::text),
                button(text("Delete").size(14))
                    .on_press(Message::DeleteFile(path.clone()))
                    .padding([8, 20])
                    .style(button::secondary),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
        ]
        .spacing(20)
        .into(),
        _ => {
            column![
                text(title).size(18).color(theme::TEXT_PRIMARY),
                text_input("Enter name...", input_value)
                    .on_input(Message::NameModalInputChanged)
                    .padding(10),
                row![
                    button(text("Cancel").size(14))
                        .on_press(Message::NameModalCancel)
                        .padding([8, 20])
                        .style(button::text),
                    button(text("Confirm").size(14))
                        .on_press(Message::NameModalSubmit(input_value.to_string())) // This needs a "Submit" message
                        .padding([8, 20])
                        .style(button::primary),
                ]
                .spacing(10)
                .align_y(Alignment::Center)
            ]
            .spacing(20)
            .into()
        }
    };

    container(
        container(content)
            .width(Length::Fixed(400.0))
            .padding(30)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(theme::BG_SECONDARY)),
                border: iced::Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            }),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .style(|_| container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgba(
            0.0, 0.0, 0.0, 0.7,
        ))),
        ..Default::default()
    })
    .into()
}

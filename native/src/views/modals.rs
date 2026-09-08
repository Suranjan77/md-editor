use iced::widget::{button, column, container, row, text, text_input};
use iced::{Alignment, Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::theme;
use crate::views::link_note_picker;

#[derive(Debug, Clone, PartialEq)]
pub enum ModalType {
    CreateFile,
    CreateFolder,
    Delete(String),    // path
    QuickNote(String), // annotation ID
    LinkNote(String),  // annotation ID
}

pub fn view<'a>(
    modal_type: &ModalType,
    input_value: &str,
    picker_search: &str,
    vault_entries: &'a [md_editor_core::types::FileEntry],
) -> Element<'a, Message, Theme, Renderer> {
    let title = match modal_type {
        ModalType::CreateFile => "Create New File",
        ModalType::CreateFolder => "Create New Folder",
        ModalType::Delete(_) => "Delete Confirmation",
        ModalType::QuickNote(_) => "Edit Quick Note",
        ModalType::LinkNote(_) => "Create Linked Note",
    };

    let content: Element<'a, Message, Theme, Renderer> = match modal_type {
        ModalType::Delete(path) => column![
            text(format!("Are you sure you want to delete '{}'?", path)).color(theme::TEXT_PRIMARY),
            text("This action cannot be undone.")
                .size(theme::TEXT_SM)
                .color(theme::TEXT_MUTED),
            row![
                button(text("Cancel").size(theme::TEXT_BASE))
                    .on_press(Message::NameModalCancel)
                    .padding([theme::SPACE_3, theme::SPACE_6])
                    .style(button::text),
                button(text("Delete").size(theme::TEXT_BASE))
                    .on_press(Message::DeleteFile(path.clone()))
                    .padding([theme::SPACE_3, theme::SPACE_6])
                    .style(button::secondary),
            ]
            .spacing(theme::SPACE_4)
            .align_y(Alignment::Center)
        ]
        .spacing(theme::SPACE_6)
        .into(),
        ModalType::LinkNote(_) => link_note_picker::view(input_value, picker_search, vault_entries),
        _ => {
            column![
                text(title).size(theme::TEXT_LG).color(theme::TEXT_PRIMARY),
                text_input("Enter name...", input_value)
                    .on_input(Message::NameModalInputChanged)
                    .padding(theme::SPACE_4),
                row![
                    button(text("Cancel").size(theme::TEXT_BASE))
                        .on_press(Message::NameModalCancel)
                        .padding([theme::SPACE_3, theme::SPACE_6])
                        .style(button::text),
                    button(text("Confirm").size(theme::TEXT_BASE))
                        .on_press(Message::NameModalSubmit(input_value.to_string())) // This needs a "Submit" message
                        .padding([theme::SPACE_3, theme::SPACE_6])
                        .style(button::primary),
                ]
                .spacing(theme::SPACE_4)
                .align_y(Alignment::Center)
            ]
            .spacing(theme::SPACE_6)
            .into()
        }
    };

    container(
        container(content)
            .width(Length::Fixed(match modal_type {
                ModalType::LinkNote(_) => 560.0,
                _ => 400.0,
            }))
            .padding(theme::SPACE_6)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(theme::BG_SECONDARY)),
                border: iced::Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: theme::RADIUS_MD.into(),
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

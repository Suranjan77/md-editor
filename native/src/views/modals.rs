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
    GoToPage(u16),     // total pages
}

/// Go-to-page dialog — separate inline render so the
/// text_input sits at the top level and can receive focus.
fn view_go_to_page<'a>(total_pages: u16) -> Element<'a, Message, Theme, Renderer> {
    container(
        column![
            text("Go to Page").size(18).color(theme::TEXT_PRIMARY),
            text_input("[ 1 ]", "")
                .on_input(Message::NameModalInputChanged)
                .on_submit(Message::NameModalSubmitCurrent)
                .padding(10)
                .width(Length::Fixed(120.0)),
            text(format!("/ {}", total_pages))
                .size(18)
                .color(theme::TEXT_MUTED),
            text("Press Enter to navigate, Esc to cancel.")
                .size(11)
                .color(theme::TEXT_MUTED),
            row![
                button(text("Cancel").size(14))
                    .on_press(Message::NameModalCancel)
                    .padding([8, 20])
                    .style(button::text),
                button(text("Go").size(14))
                    .on_press(Message::NameModalSubmitCurrent)
                    .padding([8, 20])
                    .style(button::primary),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ]
        .spacing(12),
    )
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
    })
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

pub fn view<'a>(
    modal_type: &ModalType,
    input_value: &str,
    picker_search: &str,
    vault_entries: &'a [md_editor_core::types::FileEntry],
) -> Element<'a, Message, Theme, Renderer> {
    // GoToPage renders its own full-screen overlay with focused input.
    if let &ModalType::GoToPage(total) = modal_type {
        return view_go_to_page(total);
    }

    let title = match modal_type {
        &ModalType::GoToPage(_) => "", // unreachable — handled above
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
        ModalType::LinkNote(_) => link_note_picker::view(input_value, picker_search, vault_entries),
        _ => column![
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
                    .on_press(Message::NameModalSubmit(input_value.to_string()))
                    .padding([8, 20])
                    .style(button::primary),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
        ]
        .spacing(20)
        .into(),
    };

    container(
        container(content)
            .width(Length::Fixed(match modal_type {
                ModalType::LinkNote(_) => 560.0,
                _ => 400.0,
            }))
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

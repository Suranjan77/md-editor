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
    Rename(String),    // old path
    UnsavedChanges(String), // current file name
    QuickNote(String), // annotation ID
    LinkNote(String),  // annotation ID
}

fn delete_confirmation(path: &str, entries: &[md_editor_core::types::FileEntry]) -> String {
    let is_dir = entries.iter().any(|entry| entry.path == path && entry.is_dir);
    if is_dir {
        let prefix = format!("{}/", path.trim_end_matches('/'));
        let count = entries
            .iter()
            .filter(|entry| !entry.is_dir && entry.path.starts_with(&prefix))
            .count();
        format!("Delete folder '{path}' and its {count} files? This cannot be undone.")
    } else {
        format!("Are you sure you want to delete '{path}'?")
    }
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
        ModalType::Rename(_) => "Rename",
        ModalType::UnsavedChanges(_) => "Unsaved Changes",
        ModalType::QuickNote(_) => "Edit Quick Note",
        ModalType::LinkNote(_) => "Create Linked Note",
    };

    let content: Element<'a, Message, Theme, Renderer> = match modal_type {
        ModalType::Delete(path) => column![
            text(delete_confirmation(path, vault_entries)).color(theme::TEXT_PRIMARY),
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
        ModalType::UnsavedChanges(path) => column![
            text("Unsaved Changes").size(18).color(theme::TEXT_PRIMARY),
            text(format!("Save changes to '{}'?", path)).color(theme::TEXT_PRIMARY),
            row![
                button(text("Cancel").size(14))
                    .on_press(Message::UnsavedChangesCancel)
                    .padding([8, 20])
                    .style(button::text),
                button(text("Discard").size(14))
                    .on_press(Message::UnsavedChangesDiscard)
                    .padding([8, 20])
                    .style(button::secondary),
                button(text("Save").size(14))
                    .on_press(Message::UnsavedChangesSave)
                    .padding([8, 20])
                    .style(button::primary),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
        ]
        .spacing(20)
        .into(),
        ModalType::LinkNote(_) => link_note_picker::view(input_value, picker_search, vault_entries),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_delete_copy_counts_only_contained_files() {
        let entries = vec![
            md_editor_core::types::FileEntry { path: "dir".into(), name: "dir".into(), is_dir: true },
            md_editor_core::types::FileEntry { path: "dir/a.md".into(), name: "a.md".into(), is_dir: false },
            md_editor_core::types::FileEntry { path: "dir/sub".into(), name: "sub".into(), is_dir: true },
            md_editor_core::types::FileEntry { path: "dir/sub/b.md".into(), name: "b.md".into(), is_dir: false },
        ];
        assert_eq!(
            delete_confirmation("dir", &entries),
            "Delete folder 'dir' and its 2 files? This cannot be undone."
        );
    }
}

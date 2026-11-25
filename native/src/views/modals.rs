//! Modals: create, rename, delete, quick note and linked note built on `ui::dialog` with inline validation.

use iced::widget::{column, text};
use iced::{Element, Renderer, Theme};

use crate::design::metrics::{DIALOG_WIDTH_MD, DIALOG_WIDTH_SM, SPACE_3};
use crate::design::{TextRole, tokens};
use crate::messages::{Message, VaultMessage};
use crate::ui::{Button, ButtonVariant, Icon, TextField, dialog};
use crate::views::link_note_picker;

#[derive(Debug, Clone, PartialEq)]
pub enum ModalType {
    CreateFile,
    CreateFolder,
    Delete(String),
    Rename(String),
    QuickNote(String),
    LinkNote(String),
}

fn delete_confirmation(path: &str, entries: &[md_editor_core::types::FileEntry]) -> String {
    let prefix = format!("{}/", path.trim_end_matches('/'));
    let is_dir = entries
        .iter()
        .any(|entry| (entry.path == path && entry.is_dir) || entry.path.starts_with(&prefix));
    if is_dir {
        let count = entries
            .iter()
            .filter(|entry| !entry.is_dir && entry.path.starts_with(&prefix))
            .count();
        format!("Delete folder '{path}' and its {count} files? This cannot be undone.")
    } else {
        format!("Are you sure you want to delete '{path}'? This cannot be undone.")
    }
}

pub fn view<'a>(
    modal_type: &'a ModalType,
    input_value: &'a str,
    modal_error: Option<&'a str>,
    picker_search: &'a str,
    vault_entries: &'a [md_editor_core::types::FileEntry],
) -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();

    match modal_type {
        ModalType::Delete(path) => {
            let question = delete_confirmation(path, vault_entries);
            let body = column![
                text(question)
                    .size(TextRole::Caption.size())
                    .color(t.fg_secondary)
                    .font(TextRole::Caption.font()),
            ]
            .spacing(SPACE_3);

            let actions = vec![
                Button::new("Cancel")
                    .variant(ButtonVariant::Ghost)
                    .on_press(Message::Vault(VaultMessage::NameModalCancel))
                    .build(),
                Button::new("Delete")
                    .variant(ButtonVariant::Danger)
                    .icon(Icon::Trash)
                    .on_press(Message::Vault(VaultMessage::DeleteFile(path.clone())))
                    .build(),
            ];

            dialog(
                "Confirm Delete",
                body,
                actions,
                Message::Vault(VaultMessage::NameModalCancel),
                Some(DIALOG_WIDTH_SM),
                1.0,
            )
        }
        ModalType::LinkNote(_) => {
            let body =
                link_note_picker::view(input_value, modal_error, picker_search, vault_entries);
            let actions = vec![
                Button::new("Cancel")
                    .variant(ButtonVariant::Ghost)
                    .on_press(Message::Vault(VaultMessage::NameModalCancel))
                    .build(),
                Button::new("Link Note")
                    .variant(ButtonVariant::Primary)
                    .icon(Icon::Link)
                    .on_press(Message::Vault(VaultMessage::NameModalSubmit(
                        input_value.to_string(),
                    )))
                    .build(),
            ];
            dialog(
                "Link PDF Highlight",
                body,
                actions,
                Message::Vault(VaultMessage::NameModalCancel),
                Some(DIALOG_WIDTH_MD),
                1.0,
            )
        }
        _ => {
            let (title, placeholder, action_label) = match modal_type {
                ModalType::CreateFile => ("New Note", "Note name (e.g. Research Notes)", "Create"),
                ModalType::CreateFolder => ("New Folder", "Folder name", "Create"),
                ModalType::Rename(_) => ("Rename", "New name", "Rename"),
                ModalType::QuickNote(_) => ("Highlight Note", "Add an annotation note…", "Save"),
                _ => ("Dialog", "Enter value…", "Confirm"),
            };

            let mut field = TextField::new(placeholder, input_value)
                .id("modal_name_input")
                .on_input(|v| Message::Vault(VaultMessage::NameModalInputChanged(v)))
                .on_submit(Message::Vault(VaultMessage::NameModalSubmit(
                    input_value.to_string(),
                )));
            if let Some(err) = modal_error {
                field = field.error(Some(err.to_string()));
            }

            let actions = vec![
                Button::new("Cancel")
                    .variant(ButtonVariant::Ghost)
                    .on_press(Message::Vault(VaultMessage::NameModalCancel))
                    .build(),
                Button::new(action_label)
                    .variant(ButtonVariant::Primary)
                    .on_press(Message::Vault(VaultMessage::NameModalSubmit(
                        input_value.to_string(),
                    )))
                    .build(),
            ];

            dialog(
                title,
                field.build(),
                actions,
                Message::Vault(VaultMessage::NameModalCancel),
                Some(DIALOG_WIDTH_SM),
                1.0,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_delete_copy_counts_only_contained_files() {
        let entries = vec![
            md_editor_core::types::FileEntry {
                path: "dir".into(),
                name: "dir".into(),
                is_dir: true,
            },
            md_editor_core::types::FileEntry {
                path: "dir/a.md".into(),
                name: "a.md".into(),
                is_dir: false,
            },
            md_editor_core::types::FileEntry {
                path: "dir/sub".into(),
                name: "sub".into(),
                is_dir: true,
            },
            md_editor_core::types::FileEntry {
                path: "dir/sub/b.md".into(),
                name: "b.md".into(),
                is_dir: false,
            },
        ];
        assert_eq!(
            delete_confirmation("dir", &entries),
            "Delete folder 'dir' and its 2 files? This cannot be undone."
        );
    }
}

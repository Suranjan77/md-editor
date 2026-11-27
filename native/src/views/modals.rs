//! Modals: create, rename, delete, quick note, linked note, and saved desks built on `ui::dialog` with inline validation.

use iced::widget::{column, container, row, scrollable, text};
use iced::{Alignment, Element, Length, Renderer, Theme};

use crate::design::metrics::{
    DIALOG_WIDTH_MD, DIALOG_WIDTH_SM, PICKER_LIST_HEIGHT, SPACE_1, SPACE_2, SPACE_3,
};
use crate::design::{TextRole, tokens};
use crate::graph_state::pinboards::Pinboard;
use crate::messages::{DesksMessage, GraphMessage, Message, VaultMessage};
use crate::ui::{
    Button, ButtonVariant, Icon, IconButton, ListRowProps, TextField, dialog, empty_state, list_row,
};
use crate::views::link_note_picker;

#[derive(Debug, Clone, PartialEq)]
pub enum ModalType {
    CreateFile,
    CreateFolder,
    Delete(String),
    Rename(String),
    QuickNote(String),
    LinkNote(String),
    SaveDesk,
    Desks,
    SavePinboard,
    Pinboards,
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
    desks: &'a [crate::desks::Desk],
    pinboards: &'a [Pinboard],
) -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();

    match modal_type {
        ModalType::Desks => desks_modal(desks),
        ModalType::SaveDesk => save_named_modal(
            "Save Desk",
            "Desk name (e.g. Paper Review)",
            input_value,
            modal_error,
            Message::Desks(DesksMessage::Save(input_value.to_string())),
        ),
        ModalType::Pinboards => pinboards_modal(pinboards),
        ModalType::SavePinboard => save_named_modal(
            "Save Pinboard",
            "Pinboard name (e.g. Core Arguments)",
            input_value,
            modal_error,
            Message::Graph(GraphMessage::SavePinboard(input_value.to_string())),
        ),
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

fn save_named_modal<'a>(
    title: &'a str,
    placeholder: &'a str,
    input_value: &'a str,
    modal_error: Option<&'a str>,
    submit_message: Message,
) -> Element<'a, Message, Theme, Renderer> {
    let mut field = TextField::new(placeholder, input_value)
        .id("modal_name_input")
        .on_input(|v| Message::Vault(VaultMessage::NameModalInputChanged(v)))
        .on_submit(submit_message.clone());
    if let Some(err) = modal_error {
        field = field.error(Some(err.to_string()));
    }
    let actions = vec![
        Button::new("Cancel")
            .variant(ButtonVariant::Ghost)
            .on_press(Message::Vault(VaultMessage::NameModalCancel))
            .build(),
        Button::new("Save")
            .variant(ButtonVariant::Primary)
            .on_press(submit_message)
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

fn preset_picker_modal<'a, I>(
    title: &'a str,
    icon: Icon,
    empty_title: &'a str,
    empty_detail: &'a str,
    save_label: &'a str,
    save_message: Message,
    items: I,
) -> Element<'a, Message, Theme, Renderer>
where
    I: Iterator<Item = (String, String, Message, &'a str, Message)>,
{
    let mut rows = column![].spacing(SPACE_1);
    let mut count = 0usize;
    for (name, subtitle, on_open, delete_tooltip, on_delete) in items {
        count += 1;
        let item = list_row(ListRowProps {
            title: name.into(),
            subtitle: Some(subtitle.into()),
            leading_icon: Some(icon),
            shortcut: None,
            selected: false,
            active_doc: false,
            focused: false,
            drag_over: false,
            max_title_chars: 36,
            on_press: Some(on_open),
        });
        let delete_btn = IconButton::new(Icon::Trash, delete_tooltip)
            .danger(true)
            .on_press(on_delete)
            .build();
        rows = rows.push(
            row![item, delete_btn]
                .spacing(SPACE_2)
                .align_y(Alignment::Center),
        );
    }

    let body: Element<'a, Message, Theme, Renderer> = if count == 0 {
        container(empty_state(icon, empty_title, empty_detail, None))
            .height(Length::Fixed(PICKER_LIST_HEIGHT))
            .width(Length::Fill)
            .into()
    } else {
        container(scrollable(rows).height(Length::Shrink))
            .max_height(PICKER_LIST_HEIGHT)
            .width(Length::Fill)
            .into()
    };

    let actions = vec![
        Button::new("Close")
            .variant(ButtonVariant::Ghost)
            .on_press(Message::Vault(VaultMessage::NameModalCancel))
            .build(),
        Button::new(save_label)
            .variant(ButtonVariant::Primary)
            .icon(icon)
            .on_press(save_message)
            .build(),
    ];

    dialog(
        title,
        body,
        actions,
        Message::Vault(VaultMessage::NameModalCancel),
        Some(DIALOG_WIDTH_MD),
        1.0,
    )
}

fn desks_modal<'a>(desks: &'a [crate::desks::Desk]) -> Element<'a, Message, Theme, Renderer> {
    preset_picker_modal(
        "Saved Desks",
        Icon::Desktop,
        "No saved desks",
        "Save the current panes, tabs and layout to switch back in one click.",
        "Save Current Desk…",
        Message::Desks(DesksMessage::OpenSaveDialog),
        desks.iter().map(|desk| {
            (
                desk.name.clone(),
                desk.summary(),
                Message::Desks(DesksMessage::Switch(desk.name.clone())),
                "Delete desk",
                Message::Desks(DesksMessage::Delete(desk.name.clone())),
            )
        }),
    )
}

fn pinboards_modal<'a>(pinboards: &'a [Pinboard]) -> Element<'a, Message, Theme, Renderer> {
    preset_picker_modal(
        "Graph Pinboards",
        Icon::Pin,
        "No saved pinboards",
        "Arrange and pin nodes on the canvas, then save a pinboard to revisit the map.",
        "Save Current Pinboard…",
        Message::Graph(GraphMessage::OpenSavePinboardModal),
        pinboards.iter().map(|board| {
            (
                board.name.clone(),
                board.summary(),
                Message::Graph(GraphMessage::LoadPinboard(board.name.clone())),
                "Delete pinboard",
                Message::Graph(GraphMessage::DeletePinboard(board.name.clone())),
            )
        }),
    )
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

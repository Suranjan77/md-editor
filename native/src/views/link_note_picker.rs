//! Picker body for attaching a PDF highlight to an existing or new Markdown note.

use iced::widget::{Column, column, container, scrollable, text};
use iced::{Background, Border, Element, Length, Renderer, Theme};

use crate::design::metrics::{PICKER_LIST_HEIGHT, RADIUS_MD, SPACE_1, SPACE_2, SPACE_3};
use crate::design::{TextRole, tokens};
use crate::messages::{Message, VaultMessage};
use crate::ui::{Icon, ListRowProps, TextField, list_row};

pub fn view<'a>(
    input_value: &'a str,
    modal_error: Option<&'a str>,
    search_query: &'a str,
    vault_entries: &'a [md_editor_core::types::FileEntry],
) -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();

    let mut entries: Vec<&md_editor_core::types::FileEntry> = vault_entries
        .iter()
        .filter(|entry| is_pickable_note_target(entry) && matches_query(&entry.path, search_query))
        .collect();

    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.path.to_lowercase().cmp(&b.path.to_lowercase()),
    });

    let mut items: Vec<Element<'a, Message, Theme, Renderer>> = Vec::new();
    if search_query.trim().is_empty() || matches_query("Vault root", search_query) {
        let selected = input_value.is_empty() || !input_value.contains('/');
        items.push(list_row(ListRowProps::simple(
            "Vault root",
            Some(Icon::Folder),
            selected,
            Message::Vault(VaultMessage::PdfLinkNoteFolderSelected(String::new())),
        )));
    }

    for entry in entries {
        let selected = normalize_for_compare(input_value) == normalize_for_compare(&entry.path);
        let msg = if entry.is_dir {
            Message::Vault(VaultMessage::PdfLinkNoteFolderSelected(entry.path.clone()))
        } else {
            Message::Vault(VaultMessage::PdfLinkNoteFileSelected(entry.path.clone()))
        };
        let leading = if entry.is_dir {
            Icon::Folder
        } else {
            Icon::FileText
        };
        items.push(list_row(ListRowProps::simple(
            &entry.path,
            Some(leading),
            selected,
            msg,
        )));
    }

    let list: Element<'a, Message, Theme, Renderer> = if items.is_empty() {
        container(
            text("No matching folders or notes.")
                .size(TextRole::Caption.size())
                .color(t.fg_tertiary)
                .font(TextRole::Caption.font()),
        )
        .padding(SPACE_3)
        .into()
    } else {
        Column::with_children(items).spacing(SPACE_1 * 0.5).into()
    };

    let mut path_field = TextField::new("pdf-notes/example.md", input_value)
        .on_input(|value| Message::Vault(VaultMessage::NameModalInputChanged(value)));
    if let Some(err) = modal_error {
        path_field = path_field.error(Some(err.to_string()));
    }

    column![
        text("Select an existing note, or choose a folder and customize the filename.")
            .size(TextRole::Caption.size())
            .color(t.fg_secondary)
            .font(TextRole::Caption.font()),
        TextField::search("Search folders and notes…", search_query)
            .on_input(|value| Message::Vault(VaultMessage::PdfLinkNotePickerSearchChanged(value)))
            .build(),
        container(scrollable(list).height(Length::Fixed(PICKER_LIST_HEIGHT)))
            .height(Length::Fixed(PICKER_LIST_HEIGHT))
            .padding(SPACE_2)
            .style(move |_| container::Style {
                background: Some(Background::Color(t.bg_canvas)),
                border: Border {
                    color: t.border_subtle,
                    width: 1.0,
                    radius: RADIUS_MD.into(),
                },
                ..Default::default()
            }),
        text("Target note path")
            .size(TextRole::Label.size())
            .color(t.fg_secondary)
            .font(TextRole::Label.font()),
        path_field.build(),
    ]
    .spacing(SPACE_3)
    .into()
}

fn is_pickable_note_target(entry: &md_editor_core::types::FileEntry) -> bool {
    entry.is_dir
        || entry.path.to_lowercase().ends_with(".md")
        || entry.path.to_lowercase().ends_with(".markdown")
}

fn matches_query(path: &str, query: &str) -> bool {
    let haystack = path.replace(['-', '_', '/'], " ").to_lowercase();
    query
        .split_whitespace()
        .map(str::to_lowercase)
        .all(|term| haystack.contains(&term))
}

fn normalize_for_compare(path: &str) -> String {
    path.trim()
        .trim_end_matches('/')
        .replace('\\', "/")
        .to_lowercase()
}

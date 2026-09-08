use iced::advanced::text::Wrapping;
use iced::widget::{Column, Space, button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Background, Border, Color, Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::theme;
use crate::views::icons::{self, Icon};

pub fn view<'a>(
    input_value: &str,
    search_query: &str,
    vault_entries: &'a [md_editor_core::types::FileEntry],
) -> Element<'a, Message, Theme, Renderer> {
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
        items.push(picker_row(
            "",
            "Vault root",
            true,
            input_value.is_empty() || !input_value.contains('/'),
        ));
    }

    for entry in entries {
        let selected = normalize_for_compare(input_value) == normalize_for_compare(&entry.path);
        items.push(picker_row(&entry.path, &entry.path, entry.is_dir, selected));
    }

    let list: Element<'a, Message, Theme, Renderer> = if items.is_empty() {
        column![text("No matching folders or markdown notes.").color(theme::TEXT_MUTED)].into()
    } else {
        Column::with_children(items).spacing(theme::SPACE_1).into()
    };

    column![
        text("Link PDF Highlight")
            .size(theme::TEXT_LG)
            .color(theme::TEXT_PRIMARY),
        text("Select an existing note, or select a folder and edit the note filename.")
            .size(theme::TEXT_SM)
            .color(theme::TEXT_MUTED),
        text_input("Search folders and notes...", search_query)
            .on_input(Message::PdfLinkNotePickerSearchChanged)
            .padding(theme::SPACE_4),
        container(scrollable(list).height(Length::Fixed(300.0)))
            .height(Length::Fixed(300.0))
            .padding(theme::SPACE_3)
            .style(|_| container::Style {
                background: Some(Background::Color(theme::BG_PRIMARY)),
                border: Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: theme::RADIUS_MD.into(),
                },
                ..Default::default()
            }),
        text_input("pdf-notes/example.md", input_value)
            .on_input(Message::NameModalInputChanged)
            .padding(theme::SPACE_4),
        row![
            button(text("Cancel").size(theme::TEXT_BASE))
                .on_press(Message::NameModalCancel)
                .padding([theme::SPACE_3, theme::SPACE_6])
                .style(button::text),
            button(text("Link Note").size(theme::TEXT_BASE))
                .on_press(Message::NameModalSubmit(input_value.to_string()))
                .padding([theme::SPACE_3, theme::SPACE_6])
                .style(button::primary),
        ]
        .spacing(theme::SPACE_4)
        .align_y(Alignment::Center)
    ]
    .spacing(theme::SPACE_4)
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

fn picker_row<'a>(
    path: &str,
    label: &'a str,
    is_dir: bool,
    selected: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let icon = if is_dir { Icon::Folder } else { Icon::FileText };
    let color = if selected {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_SECONDARY
    };
    let msg = if is_dir {
        Message::PdfLinkNoteFolderSelected(path.to_string())
    } else {
        Message::PdfLinkNoteFileSelected(path.to_string())
    };

    let content = row![
        icons::view(icon, color, 15.0),
        text(label)
            .size(theme::TEXT_BASE)
            .color(color)
            .wrapping(Wrapping::WordOrGlyph)
            .width(Length::Fill),
        Space::new().width(Length::Fixed(1.0)),
    ]
    .spacing(theme::SPACE_3)
    .align_y(Alignment::Center);

    let style = move |theme: &Theme, status: button::Status| {
        let mut style = button::text(theme, status);
        style.border.radius = 6.0.into();
        if selected {
            style.background = Some(Background::Color(crate::theme::BG_TERTIARY));
            style.border.color = crate::theme::BORDER;
            style.border.width = 1.0;
        } else if status == button::Status::Hovered {
            style.background = Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.05)));
        }
        style
    };

    button(content)
        .on_press(msg)
        .padding([theme::SPACE_3, theme::SPACE_4])
        .width(Length::Fill)
        .style(style)
        .into()
}

fn normalize_for_compare(path: &str) -> String {
    path.trim()
        .trim_end_matches('/')
        .replace('\\', "/")
        .to_lowercase()
}

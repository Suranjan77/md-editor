use std::collections::BTreeSet;

use iced::widget::{button, column, container, row, scrollable, text, tooltip, Column, Space};
use iced::{Alignment, Element, Length, Theme, Renderer, Color, Border, Background};

use crate::messages::Message;
use crate::theme;

/// Render a tree level recursively using the flat list of entries.
fn render_tree_level<'a>(
    entries: &'a [md_editor_core::types::FileEntry],
    prefix: &str,
    depth: usize,
    selected_path: Option<&str>,
    active_path: Option<&str>,
    expanded: &'a BTreeSet<String>,
) -> Vec<Element<'a, Message, Theme, Renderer>> {
    let mut elements: Vec<Element<'a, Message, Theme, Renderer>> = Vec::new();

    let mut immediate_children = Vec::new();
    let mut seen = BTreeSet::new();

    for entry in entries {
        if !entry.path.starts_with(prefix) || entry.path == prefix {
            continue;
        }

        let relative = if prefix.is_empty() {
            &entry.path
        } else {
            entry.path.strip_prefix(prefix).unwrap().trim_start_matches('/')
        };

        let first_part = relative.split('/').next().unwrap();
        if seen.contains(first_part) {
            continue;
        }
        seen.insert(first_part);

        let is_dir = relative.contains('/') || entry.is_dir;
        let child_path = if prefix.is_empty() {
            first_part.to_string()
        } else {
            format!("{}/{}", prefix, first_part)
        };

        immediate_children.push((first_part, child_path, is_dir));
    }

    immediate_children.sort_by(|a, b| {
        if a.2 == b.2 {
            a.0.to_lowercase().cmp(&b.0.to_lowercase())
        } else if a.2 {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });

    for (name, path, is_dir) in immediate_children {
        let is_selected = selected_path.map_or(false, |s| s == path);
        let is_active = active_path.map_or(false, |s| s == path);
        let indent = depth as f32 * 12.0;

        // Fallback to basic icons if font doesn't support them
        let icon = if is_dir {
            if expanded.contains(&path) { "▼" } else { "▶" }
        } else if name.ends_with(".pdf") {
            "PDF"
        } else {
            " "
        };

        let name_color = if is_active {
            theme::ACCENT
        } else if is_selected {
            theme::TEXT_PRIMARY
        } else {
            theme::TEXT_SECONDARY
        };

        let content = row![
            Space::new().width(Length::Fixed(indent)),
            text(icon).size(10).color(theme::TEXT_MUTED),
            text(name).size(13).color(name_color),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let msg = if is_dir {
            Message::SidebarFolderToggled(path.clone())
        } else {
            Message::SidebarFileClicked(path.clone())
        };

        let style = move |theme: &Theme, status: button::Status| {
            let mut style = button::text(theme, status);
            if is_active {
                style.background = Some(Background::Color(theme::ACCENT_DIM));
                style.border.color = theme::ACCENT;
                style.border.width = 0.0;
            } else if is_selected {
                style.background = Some(Background::Color(theme::BG_TERTIARY));
            } else if status == button::Status::Hovered {
                style.background = Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.05)));
            }
            style
        };

        let btn = button(content)
            .on_press(msg)
            .padding([6, 12])
            .width(Length::Fill)
            .style(style);

        let delete_btn = tooltip(
            button(text("×").size(13).color(theme::TEXT_MUTED))
                .on_press(Message::DeleteFileDialog(path.clone()))
                .padding([4, 6])
                .style(button::text),
            "Delete",
            tooltip::Position::FollowCursor,
        );

        // Add a small indicator for active file
        let item = if is_active {
            row![
                container(Space::new().width(Length::Fixed(2.0)).height(Length::Fixed(16.0)))
                    .style(|_| container::Style {
                        background: Some(Background::Color(theme::ACCENT)),
                        border: Border { radius: 1.0.into(), ..Default::default() },
                        ..Default::default()
                    }),
                btn,
                delete_btn
            ].spacing(0).align_y(Alignment::Center).into()
        } else {
            row![btn, delete_btn].spacing(0).align_y(Alignment::Center).into()
        };

        elements.push(item);

        if is_dir && expanded.contains(&path) {
            let child_elements =
                render_tree_level(entries, &path, depth + 1, selected_path, active_path, expanded);
            elements.extend(child_elements);
        }
    }

    elements
}

/// Render the file tree sidebar.
pub fn view<'a>(
    entries: &'a [md_editor_core::types::FileEntry],
    selected_path: Option<&'a str>,
    active_path: Option<&'a str>,
    expanded_folders: &'a BTreeSet<String>,
    collapsed: bool,
) -> Element<'a, Message, Theme, Renderer> {
    if collapsed {
        return container(Space::new())
            .width(Length::Fixed(0.0))
            .into();
    }

    let header = row![
        text("EXPLORER").size(11).color(theme::TEXT_MUTED).font(iced::Font::default()),
        Space::new().width(Length::Fill),
        tooltip(button(text("+").size(14).color(theme::TEXT_MUTED))
            .on_press(Message::CreateFileDialog)
            .padding([4, 8])
            .style(button::text), "New file", tooltip::Position::FollowCursor),
        tooltip(button(text("▣").size(14).color(theme::TEXT_MUTED))
            .on_press(Message::CreateFolderDialog)
            .padding([4, 8])
            .style(button::text), "New folder", tooltip::Position::FollowCursor),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .padding([12, 16]);

    let tree_elements = render_tree_level(entries, "", 0, selected_path, active_path, expanded_folders);
    let file_list = Column::with_children(tree_elements).spacing(2);

    let content = column![
        header,
        scrollable(file_list.padding([0, 4]))
            .height(Length::Fill)
    ]
    .width(Length::Fixed(260.0));

    container(content)
        .style(|_| container::Style {
            background: Some(Background::Color(theme::BG_SECONDARY)),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .height(Length::Fill)
        .into()
}

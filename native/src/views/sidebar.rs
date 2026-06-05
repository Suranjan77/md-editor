use std::collections::BTreeSet;

use iced::advanced::text::Wrapping;
use iced::widget::{Column, Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Renderer, Theme};

use crate::messages::Message;
use crate::theme;
use crate::views::icons::{self, Icon};

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
            entry
                .path
                .strip_prefix(prefix)
                .unwrap()
                .trim_start_matches('/')
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
        let indent = depth as f32 * 14.0;
        let lower_name = name.to_lowercase();
        let disclosure: Element<'_, Message, Theme, Renderer> = if is_dir {
            icons::view(
                if expanded.contains(&path) {
                    Icon::ChevronDown
                } else {
                    Icon::ChevronRight
                },
                theme::text_muted(),
                13.0,
            )
        } else {
            Space::new().width(Length::Fixed(13.0)).into()
        };
        let file_icon = if is_dir && expanded.contains(&path) {
            Icon::FolderOpen
        } else if is_dir {
            Icon::Folder
        } else if crate::app::is_supported_image_path(&lower_name) {
            Icon::Image
        } else if lower_name.ends_with(".md") || lower_name.ends_with(".markdown") {
            Icon::FileText
        } else {
            Icon::File
        };

        let name_color = if is_active {
            theme::accent()
        } else if is_selected {
            theme::text_primary()
        } else {
            theme::text_secondary()
        };

        let content = row![
            Space::new().width(Length::Fixed(indent)),
            disclosure,
            icons::view(file_icon, name_color, 15.0),
            text(name)
                .size(13)
                .color(name_color)
                .wrapping(Wrapping::WordOrGlyph)
                .width(Length::Fill),
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
            style.border.radius = 6.0.into();
            if is_active {
                style.background = Some(Background::Color(theme::accent_dim()));
                style.border.color = theme::accent();
                style.border.width = 1.0;
            } else if is_selected {
                style.background = Some(Background::Color(theme::bg_tertiary()));
            } else if status == button::Status::Hovered {
                style.background = Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.05)));
            }
            style
        };

        let btn = button(content)
            .on_press(msg)
            .padding([7, 10])
            .width(Length::Fill)
            .style(style);

        let delete_btn = button(icons::view(Icon::Trash, theme::text_muted(), 14.0))
            .on_press(Message::DeleteFileDialog(path.clone()))
            .padding(7)
            .style(button::text);

        // Add a small indicator for active file
        let item = if is_active {
            row![
                container(
                    Space::new()
                        .width(Length::Fixed(3.0))
                        .height(Length::Fixed(20.0))
                )
                .style(|_| container::Style {
                    background: Some(Background::Color(theme::accent())),
                    border: Border {
                        radius: 1.5.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
                container(btn).width(Length::Fill),
                delete_btn
            ]
            .spacing(4)
            .align_y(Alignment::Center)
            .into()
        } else {
            row![btn, delete_btn]
                .spacing(4)
                .align_y(Alignment::Center)
                .into()
        };

        elements.push(item);

        if is_dir && expanded.contains(&path) {
            let child_elements = render_tree_level(
                entries,
                &path,
                depth + 1,
                selected_path,
                active_path,
                expanded,
            );
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
    width: f32,
) -> Element<'a, Message, Theme, Renderer> {
    if collapsed {
        return container(Space::new()).width(Length::Fixed(0.0)).into();
    }

    let header = row![
        text("FILES")
            .size(11)
            .color(theme::text_muted())
            .font(iced::Font::default()),
        Space::new().width(Length::Fill),
        button(icons::view(Icon::FolderOpen, theme::text_muted(), 16.0))
            .on_press(Message::OpenVaultDialog)
            .padding(4)
            .style(button::text),
        button(icons::view(Icon::FileText, theme::text_muted(), 16.0))
            .on_press(Message::CreateFileDialog)
            .padding(4)
            .style(button::text),
        button(icons::view(Icon::Folder, theme::text_muted(), 16.0))
            .on_press(Message::CreateFolderDialog)
            .padding(4)
            .style(button::text),
    ]
    .spacing(4)
    .align_y(Alignment::Center)
    .padding([12, 16]);

    let divider = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_| container::Style {
            background: Some(Background::Color(theme::border_subtle())),
            ..Default::default()
        });

    let tree_elements =
        render_tree_level(entries, "", 0, selected_path, active_path, expanded_folders);

    let main_content: Element<'_, Message, Theme, Renderer> = if tree_elements.is_empty() {
        container(
            column![
                icons::view(Icon::FolderOpen, theme::text_muted(), 32.0),
                text("No files yet").size(13).color(theme::text_muted()),
                button(text("Create file").size(12))
                    .on_press(Message::CreateFileDialog)
                    .padding([6, 12])
            ]
            .spacing(12)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([40, 20])
        .into()
    } else {
        let file_list = Column::with_children(tree_elements).spacing(3);
        container(scrollable(file_list.padding([0, 8])).height(Length::Fill))
            .padding([8, 8])
            .height(Length::Fill)
            .into()
    };

    let content = column![header, divider, main_content].width(Length::Fixed(width));

    container(content)
        .style(|_| container::Style {
            background: Some(Background::Color(theme::bg_secondary())),
            border: Border {
                color: theme::border(),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .height(Length::Fill)
        .into()
}

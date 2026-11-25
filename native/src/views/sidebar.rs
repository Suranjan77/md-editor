//! The vault file-tree sidebar: filter box, Phosphor icons, indent guides, context-menu hooks and empty states.

use std::collections::BTreeSet;

use iced::widget::{Column, Space, column, container, mouse_area, scrollable};
use iced::{Element, Length, Renderer, Theme};

use crate::design::metrics::{ControlSize, SPACE_0, SPACE_1, SPACE_2, SPACE_3};
use crate::messages::{ContextMenuTarget, Message, ShellMessage, TabsMessage, VaultMessage};
use crate::ui::{
    Button, ButtonVariant, Icon, IconButton, PanelSide, TextField, TreeRowProps, empty_state,
    panel, tree_row,
};

/// Props required to render the vault file-tree sidebar.
pub struct SidebarProps<'a> {
    pub entries: &'a [md_editor_core::types::FileEntry],
    pub selected_path: Option<&'a str>,
    pub active_path: Option<&'a str>,
    pub expanded_folders: &'a BTreeSet<String>,
    pub filter_query: &'a str,
    pub width: f32,
    pub collapsed: bool,
    pub cursor_pos: (f32, f32),
    /// The sidebar is the focused region (F6): draw the focus ring.
    pub focused: bool,
}

pub fn view<'a>(props: SidebarProps<'a>) -> Element<'a, Message, Theme, Renderer> {
    if props.collapsed {
        return container(Space::new()).width(Length::Fixed(SPACE_0)).into();
    }

    let actions = vec![
        IconButton::new(Icon::FolderOpen, "Open vault")
            .on_press(Message::Vault(VaultMessage::OpenVaultDialog))
            .build(),
        IconButton::new(Icon::FilePlus, "New note")
            .on_press(Message::Vault(VaultMessage::CreateFileDialog))
            .build(),
        IconButton::new(Icon::FolderPlus, "New folder")
            .on_press(Message::Vault(VaultMessage::CreateFolderDialog))
            .build(),
    ];

    let filter = container(
        TextField::search("Filter files…", props.filter_query)
            .on_input(|q| Message::Vault(VaultMessage::SidebarFilterChanged(q)))
            .build(),
    )
    .padding([SPACE_2, SPACE_3]);

    let query_trim = props.filter_query.trim().to_lowercase();
    let tree_elements = if !query_trim.is_empty() {
        render_filtered(
            props.entries,
            &query_trim,
            props.selected_path,
            props.active_path,
            props.cursor_pos,
        )
    } else {
        render_tree_level(
            props.entries,
            "",
            0,
            props.selected_path,
            props.active_path,
            props.expanded_folders,
            props.cursor_pos,
        )
    };

    let body_content: Element<'a, Message, Theme, Renderer> = if props.entries.is_empty() {
        empty_state(
            Icon::FolderOpen,
            "Empty vault",
            "Create your first note or drop files here to begin.",
            Some(
                Button::new("New note")
                    .variant(ButtonVariant::Primary)
                    .icon(Icon::FilePlus)
                    .on_press(Message::Vault(VaultMessage::CreateFileDialog))
                    .build(),
            ),
        )
    } else if tree_elements.is_empty() {
        empty_state(
            Icon::Search,
            "No matching files",
            "Try a shorter filter or clear the search box.",
            Some(
                Button::new("Clear filter")
                    .variant(ButtonVariant::Secondary)
                    .on_press(Message::Vault(VaultMessage::SidebarFilterChanged(
                        String::new(),
                    )))
                    .build(),
            ),
        )
    } else {
        let list = Column::with_children(tree_elements)
            .spacing(SPACE_1 * 0.5)
            .padding([SPACE_1, SPACE_2]);
        mouse_area(container(scrollable(list).height(Length::Fill)).height(Length::Fill))
            .on_press(Message::Vault(VaultMessage::SidebarSelectionCleared))
            .on_right_press(Message::Shell(ShellMessage::OpenContextMenu(
                ContextMenuTarget::SidebarEmpty,
                props.cursor_pos.0,
                props.cursor_pos.1,
            )))
            .into()
    };

    container(panel(
        "Files",
        None,
        actions,
        column![filter, body_content].height(Length::Fill),
        PanelSide::Left,
        props.focused,
    ))
    .width(Length::Fixed(props.width))
    .height(Length::Fill)
    .into()
}

fn render_filtered<'a>(
    entries: &'a [md_editor_core::types::FileEntry],
    query: &str,
    selected_path: Option<&str>,
    active_path: Option<&str>,
    cursor_pos: (f32, f32),
) -> Vec<Element<'a, Message, Theme, Renderer>> {
    let mut matched: Vec<&md_editor_core::types::FileEntry> = entries
        .iter()
        .filter(|e| !e.is_dir && e.path.to_lowercase().contains(query))
        .collect();
    matched.sort_by_key(|a| a.path.to_lowercase());

    matched
        .into_iter()
        .map(|entry| {
            let is_selected = selected_path == Some(entry.path.as_str());
            let is_active = active_path == Some(entry.path.as_str());
            let path = entry.path.clone();
            tree_row(TreeRowProps {
                name: &entry.path,
                full_path: &entry.path,
                depth: 0,
                is_dir: false,
                expanded: false,
                selected: is_selected,
                active_doc: is_active,
                focused: false,
                drag_over: false,
                max_chars: 48,
                on_click: Message::Vault(VaultMessage::SidebarFileClicked(path.clone())),
                on_middle_click: Some(Message::Tabs(TabsMessage::OpenBackground(path.clone()))),
                on_right_click: Some(Message::Shell(ShellMessage::OpenContextMenu(
                    ContextMenuTarget::SidebarEntry {
                        path: path.clone(),
                        is_dir: false,
                    },
                    cursor_pos.0,
                    cursor_pos.1,
                ))),
                actions: build_row_actions(&path, is_selected),
            })
        })
        .collect()
}

fn render_tree_level<'a>(
    entries: &'a [md_editor_core::types::FileEntry],
    prefix: &str,
    depth: usize,
    selected_path: Option<&str>,
    active_path: Option<&str>,
    expanded: &'a BTreeSet<String>,
    cursor_pos: (f32, f32),
) -> Vec<Element<'a, Message, Theme, Renderer>> {
    let mut elements = Vec::new();
    let mut immediate_children = Vec::new();
    let mut seen = BTreeSet::new();

    for entry in entries {
        if !entry.path.starts_with(prefix) || entry.path == prefix {
            continue;
        }
        let Some(relative) = entry.path.strip_prefix(prefix) else {
            continue;
        };
        let relative = relative.trim_start_matches('/');
        let first_part = relative
            .split_once('/')
            .map_or(relative, |(first, _)| first);
        if !seen.insert(first_part) {
            continue;
        }
        let is_dir = relative.contains('/') || entry.is_dir;
        let child_path = if prefix.is_empty() {
            first_part.to_string()
        } else {
            format!("{prefix}/{first_part}")
        };
        immediate_children.push((first_part, child_path, is_dir));
    }

    immediate_children.sort_by(|a, b| match (a.2, b.2) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
    });

    for (name, path, is_dir) in immediate_children {
        let is_selected = selected_path == Some(path.as_str());
        let is_active = active_path == Some(path.as_str());
        let is_expanded = is_dir && expanded.contains(&path);

        let on_click = if is_dir {
            Message::Vault(VaultMessage::SidebarFolderToggled(path.clone()))
        } else {
            Message::Vault(VaultMessage::SidebarFileClicked(path.clone()))
        };
        let full_path = entries
            .iter()
            .find(|e| e.path == path)
            .map_or(name, |e| e.path.as_str());

        elements.push(tree_row(TreeRowProps {
            name,
            full_path,
            depth,
            is_dir,
            expanded: is_expanded,
            selected: is_selected,
            active_doc: is_active,
            focused: false,
            drag_over: false,
            max_chars: 40,
            on_click,
            on_middle_click: (!is_dir)
                .then(|| Message::Tabs(TabsMessage::OpenBackground(path.clone()))),
            on_right_click: Some(Message::Shell(ShellMessage::OpenContextMenu(
                ContextMenuTarget::SidebarEntry {
                    path: path.clone(),
                    is_dir,
                },
                cursor_pos.0,
                cursor_pos.1,
            ))),
            actions: build_row_actions(&path, is_selected),
        }));

        if is_expanded {
            elements.extend(render_tree_level(
                entries,
                &path,
                depth + 1,
                selected_path,
                active_path,
                expanded,
                cursor_pos,
            ));
        }
    }

    elements
}

fn build_row_actions<'a>(
    path: &str,
    is_selected: bool,
) -> Vec<Element<'a, Message, Theme, Renderer>> {
    if !is_selected {
        return Vec::new();
    }
    vec![
        IconButton::new(Icon::Pencil, "Rename")
            .size(ControlSize::Sm)
            .on_press(Message::Vault(VaultMessage::RenameEntryDialog(
                path.to_string(),
            )))
            .build(),
        IconButton::new(Icon::Trash, "Delete")
            .size(ControlSize::Sm)
            .danger(true)
            .on_press(Message::Vault(VaultMessage::DeleteFileDialog(
                path.to_string(),
            )))
            .build(),
    ]
}

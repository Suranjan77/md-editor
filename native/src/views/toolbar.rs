//! Top application toolbar: sidebar toggle, clickable path breadcrumbs, save badge and grouped action buttons.

use iced::widget::{Row, Space, button, row, text};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use crate::design::metrics::{RADIUS_SM, SPACE_0, SPACE_1, SPACE_2, SPACE_3};
use crate::design::{TextRole, tokens};
use crate::messages::{
    EditorMessage, GraphMessage, InkMessage, Message, SearchMessage, SettingsMessage, ShellMessage,
    Shortcut, VaultMessage,
};
use crate::shortcuts;
use crate::ui::{
    BadgeTone, Button, ButtonVariant, Icon, IconButton, badge, toolbar_frame, toolbar_separator,
};

/// Everything the top toolbar renders from.
pub struct ToolbarProps<'a> {
    pub active_path: Option<&'a str>,
    pub active_pdf_path: Option<&'a str>,
    pub dirty: bool,
    pub saving: bool,
    pub save_failed: bool,
    pub sidebar_visible: bool,
    pub backlinks_visible: bool,
    pub graph_visible: bool,
    pub tracker_visible: bool,
    pub settings_visible: bool,
    pub toc_visible: bool,
    pub toc_available: bool,
    pub split_view_active: bool,
    pub split_available: bool,
    pub ink_visible: bool,
    pub region_focused: bool,
}

pub fn view<'a>(props: ToolbarProps<'a>) -> Element<'a, Message, Theme, Renderer> {
    let sidebar_btn = IconButton::new(Icon::LayoutPanelLeft, "Toggle sidebar")
        .shortcut(shortcuts::chord(Shortcut::ToggleSidebar))
        .selected(props.sidebar_visible)
        .on_press(Message::Vault(VaultMessage::SidebarToggle))
        .build();

    let breadcrumbs = build_breadcrumbs(props.active_path.or(props.active_pdf_path));

    let status_pill: Element<'a, Message, Theme, Renderer> = if props.save_failed {
        row![
            badge("Save failed", BadgeTone::Danger, None),
            Button::new("Retry")
                .variant(ButtonVariant::Secondary)
                .icon(Icon::Redo)
                .on_press(Message::Shell(ShellMessage::RetrySave))
                .build(),
        ]
        .spacing(SPACE_2)
        .align_y(Alignment::Center)
        .into()
    } else if props.dirty {
        badge(
            if props.saving { "Saving…" } else { "Unsaved" },
            BadgeTone::Warning,
            None,
        )
    } else if props.active_path.is_some() {
        badge("Saved", BadgeTone::Neutral, None)
    } else {
        Space::new().width(Length::Fixed(SPACE_0)).into()
    };

    let nav_group = row![
        IconButton::new(Icon::Search, "Search vault")
            .shortcut(shortcuts::chord(Shortcut::GlobalSearch))
            .on_press(Message::Search(SearchMessage::GlobalSearchOpen))
            .build(),
        IconButton::new(Icon::Command, "Command palette")
            .shortcut(shortcuts::chord(Shortcut::CommandPalette))
            .on_press(Message::Shell(ShellMessage::CommandPaletteOpen))
            .build(),
    ]
    .spacing(SPACE_1)
    .align_y(Alignment::Center);

    let mut panels_row = Row::new().spacing(SPACE_1).align_y(Alignment::Center);
    if props.split_available {
        panels_row = panels_row.push(
            IconButton::new(Icon::Split, "Split view")
                .shortcut(shortcuts::chord(Shortcut::SplitView))
                .selected(props.split_view_active)
                .on_press(Message::Shell(ShellMessage::SplitViewToggle))
                .build(),
        );
    }
    if props.toc_available {
        panels_row = panels_row.push(
            IconButton::new(Icon::ListTree, "Outline")
                .shortcut(shortcuts::chord(Shortcut::TableOfContents))
                .selected(props.toc_visible)
                .on_press(Message::Editor(EditorMessage::ToggleTOC))
                .build(),
        );
    }
    panels_row = panels_row.push(
        IconButton::new(Icon::Link, "Backlinks")
            .shortcut(shortcuts::chord(Shortcut::ToggleBacklinks))
            .selected(props.backlinks_visible)
            .on_press(Message::KeyboardShortcut(Shortcut::ToggleBacklinks))
            .build(),
    );

    let workspaces_group = row![
        IconButton::new(Icon::Network, "Knowledge graph")
            .shortcut(shortcuts::chord(Shortcut::KnowledgeGraph))
            .selected(props.graph_visible)
            .on_press(Message::Graph(GraphMessage::Toggle))
            .build(),
        IconButton::new(Icon::Pen, "Handwriting canvas")
            .shortcut(shortcuts::chord(Shortcut::Handwriting))
            .selected(props.ink_visible)
            .on_press(Message::Ink(InkMessage::Toggle))
            .build(),
        IconButton::new(Icon::Clock, "Study tracker")
            .shortcut(shortcuts::chord(Shortcut::StudyTracker))
            .selected(props.tracker_visible)
            .on_press(Message::Shell(ShellMessage::TrackerToggle))
            .build(),
    ]
    .spacing(SPACE_1)
    .align_y(Alignment::Center);

    let app_group = row![
        IconButton::new(Icon::Gear, "Settings")
            .shortcut(shortcuts::chord(Shortcut::Settings))
            .selected(props.settings_visible)
            .on_press(Message::Settings(SettingsMessage::Toggle))
            .build(),
    ]
    .spacing(SPACE_1)
    .align_y(Alignment::Center);

    toolbar_frame(
        vec![
            sidebar_btn,
            toolbar_separator(),
            breadcrumbs,
            status_pill,
            Space::new().width(Length::Fill).into(),
            nav_group.into(),
            toolbar_separator(),
            panels_row.into(),
            toolbar_separator(),
            workspaces_group.into(),
            toolbar_separator(),
            app_group.into(),
        ],
        props.region_focused,
    )
}

fn build_breadcrumbs<'a>(path: Option<&'a str>) -> Element<'a, Message, Theme, Renderer> {
    let t = tokens();
    let Some(path) = path else {
        return text("No document open")
            .size(TextRole::Caption.size())
            .color(t.fg_tertiary)
            .font(TextRole::Caption.font())
            .into();
    };

    let parts: Vec<&str> = path.split('/').collect();
    let mut row = Row::new().spacing(SPACE_1).align_y(Alignment::Center);
    let mut accum = String::new();

    for (idx, part) in parts.iter().enumerate() {
        if idx > 0 {
            accum.push('/');
            row = row.push(
                text("/")
                    .size(TextRole::Caption.size())
                    .color(t.fg_tertiary)
                    .font(TextRole::Caption.font()),
            );
        }
        accum.push_str(part);
        let is_last = idx + 1 == parts.len();
        if is_last {
            row = row.push(
                text(*part)
                    .size(TextRole::Caption.size())
                    .color(t.fg_primary)
                    .font(crate::design::fonts::UI_FONT_MEDIUM),
            );
        } else {
            let folder_path = accum.clone();
            let crumb = button(
                text(*part)
                    .size(TextRole::Caption.size())
                    .color(t.fg_secondary)
                    .font(TextRole::Caption.font()),
            )
            .padding([SPACE_1 * 0.5, SPACE_1])
            .on_press(Message::Vault(VaultMessage::SidebarFolderToggled(
                folder_path,
            )))
            .style(move |_, status| {
                let bg = matches!(status, button::Status::Hovered | button::Status::Pressed)
                    .then_some(Background::Color(t.bg_raised));
                button::Style {
                    background: bg,
                    text_color: t.fg_secondary,
                    border: Border {
                        radius: RADIUS_SM.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            });
            row = row.push(crumb);
        }
        let _ = SPACE_3;
    }

    row.into()
}

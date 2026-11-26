//! The panes area: each pane's tab strip and sheet built from its own state, the divider, and the empty pane.

use super::workspace::{PaneId, SplitAxis};
use super::*;
use crate::design::metrics::{
    BORDER_WIDTH, ControlSize, Elevation, SHEET_GUTTER, SHEET_RADIUS, SPACE_0, SPACE_2,
};
use crate::design::tokens;
use crate::messages::{
    ContextMenuTarget, EditorMessage, GraphMessage, Message, ShellMessage, Shortcut, TabsMessage,
    VaultMessage,
};
use crate::shortcuts;
use crate::tabs::TabKind;
use crate::ui::{
    Button, ButtonVariant, DocTabItem, Icon, IconButton, TabStripEvents, empty_state, split_handle,
    split_handle_across, tab_strip,
};
use iced::widget::column;

/// A pane's state, borrowed for building its view.
pub(super) struct PaneView<'a> {
    pub id: PaneId,
    /// Has the keyboard: find bars and drafts show only here.
    pub focused: bool,
    pub tabs: &'a crate::tabs::Tabs,
    pub active_path: Option<&'a str>,
    pub editor: &'a crate::editor_state::EditorPane,
    pub pdf: &'a crate::pdf_pane::PdfPane,
    pub active_image_path: Option<&'a str>,
    pub active_image: Option<&'a (iced::widget::image::Handle, f32, f32)>,
    pub showing_pdf: bool,
    pub image_zoom: Option<f32>,
}

/// An empty page index for a pane find is not running in.
pub(super) static NO_PAGE_INDEX: std::sync::LazyLock<std::collections::HashMap<u16, Vec<usize>>> =
    std::sync::LazyLock::new(std::collections::HashMap::new);

impl MdEditor {
    /// Toggles at the trailing end of a pane's tab strip. The focused pane's
    /// strip carries the document toggles; with two panes, each strip can
    /// close its pane.
    fn document_actions<'a>(
        &'a self,
        pane: &PaneView<'a>,
    ) -> Vec<Element<'a, Message, Theme, iced::Renderer>> {
        let mut actions = Vec::new();
        if self.workspace.is_split() {
            actions.push(
                IconButton::new(Icon::Split, "Unsplit: close this pane, its tabs move over")
                    .shortcut(shortcuts::chord(Shortcut::SplitView))
                    .size(ControlSize::Sm)
                    .selected(true)
                    .on_press(Message::Shell(ShellMessage::ClosePane(pane.id)))
                    .build(),
            );
        } else {
            actions.push(
                IconButton::new(Icon::Split, "Split")
                    .shortcut(shortcuts::chord(Shortcut::SplitView))
                    .size(ControlSize::Sm)
                    .on_press(Message::Shell(ShellMessage::SplitViewToggle))
                    .build(),
            );
        }
        if !pane.focused {
            return actions;
        }
        if pane.active_path.is_some() || pane.pdf.active_path.is_some() {
            actions.push(
                IconButton::new(Icon::ListTree, "Outline")
                    .shortcut(shortcuts::chord(Shortcut::TableOfContents))
                    .size(ControlSize::Sm)
                    .selected(self.editor.toc_visible)
                    .on_press(Message::Editor(EditorMessage::ToggleTOC))
                    .build(),
            );
        }
        actions.push(
            IconButton::new(Icon::Link, "Backlinks")
                .shortcut(shortcuts::chord(Shortcut::ToggleBacklinks))
                .size(ControlSize::Sm)
                .selected(self.vault.backlinks_visible)
                .on_press(Message::KeyboardShortcut(Shortcut::ToggleBacklinks))
                .build(),
        );
        actions
    }

    fn build_tab_strip<'a>(
        &'a self,
        pane: &PaneView<'a>,
    ) -> Element<'a, Message, Theme, iced::Renderer> {
        if pane.tabs.is_empty() && !self.workspace.is_split() {
            return container(Space::new())
                .height(Length::Fixed(SPACE_0))
                .into();
        }
        let active_id = pane.tabs.active_id();
        let items: Vec<DocTabItem> = pane
            .tabs
            .list()
            .iter()
            .map(|tab| DocTabItem {
                tab,
                label: pane.tabs.label_for(tab),
                dirty: tab.kind == TabKind::Note
                    && pane.active_path == Some(tab.path.as_str())
                    && pane.editor.buffer.dirty,
                // The other pane's active tab is shown as active, dimmed by
                // the strip itself not being focused.
                is_active: active_id == Some(tab.id) && pane.focused,
                is_split_secondary: active_id == Some(tab.id) && !pane.focused,
            })
            .collect();

        let cursor_pos = self.ui.cursor_pos;
        tab_strip(
            items,
            self.ui.dragging_tab.filter(|_| pane.focused),
            self.ui.tab_overflow_open && pane.focused,
            TabStripEvents {
                on_activate: Box::new(|id| Message::Tabs(TabsMessage::Activate(id))),
                on_close: Box::new(|id| Message::Tabs(TabsMessage::Close(id))),
                on_drag_start: Box::new(|id| Message::Tabs(TabsMessage::DragStart(id))),
                on_drag_over: Box::new(|idx| Message::Tabs(TabsMessage::DragOver(idx))),
                on_drag_end: Message::Tabs(TabsMessage::DragEnd),
                on_context_menu: Box::new(move |id| {
                    Message::Shell(ShellMessage::OpenContextMenu(
                        ContextMenuTarget::Tab(id),
                        cursor_pos.0,
                        cursor_pos.1,
                    ))
                }),
                on_new_tab: Message::Vault(VaultMessage::CreateFileDialog),
                on_toggle_overflow: Message::Tabs(TabsMessage::ToggleOverflowMenu),
            },
            self.document_actions(pane),
        )
    }

    /// The panes area: one pane, or two split by a draggable divider. Each
    /// pane's messages are tagged with it (`Message::FromPane`).
    pub(super) fn build_workspace(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        if self.ui.settings_visible {
            return self.sheet(
                views::settings::view(&self.ui, self.vault.root.as_deref()),
                false,
            );
        }
        if self.graph.visible {
            return self.sheet(views::graph::view(&self.graph), false);
        }
        if self.tracker.visible {
            return self.sheet(self.tracker.view(), false);
        }
        let panes: Vec<Element<'_, Message, Theme, iced::Renderer>> = self
            .workspace
            .panes()
            .into_iter()
            .filter_map(|id| self.pane_view(id))
            .map(|pane| {
                let id = pane.id;
                let has_tabs = !pane.tabs.is_empty() || self.workspace.is_split();
                let highlight = self.workspace.is_split() && pane.focused;
                let column = column![
                    self.build_tab_strip(&pane),
                    self.sheet(self.build_pane_content(&pane), highlight)
                ]
                .spacing(SPACE_0)
                .width(Length::Fill)
                .height(Length::Fill);
                let element: Element<'_, Message, Theme, iced::Renderer> = container(column)
                    .padding(iced::Padding {
                        top: if has_tabs { SPACE_0 } else { SHEET_GUTTER },
                        ..iced::Padding::ZERO
                    })
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
                element.map(move |message| Message::FromPane(id, Box::new(message)))
            })
            .collect();
        let mut panes = panes.into_iter();
        let Some(first) = panes.next() else {
            return Space::new().into();
        };
        let Some(second) = panes.next() else {
            return first;
        };
        let first_portion = (self.ui.split_ratio * 1000.0).round() as u16;
        let second_portion = 1000u16.saturating_sub(first_portion);
        let dragging = self.ui.is_resizing_split;
        let handle = |axis| {
            let (start, end, reset) = (
                Message::Shell(ShellMessage::SplitViewDragStart),
                Message::Shell(ShellMessage::SplitViewDragEnd),
                Message::Shell(ShellMessage::SplitViewReset),
            );
            match axis {
                SplitAxis::Vertical => split_handle(dragging, start, end, reset),
                SplitAxis::Horizontal => split_handle_across(dragging, start, end, reset),
            }
        };
        let area: Element<'_, Message, Theme, iced::Renderer> = match self.workspace.axis {
            SplitAxis::Vertical => row![
                container(first).width(Length::FillPortion(first_portion)),
                handle(SplitAxis::Vertical),
                container(second).width(Length::FillPortion(second_portion)),
            ]
            .spacing(SPACE_0)
            .height(Length::Fill)
            .into(),
            SplitAxis::Horizontal => column![
                container(first).height(Length::FillPortion(first_portion)),
                handle(SplitAxis::Horizontal),
                container(second).height(Length::FillPortion(second_portion)),
            ]
            .spacing(SPACE_0)
            .width(Length::Fill)
            .into(),
        };
        // The divider turns pointer travel into a ratio of this measured size.
        iced::widget::sensor(area)
            .on_show(|size| Message::Shell(ShellMessage::WorkspaceMeasured(size)))
            .on_resize(|size| Message::Shell(ShellMessage::WorkspaceMeasured(size)))
            .into()
    }

    /// The rounded sheet a pane's document floats on. `focused` outlines it,
    /// to show which of two panes has the keyboard.
    fn sheet<'a>(
        &self,
        content: Element<'a, Message, Theme, iced::Renderer>,
        focused: bool,
    ) -> Element<'a, Message, Theme, iced::Renderer> {
        let t = tokens();
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(t.bg_canvas)),
                border: iced::Border {
                    color: if focused {
                        t.accent_muted
                    } else {
                        t.border_subtle
                    },
                    width: BORDER_WIDTH,
                    radius: SHEET_RADIUS.into(),
                },
                shadow: Elevation::Sheet.shadow(),
                ..Default::default()
            })
            .into()
    }

    /// Borrow `id`'s state: the swapped-in fields, or the parked bundle.
    pub(super) fn pane_view(&self, id: PaneId) -> Option<PaneView<'_>> {
        let focused = id == self.workspace.focused;
        if id == self.workspace.current {
            return Some(PaneView {
                id,
                focused,
                tabs: &self.tabs,
                active_path: self.active_path.as_deref(),
                editor: &self.editor,
                pdf: &self.pdf,
                active_image_path: self.active_image_path.as_deref(),
                active_image: self.active_image.as_ref(),
                showing_pdf: self.showing_pdf,
                image_zoom: self.ui.image_zoom,
            });
        }
        let docs = self.parked()?;
        Some(PaneView {
            id,
            focused,
            tabs: &docs.tabs,
            active_path: docs.active_path.as_deref(),
            editor: &docs.editor,
            pdf: &docs.pdf,
            active_image_path: docs.active_image_path.as_deref(),
            active_image: docs.active_image.as_ref(),
            showing_pdf: docs.showing_pdf,
            image_zoom: docs.image_zoom,
        })
    }

    /// The document a pane shows, or what to do in an empty one.
    fn build_pane_content<'a>(
        &'a self,
        pane: &PaneView<'a>,
    ) -> Element<'a, Message, Theme, iced::Renderer> {
        if pane.showing_pdf && pane.pdf.active_path.is_some() {
            return self.build_pdf_view(pane);
        }
        if pane.active_image.is_some() {
            return self.build_image_view(pane);
        }
        if pane.active_path.is_some() {
            return self.build_editor_view(pane);
        }
        if self.workspace.is_split() {
            return empty_state(
                Icon::FileText,
                "Empty pane",
                "Open a file from the sidebar or the Command Palette and it opens here.",
                None,
            );
        }

        // Empty tab state (all tabs closed or fresh vault before opening a file).
        empty_state(
            Icon::FileText,
            "No document open",
            "Select a file from the sidebar, open the Command Palette, or create a new note.",
            Some(
                row![
                    Button::new("New Note")
                        .variant(ButtonVariant::Primary)
                        .icon(Icon::FilePlus)
                        .on_press(Message::Vault(VaultMessage::CreateFileDialog))
                        .build(),
                    Button::new("Command Palette")
                        .variant(ButtonVariant::Secondary)
                        .icon(Icon::Command)
                        .on_press(Message::Shell(ShellMessage::CommandPaletteOpen))
                        .build(),
                    Button::new("Knowledge Graph")
                        .variant(ButtonVariant::Secondary)
                        .icon(Icon::Network)
                        .on_press(Message::Graph(GraphMessage::Toggle))
                        .build(),
                ]
                .spacing(SPACE_2)
                .into(),
            ),
        )
    }
}

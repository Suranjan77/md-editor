use crate::messages::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivePanel {
    Markdown,
    Pdf,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum ShellMessage {
    SidebarToggle,
    ToggleToc,
    TocClicked(usize),
    SplitViewToggle,
    SplitViewDragStart,
    SplitViewDragging(f32),
    SplitViewDragEnd,
    SplitViewDividerHovered(bool),
    WindowResized(f32, f32),
    KeyboardModifiersChanged(iced::keyboard::Modifiers),
}

#[allow(dead_code, non_snake_case, non_upper_case_globals)]
impl Message {
    pub(crate) const SidebarToggle: Self = Self::Shell(ShellMessage::SidebarToggle);
    pub(crate) const ToggleTOC: Self = Self::Shell(ShellMessage::ToggleToc);
    pub(crate) const SplitViewToggle: Self = Self::Shell(ShellMessage::SplitViewToggle);
    pub(crate) const SplitViewDragStart: Self = Self::Shell(ShellMessage::SplitViewDragStart);
    pub(crate) const SplitViewDragEnd: Self = Self::Shell(ShellMessage::SplitViewDragEnd);

    pub(crate) fn TocClicked(index: usize) -> Self {
        Self::Shell(ShellMessage::TocClicked(index))
    }

    pub(crate) fn SplitViewDragging(position: f32) -> Self {
        Self::Shell(ShellMessage::SplitViewDragging(position))
    }

    pub(crate) fn SplitViewDividerHovered(hovered: bool) -> Self {
        Self::Shell(ShellMessage::SplitViewDividerHovered(hovered))
    }

    pub(crate) fn WindowResized(width: f32, height: f32) -> Self {
        Self::Shell(ShellMessage::WindowResized(width, height))
    }

    pub(crate) fn KeyboardModifiersChanged(modifiers: iced::keyboard::Modifiers) -> Self {
        Self::Shell(ShellMessage::KeyboardModifiersChanged(modifiers))
    }
}

pub(crate) struct ShellState {
    pub(crate) sidebar_visible: bool,
    pub(crate) toc_visible: bool,
    pub(crate) pdf_annotations_visible: bool,
    pub(crate) split_view_active: bool,
    pub(crate) split_ratio: f32,
    pub(crate) is_resizing_split: bool,
    pub(crate) pdf_split_ratio: f32,
    pub(crate) active_panel: ActivePanel,
    pub(crate) keyboard_modifiers: iced::keyboard::Modifiers,
    pub(crate) window_width: f32,
    pub(crate) window_height: f32,
}

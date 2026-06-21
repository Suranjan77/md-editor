//! Shell UI chrome sub-state.
//!
//! Groups the assorted top-level UI fields that aren't tied to a content
//! domain: the active modal, the command palette, the transient toast, the
//! split-view divider, and the current window geometry. These have no shared
//! behavior beyond living on the shell, so this is a plain field container the
//! shell reads and writes directly.
//!
//! Part of the `MdEditor` decomposition; see
//! `docs/refactor-mdeditor-decomposition.md`.

use crate::views;

pub struct UiState {
    // Modals
    pub active_modal: Option<views::modals::ModalType>,
    pub modal_input: String,
    pub link_note_picker_search: String,

    // Command palette
    pub command_palette_visible: bool,
    pub command_palette_query: String,
    pub commands: Vec<views::command_palette::Command>,

    // Transient toast
    pub toast: Option<String>,

    // Split view
    pub split_view_active: bool,
    pub split_ratio: f32,
    pub is_resizing_split: bool,

    // Window geometry
    pub window_width: f32,
    pub window_height: f32,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            active_modal: None,
            modal_input: String::new(),
            link_note_picker_search: String::new(),
            command_palette_visible: false,
            command_palette_query: String::new(),
            commands: views::command_palette::get_commands(),
            toast: None,
            split_view_active: false,
            split_ratio: 0.5,
            is_resizing_split: false,
            window_width: 1200.0,
            window_height: 800.0,
        }
    }
}

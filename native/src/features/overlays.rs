use crate::messages::{CitationItem, Message, Shortcut};
use crate::views::modals::ModalType;

#[derive(Debug, Clone)]
pub(crate) enum OverlayMessage {
    CommandPaletteOpen,
    CommandPaletteQueryChanged(String),
    CommandPaletteCommandClicked(Shortcut),
    NameModalInputChanged(String),
    NameModalSubmit(String),
    NameModalSubmitCurrent,
    NameModalCancel,
    ToastHide,
}

#[allow(non_snake_case, non_upper_case_globals)]
impl Message {
    pub(crate) const CommandPaletteOpen: Self = Self::Overlay(OverlayMessage::CommandPaletteOpen);
    pub(crate) const NameModalSubmitCurrent: Self =
        Self::Overlay(OverlayMessage::NameModalSubmitCurrent);
    pub(crate) const NameModalCancel: Self = Self::Overlay(OverlayMessage::NameModalCancel);
    pub(crate) const ToastHide: Self = Self::Overlay(OverlayMessage::ToastHide);

    pub(crate) fn CommandPaletteQueryChanged(query: String) -> Self {
        Self::Overlay(OverlayMessage::CommandPaletteQueryChanged(query))
    }

    pub(crate) fn CommandPaletteCommandClicked(shortcut: Shortcut) -> Self {
        Self::Overlay(OverlayMessage::CommandPaletteCommandClicked(shortcut))
    }

    pub(crate) fn NameModalInputChanged(input: String) -> Self {
        Self::Overlay(OverlayMessage::NameModalInputChanged(input))
    }

    pub(crate) fn NameModalSubmit(input: String) -> Self {
        Self::Overlay(OverlayMessage::NameModalSubmit(input))
    }
}

#[derive(Debug, Default)]
pub(crate) struct OverlayState {
    pub(crate) active_modal: Option<ModalType>,
    pub(crate) modal_input: String,
    pub(crate) link_note_picker_search: String,
    pub(crate) command_palette_visible: bool,
    pub(crate) command_palette_query: String,
    pub(crate) citation_palette_visible: bool,
    pub(crate) citation_palette_query: String,
    pub(crate) excerpt_mode_active: bool,
    pub(crate) excerpts_queue: Vec<CitationItem>,
    pub(crate) toast: Option<String>,
}

impl OverlayState {
    pub(crate) fn close_modal(&mut self) {
        self.active_modal = None;
        self.modal_input.clear();
        self.link_note_picker_search.clear();
    }

    pub(crate) fn close_command_palette(&mut self) {
        self.command_palette_visible = false;
        self.command_palette_query.clear();
    }

    pub(crate) fn close_citation_palette(&mut self) {
        self.citation_palette_visible = false;
        self.citation_palette_query.clear();
    }

    #[cfg(test)]
    fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_modal_clears_modal_inputs_without_touching_other_overlays() {
        let mut state = OverlayState {
            active_modal: Some(ModalType::CreateFile),
            modal_input: "note.md".to_string(),
            link_note_picker_search: "note".to_string(),
            command_palette_visible: true,
            toast: Some("Saved".to_string()),
            ..OverlayState::default()
        };

        state.close_modal();

        assert!(state.active_modal.is_none());
        assert!(state.modal_input.is_empty());
        assert!(state.link_note_picker_search.is_empty());
        assert!(state.command_palette_visible);
        assert_eq!(state.toast.as_deref(), Some("Saved"));
    }

    #[test]
    fn palette_close_clears_only_matching_palette() {
        let mut state = OverlayState {
            command_palette_visible: true,
            command_palette_query: "open".to_string(),
            citation_palette_visible: true,
            citation_palette_query: "smith".to_string(),
            ..OverlayState::default()
        };

        state.close_command_palette();

        assert!(!state.command_palette_visible);
        assert!(state.command_palette_query.is_empty());
        assert!(state.citation_palette_visible);
        assert_eq!(state.citation_palette_query, "smith");

        state.close_citation_palette();

        assert!(!state.citation_palette_visible);
        assert!(state.citation_palette_query.is_empty());
    }

    #[test]
    fn reset_clears_all_overlay_state() {
        let mut state = OverlayState {
            active_modal: Some(ModalType::CreateFolder),
            modal_input: "folder".to_string(),
            link_note_picker_search: "link".to_string(),
            command_palette_visible: true,
            command_palette_query: "command".to_string(),
            citation_palette_visible: true,
            citation_palette_query: "citation".to_string(),
            excerpt_mode_active: true,
            excerpts_queue: vec![CitationItem::Selection {
                text: "quote".to_string(),
                page_index: 0,
            }],
            toast: Some("Queued".to_string()),
        };

        state.reset();

        assert!(state.active_modal.is_none());
        assert!(state.modal_input.is_empty());
        assert!(state.link_note_picker_search.is_empty());
        assert!(!state.command_palette_visible);
        assert!(state.command_palette_query.is_empty());
        assert!(!state.citation_palette_visible);
        assert!(state.citation_palette_query.is_empty());
        assert!(!state.excerpt_mode_active);
        assert!(state.excerpts_queue.is_empty());
        assert!(state.toast.is_none());
    }
}

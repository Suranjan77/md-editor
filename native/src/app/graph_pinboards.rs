//! Saved graph pinboard handlers: opening dialogs, saving, loading, and deleting pinboards.

use super::*;
use crate::messages::GraphMessage;
use crate::ui::ToastKind;

impl MdEditor {
    pub(super) fn open_pinboards_modal(&mut self) -> Task<Message> {
        if self.vault.root.is_none() {
            self.ui.notify(
                ToastKind::Warning,
                "Open a vault to manage pinboards",
                false,
            );
            return Task::none();
        }
        self.ui.active_modal = Some(views::modals::ModalType::Pinboards);
        self.ui.modal_input.clear();
        self.ui.modal_error = None;
        self.ui.modal_parent_override = None;
        self.ui.link_note_picker_search.clear();
        self.ui.context_menu = None;
        self.ui.tab_overflow_open = false;
        Task::none()
    }

    pub(super) fn open_save_pinboard_modal(&mut self) -> Task<Message> {
        if self.vault.root.is_none() {
            self.ui.notify(
                ToastKind::Warning,
                "Open a vault before saving a pinboard",
                false,
            );
            return Task::none();
        }
        self.ui.active_modal = Some(views::modals::ModalType::SavePinboard);
        self.ui.modal_input.clear();
        self.ui.modal_error = None;
        self.ui.modal_parent_override = None;
        self.ui.link_note_picker_search.clear();
        self.ui.context_menu = None;
        self.ui.tab_overflow_open = false;
        iced::widget::operation::focus(iced::advanced::widget::Id::new("modal_name_input"))
    }

    pub(super) fn submit_save_pinboard_modal(&mut self, input: &str) -> Task<Message> {
        let Some(vault_root) = self.vault.root.clone() else {
            self.ui.modal_error = Some("Open a vault before saving a pinboard".to_string());
            return Task::none();
        };
        let board = self.graph.capture_pinboard(input);
        let name = board.name.clone();
        match self.pinboards.save(board) {
            Ok(()) => {
                self.pinboards.persist(&self.state, &vault_root);
                self.ui.active_modal = None;
                self.ui.modal_input.clear();
                self.ui.modal_error = None;
                self.ui.notify(
                    ToastKind::Success,
                    format!("Saved pinboard '{name}'"),
                    false,
                );
                Task::none()
            }
            Err(err) => {
                self.ui.modal_error = Some(err);
                Task::none()
            }
        }
    }

    pub(super) fn load_pinboard(&mut self, name: &str) -> Task<Message> {
        let Some(board) = self.pinboards.get(name).cloned() else {
            self.ui.notify(
                ToastKind::Warning,
                format!("Pinboard '{name}' not found"),
                false,
            );
            return Task::none();
        };
        self.graph.apply_pinboard(&board);
        self.ui.active_modal = None;
        self.ui.modal_input.clear();
        self.ui.modal_error = None;
        self.ui.notify(
            ToastKind::Info,
            format!("Loaded pinboard '{}'", board.name),
            false,
        );
        Task::none()
    }

    pub(super) fn delete_pinboard(&mut self, name: &str) -> Task<Message> {
        let Some(vault_root) = self.vault.root.clone() else {
            return Task::none();
        };
        if self.pinboards.delete(name) {
            self.pinboards.persist(&self.state, &vault_root);
            self.ui
                .notify(ToastKind::Info, format!("Deleted pinboard '{name}'"), false);
        }
        let _ = GraphMessage::OpenPinboardsModal;
        Task::none()
    }
}

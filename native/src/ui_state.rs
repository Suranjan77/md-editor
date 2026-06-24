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

use iced::Task;

use crate::messages::Message;
use crate::pdf_notes::{normalize_note_path, note_filename_from_path};
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
    /// OS display scale factor (device pixels per logical pixel) for the
    /// window. Drives PDF supersampling so pages stay sharp on HiDPI/fractional
    /// displays without over-rendering on 1× screens. Defaults to 1.0 until the
    /// real value is reported by the window.
    pub scale_factor: f32,
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
            scale_factor: 1.0,
        }
    }

    /// Handle messages that mutate only this UI chrome state: the modal
    /// stack, the link-note picker, the command palette, the transient toast,
    /// and the split-view drag start. Cross-cutting arms (e.g. modal *submit*,
    /// which creates vault entries) stay on the shell.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // Modals
            Message::CreateFileDialog => {
                self.active_modal = Some(views::modals::ModalType::CreateFile);
                self.modal_input.clear();
                self.link_note_picker_search.clear();
                Task::none()
            }
            Message::CreateFolderDialog => {
                self.active_modal = Some(views::modals::ModalType::CreateFolder);
                self.modal_input.clear();
                self.link_note_picker_search.clear();
                Task::none()
            }
            Message::DeleteFileDialog(path) => {
                self.active_modal = Some(views::modals::ModalType::Delete(path));
                Task::none()
            }
            Message::NameModalInputChanged(input) => {
                self.modal_input = input;
                Task::none()
            }
            Message::PdfLinkNoteFolderSelected(folder) => {
                if matches!(self.active_modal, Some(views::modals::ModalType::LinkNote(_))) {
                    let filename = note_filename_from_path(&self.modal_input);
                    self.modal_input = if folder.is_empty() {
                        filename
                    } else {
                        format!("{}/{}", folder.trim_end_matches('/'), filename)
                    };
                }
                Task::none()
            }
            Message::PdfLinkNoteFileSelected(path) => {
                if matches!(self.active_modal, Some(views::modals::ModalType::LinkNote(_))) {
                    self.modal_input = normalize_note_path(&path);
                }
                Task::none()
            }
            Message::PdfLinkNotePickerSearchChanged(query) => {
                if matches!(self.active_modal, Some(views::modals::ModalType::LinkNote(_))) {
                    self.link_note_picker_search = query;
                }
                Task::none()
            }
            Message::NameModalCancel => {
                self.active_modal = None;
                self.modal_input.clear();
                self.link_note_picker_search.clear();
                Task::none()
            }
            Message::NameModalSubmitCurrent => {
                if matches!(
                    self.active_modal,
                    Some(views::modals::ModalType::CreateFile)
                        | Some(views::modals::ModalType::CreateFolder)
                        | Some(views::modals::ModalType::QuickNote(_))
                        | Some(views::modals::ModalType::LinkNote(_))
                ) {
                    Task::done(Message::NameModalSubmit(self.modal_input.clone()))
                } else {
                    Task::none()
                }
            }

            // Command palette
            Message::CommandPaletteOpen => {
                self.command_palette_visible = true;
                self.command_palette_query.clear();
                Task::none()
            }
            Message::CommandPaletteQueryChanged(query) => {
                self.command_palette_query = query;
                Task::none()
            }

            // Transient toast
            Message::ShowToast(text) => {
                self.toast = Some(text);
                Task::none()
            }
            Message::ToastHide => {
                self.toast = None;
                Task::none()
            }

            // Split view
            Message::SplitViewDragStart => {
                self.is_resizing_split = true;
                Task::none()
            }

            _ => Task::none(),
        }
    }
}

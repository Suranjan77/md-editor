//! Shell UI chrome sub-state: modals, command palette, toast queue, panels, context menus, drag-and-drop and settings.

use iced::Task;

use crate::design::metrics::{SIDE_PANEL_DEFAULT_WIDTH, SIDEBAR_DEFAULT_WIDTH};
use crate::messages::{ContextMenuTarget, Message, SettingsSection, ShellMessage, VaultMessage};
use crate::pdf_notes::{normalize_note_path, note_filename_from_path};
use crate::ui::{FocusRegion, ToastKind};
use crate::views;

/// Which side panel handle is currently being dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizingPanel {
    Sidebar,
    SidePanel,
}

/// A queued feedback toast.
#[derive(Debug, Clone)]
pub struct ToastEntry {
    pub id: u64,
    pub kind: ToastKind,
    pub text: String,
    pub persistent: bool,
    pub can_retry_save: bool,
}

/// Open right-click context menu and its anchor position in window coordinates.
#[derive(Debug, Clone)]
pub struct ActiveContextMenu {
    pub target: ContextMenuTarget,
    pub x: f32,
    pub y: f32,
}

pub struct UiState {
    // Modals & inline validation
    pub active_modal: Option<views::modals::ModalType>,
    pub modal_input: String,
    pub modal_error: Option<String>,
    pub modal_parent_override: Option<String>,
    pub link_note_picker_search: String,

    // Command palette
    pub command_palette_visible: bool,
    pub command_palette_query: String,
    pub commands: Vec<views::command_palette::Command>,
    pub palette_selected: usize,

    // Transient & persistent toasts (`toast` mirrors the newest message for e2e probes)
    pub toast: Option<String>,
    pub toast_queue: Vec<ToastEntry>,
    pub next_toast_id: u64,
    pub last_save_failed: bool,

    // Split view & resizable panel widths
    /// Pane A's share of the main area while split.
    pub split_ratio: f32,
    pub is_resizing_split: bool,
    /// Where a divider drag started (pointer along the split, ratio then).
    pub split_drag_origin: Option<(f32, f32)>,
    /// The panes area, as last measured.
    pub workspace_size: iced::Size,
    /// The last session was split: split again once the vault's tabs load.
    pub restore_split: bool,
    pub sidebar_width: f32,
    pub side_panel_width: f32,
    pub resizing_panel: Option<ResizingPanel>,

    // Context menu, tab overflow & drag-and-drop
    pub context_menu: Option<ActiveContextMenu>,
    pub tab_overflow_open: bool,
    pub dragging_tab: Option<crate::tabs::TabId>,
    pub cursor_pos: (f32, f32),
    pub hovered_drop_files: Vec<std::path::PathBuf>,

    // Settings view & preferences drafts
    pub settings_visible: bool,
    pub settings_section: SettingsSection,
    pub attachments_mode: crate::messages::AttachmentsMode,
    pub attachments_custom: String,
    pub excluded_folders: String,
    pub recent_vaults: Vec<String>,

    // Viewer toolbar editable drafts & image zoom
    pub pdf_page_input: Option<String>,
    pub pdf_zoom_input: Option<String>,
    pub image_zoom: Option<f32>,

    // Keyboard region focus (`F6` / `Shift+F6`)
    pub focused_region: FocusRegion,

    // Window geometry
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            active_modal: None,
            modal_input: String::new(),
            modal_error: None,
            modal_parent_override: None,
            link_note_picker_search: String::new(),
            command_palette_visible: false,
            command_palette_query: String::new(),
            commands: views::command_palette::get_commands(),
            palette_selected: 0,
            toast: None,
            toast_queue: Vec::new(),
            next_toast_id: 1,
            last_save_failed: false,
            split_ratio: 0.5,
            is_resizing_split: false,
            split_drag_origin: None,
            workspace_size: iced::Size::ZERO,
            restore_split: false,
            sidebar_width: SIDEBAR_DEFAULT_WIDTH,
            side_panel_width: SIDE_PANEL_DEFAULT_WIDTH,
            resizing_panel: None,
            context_menu: None,
            tab_overflow_open: false,
            dragging_tab: None,
            cursor_pos: (400.0, 300.0),
            hovered_drop_files: Vec::new(),
            settings_visible: false,
            settings_section: SettingsSection::Appearance,
            attachments_mode: crate::messages::AttachmentsMode::VaultRoot,
            attachments_custom: String::from("attachments"),
            excluded_folders: String::from("node_modules, target, build, dist, __pycache__"),
            recent_vaults: Vec::new(),
            pdf_page_input: None,
            pdf_zoom_input: None,
            image_zoom: None,
            focused_region: FocusRegion::Main,
            window_width: 1200.0,
            window_height: 800.0,
            scale_factor: 1.0,
        }
    }

    /// Push a typed toast and synchronize `self.toast` for animation/e2e probes.
    pub fn notify(&mut self, kind: ToastKind, text: impl Into<String>, can_retry_save: bool) {
        let text = text.into();
        let persistent = kind == ToastKind::Error;
        let id = self.next_toast_id;
        self.next_toast_id = self.next_toast_id.wrapping_add(1);
        self.toast = Some(text.clone());
        // Keep at most 3 visible toasts.
        if self.toast_queue.len() >= 3 {
            self.toast_queue.remove(0);
        }
        self.toast_queue.push(ToastEntry {
            id,
            kind,
            text,
            persistent,
            can_retry_save,
        });
    }

    /// Synchronize `toast_queue` when callers assign `self.ui.toast = Some(...)` directly.
    pub fn sync_legacy_toast(&mut self) {
        let Some(ref current) = self.toast else {
            self.toast_queue.retain(|t| t.persistent);
            return;
        };
        if self.toast_queue.last().is_some_and(|t| &t.text == current) {
            return;
        }
        let lower = current.to_lowercase();
        let kind = if [
            "fail",
            "error",
            "could not",
            "cannot",
            "can't",
            "unable",
            "not switching",
            "not closing",
            "not renaming",
        ]
        .iter()
        .any(|w| lower.contains(w))
        {
            ToastKind::Error
        } else if [
            "conflict",
            "large image",
            "skipped",
            "moved or deleted",
            "read-only",
        ]
        .iter()
        .any(|w| lower.contains(w))
        {
            ToastKind::Warning
        } else if ["saved", "created", "renamed", "deleted", "copied", "added"]
            .iter()
            .any(|w| lower.contains(w))
        {
            ToastKind::Success
        } else {
            ToastKind::Info
        };
        let id = self.next_toast_id;
        self.next_toast_id = self.next_toast_id.wrapping_add(1);
        if self.toast_queue.len() >= 3 {
            self.toast_queue.remove(0);
        }
        self.toast_queue.push(ToastEntry {
            id,
            kind,
            text: current.clone(),
            persistent: false,
            can_retry_save: self.last_save_failed && kind == ToastKind::Error,
        });
    }

    /// Dismiss a specific toast by id.
    pub fn dismiss_toast(&mut self, id: u64) {
        self.toast_queue.retain(|t| t.id != id);
        self.toast = self.toast_queue.last().map(|t| t.text.clone());
    }

    /// Handle messages that mutate only UI chrome state.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        use views::modals::ModalType;
        match message {
            Message::Vault(VaultMessage::CreateFileDialog) => {
                self.open_name_modal(ModalType::CreateFile, None);
            }
            Message::Vault(VaultMessage::CreateFolderDialog) => {
                self.open_name_modal(ModalType::CreateFolder, None);
            }
            Message::Vault(VaultMessage::NewNoteInFolder(folder)) => {
                self.open_name_modal(ModalType::CreateFile, Some(folder));
            }
            Message::Vault(VaultMessage::NewFolderInFolder(folder)) => {
                self.open_name_modal(ModalType::CreateFolder, Some(folder));
            }
            Message::Vault(VaultMessage::DeleteFileDialog(path)) => {
                self.active_modal = Some(ModalType::Delete(path));
                self.modal_error = None;
                self.context_menu = None;
            }
            Message::Vault(VaultMessage::RenameEntryDialog(path)) => {
                self.modal_input = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&path)
                    .to_string();
                self.modal_error = None;
                self.active_modal = Some(ModalType::Rename(path));
                self.context_menu = None;
            }
            Message::Vault(VaultMessage::NameModalInputChanged(input)) => {
                self.modal_input = input;
                self.modal_error = None;
            }
            Message::Vault(VaultMessage::PdfLinkNoteFolderSelected(folder)) => {
                if self.link_note_modal_open() {
                    let filename = note_filename_from_path(&self.modal_input);
                    self.modal_input = if folder.is_empty() {
                        filename
                    } else {
                        format!("{}/{}", folder.trim_end_matches('/'), filename)
                    };
                    self.modal_error = None;
                }
            }
            Message::Vault(VaultMessage::PdfLinkNoteFileSelected(path)) => {
                if self.link_note_modal_open() {
                    self.modal_input = normalize_note_path(&path);
                    self.modal_error = None;
                }
            }
            Message::Vault(VaultMessage::PdfLinkNotePickerSearchChanged(query)) => {
                if self.link_note_modal_open() {
                    self.link_note_picker_search = query;
                }
            }
            Message::Vault(VaultMessage::NameModalCancel) => {
                self.active_modal = None;
                self.modal_input.clear();
                self.modal_error = None;
                self.modal_parent_override = None;
                self.link_note_picker_search.clear();
            }
            Message::Vault(VaultMessage::NameModalSubmitCurrent) => {
                if matches!(self.active_modal, Some(ModalType::SaveDesk)) {
                    return Task::done(Message::Desks(crate::messages::DesksMessage::Save(
                        self.modal_input.clone(),
                    )));
                }
                if matches!(self.active_modal, Some(ModalType::SavePinboard)) {
                    return Task::done(Message::Graph(
                        crate::messages::GraphMessage::SavePinboard(self.modal_input.clone()),
                    ));
                }
                if matches!(
                    self.active_modal,
                    Some(
                        ModalType::CreateFile
                            | ModalType::CreateFolder
                            | ModalType::QuickNote(_)
                            | ModalType::LinkNote(_)
                            | ModalType::Rename(_)
                    )
                ) {
                    return Task::done(Message::Vault(VaultMessage::NameModalSubmit(
                        self.modal_input.clone(),
                    )));
                }
            }
            Message::Shell(ShellMessage::CommandPaletteOpen) => {
                self.command_palette_visible = true;
                self.command_palette_query.clear();
                self.palette_selected = 0;
                self.context_menu = None;
                self.tab_overflow_open = false;
            }
            Message::Shell(ShellMessage::CommandPaletteQueryChanged(query)) => {
                self.command_palette_query = query;
                self.palette_selected = 0;
            }
            Message::Shell(ShellMessage::ShowToast(text)) => {
                self.toast = Some(text);
                self.sync_legacy_toast();
            }
            Message::Shell(ShellMessage::ToastHide) => {
                self.toast_queue.retain(|t| t.persistent);
                self.toast = self.toast_queue.last().map(|t| t.text.clone());
            }
            Message::Shell(ShellMessage::DismissToast(id)) => self.dismiss_toast(id),
            Message::Shell(ShellMessage::SplitViewDragStart) => self.is_resizing_split = true,
            Message::Shell(ShellMessage::SplitViewReset) => self.split_ratio = 0.5,
            Message::Shell(ShellMessage::OpenContextMenu(target, x, y)) => {
                self.context_menu = Some(ActiveContextMenu { target, x, y });
                self.tab_overflow_open = false;
            }
            Message::Shell(ShellMessage::CloseContextMenu) => {
                self.context_menu = None;
                self.tab_overflow_open = false;
            }
            _ => {}
        }
        Task::none()
    }

    /// Open the create-file or create-folder modal with an empty name field,
    /// optionally inside `parent` instead of the selected folder.
    fn open_name_modal(&mut self, modal: views::modals::ModalType, parent: Option<String>) {
        self.active_modal = Some(modal);
        self.modal_input.clear();
        self.modal_error = None;
        self.modal_parent_override = parent;
        self.link_note_picker_search.clear();
        self.context_menu = None;
    }

    fn link_note_modal_open(&self) -> bool {
        matches!(
            self.active_modal,
            Some(views::modals::ModalType::LinkNote(_))
        )
    }
}

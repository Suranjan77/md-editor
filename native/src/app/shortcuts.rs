//! Application keyboard shortcuts, routed to whichever surface is in front.

use super::*;
use crate::messages::{
    EditorMessage, GraphMessage, InkMessage, SearchMessage, SettingsMessage, ShellMessage,
    TabsMessage, VaultMessage,
};

impl MdEditor {
    /// Handle a message routed here by [`MdEditor::update_inner`].
    pub(super) fn update_shortcuts(&mut self, s: Shortcut) -> Task<Message> {
        match s {
            Shortcut::Escape => {
                if self.ink.is_visible() && self.ink.clear_selection() {
                    return Task::none();
                }
                if self.ui.context_menu.is_some() || self.ui.tab_overflow_open {
                    self.ui.context_menu = None;
                    self.ui.tab_overflow_open = false;
                } else if self.pdf.selection.is_some() {
                    self.pdf.selection = None;
                } else if self.pdf.focused_annotation_id.is_some() {
                    self.pdf.focused_annotation_id = None;
                } else if self.pdf.link_preview.is_some() {
                    self.pdf.link_preview = None;
                    self.pdf.link_preview_size = None;
                } else if self.ui.active_modal.is_some() {
                    self.ui.active_modal = None;
                    self.ui.modal_input.clear();
                    self.ui.modal_error = None;
                    self.ui.link_note_picker_search.clear();
                } else if self.ui.command_palette_visible {
                    self.ui.command_palette_visible = false;
                } else if self.search.file_visible {
                    self.search.file_visible = false;
                    return self.restore_scroll_positions();
                } else if self.search.visible {
                    self.search.visible = false;
                    return self.restore_scroll_positions();
                } else if self.ui.settings_visible {
                    self.ui.settings_visible = false;
                } else if self.tracker.visible {
                    self.tracker.hide();
                } else if self.graph.visible {
                    self.graph.visible = false;
                } else if self.editor.toc_visible {
                    self.editor.toc_visible = false;
                }
                Task::none()
            }
            Shortcut::ToggleSidebar => {
                self.vault.sidebar_visible = !self.vault.sidebar_visible;
                self.persist_layout();
                self.refit_pdf_if_needed()
            }
            Shortcut::Handwriting => Task::done(Message::Ink(InkMessage::Toggle)),
            Shortcut::InsertSketch => self.insert_sketch(),
            Shortcut::Save if self.ink.is_visible() => Task::done(Message::Ink(InkMessage::Save)),
            Shortcut::Save => Task::done(Message::Editor(EditorMessage::Save)),
            Shortcut::OpenVault => Task::done(Message::Vault(VaultMessage::OpenVaultDialog)),
            Shortcut::NewFile => Task::done(Message::Vault(VaultMessage::CreateFileDialog)),
            Shortcut::InsertImage => Task::done(Message::Editor(EditorMessage::InsertImageDialog)),
            Shortcut::GlobalSearch => Task::done(Message::Search(SearchMessage::GlobalSearchOpen)),
            Shortcut::Search => self.open_contextual_search(),
            Shortcut::CommandPalette => {
                Task::done(Message::Shell(ShellMessage::CommandPaletteOpen))
            }
            Shortcut::CloseTab => Task::done(Message::Tabs(TabsMessage::CloseActive)),
            Shortcut::ReopenTab => Task::done(Message::Tabs(TabsMessage::Reopen)),
            Shortcut::NextTab => Task::done(Message::Tabs(TabsMessage::MruNext)),
            Shortcut::PreviousTab => Task::done(Message::Tabs(TabsMessage::MruPrevious)),
            Shortcut::ToggleBacklinks => {
                self.vault.backlinks_visible = !self.vault.backlinks_visible;
                self.persist_layout();
                self.refit_pdf_if_needed()
            }
            Shortcut::KnowledgeGraph => Task::done(Message::Graph(GraphMessage::Toggle)),
            Shortcut::TableOfContents => Task::done(Message::Editor(EditorMessage::ToggleTOC)),
            Shortcut::StudyTracker => {
                self.graph.visible = false;
                self.ui.settings_visible = false;
                self.tracker.toggle_visible();
                self.persist_layout();
                Task::none()
            }
            Shortcut::SplitView => Task::done(Message::Shell(ShellMessage::SplitViewToggle)),
            Shortcut::FocusOtherPane => self.focus_other_pane(),
            Shortcut::MoveTabToOtherPane => self.move_active_tab_to_other_pane(),
            Shortcut::ToggleSplitAxis => {
                let axis = match self.workspace.axis {
                    super::workspace::SplitAxis::Vertical => {
                        super::workspace::SplitAxis::Horizontal
                    }
                    super::workspace::SplitAxis::Horizontal => {
                        super::workspace::SplitAxis::Vertical
                    }
                };
                self.split(axis)
            }
            Shortcut::Settings => Task::done(Message::Settings(SettingsMessage::Toggle)),
            Shortcut::RenameEntry => {
                if let Some(path) = self
                    .vault
                    .selected_path
                    .clone()
                    .or_else(|| self.active_path.clone())
                {
                    Task::done(Message::Vault(VaultMessage::RenameEntryDialog(path)))
                } else {
                    Task::none()
                }
            }
            Shortcut::CycleFocusNext => Task::done(Message::Shell(ShellMessage::FocusRegionNext)),
            Shortcut::CycleFocusPrev => Task::done(Message::Shell(ShellMessage::FocusRegionPrev)),
            Shortcut::FocusMode => {
                let in_focus = !self.vault.sidebar_visible
                    && !self.vault.backlinks_visible
                    && !self.editor.toc_visible;
                if in_focus {
                    self.vault.sidebar_visible = true;
                } else {
                    self.vault.sidebar_visible = false;
                    self.vault.backlinks_visible = false;
                    self.editor.toc_visible = false;
                    self.tracker.hide();
                    self.graph.visible = false;
                    self.ui.settings_visible = false;
                }
                self.persist_layout();
                self.refit_pdf_if_needed()
            }
        }
    }

    pub(super) fn open_contextual_search(&mut self) -> Task<Message> {
        if self.pdf.active_path.is_some() && self.showing_pdf {
            self.search.file_visible = true;
            self.search.visible = false;
            if !self.search.query.trim().is_empty() {
                return Task::batch(vec![
                    self.search_pdf(),
                    focus_pdf_search_input(),
                    self.restore_scroll_positions(),
                ]);
            }
            Task::batch(vec![
                focus_pdf_search_input(),
                self.restore_scroll_positions(),
            ])
        } else if self.active_path.is_some() {
            self.search.file_visible = true;
            self.search.visible = false;
            Task::batch(vec![
                focus_file_search_input(),
                self.restore_scroll_positions(),
            ])
        } else {
            self.search.visible = true;
            focus_global_search_input()
        }
    }
}

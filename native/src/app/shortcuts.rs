//! Application keyboard shortcuts, routed to whichever surface is in front.

use super::*;
use crate::messages::{EditorMessage, GraphMessage, InkMessage, ShellMessage, VaultMessage};

impl MdEditor {
    /// Handle a message routed here by [`MdEditor::update_inner`].
    pub(super) fn update_shortcuts(&mut self, s: Shortcut) -> Task<Message> {
        {
            {
                match s {
                    Shortcut::Escape => {
                        // Close overlays in priority order. The ink surface is
                        // first: while it is up it owns the screen, and Escape
                        // should drop its selection before anything else.
                        if self.ink.is_visible() && self.ink.clear_selection() {
                            return Task::none();
                        }
                        if self.pdf.selection.is_some() {
                            self.pdf.selection = None;
                        } else if self.pdf.focused_annotation_id.is_some() {
                            self.pdf.focused_annotation_id = None;
                        } else if self.pdf.link_preview.is_some() {
                            self.pdf.link_preview = None;
                            self.pdf.link_preview_size = None;
                        } else if self.ui.active_modal.is_some() {
                            self.ui.active_modal = None;
                            self.ui.modal_input.clear();
                            self.ui.link_note_picker_search.clear();
                        } else if self.tracker.visible {
                            self.tracker.hide();
                        } else if self.search.file_visible {
                            self.search.file_visible = false;
                            return self.restore_scroll_positions();
                        } else if self.search.visible {
                            self.search.visible = false;
                            return self.restore_scroll_positions();
                        } else if self.ui.command_palette_visible {
                            self.ui.command_palette_visible = false;
                        } else if self.graph.visible {
                            self.graph.visible = false;
                        } else if self.editor.toc_visible {
                            self.editor.toc_visible = false;
                        }
                        Task::none()
                    }
                    Shortcut::ToggleSidebar => {
                        self.vault.sidebar_visible = !self.vault.sidebar_visible;
                        self.refit_pdf_if_needed()
                    }
                    // Ctrl+S saves whatever surface is in front.
                    Shortcut::Handwriting => Task::done(Message::Ink(InkMessage::Toggle)),
                    Shortcut::Save if self.ink.is_visible() => {
                        Task::done(Message::Ink(InkMessage::Save))
                    }
                    Shortcut::Save => Task::done(Message::Editor(EditorMessage::Save)),
                    Shortcut::OpenVault => {
                        Task::done(Message::Vault(VaultMessage::OpenVaultDialog))
                    }
                    Shortcut::NewFile => Task::done(Message::Vault(VaultMessage::CreateFileDialog)),
                    Shortcut::Search => {
                        if self.ui.split_view_active && self.active_path.is_some() {
                            self.search.file_visible = true;
                            self.search.visible = false;
                            if self.active_panel == ActivePanel::Pdf
                                && self.pdf.active_path.is_some()
                            {
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
                            } else {
                                self.search.pdf_results.clear();
                                self.search.pdf_indices_by_page.clear();
                                Task::batch(vec![
                                    focus_file_search_input(),
                                    self.restore_scroll_positions(),
                                ])
                            }
                        } else if self.pdf.active_path.is_some() && self.showing_pdf {
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
                    // Routed through the message rather than setting the flag
                    // here, so the shortcut and any other opener share one
                    // path — including focusing the query field.
                    Shortcut::CommandPalette => {
                        Task::done(Message::Shell(ShellMessage::CommandPaletteOpen))
                    }
                    Shortcut::ToggleBacklinks => {
                        self.vault.backlinks_visible = !self.vault.backlinks_visible;
                        Task::none()
                    }
                    Shortcut::KnowledgeGraph => Task::done(Message::Graph(GraphMessage::Toggle)),
                    Shortcut::TableOfContents => {
                        Task::done(Message::Editor(EditorMessage::ToggleTOC))
                    }
                    Shortcut::StudyTracker => {
                        self.tracker.toggle_visible();
                        Task::none()
                    }
                    Shortcut::SplitView => {
                        Task::done(Message::Shell(ShellMessage::SplitViewToggle))
                    }
                    Shortcut::FocusMode => {
                        self.vault.sidebar_visible = false;
                        self.vault.backlinks_visible = false;
                        self.editor.toc_visible = false;
                        self.tracker.hide();
                        self.graph.visible = false;
                        Task::none()
                    }
                }
            }
        }
    }
}

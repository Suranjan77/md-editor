//! Saved desk handlers: capturing, switching (flushing dirty buffers first), deleting, and dialog state.

use super::workspace::{PaneDocs, PaneId, route_to};
use super::*;
use crate::desks::Desk;
use crate::messages::DesksMessage;
use crate::tabs::Tabs;
use crate::ui::ToastKind;

impl MdEditor {
    /// Route `DesksMessage` updates exhaustively.
    pub(super) fn update_desks(&mut self, message: DesksMessage) -> Task<Message> {
        match message {
            DesksMessage::OpenDialog => self.open_desks_dialog(),
            DesksMessage::OpenSaveDialog => self.open_save_desk_dialog(),
            DesksMessage::Save(name) => self.submit_save_desk_modal(&name),
            DesksMessage::Switch(name) => self.switch_to_desk(&name),
            DesksMessage::Delete(name) => {
                self.delete_desk(&name);
                Task::none()
            }
            DesksMessage::OpenFromPalette(name) => {
                self.ui.command_palette_visible = false;
                self.ui.command_palette_query.clear();
                self.switch_to_desk(&name)
            }
        }
    }

    pub(super) fn open_desks_dialog(&mut self) -> Task<Message> {
        if self.vault.root.is_none() {
            self.ui
                .notify(ToastKind::Warning, "Open a vault to manage desks", false);
            return Task::none();
        }
        self.ui.active_modal = Some(views::modals::ModalType::Desks);
        self.ui.modal_input.clear();
        self.ui.modal_error = None;
        self.ui.modal_parent_override = None;
        self.ui.link_note_picker_search.clear();
        self.ui.context_menu = None;
        self.ui.tab_overflow_open = false;
        Task::none()
    }

    pub(super) fn open_save_desk_dialog(&mut self) -> Task<Message> {
        if self.vault.root.is_none() {
            self.ui.notify(
                ToastKind::Warning,
                "Open a vault before saving a desk",
                false,
            );
            return Task::none();
        }
        self.ui.active_modal = Some(views::modals::ModalType::SaveDesk);
        self.ui.modal_input.clear();
        self.ui.modal_error = None;
        self.ui.modal_parent_override = None;
        self.ui.link_note_picker_search.clear();
        self.ui.context_menu = None;
        self.ui.tab_overflow_open = false;
        iced::widget::operation::focus(iced::advanced::widget::Id::new("modal_name_input"))
    }

    /// Submit `ModalType::SaveDesk` from the modal dialog with inline validation.
    pub(super) fn submit_save_desk_modal(&mut self, input: &str) -> Task<Message> {
        match self.save_desk(input) {
            Ok(_) => {
                self.ui.active_modal = None;
                self.ui.modal_input.clear();
                self.ui.modal_error = None;
                Task::none()
            }
            Err(err) => {
                self.ui.modal_error = Some(err);
                Task::none()
            }
        }
    }

    /// Snapshot every pane's tabs, viewer state, axis, split ratio, and panels as a named desk.
    pub(super) fn save_desk(&mut self, name: &str) -> Result<String, String> {
        let Some(vault_root) = self.vault.root.clone() else {
            return Err("Open a vault before saving a desk".to_string());
        };
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            return Err("Desk name cannot be empty".to_string());
        }

        // Refresh PDF page/zoom/scroll, image zoom, and note cursor/scroll in every pane.
        let _ = self.for_each_pane(|app| {
            app.save_current_tab_view();
            app.persist_document_position();
            Task::none()
        });

        let pane_a_tabs = self.with_pane(PaneId::A, |app| app.tabs.to_json());
        let pane_b_tabs = self
            .workspace
            .is_split()
            .then(|| self.with_pane(PaneId::B, |app| app.tabs.to_json()));

        let desk = Desk {
            name: trimmed.clone(),
            pane_a_tabs,
            pane_b_tabs,
            axis: self.workspace.axis,
            split_ratio: self.ui.split_ratio.clamp(0.2, 0.8),
            focused: self.workspace.focused,
            sidebar_visible: self.vault.sidebar_visible,
            backlinks_visible: self.vault.backlinks_visible,
            toc_visible: self.editor.toc_visible,
        };
        self.desks.save(desk)?;
        self.desks.persist(&self.state, &vault_root);
        self.ui
            .notify(ToastKind::Success, format!("Saved desk '{trimmed}'"), false);
        Ok(trimmed)
    }

    /// Flush every open pane's dirty buffer (refusing on failure), then restore the saved desk.
    pub(super) fn switch_to_desk(&mut self, name: &str) -> Task<Message> {
        let Some(desk) = self.desks.get(name).cloned() else {
            self.ui.notify(
                ToastKind::Warning,
                format!("Desk '{name}' not found"),
                false,
            );
            return Task::none();
        };
        let Some(vault_root) = self.vault.root.clone() else {
            return Task::none();
        };

        // Never drop a dirty buffer: flush all panes before changing workspace state.
        let mut save_error: Option<String> = None;
        let _ = self.for_each_pane(|app| {
            app.save_current_tab_view();
            if let SaveOutcome::Failed(err) = app.save_active_document() {
                save_error.get_or_insert_with(|| err.to_string());
            } else {
                app.persist_document_position();
            }
            Task::none()
        });
        if let Some(err) = save_error {
            self.ui.last_save_failed = true;
            self.ui.notify(
                ToastKind::Error,
                format!("Not switching desks: {err}. Your edits are still open."),
                true,
            );
            return Task::none();
        }

        let _ = self.for_each_pane(|app| {
            app.park_active_buffer();
            Task::none()
        });

        self.dismiss_overlays_for_desk_switch();
        self.reset_panes_to_single();

        let root_path = std::path::PathBuf::from(&vault_root);
        let mut tabs_a = Tabs::new();
        tabs_a.restore_from_json(&desk.pane_a_tabs, |rel| {
            md_editor_core::vault::resolve_vault_path(&root_path, rel).is_file()
        });
        let pane_a_paths: std::collections::HashSet<String> =
            tabs_a.list().iter().map(|t| t.path.clone()).collect();
        self.tabs = tabs_a;

        let mut tasks = Vec::new();
        if let Some(active_id) = self.tabs.active_id() {
            tasks.push(route_to(PaneId::A, self.activate_tab(active_id)));
        } else {
            self.persist_tabs();
        }

        if let Some(ref raw_b) = desk.pane_b_tabs {
            self.workspace.other = Some(PaneDocs::new(PaneId::B));
            let task_b = self.with_pane(PaneId::B, |app| {
                let mut tabs_b = Tabs::new();
                tabs_b.restore_from_json(raw_b, |rel| {
                    !pane_a_paths.contains(rel)
                        && md_editor_core::vault::resolve_vault_path(&root_path, rel).is_file()
                });
                app.tabs = tabs_b;
                if let Some(active_id) = app.tabs.active_id() {
                    app.activate_tab(active_id)
                } else {
                    app.persist_tabs();
                    Task::none()
                }
            });
            tasks.push(route_to(PaneId::B, task_b));
        } else {
            let _ = md_editor_core::config::set_sys_config(
                &self.state,
                &format!("tabs_b:{vault_root}"),
                "",
            );
        }

        self.workspace.axis = desk.axis;
        self.ui.split_ratio = desk.split_ratio.clamp(0.2, 0.8);
        self.vault.sidebar_visible = desk.sidebar_visible;
        self.vault.backlinks_visible = desk.backlinks_visible;
        self.editor.toc_visible = desk.toc_visible;

        let target_focus = if self.workspace.is_split() {
            desk.focused
        } else {
            PaneId::A
        };
        if target_focus != self.workspace.focused {
            tasks.push(self.focus_pane(target_focus));
        } else {
            tasks.push(self.sync_focused_pane_metadata());
        }
        if let Some(path) = self
            .active_path
            .clone()
            .filter(|_| !self.showing_pdf)
            .or_else(|| self.pdf.active_path.clone())
            .or_else(|| self.active_image_path.clone())
        {
            let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", &path);
        } else {
            let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", "");
        }

        self.persist_layout();
        tasks.push(self.refit_all_pdfs());
        self.ui.notify(
            ToastKind::Info,
            format!("Switched to desk '{}'", desk.name),
            false,
        );
        Task::batch(tasks)
    }

    pub(super) fn delete_desk(&mut self, name: &str) {
        let Some(vault_root) = self.vault.root.clone() else {
            return;
        };
        if self.desks.delete(name) {
            self.desks.persist(&self.state, &vault_root);
            self.ui
                .notify(ToastKind::Info, format!("Deleted desk '{name}'"), false);
        }
    }

    /// Open today's `daily/YYYY-MM-DD.md` note (synthesizing tasks and brief if new).
    pub(super) fn open_daily_desk(&mut self) -> Task<Message> {
        if self.vault.root.is_none() {
            self.ui.notify(
                ToastKind::Warning,
                "Open a vault before opening your Daily Desk",
                false,
            );
            return Task::none();
        }
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let outcome = match md_editor_core::daily::open_or_create_daily_note(&self.state, &today) {
            Ok(outcome) => outcome,
            Err(err) => {
                self.ui.notify(
                    ToastKind::Error,
                    format!("Could not open daily note: {err}"),
                    false,
                );
                return Task::none();
            }
        };
        if outcome.created {
            self.vault.entries = md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
            self.invalidate_graph();
        }
        self.reveal_in_sidebar(&outcome.path);
        let open_task = self.on_sidebar_file_clicked(outcome.path.clone());
        if outcome.created {
            let target = self
                .pane_holding(&outcome.path)
                .unwrap_or(self.workspace.focused);
            let cursor_task = self.with_pane(target, |app| {
                app.run_editor_command(EditorCommand::MoveCursor {
                    movement: crate::editor::buffer::Movement::DocEnd,
                    extend: false,
                })
            });
            return Task::batch(vec![open_task, route_to(target, cursor_task)]);
        }
        open_task
    }

    fn dismiss_overlays_for_desk_switch(&mut self) {
        self.graph.visible = false;
        self.tracker.hide();
        self.ui.settings_visible = false;
        self.ui.active_modal = None;
        self.ui.modal_input.clear();
        self.ui.modal_error = None;
        self.ui.modal_parent_override = None;
        self.ui.link_note_picker_search.clear();
        self.ui.command_palette_visible = false;
        self.ui.command_palette_query.clear();
        self.ui.context_menu = None;
        self.ui.tab_overflow_open = false;
        self.search.visible = false;
        self.search.file_visible = false;
        self.search.pdf_results.clear();
        self.search.pdf_indices_by_page.clear();
        self.search.match_index = None;
        self.search.pdf_search_seq = self.search.pdf_search_seq.wrapping_add(1);
        self.ui.pdf_page_input = None;
        self.ui.pdf_zoom_input = None;
        self.motion.editor_scroll = None;
    }

    fn reset_panes_to_single(&mut self) {
        self.swap_in(PaneId::A);
        if let Some(mut other) = self.workspace.other.take() {
            self.editor.absorb_retained(&mut other.editor);
        }
        self.clear_all_document_panes();
        self.tabs = Tabs::new();
        self.workspace.current = PaneId::A;
        self.workspace.focused = PaneId::A;
        super::workspace::set_current(PaneId::A);
    }
}

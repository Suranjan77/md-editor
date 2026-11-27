//! Document tab strip handlers: activating, closing, reopening, reordering, split-view pairing and session persistence.

use super::*;
use crate::messages::{PdfMessage, TabsMessage};
use crate::tabs::{TabId, TabKind, TabView, Tabs};

impl MdEditor {
    /// Route `TabsMessage` actions.
    pub(super) fn update_tabs(&mut self, message: TabsMessage) -> Task<Message> {
        self.ui.context_menu = None;
        match message {
            TabsMessage::Activate(id) => {
                self.ui.tab_overflow_open = false;
                self.activate_tab(id)
            }
            TabsMessage::OpenBackground(path) => self.open_in_background(&path),
            TabsMessage::Close(id) => {
                self.ui.tab_overflow_open = false;
                self.close_tab_by_id(id)
            }
            TabsMessage::CloseActive => {
                if let Some(id) = self.tabs.active_id() {
                    self.close_tab_by_id(id)
                } else {
                    Task::none()
                }
            }
            TabsMessage::CloseOthers(keep_id) => {
                self.ui.tab_overflow_open = false;
                self.close_other_tabs(keep_id)
            }
            TabsMessage::Reopen => self.reopen_closed_tab(),
            TabsMessage::Next => {
                self.save_current_tab_view();
                if let Some(id) = self.tabs.step_strip(1) {
                    self.activate_tab(id)
                } else {
                    Task::none()
                }
            }
            TabsMessage::Previous => {
                self.save_current_tab_view();
                if let Some(id) = self.tabs.step_strip(-1) {
                    self.activate_tab(id)
                } else {
                    Task::none()
                }
            }
            TabsMessage::MruNext => {
                self.save_current_tab_view();
                if let Some(id) = self.tabs.step_mru(true) {
                    self.activate_tab(id)
                } else {
                    Task::none()
                }
            }
            TabsMessage::MruPrevious => {
                self.save_current_tab_view();
                if let Some(id) = self.tabs.step_mru(false) {
                    self.activate_tab(id)
                } else {
                    Task::none()
                }
            }
            TabsMessage::SelectOrdinal(ord) => {
                if let Some(id) = self.tabs.select_ordinal(ord) {
                    self.activate_tab(id)
                } else {
                    Task::none()
                }
            }
            TabsMessage::MoveBy(delta) => {
                self.tabs.move_active_by(delta);
                self.persist_tabs();
                Task::none()
            }
            TabsMessage::DragStart(id) => {
                self.ui.dragging_tab = Some(id);
                Task::none()
            }
            TabsMessage::DragOver(target_idx) => {
                if let Some(id) = self.ui.dragging_tab {
                    self.tabs.move_tab(id, target_idx);
                    self.persist_tabs();
                }
                Task::none()
            }
            TabsMessage::DragEnd => {
                self.ui.dragging_tab = None;
                Task::none()
            }
            TabsMessage::MoveToOtherPane(id) => {
                self.ui.context_menu = None;
                match self.tabs.get(id).map(|t| t.path.clone()) {
                    Some(path) => self.move_tab_to_other_pane(&path),
                    None => Task::none(),
                }
            }
            TabsMessage::ToggleOverflowMenu => {
                self.ui.tab_overflow_open = !self.ui.tab_overflow_open;
                Task::none()
            }
        }
    }

    /// Snapshot current viewer state into `self.tabs` before switching away.
    pub(super) fn save_current_tab_view(&mut self) {
        if let Some(ref pdf_path) = self.pdf.active_path {
            self.tabs.save_view_by_path(
                pdf_path,
                TabView::Pdf {
                    page: self.pdf.current_page,
                    zoom: self.pdf.zoom,
                    fit_to_width: self.pdf.fit_to_width,
                    fit_fills_pane: self.pdf.fit_fills_pane,
                    scroll_y: self.pdf.scroll_y,
                },
            );
        }
        if let Some(ref img_path) = self.active_image_path {
            self.tabs.save_view_by_path(
                img_path,
                TabView::Image {
                    zoom: self.ui.image_zoom.unwrap_or(1.0),
                    fit: self.ui.image_zoom.is_none(),
                },
            );
        }
    }

    /// Open a vault file in a background tab without leaving the active tab.
    pub(super) fn open_in_background(&mut self, path: &str) -> Task<Message> {
        let Some(kind) = TabKind::from_path(path) else {
            return Task::none();
        };
        let Some(abs) = self.resolve_active_path(path) else {
            return Task::none();
        };
        if !abs.is_file() {
            return Task::none();
        }
        self.tabs.open(path, kind, true);
        self.persist_tabs();
        Task::none()
    }

    /// Activate an existing tab by `TabId`, flushing any unsaved edits first.
    pub(super) fn activate_tab(&mut self, id: TabId) -> Task<Message> {
        let Some(target) = self.tabs.get(id).cloned() else {
            return Task::none();
        };
        self.graph.visible = false;
        self.tracker.hide();
        self.ui.settings_visible = false;
        self.save_current_tab_view();

        match target.kind {
            TabKind::Note => {
                let task = self.open_file(&target.path);
                if self.active_path.as_deref() == Some(target.path.as_str()) {
                    self.tabs.activate(id);
                    self.reveal_in_sidebar(&target.path);
                    self.persist_tabs();
                }
                task
            }
            TabKind::Pdf => {
                if let Err(err) = self.leave_active_document() {
                    self.ui.last_save_failed = true;
                    self.ui.notify(
                        crate::ui::ToastKind::Error,
                        format!("Not switching tabs: {err}. Your edits are still open here."),
                        true,
                    );
                    return Task::none();
                }
                self.tabs.activate(id);
                self.reveal_in_sidebar(&target.path);
                let task = self.open_pdf_restoring_view(&target.path, Some(target.view));
                self.persist_tabs();
                task
            }
            TabKind::Image => {
                let task = self.open_image(&target.path);
                if self.active_image_path.as_deref() == Some(target.path.as_str()) {
                    if let TabView::Image { zoom, fit } = target.view {
                        self.ui.image_zoom = (!fit).then_some(zoom);
                    }
                    self.tabs.activate(id);
                    self.reveal_in_sidebar(&target.path);
                    self.persist_tabs();
                }
                task
            }
        }
    }

    /// Close a tab, flushing unsaved note edits first and refusing on write failure.
    pub(super) fn close_tab_by_id(&mut self, id: TabId) -> Task<Message> {
        let Some(closing) = self.tabs.get(id).cloned() else {
            return Task::none();
        };
        // Never drop a dirty buffer: refuse to close when the flush fails.
        if closing.kind == TabKind::Note
            && self.active_path.as_deref() == Some(&closing.path)
            && let SaveOutcome::Failed(err) = self.save_active_document()
        {
            self.ui.last_save_failed = true;
            self.ui.notify(
                crate::ui::ToastKind::Error,
                format!("Not closing tab: {err}"),
                true,
            );
            return Task::none();
        }
        self.save_current_tab_view();
        let was_active = self.tabs.active_id() == Some(id);
        let Some(closed) = self.tabs.close(id) else {
            return Task::none();
        };

        // Release per-tab resources (T2 memory bound).
        match closed.kind {
            TabKind::Note => {
                self.editor.forget_retained(&closed.path);
                if self.active_path.as_deref() == Some(&closed.path) {
                    self.active_path = None;
                    self.editor.buffer = DocBuffer::new();
                    self.editor.autosave_pending_since = None;
                    self.editor.highlight = Default::default();
                    self.editor.toc_entries.clear();
                }
            }
            TabKind::Pdf => {
                if self.pdf.active_path.as_deref() == Some(&closed.path) {
                    self.clear_pdf_pane();
                }
            }
            TabKind::Image => {
                if self.active_image_path.as_deref() == Some(&closed.path) {
                    self.active_image_path = None;
                    self.active_image = None;
                }
            }
        }

        let task = self.reconcile_after_tab_close(was_active);
        self.persist_tabs();
        task
    }

    /// Close every tab except `keep_id`.
    pub(super) fn close_other_tabs(&mut self, keep_id: TabId) -> Task<Message> {
        if self.active_path.is_some()
            && self
                .tabs
                .get(keep_id)
                .is_none_or(|t| Some(t.path.as_str()) != self.active_path.as_deref())
            && let SaveOutcome::Failed(err) = self.save_active_document()
        {
            self.ui.last_save_failed = true;
            self.ui.notify(
                crate::ui::ToastKind::Error,
                format!("Not closing tabs: {err}"),
                true,
            );
            return Task::none();
        }
        self.save_current_tab_view();
        let removed = self.tabs.close_others(keep_id);
        for closed in removed {
            if closed.kind == TabKind::Note {
                self.editor.forget_retained(&closed.path);
            }
            if self.pdf.active_path.as_deref() == Some(&closed.path) {
                self.clear_pdf_pane();
            }
            if self.active_image_path.as_deref() == Some(&closed.path) {
                self.active_image_path = None;
                self.active_image = None;
            }
        }
        let task = self.activate_tab(keep_id);
        self.persist_tabs();
        task
    }

    fn reconcile_after_tab_close(&mut self, was_active: bool) -> Task<Message> {
        if let Some(next_id) = self.tabs.active_id() {
            if was_active
                || (self.active_path.is_none()
                    && self.pdf.active_path.is_none()
                    && self.active_image_path.is_none())
            {
                return self.activate_tab(next_id);
            }
            Task::none()
        } else {
            self.clear_all_document_panes();
            let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", "");
            Task::none()
        }
    }

    /// Reopen the most recently closed tab (`Ctrl+Shift+T`), skipping deleted files.
    pub(super) fn reopen_closed_tab(&mut self) -> Task<Message> {
        let Some(root) = self.vault.root.clone() else {
            return Task::none();
        };
        let root_path = std::path::PathBuf::from(root);
        if let Some(id) = self
            .tabs
            .reopen_last(|rel| md_editor_core::vault::resolve_vault_path(&root_path, rel).is_file())
        {
            self.activate_tab(id)
        } else {
            Task::none()
        }
    }

    pub(super) fn clear_pdf_pane(&mut self) {
        self.pdf.active_path = None;
        self.pdf.pages.clear();
        self.pdf.dimensions.clear();
        self.pdf.page_sizes.clear();
        self.pdf.pending_pages.clear();
        self.pdf.pending_links.clear();
        self.pdf.pending_text.clear();
        self.pdf.toc_entries.clear();
        self.pdf.render_generation = self.pdf.render_generation.wrapping_add(1);
        self.pdf.document_generation = self.pdf.document_generation.wrapping_add(1);
        self.showing_pdf = false;
    }

    pub(super) fn clear_all_document_panes(&mut self) {
        self.active_path = None;
        self.editor.buffer = DocBuffer::new();
        self.editor.autosave_pending_since = None;
        self.editor.highlight = Default::default();
        self.editor.toc_entries.clear();
        self.clear_pdf_pane();
        self.active_image_path = None;
        self.active_image = None;
        self.vault.backlinks.clear();
    }

    /// Open a PDF and apply saved `TabView::Pdf` zoom/page/scroll if present.
    pub(super) fn open_pdf_restoring_view(
        &mut self,
        path: &str,
        saved: Option<TabView>,
    ) -> Task<Message> {
        if let Some(TabView::Pdf {
            page,
            zoom,
            fit_to_width,
            fit_fills_pane,
            scroll_y,
        }) = saved
        {
            if self.pdf.initial_target_page.is_none()
                && self.pdf.initial_target_annotation.is_none()
            {
                self.pdf.initial_target_page = (page > 0).then_some(page);
            }
            let task = self.open_pdf(path);
            self.pdf.fit_fills_pane = fit_to_width && fit_fills_pane;
            if !fit_to_width {
                self.pdf.fit_to_width = false;
                self.pdf.zoom = zoom;
                self.pdf.scroll_y = scroll_y;
            }
            return task;
        }
        let _ = PdfMessage::FitToWidth;
        self.open_pdf(path)
    }

    /// Persist the current vault's tab strip to `settings`.
    pub(super) fn persist_tabs(&self) {
        let Some(ref root) = self.vault.root else {
            return;
        };
        let key = tabs_key(root, self.workspace.current);
        let _ = md_editor_core::config::set_sys_config(&self.state, &key, &self.tabs.to_json());
    }

    /// Restore the vault's panes: pane A's tabs, and pane B's when the last
    /// session was split.
    pub(super) fn restore_workspace(
        &mut self,
        vault_root: &str,
        fallback_file: Option<&str>,
    ) -> Task<Message> {
        let first = self.restore_vault_tabs(vault_root, fallback_file);
        if !std::mem::take(&mut self.ui.restore_split) || self.workspace.is_split() {
            return first;
        }
        let pane = super::workspace::PaneId::B;
        self.workspace.other = Some(super::workspace::PaneDocs::new(pane));
        let second = self.with_pane(pane, |app| app.restore_vault_tabs(vault_root, None));
        Task::batch(vec![first, super::workspace::route_to(pane, second)])
    }

    /// Restore the tab strip for `vault_root` on vault open or cold launch.
    pub(super) fn restore_vault_tabs(
        &mut self,
        vault_root: &str,
        fallback_file: Option<&str>,
    ) -> Task<Message> {
        let key = tabs_key(vault_root, self.workspace.current);
        let root_path = std::path::PathBuf::from(vault_root);
        let mut restored = Tabs::new();
        if let Ok(Some(json)) = md_editor_core::config::get_sys_config(&self.state, &key) {
            // Tabs whose files were deleted between sessions are dropped.
            restored.restore_from_json(&json, |rel| {
                md_editor_core::vault::resolve_vault_path(&root_path, rel).is_file()
            });
        }

        if restored.is_empty() {
            if let Some(file_path) = fallback_file
                && !file_path.is_empty()
                && md_editor_core::vault::resolve_vault_path(&root_path, file_path).is_file()
            {
                return self.open_startup_file(file_path);
            }
            self.tabs = Tabs::new();
            return Task::none();
        }

        self.tabs = restored;
        if let Some(active_id) = self.tabs.active_id() {
            self.activate_tab(active_id)
        } else {
            Task::none()
        }
    }
}

/// Where a pane's tab strip is saved: pane A under the key sessions have
/// always used, so older sessions restore.
fn tabs_key(vault_root: &str, pane: super::workspace::PaneId) -> String {
    match pane {
        super::workspace::PaneId::A => format!("tabs:{vault_root}"),
        super::workspace::PaneId::B => format!("tabs_b:{vault_root}"),
    }
}

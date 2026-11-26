//! Pane commands: split and unsplit, focus, moving tabs between panes, opening a file to the side, and keeping each file in one pane.

use super::workspace::{PaneDocs, PaneId, SplitAxis, route_to};
use super::*;
use crate::tabs::{TabKind, TabView};

impl MdEditor {
    /// The pane whose tab strip holds `path`, if any.
    pub(super) fn pane_holding(&self, path: &str) -> Option<PaneId> {
        let current = self.workspace.current;
        if self.tabs.find_by_path(path).is_some() {
            return Some(current);
        }
        self.parked()
            .filter(|docs| docs.tabs.find_by_path(path).is_some())
            .map(|_| current.other())
    }

    /// Split the main area (`axis`), or change the axis if already split.
    ///
    /// A new pane takes something worth seeing beside the focused document:
    /// the most recent PDF tab beside a note, or the most recent note beside
    /// a PDF (the old Note + PDF split, in one keystroke). Otherwise it
    /// starts empty, ready for the next file opened.
    pub(super) fn split(&mut self, axis: SplitAxis) -> Task<Message> {
        self.workspace.axis = axis;
        if self.workspace.is_split() {
            self.persist_layout();
            return Task::none();
        }
        self.save_current_tab_view();
        let companion = if self.showing_pdf {
            self.tabs.mru_of_kind(TabKind::Note)
        } else if self.active_path.is_some() {
            self.tabs.mru_of_kind(TabKind::Pdf)
        } else {
            None
        }
        .map(|tab| tab.path.clone());

        self.workspace.other = Some(PaneDocs::new(self.workspace.current.other()));
        let task = match companion {
            Some(path) => self.move_to_pane(&path, self.workspace.focused.other(), false),
            None => Task::none(),
        };
        self.persist_layout();
        Task::batch(vec![task, self.refit_all_pdfs()])
    }

    /// Close `pane`: its note is saved and its tabs move to the pane that
    /// stays, in the background. Refused (with a toast) if the save fails.
    pub(super) fn close_pane(&mut self, pane: PaneId) -> Task<Message> {
        if !self.workspace.is_split() {
            return Task::none();
        }
        let saved = self.with_pane(pane, |app| {
            app.save_current_tab_view();
            app.save_active_document()
        });
        if let SaveOutcome::Failed(err) = saved {
            self.ui.notify(
                crate::ui::ToastKind::Error,
                format!("Not closing the pane: {err}"),
                true,
            );
            return Task::none();
        }
        let keep = pane.other();
        let moved: Vec<(String, TabView)> = self.with_pane(pane, |app| {
            app.tabs
                .list()
                .iter()
                .map(|tab| (tab.path.clone(), tab.view.clone()))
                .collect()
        });
        // The pane that stays is swapped in and becomes pane A.
        self.swap_in(keep);
        self.workspace.other = None;
        self.workspace.current = PaneId::A;
        self.workspace.focused = PaneId::A;
        super::workspace::set_current(PaneId::A);
        for (path, view) in moved {
            if self.tabs.find_by_path(&path).is_none()
                && let Some(kind) = TabKind::from_path(&path)
            {
                let id = self.tabs.open(&path, kind, true);
                if let Some(tab) = self.tabs.get_mut(id) {
                    tab.view = view;
                }
            }
        }
        self.persist_tabs();
        self.persist_layout();
        let reopen = if self.active_path.is_none()
            && self.pdf.active_path.is_none()
            && self.active_image_path.is_none()
        {
            self.tabs
                .active_id()
                .map_or_else(Task::none, |id| self.activate_tab(id))
        } else {
            Task::none()
        };
        // The widgets were pane B's: put scroll positions and the fit back.
        Task::batch(vec![
            reopen,
            self.restore_scroll_positions(),
            self.refit_pdf_if_needed(),
        ])
    }

    /// `Ctrl+\`: split side by side, or close the second pane.
    pub(super) fn toggle_split(&mut self) -> Task<Message> {
        if self.workspace.is_split() {
            self.close_pane(PaneId::B)
        } else {
            self.split(SplitAxis::Vertical)
        }
    }

    /// Focus the other pane (splitting first if there is none).
    pub(super) fn focus_other_pane(&mut self) -> Task<Message> {
        if !self.workspace.is_split() {
            return self.split(self.workspace.axis);
        }
        self.focus_pane(self.workspace.focused.other())
    }

    /// Move the focused pane's active tab to the other pane (splitting first).
    pub(super) fn move_active_tab_to_other_pane(&mut self) -> Task<Message> {
        match self.tabs.active_tab().map(|t| t.path.clone()) {
            Some(path) => self.move_tab_to_other_pane(&path),
            None => Task::none(),
        }
    }

    /// Move `path`'s tab from the focused pane to the other (splitting first)
    /// and follow it there.
    pub(super) fn move_tab_to_other_pane(&mut self, path: &str) -> Task<Message> {
        if !self.workspace.is_split() {
            self.workspace.other = Some(PaneDocs::new(self.workspace.current.other()));
            self.persist_layout();
        }
        let target = self.workspace.focused.other();
        Task::batch(vec![
            self.move_to_pane(path, target, true),
            self.refit_all_pdfs(),
        ])
    }

    /// Open `path` in the pane beside the focused one, splitting if needed,
    /// and focus it there. A file already open in a pane is focused instead.
    pub(super) fn open_to_side(&mut self, path: &str) -> Task<Message> {
        if !self.workspace.is_split() {
            self.workspace.other = Some(PaneDocs::new(self.workspace.current.other()));
            self.persist_layout();
        }
        let target = self.workspace.focused.other();
        match self.pane_holding(path) {
            Some(pane) if pane != target => self.move_to_pane(path, target, true),
            _ => {
                let focus = self.focus_pane(target);
                let open = self.open_here(path);
                Task::batch(vec![focus, open])
            }
        }
    }

    /// Move `path`'s tab from whichever pane holds it to `target` and show it
    /// there; `focus` also gives `target` the keyboard. The closing side goes
    /// through the normal tab close (note saved, undo history parked).
    pub(super) fn move_to_pane(
        &mut self,
        path: &str,
        target: PaneId,
        focus: bool,
    ) -> Task<Message> {
        let mut tasks = Vec::new();
        if let Some(source) = self.pane_holding(path).filter(|p| *p != target) {
            let view = self.with_pane(source, |app| {
                app.save_current_tab_view();
                app.tabs.find_by_path(path).map(|t| (t.id, t.view.clone()))
            });
            let Some((id, view)) = view else {
                return Task::none();
            };
            let closed = self.with_pane(source, |app| {
                let task = app.close_tab_by_id(id);
                (app.tabs.find_by_path(path).is_none(), task)
            });
            tasks.push(route_to(source, closed.1));
            if !closed.0 {
                // The close was refused (the note could not be saved).
                return Task::batch(tasks);
            }
            self.with_pane(target, |app| {
                if let Some(kind) = TabKind::from_path(path) {
                    let id = app.tabs.open(path, kind, true);
                    if let Some(tab) = app.tabs.get_mut(id) {
                        tab.view = view;
                    }
                }
            });
        }
        if focus {
            tasks.push(self.focus_pane(target));
        }
        let open = self.with_pane(target, |app| {
            let task = match app.tabs.find_by_path(path).map(|t| t.id) {
                Some(id) => app.activate_tab(id),
                None => app.open_here(path),
            };
            app.persist_tabs();
            task
        });
        tasks.push(route_to(target, open));
        Task::batch(tasks)
    }

    /// Open `path` in the pane swapped in (the normal open path).
    fn open_here(&mut self, path: &str) -> Task<Message> {
        self.update_inner(Message::Vault(
            crate::messages::VaultMessage::SidebarFileClicked(path.to_string()),
        ))
    }

    /// Refit every pane's PDF (their widths change when panes come or go).
    pub(super) fn refit_all_pdfs(&mut self) -> Task<Message> {
        self.for_each_pane(|app| app.refit_pdf_if_needed())
    }

    /// Focus the pane `path` should show in, beside what is focused now: the
    /// pane that already holds it; otherwise, when `beside` (a link followed
    /// from a note to a PDF or back), the other pane, splitting if needed;
    /// otherwise the focused pane. Callers then work on `self` (that pane is
    /// swapped in) and route their tasks with the returned id.
    pub(super) fn focus_pane_for(&mut self, path: &str, beside: bool) -> (PaneId, Task<Message>) {
        let target = match self.pane_holding(path) {
            Some(pane) => pane,
            None if beside => {
                if !self.workspace.is_split() {
                    self.workspace.other = Some(PaneDocs::new(self.workspace.current.other()));
                    self.persist_layout();
                }
                self.workspace.focused.other()
            }
            None => self.workspace.focused,
        };
        let refit = if beside {
            self.refit_all_pdfs()
        } else {
            Task::none()
        };
        let focus = self.focus_pane(target);
        (target, Task::batch(vec![refit, focus]))
    }
}

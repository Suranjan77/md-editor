use super::*;
use md3_vault::{LinkGraph, atomic_save, rewrite_links};

impl Shell {
    pub(super) fn is_tab_dirty(&self, tab: TabId) -> bool {
        let Some((_, tab)) = self.ws.panes.find_tab(tab) else {
            return false;
        };
        self.sessions
            .md
            .get(&tab.document)
            .is_some_and(|session| session.doc.buffer().is_dirty())
    }

    pub(super) fn tab_name(&self, tab: TabId) -> String {
        let Some((_, tab)) = self.ws.panes.find_tab(tab) else {
            return String::new();
        };
        self.ws
            .docs
            .get(tab.document)
            .map(|doc| doc.path.rsplit('/').next().unwrap_or(&doc.path).to_string())
            .unwrap_or_else(|| "?".to_string())
    }

    pub(super) fn is_any_tab_dirty(&self) -> bool {
        self.sessions
            .md
            .values()
            .any(|session| session.doc.buffer().is_dirty())
    }

    pub(super) fn close_tab(&mut self, tab: TabId) {
        if let Err(error) = self.ws.close_tab(tab) {
            self.status = error.to_string();
        }
        self.sessions.gc(&self.ws.docs);
        self.save_session();
    }

    pub(super) fn close_pane(&mut self, pane: PaneId) {
        let tabs = self
            .ws
            .panes
            .pane(pane)
            .map(|pane| pane.tabs().iter().map(|tab| tab.id).collect::<Vec<_>>())
            .unwrap_or_default();
        if tabs.is_empty() {
            if self.ws.panes.pane_count() > 1
                && let Err(error) = self.ws.panes.close_empty_pane(pane)
            {
                self.status = error.to_string();
            }
        } else {
            for tab in tabs {
                if self.ws.panes.find_tab(tab).is_some() {
                    self.close_tab(tab);
                }
            }
        }
        self.sessions.gc(&self.ws.docs);
        self.save_session();
    }

    pub(super) fn run_pane_command(&mut self, pane: PaneId, command: CommandId) -> Task<Message> {
        if let Some(tab) = self
            .ws
            .panes
            .pane(pane)
            .and_then(|pane| pane.active_tab())
            .map(|tab| tab.id)
        {
            let _ = self.ws.focus_tab(tab);
        }
        match command.0 {
            "workspace.split-right" | "workspace.split-down" => {
                let axis = if command.0 == "workspace.split-down" {
                    SplitAxis::Vertical
                } else {
                    SplitAxis::Horizontal
                };
                if let Some(tab) = self.ws.panes.pane(pane).and_then(|pane| pane.active_tab()) {
                    let doc = self.ws.docs.get(tab.document).cloned();
                    if let Some(doc) = doc {
                        match self.ws.panes.split(pane, axis) {
                            Ok(target) => {
                                let _ = self.open_document_in(target, &doc.path);
                            }
                            Err(error) => self.status = error.to_string(),
                        }
                    }
                } else if let Err(error) = self.ws.panes.split(pane, axis) {
                    self.status = error.to_string();
                }
                self.save_session();
                Task::none()
            }
            "workspace.close-pane" => {
                self.close_pane(pane);
                Task::none()
            }
            _ => self.run_command(command),
        }
    }

    pub(super) fn selected_target(&self) -> Option<String> {
        self.tree_selected
            .as_ref()
            .filter(|path| self.vault_root.join(path).exists())
            .cloned()
            .or_else(|| self.focused_doc_info().map(|(path, _)| path))
    }

    pub(super) fn selected_parent(&self) -> String {
        let Some(target) = self.selected_target() else {
            return String::new();
        };
        if self.vault_root.join(&target).is_dir() {
            target
        } else {
            Path::new(&target)
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_default()
        }
    }

    pub(super) fn create_note(&mut self, parent: &str, input: &str) {
        let Some(mut rel) = safe_relative(input) else {
            self.status = "new note: enter a vault-relative name".to_string();
            return;
        };
        if rel.extension().is_none() {
            rel.set_extension("md");
        }
        let rel = Path::new(parent).join(rel);
        let abs = self.vault_root.join(&rel);
        if abs.exists() {
            self.status = format!("new note: {} already exists", rel.display());
            return;
        }
        if let Some(dir) = abs.parent()
            && let Err(error) = std::fs::create_dir_all(dir)
        {
            self.status = format!("new note {}: {error}", rel.display());
            return;
        }
        if let Err(error) = atomic_save(&abs, b"") {
            self.status = format!("new note {}: {error}", rel.display());
            return;
        }
        let rel = rel.to_string_lossy().to_string();
        self.refresh_after_file_change();
        self.tree_selected = Some(rel.clone());
        self.open_document(&rel);
        self.status = format!("created {rel}");
    }

    pub(super) fn create_folder(&mut self, parent: &str, input: &str) {
        let Some(rel) = safe_relative(input) else {
            self.status = "new folder: enter a vault-relative name".to_string();
            return;
        };
        let rel = Path::new(parent).join(rel);
        let abs = self.vault_root.join(&rel);
        if abs.exists() {
            self.status = format!("new folder: {} already exists", rel.display());
            return;
        }
        if let Err(error) = std::fs::create_dir_all(&abs) {
            self.status = format!("new folder {}: {error}", rel.display());
            return;
        }
        let rel = rel.to_string_lossy().to_string();
        if let Some(parent) = Path::new(&rel).parent()
            && !parent.as_os_str().is_empty()
        {
            self.tree_expanded
                .insert(parent.to_string_lossy().to_string());
        }
        self.tree_selected = Some(rel.clone());
        self.files = scan_vault(&self.vault_root);
        self.save_session();
        self.status = format!("created {rel}");
    }

    pub(super) fn rename_path(&mut self, target: &str, input: &str) {
        let Some(mut name) = safe_relative(input) else {
            self.status = "rename: enter a valid name".to_string();
            return;
        };
        let old_rel = PathBuf::from(target);
        if old_rel.extension().is_some() && name.extension().is_none() {
            name.set_extension(old_rel.extension().unwrap_or_default());
        }
        let new_rel = old_rel.parent().unwrap_or_else(|| Path::new("")).join(name);
        if new_rel == old_rel {
            return;
        }
        let old_abs = self.vault_root.join(&old_rel);
        let new_abs = self.vault_root.join(&new_rel);
        if new_abs.exists() {
            self.status = format!("rename: {} already exists", new_rel.display());
            return;
        }

        let mut graph = LinkGraph::new();
        for rel in scan_vault(&self.vault_root) {
            if rel.ends_with(".md")
                && let Ok(content) = std::fs::read_to_string(self.vault_root.join(&rel))
            {
                graph.update_file(Path::new(&rel), &content);
            }
        }
        let referrers = if old_rel.extension().is_some_and(|ext| ext == "md") {
            graph.rename_file(&old_rel, &new_rel)
        } else {
            Vec::new()
        };

        if let Err(error) = std::fs::rename(&old_abs, &new_abs) {
            self.status = format!("rename {}: {error}", old_rel.display());
            return;
        }

        for referrer in referrers {
            let abs = self.vault_root.join(&referrer);
            let Ok(content) = std::fs::read_to_string(&abs) else {
                continue;
            };
            let Some(rewritten) = rewrite_links(&content, &old_rel, &new_rel) else {
                continue;
            };
            if atomic_save(&abs, rewritten.as_bytes()).is_ok() {
                self.replace_open_note(&referrer.to_string_lossy(), &rewritten);
            }
        }

        self.rename_open_documents(&old_rel, &new_rel);
        let new_rel = new_rel.to_string_lossy().to_string();
        self.tree_selected = Some(new_rel.clone());
        self.refresh_after_file_change();
        self.status = format!("renamed {target} to {new_rel}");
    }

    fn rename_open_documents(&mut self, old: &Path, new: &Path) {
        let mut changes = Vec::new();
        for pane in self.ws.panes.panes() {
            for tab in pane.tabs() {
                let Some(doc) = self.ws.docs.get(tab.document) else {
                    continue;
                };
                let path = Path::new(&doc.path);
                let replacement = if path == old {
                    Some(new.to_path_buf())
                } else {
                    path.strip_prefix(old).ok().map(|suffix| new.join(suffix))
                };
                if let Some(replacement) = replacement {
                    changes.push((tab.document, replacement.to_string_lossy().to_string()));
                }
            }
        }
        changes.sort_by_key(|(id, _)| *id);
        changes.dedup_by_key(|(id, _)| *id);
        for (id, path) in changes {
            if self.ws.docs.rename(id, &path) {
                if let Some(session) = self.sessions.md.get_mut(&id) {
                    session.rel_path = path.clone();
                }
                if let Some(session) = self.sessions.pdf.get_mut(&id) {
                    session.rel_path = path;
                }
            }
        }
    }

    fn replace_open_note(&mut self, rel_path: &str, content: &str) {
        for session in self.sessions.md.values_mut() {
            if session.rel_path == rel_path && session.doc.buffer().text() != content {
                session.apply(Command::SelectAll);
                session.apply(Command::Insert(content.to_string()));
                session.doc.mark_saved();
            }
        }
    }

    pub(super) fn delete_path(&mut self, target: &str) {
        let abs = self.vault_root.join(target);
        let result = if abs.is_dir() {
            std::fs::remove_dir_all(&abs)
        } else {
            std::fs::remove_file(&abs)
        };
        if let Err(error) = result {
            self.status = format!("delete {target}: {error}");
            return;
        }

        let target_path = Path::new(target);
        let tabs: Vec<TabId> = self
            .ws
            .panes
            .panes()
            .into_iter()
            .flat_map(|pane| pane.tabs())
            .filter_map(|tab| {
                let path = Path::new(&self.ws.docs.get(tab.document)?.path);
                (path == target_path || path.starts_with(target_path)).then_some(tab.id)
            })
            .collect();
        for tab in tabs {
            let _ = self.ws.close_tab(tab);
        }
        self.sessions.gc(&self.ws.docs);
        self.tree_selected = None;
        self.refresh_after_file_change();
        self.status = format!("deleted {target}");
    }

    fn refresh_after_file_change(&mut self) {
        self.files = scan_vault(&self.vault_root);
        self.ensure_index();
        if let Some(mut index) = self.index.take() {
            if let Err(error) = self.sync_index(&mut index) {
                self.status = format!("index: {error}");
            }
            self.index = Some(index);
        }
        self.save_session();
    }

    pub(super) fn open_document(&mut self, rel: &str) {
        let pane = self
            .ws
            .focused_pane()
            .unwrap_or_else(|| self.ws.panes.first_pane());
        if self.open_document_in(pane, rel).is_some() {
            if self.tree_open {
                self.files = scan_vault(&self.vault_root);
            }
            self.save_session();
        }
    }

    /// Open `rel` as a tab of `pane`. Returns tab on success.
    pub(super) fn open_document_in(&mut self, pane: PaneId, rel: &str) -> Option<TabId> {
        let kind = if rel.ends_with(".pdf") {
            EditorKind::Pdf
        } else {
            EditorKind::Markdown
        };
        let tab = match self.ws.open_in(pane, rel, kind) {
            Ok(tab) => tab,
            Err(error) => {
                self.status = error.to_string();
                return None;
            }
        };
        let doc = self.tab_document(tab)?;
        let abs = self.vault_root.join(rel);
        let worker = self.pdf_worker.clone();
        match kind {
            EditorKind::Markdown => {
                if let std::collections::hash_map::Entry::Vacant(entry) =
                    self.sessions.md.entry(doc)
                {
                    match std::fs::read_to_string(&abs) {
                        Ok(text) => {
                            entry.insert(MdSession::new(rel, &text, self.measurer.clone()));
                        }
                        Err(error) => {
                            self.status = format!("open {rel}: {error}");
                            let _ = self.ws.close_tab(tab);
                            self.sessions.gc(&self.ws.docs);
                            return None;
                        }
                    }
                }
            }
            EditorKind::Pdf => {
                self.ensure_annotations();
                let hash = md3_vault::document_hash(&abs).ok();
                if let (Some(store), Some(hash)) = (self.annotations.as_mut(), &hash) {
                    let _ = store.record_document(hash, rel);
                }
                let entry = self
                    .sessions
                    .pdf
                    .entry(doc)
                    .or_insert_with(|| PdfSession::new(rel));
                entry.doc_hash = hash;
                pdf_view::load_geometry(entry, &abs);
                pdf_view::ensure_tiles(entry, &abs, worker.as_ref());
                if let Some(store) = self.annotations.as_ref() {
                    refresh_annotations(store, entry);
                }
            }
            _ => {}
        }
        if matches!(kind, EditorKind::Markdown) {
            self.schedule_open_markdown_work();
        }
        self.sync_status();
        Some(tab)
    }
}

impl Shell {
    pub(super) fn run_file_command(&mut self, cmd: &str) -> Option<Task<Message>> {
        match cmd {
            "file.quick-open" => {
                self.files = super::scan_vault(&self.vault_root);
                self.open_overlay(Overlay::QuickOpen {
                    input: String::new(),
                    selected: 0,
                });
            }
            "vault.open" => {
                return Some(Task::perform(
                    crate::vault_picker::pick_vault_async(),
                    Message::VaultPicked,
                ));
            }
            "file.new-note" => {
                self.open_overlay(Overlay::NameInput {
                    purpose: NamePurpose::NewNote {
                        parent: self.selected_parent(),
                    },
                    input: String::new(),
                });
            }
            "file.new-folder" => {
                self.open_overlay(Overlay::NameInput {
                    purpose: NamePurpose::NewFolder {
                        parent: self.selected_parent(),
                    },
                    input: String::new(),
                });
            }
            "file.rename" => {
                let Some(target) = self.selected_target() else {
                    self.status = "rename: select a file or folder".to_string();
                    return Some(Task::none());
                };
                let input = std::path::Path::new(&target)
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();
                self.open_overlay(Overlay::NameInput {
                    purpose: NamePurpose::Rename { target },
                    input,
                });
            }
            "file.delete" => {
                let Some(target) = self.selected_target() else {
                    self.status = "delete: select a file or folder".to_string();
                    return Some(Task::none());
                };
                let is_dir = self.vault_root.join(&target).is_dir();
                self.open_overlay(Overlay::ConfirmDelete { target, is_dir });
            }
            "workspace.refresh-files" => {
                self.files = super::scan_vault(&self.vault_root);
                return Some(self.success("File panel refreshed"));
            }
            "workspace.collapse-files" => {
                self.tree_expanded.clear();
                self.save_session();
            }
            "workspace.split-right" | "workspace.split-down" => {
                let focused = self.focused_doc_info();
                let axis = if cmd == "workspace.split-down" {
                    md3_kernel::SplitAxis::Vertical
                } else {
                    md3_kernel::SplitAxis::Horizontal
                };
                match focused {
                    Some((path, kind)) => {
                        if let Err(e) = self.ws.open_in_new_split(&path, kind, axis) {
                            self.status = e.to_string();
                        }
                    }
                    None => {
                        let pane = self.ws.panes.first_pane();
                        if let Err(error) = self.ws.panes.split(pane, axis) {
                            self.status = error.to_string();
                        }
                    }
                }
            }
            "workspace.close-tab" => {
                if let Some(tab) = self.ws.focused_tab() {
                    if self.is_tab_dirty(tab) {
                        let name = self.tab_name(tab);
                        self.open_overlay(Overlay::Confirm {
                            message: format!("Abandon unsaved changes in `{name}`?"),
                            on_confirm: CommandId("workspace.force-close-tab"),
                        });
                    } else {
                        self.close_tab(tab);
                    }
                }
            }
            "workspace.force-close-tab" => {
                if let Some(tab) = self.ws.focused_tab() {
                    self.close_tab(tab);
                }
            }
            "workspace.close-pane" => {
                let pane = self
                    .ws
                    .focused_pane()
                    .unwrap_or_else(|| self.ws.panes.first_pane());
                self.close_pane(pane);
            }
            "workspace.next-tab" => self.cycle_tab(),
            "workspace.toggle-files" => {
                self.tree_open = !self.tree_open;
                if self.tree_open {
                    self.files = super::scan_vault(&self.vault_root);
                }
                self.save_session();
            }
            "workspace.toggle-tracker" => {
                self.tracker_open = !self.tracker_open;
                self.save_session();
            }

            _ => return None,
        }
        Some(Task::none())
    }
}

impl Shell {
    pub(super) fn handle_tree_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TreeFileClicked(rel_path) => {
                self.tree_selected = Some(rel_path.clone());
                self.open_document(&rel_path);
                Task::none()
            }
            Message::TreeDirToggled(dir_path) => {
                self.tree_selected = Some(dir_path.clone());
                if self.tree_expanded.contains(&dir_path) {
                    self.tree_expanded.remove(&dir_path);
                } else {
                    self.tree_expanded.insert(dir_path);
                }
                self.save_session();
                Task::none()
            }
            Message::TreeContextRequested { rel_path, is_dir } => {
                self.close_overlay();
                self.close_menu();
                self.tree_selected = Some(rel_path.clone());
                self.tree_context = Some((rel_path, is_dir));
                self.ws.open_overlay("file-context");
                Task::none()
            }
            Message::TreeContextCommand(command) => {
                self.close_tree_context();
                self.run_command(command)
            }
            Message::TreeContextOpen { split } => {
                let target = self.tree_context.as_ref().map(|(path, _)| path.clone());
                self.close_tree_context();
                let Some(path) = target else {
                    return Task::none();
                };
                if split {
                    let pane = self
                        .ws
                        .focused_pane()
                        .unwrap_or_else(|| self.ws.panes.first_pane());
                    match self.ws.panes.split(pane, SplitAxis::Horizontal) {
                        Ok(pane) => {
                            let _ = self.open_document_in(pane, &path);
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                } else {
                    self.open_document(&path);
                }
                Task::none()
            }
            Message::TreeContextClosed => {
                self.close_tree_context();
                Task::none()
            }
            Message::TreeResizeStarted => {
                self.tree_resizing = true;
                Task::none()
            }
            Message::TreeResized(x) => {
                self.tree_width = x.clamp(160.0, 480.0);
                Task::none()
            }
            Message::TreeResizeFinished => {
                self.tree_resizing = false;
                self.save_session();
                Task::none()
            }

            _ => Task::none(),
        }
    }
}

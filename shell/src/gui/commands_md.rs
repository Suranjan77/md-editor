use super::*;

impl Shell {
    pub(super) fn save_focused(&mut self) -> Task<Message> {
        let root = self.vault_root.clone();
        let Some(session) = self.focused_md_mut() else {
            return Task::none();
        };
        let abs = root.join(&session.rel_path);
        let text = session.doc.buffer().text();
        match md_vault::atomic_save(&abs, text.as_bytes()) {
            Ok(()) => {
                session.doc.mark_saved();
                let rel = session.rel_path.clone();
                if let Some(index) = self.index.as_mut() {
                    let _ = index.sync_paths(&root, &[abs]);
                }
                if self.tree_open {
                    self.files = scan_vault(&root);
                }
                self.success(format!("Saved {rel}"))
            }
            Err(error) => self.error(format!("Save failed: {error}")),
        }
    }

    pub(super) fn find_in_note(&mut self, needle: &str) {
        if needle.is_empty() {
            return;
        }
        let Some(session) = self.focused_md_mut() else {
            return;
        };
        let text = session.doc.buffer().text();
        let from = session.doc.buffer().primary().head;
        let hit = text[from.min(text.len())..]
            .find(needle)
            .map(|index| index + from)
            .or_else(|| text.find(needle));
        match hit {
            Some(offset) => {
                let (line, col) = session.doc.buffer().offset_to_line_col(offset);
                session.apply(Command::SetCursor { line, col });
            }
            None => self.status = format!("not found: {needle}"),
        }
    }

    pub(super) fn note_backlinks(&self, rel_path: &str) -> Vec<String> {
        let mut graph = md_vault::LinkGraph::new();
        for rel in scan_vault(&self.vault_root) {
            if !rel.ends_with(".md") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(self.vault_root.join(&rel)) else {
                continue;
            };
            graph.update_file(Path::new(&rel), &content);
        }
        graph
            .backlinks(Path::new(rel_path))
            .into_iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect()
    }

    pub(super) fn search_vault(&mut self, query: &str) -> Vec<md_vault::Hit> {
        let Some(index) = self.index.as_ref() else {
            return Vec::new();
        };
        index.search(query, 50).unwrap_or_default()
    }
}

impl Shell {
    pub(super) fn run_md_command(&mut self, cmd: &str) -> Option<Task<Message>> {
        match cmd {
            "note.outline-panel" => {
                if let Some(session) = self.focused_md_mut() {
                    session.outline_open = !session.outline_open;
                    self.save_session();
                }
            }
            "note.backlinks" => {
                let Some((path, md_kernel::input::EditorKind::Markdown)) = self.focused_doc_info()
                else {
                    self.status = "backlinks: focus a note".to_string();
                    return Some(Task::none());
                };
                let referrers = self.note_backlinks(&path);
                if referrers.is_empty() {
                    self.status = format!("no backlinks to {path}");
                    return Some(Task::none());
                }
                self.open_overlay(Overlay::Backlinks {
                    input: String::new(),
                    selected: 0,
                    referrers,
                });
            }
            "editor.undo" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(md_editor::buffer::Command::Undo);
                }
            }
            "editor.redo" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(md_editor::buffer::Command::Redo);
                }
            }
            "editor.select-all" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(md_editor::buffer::Command::SelectAll);
                }
            }
            "editor.copy" => {
                let text = self
                    .focused_md_mut()
                    .and_then(|s| s.doc.buffer().selected_text())
                    .filter(|t| !t.is_empty());
                return match text {
                    Some(text) => {
                        self.status = format!("{} chars copied", text.chars().count());
                        Some(iced::clipboard::write(text))
                    }
                    None => {
                        self.status = "nothing selected".to_string();
                        Some(Task::none())
                    }
                };
            }
            "editor.cut" => {
                let text = self
                    .focused_md_mut()
                    .and_then(|s| s.doc.buffer().selected_text())
                    .filter(|t| !t.is_empty());
                if let Some(text) = text {
                    if let Some(s) = self.focused_md_mut() {
                        s.apply(md_editor::buffer::Command::DeleteBackward);
                    }
                    self.status = format!("{} chars cut", text.chars().count());
                    return Some(iced::clipboard::write(text));
                }
                self.status = "nothing selected".to_string();
                return Some(Task::none());
            }
            "editor.toggle-bold" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(md_editor::buffer::Command::ToggleBold);
                }
            }
            "editor.toggle-italic" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(md_editor::buffer::Command::ToggleItalic);
                }
            }
            "editor.toggle-code" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(md_editor::buffer::Command::ToggleCode);
                }
            }
            "editor.heading-cycle" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(md_editor::buffer::Command::HeadingCycle);
                }
            }
            "editor.heading-1" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(md_editor::buffer::Command::SetHeading(1));
                }
            }
            "editor.heading-2" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(md_editor::buffer::Command::SetHeading(2));
                }
            }
            "editor.heading-3" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(md_editor::buffer::Command::SetHeading(3));
                }
            }
            "editor.heading-4" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(md_editor::buffer::Command::SetHeading(4));
                }
            }
            "editor.heading-5" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(md_editor::buffer::Command::SetHeading(5));
                }
            }
            "editor.heading-6" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(md_editor::buffer::Command::SetHeading(6));
                }
            }
            "editor.toggle-bullet" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(md_editor::buffer::Command::ToggleBullet);
                }
            }
            "editor.toggle-checkbox" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(md_editor::buffer::Command::ToggleCheckbox);
                }
            }
            "editor.toggle-wikilink" => {
                if let Some(s) = self.focused_md_mut() {
                    s.apply(md_editor::buffer::Command::ToggleWikilink);
                }
            }
            "editor.save" => return Some(self.save_focused()),
            "editor.find" => {
                if let Some(session) = self.focused_md_mut() {
                    session.find_open = !session.find_open;
                    self.save_session();
                }
            }

            _ => return None,
        }
        Some(Task::none())
    }
}

impl Shell {
    pub(super) fn handle_md_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::EditorDragSelect { tab, anchor, head } => {
                if let Err(e) = self.ws.focus_tab(tab) {
                    self.status = e.to_string();
                }
                if let Some(session) = self.focused_md_mut() {
                    session.apply(Command::SetSelections(vec![
                        md_editor::buffer::Selection::new(anchor, head),
                    ]));
                }
                Task::none()
            }
            Message::EditorClicked {
                tab,
                line,
                col,
                viewport_h,
                checkbox,
                ctrl,
            } => {
                if let Err(e) = self.ws.focus_tab(tab) {
                    self.status = e.to_string();
                }
                let mut activation = None;
                if let Some(session) = self.focused_md_mut() {
                    session.viewport_h = viewport_h;
                    session.apply(Command::SetCursor { line, col });
                    if ctrl {
                        activation = super::markdown_assets::activation_at(session, line, col);
                    }
                }
                if checkbox {
                    return self.run_command(CommandId("editor.toggle-checkbox"));
                }
                match activation {
                    Some(super::markdown_assets::MarkdownActivation::Uri(url)) => {
                        self.status = format!("link: {url}");
                        if std::env::var("MD3_TEST_MODE").is_err() {
                            std::thread::spawn(move || {
                                let _ = open::that(url);
                            });
                        }
                    }
                    Some(super::markdown_assets::MarkdownActivation::WikiLink(target)) => {
                        let target = target
                            .split('|')
                            .next()
                            .unwrap_or_default()
                            .split('#')
                            .next()
                            .unwrap_or_default();
                        let path = md_vault::resolve_target(target)
                            .to_string_lossy()
                            .to_string();
                        let _ = self.open_document(&path);
                    }
                    None => {}
                }
                self.sync_status();
                Task::none()
            }
            Message::EditorScrolled {
                tab,
                dy,
                viewport_h,
            } => {
                let reduce_motion = self.reduce_motion;
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    session.viewport_h = viewport_h;
                    session.scroll_by_animated(dy, reduce_motion);
                }
                Task::none()
            }
            Message::EditorViewportChanged { tab, width, height } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    session.set_viewport(width, height);
                }
                Task::none()
            }
            Message::MdJumpToLine { tab, line } => {
                if let Err(error) = self.ws.focus_tab(tab) {
                    self.status = error.to_string();
                    return Task::none();
                }
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    session.apply(Command::SetCursor { line, col: 0 });
                }
                self.sync_status();
                Task::none()
            }
            Message::MdFindQueryChanged { tab, query } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    session.find_query = query;
                    let text = session.doc.buffer().text();
                    let matches = find_all_matches(&text, &session.find_query);
                    if !matches.is_empty() {
                        let head = session.doc.buffer().primary().head;
                        let target = matches
                            .iter()
                            .find(|&&(start, _)| start >= head)
                            .or(matches.first())
                            .copied();
                        if let Some((start, end)) = target {
                            session.apply(Command::SetSelections(vec![Selection::new(start, end)]));
                        }
                    }
                }
                Task::none()
            }
            Message::MdReplaceTextChanged { tab, text } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    session.replace_text = text;
                }
                Task::none()
            }
            Message::MdFindNext { tab } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    let text = session.doc.buffer().text();
                    let matches = find_all_matches(&text, &session.find_query);
                    if !matches.is_empty() {
                        let head = session.doc.buffer().primary().head;
                        let target = matches
                            .iter()
                            .find(|&&(start, _)| start >= head)
                            .or(matches.first())
                            .copied();
                        if let Some((start, end)) = target {
                            session.apply(Command::SetSelections(vec![Selection::new(start, end)]));
                        }
                    }
                }
                Task::none()
            }
            Message::MdFindPrev { tab } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    let text = session.doc.buffer().text();
                    let matches = find_all_matches(&text, &session.find_query);
                    if !matches.is_empty() {
                        let primary = session.doc.buffer().primary();
                        let caret_start = primary.anchor.min(primary.head);
                        let target = matches
                            .iter()
                            .rfind(|&&(start, _)| start < caret_start)
                            .or(matches.last())
                            .copied();
                        if let Some((start, end)) = target {
                            session.apply(Command::SetSelections(vec![Selection::new(start, end)]));
                        }
                    }
                }
                Task::none()
            }
            Message::MdReplace { tab } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    let text = session.doc.buffer().text();
                    let primary = session.doc.buffer().primary();
                    let (caret_start, caret_end) = (
                        primary.anchor.min(primary.head),
                        primary.anchor.max(primary.head),
                    );
                    let matches = find_all_matches(&text, &session.find_query);
                    if matches
                        .iter()
                        .any(|&(s, e)| s == caret_start && e == caret_end)
                    {
                        session.apply(Command::Insert(session.replace_text.clone()));
                        let new_text = session.doc.buffer().text();
                        let new_matches = find_all_matches(&new_text, &session.find_query);
                        if !new_matches.is_empty() {
                            let new_head = session.doc.buffer().primary().head;
                            let next_match = new_matches
                                .iter()
                                .find(|&&(start, _)| start >= new_head)
                                .or(new_matches.first())
                                .copied();
                            if let Some((start, end)) = next_match {
                                session.apply(Command::SetSelections(vec![Selection::new(
                                    start, end,
                                )]));
                            }
                        }
                    } else if !matches.is_empty() {
                        let head = session.doc.buffer().primary().head;
                        let next_match = matches
                            .iter()
                            .find(|&&(start, _)| start >= head)
                            .or(matches.first())
                            .copied();
                        if let Some((start, end)) = next_match {
                            session.apply(Command::SetSelections(vec![Selection::new(start, end)]));
                        }
                    }
                    self.save_session();
                }
                Task::none()
            }
            Message::MdReplaceAll { tab } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    let text = session.doc.buffer().text();
                    let matches = find_all_matches(&text, &session.find_query);
                    if !matches.is_empty() {
                        let selections = matches
                            .into_iter()
                            .map(|(start, end)| Selection::new(start, end))
                            .collect::<Vec<_>>();
                        session.apply(Command::SetSelections(selections));
                        session.apply(Command::Insert(session.replace_text.clone()));
                        self.status = "replaced all occurrences".to_string();
                    }
                    self.save_session();
                }
                Task::none()
            }
            Message::MdCloseFind { tab } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.md.get_mut(&d)) {
                    session.find_open = false;
                    self.save_session();
                }
                Task::none()
            }

            _ => Task::none(),
        }
    }
}

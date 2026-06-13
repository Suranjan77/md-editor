use super::*;

impl Shell {
    pub(super) fn save_focused(&mut self) -> Task<Message> {
        let root = self.vault_root.clone();
        let Some(session) = self.focused_md_mut() else {
            return Task::none();
        };
        let abs = root.join(&session.rel_path);
        let text = session.doc.buffer().text();
        match md3_vault::atomic_save(&abs, text.as_bytes()) {
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
        let mut graph = md3_vault::LinkGraph::new();
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

    pub(super) fn search_vault(&mut self, query: &str) -> Vec<md3_vault::Hit> {
        let Some(index) = self.index.as_ref() else {
            return Vec::new();
        };
        index.search(query, 50).unwrap_or_default()
    }
}

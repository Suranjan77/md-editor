//! Inline completion in notes: opening, narrowing and accepting the `[[` / `/` popup as the user types.

use super::*;
use crate::autocomplete::{self, Completion, CompletionKind};
use crate::messages::EditorMessage;

impl MdEditor {
    pub(super) fn update_completion_message(&mut self, message: EditorMessage) -> Task<Message> {
        match message {
            EditorMessage::CompletionMove(delta) => {
                if let Some(completion) = &mut self.editor.completion {
                    completion.move_selection(delta);
                }
                Task::none()
            }
            EditorMessage::CompletionAccept => {
                let index = self.editor.completion.as_ref().map(|c| c.selected);
                index.map_or_else(Task::none, |i| self.accept_completion(i))
            }
            EditorMessage::CompletionPick(index) => self.accept_completion(index),
            EditorMessage::CompletionClose => {
                self.editor.completion = None;
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// After an editor command: typing opens or narrows the popup; moving the
    /// caret away from the trigger (or deleting it) closes it.
    pub(super) fn refresh_completion(&mut self, text_changed: bool) {
        if self.active_path.is_none() || self.editor.buffer.selection.is_some() {
            self.editor.completion = None;
            return;
        }
        let buffer = &self.editor.buffer;
        let (line, col) = (buffer.cursor_line, buffer.cursor_col);
        let before: String = buffer.line_text(line).chars().take(col).collect();
        let detected = autocomplete::detect(&before);
        let same_trigger =
            |c: &Completion, kind, at| c.line == line && c.kind == kind && c.trigger_col == at;
        let keep = match (&self.editor.completion, &detected) {
            (Some(open), Some((kind, at, _))) => text_changed || same_trigger(open, *kind, *at),
            (None, Some(_)) => text_changed,
            (_, None) => false,
        };
        let Some((kind, trigger_col, query)) = detected.filter(|_| keep) else {
            self.editor.completion = None;
            return;
        };
        let items = match kind {
            CompletionKind::Wikilink => self.wikilink_completion_items(&query),
            CompletionKind::Citation => autocomplete::citation_items(&self.vault.citations, &query),
            CompletionKind::Slash => autocomplete::slash_items(&query),
        };
        if items.is_empty() {
            self.editor.completion = None;
            return;
        }
        let selected = self
            .editor
            .completion
            .as_ref()
            .filter(|c| c.query == query)
            .map_or(0, |c| c.selected.min(items.len() - 1));
        self.editor.completion = Some(Completion {
            kind,
            line,
            trigger_col,
            query,
            items,
            selected,
        });
    }

    /// Replace the trigger and query (and the `]]` auto-pairing typed ahead)
    /// with the chosen item, as one undo step.
    fn accept_completion(&mut self, index: usize) -> Task<Message> {
        let Some(completion) = self.editor.completion.take() else {
            return Task::none();
        };
        let Some(item) = completion.items.get(index).cloned() else {
            return Task::none();
        };
        let buffer = &self.editor.buffer;
        let (line, col) = (buffer.cursor_line, buffer.cursor_col);
        let after: String = buffer.line_text(line).chars().skip(col).take(2).collect();
        let end_col = if completion.kind == CompletionKind::Wikilink && after == "]]" {
            col + 2
        } else {
            col
        };
        self.editor.buffer.execute(EditorCommand::SetSelection {
            anchor_line: line,
            anchor_col: completion.trigger_col,
            focus_line: line,
            focus_col: end_col,
            affinity: Affinity::Downstream,
        });
        let task = self.run_editor_command(EditorCommand::InsertText(item.insert));
        for _ in 0..item.caret_back {
            self.editor.buffer.execute(EditorCommand::MoveCursor {
                movement: crate::editor::buffer::Movement::Left,
                extend: false,
            });
        }
        // Inserting may itself look like a trigger (`[[Name]]` does not, but a
        // `/` template could); the user chose, so leave the popup closed.
        self.editor.completion = None;
        task
    }

    fn wikilink_completion_items(&self, query: &str) -> Vec<autocomplete::CompletionItem> {
        let all_paths: Vec<&str> = self
            .vault
            .entries
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| e.path.as_str())
            .collect();

        if let Some((note_part, heading_query)) = query.split_once('#') {
            let note_part = note_part.trim();
            if note_part.is_empty() {
                let headings: Vec<String> = self
                    .editor
                    .buffer
                    .toc()
                    .into_iter()
                    .map(|entry| entry.text)
                    .collect();
                return autocomplete::heading_items(None, &headings, heading_query);
            }

            let resolved = self.resolve_completion_note_path(note_part);
            let Some(rel_path) = resolved else {
                return Vec::new();
            };
            let Some(text) = self.read_note_text_for_completion(&rel_path) else {
                return Vec::new();
            };
            let headings: Vec<String> = crate::views::toc::get_toc(&text)
                .into_iter()
                .map(|entry| entry.text)
                .collect();
            let target = autocomplete::wikilink_target_for(&all_paths, &rel_path);
            return autocomplete::heading_items(Some(&target), &headings, heading_query);
        }

        let paths: Vec<&str> = all_paths
            .into_iter()
            .filter(|p| Some(*p) != self.active_path.as_deref())
            .collect();
        autocomplete::wikilink_items(&paths, query)
    }

    fn resolve_completion_note_path(&self, raw: &str) -> Option<String> {
        let trimmed = raw.trim().replace('\\', "/");
        if let Some(exact) = self.vault.entries.iter().find(|e| {
            !e.is_dir
                && (e.path.eq_ignore_ascii_case(&trimmed)
                    || e.path.eq_ignore_ascii_case(&format!("{trimmed}.md"))
                    || e.path.eq_ignore_ascii_case(&format!("{trimmed}.markdown")))
        }) {
            return Some(exact.path.clone());
        }
        super::links::resolve_vault_note_by_name(&self.vault.entries, &trimmed)
    }

    fn read_note_text_for_completion(&self, rel_path: &str) -> Option<String> {
        const MAX_HEADING_BYTES: u64 = 512 * 1024;
        if self.active_path.as_deref() == Some(rel_path) {
            return Some(self.editor.buffer.text());
        }
        if let Some(parked) = self.parked()
            && parked.active_path.as_deref() == Some(rel_path)
        {
            return Some(parked.editor.buffer.text());
        }
        let root = self.vault.root.as_deref()?;
        let abs =
            md_editor_core::vault::resolve_vault_path_checked(std::path::Path::new(root), rel_path)
                .ok()?;
        md_editor_core::vault::read_file_capped(&abs, MAX_HEADING_BYTES).ok()
    }
}

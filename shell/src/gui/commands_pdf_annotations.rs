use super::*;
use md_vault::{NewAnnotation, Quad};

/// Highlight color cycle (`pdf.highlight-color`); new annotations start at
/// the first entry. Stored per annotation (`#rrggbb`, schema column).
const HIGHLIGHT_PALETTE: [&str; 4] = ["#ffd866", "#a9dc76", "#78dce8", "#ab9df2"];

/// Default highlight color for new annotations.
const HIGHLIGHT_COLOR: &str = HIGHLIGHT_PALETTE[0];

impl Shell {
    pub(super) fn highlight_selection(&mut self) {
        self.ensure_annotations();
        let Some(doc) = self.ws.focused_tab().and_then(|tab| self.tab_document(tab)) else {
            return;
        };
        let (Some(store), Some(session)) =
            (self.annotations.as_mut(), self.sessions.pdf.get_mut(&doc))
        else {
            return;
        };
        let Some(hash) = session.doc_hash.clone() else {
            self.status = "cannot annotate: file was unreadable on open".to_string();
            return;
        };
        let Some(selection) = session
            .selection
            .take_if(|selection| !selection.text.is_empty())
        else {
            self.status = "select text first (drag over the page)".to_string();
            return;
        };
        let annotation = NewAnnotation {
            doc_hash: hash,
            page: selection.page,
            quads: selection
                .quads
                .iter()
                .map(|quad| Quad {
                    x0: f64::from(quad.x0),
                    y0: f64::from(quad.y0),
                    x1: f64::from(quad.x1),
                    y1: f64::from(quad.y1),
                })
                .collect(),
            color: HIGHLIGHT_COLOR.to_string(),
            note: String::new(),
            linked_note: None,
        };
        match store.add(annotation) {
            Ok(id) => {
                session.selected_annotation = Some(id);
                refresh_annotations(store, session);
                self.status = "highlighted · ctrl+n adds a note".to_string();
            }
            Err(error) => {
                session.selection = Some(selection);
                self.status = format!("highlight failed: {error}");
            }
        }
    }

    pub(super) fn export_annotations(&mut self) -> Task<Message> {
        self.ensure_annotations();
        let root = self.vault_root.clone();
        let Some(session) = self.focused_pdf() else {
            return self.error("focus a pdf to export its annotations");
        };
        let (Some(store), Some(hash)) = (self.annotations.as_ref(), session.doc_hash.as_ref())
        else {
            return Task::none();
        };
        let rel = format!(
            "{}-annotations.md",
            session.rel_path.trim_end_matches(".pdf")
        );
        let markdown = match store.export_markdown(hash) {
            Ok(markdown) => markdown,
            Err(error) => return self.error(format!("export failed: {error}")),
        };
        let abs = root.join(&rel);
        match md_vault::atomic_save(&abs, markdown.as_bytes()) {
            Ok(()) => {
                if let Some(index) = self.index.as_mut() {
                    let _ = index.sync_paths(&root, &[abs]);
                }
                self.success(format!("annotations exported to {rel}"))
            }
            Err(error) => self.error(format!("export failed: {error}")),
        }
    }

    pub(super) fn cycle_highlight_color(&mut self) {
        let Some(doc) = self.ws.focused_tab().and_then(|tab| self.tab_document(tab)) else {
            return;
        };
        let (Some(store), Some(session)) =
            (self.annotations.as_mut(), self.sessions.pdf.get_mut(&doc))
        else {
            return;
        };
        let Some(current) = session.selected_annotation() else {
            self.status = "click a highlight first".to_string();
            return;
        };
        let id = current.id;
        let next = HIGHLIGHT_PALETTE
            .iter()
            .position(|color| *color == current.color)
            .map(|index| HIGHLIGHT_PALETTE[(index + 1) % HIGHLIGHT_PALETTE.len()])
            .unwrap_or(HIGHLIGHT_PALETTE[0]);
        match store.set_color(id, next) {
            Ok(()) => {
                refresh_annotations(store, session);
                self.status = format!("highlight color {next}");
            }
            Err(error) => self.status = format!("color failed: {error}"),
        }
    }

    pub(super) fn link_note_for_annotation(&mut self) {
        self.ensure_annotations();
        let root = self.vault_root.clone();
        let Some(doc) = self.ws.focused_tab().and_then(|tab| self.tab_document(tab)) else {
            return;
        };
        let (Some(store), Some(session)) =
            (self.annotations.as_mut(), self.sessions.pdf.get_mut(&doc))
        else {
            return;
        };
        let Some(current) = session.selected_annotation() else {
            self.status = "click a highlight first".to_string();
            return;
        };
        let id = current.id;
        let rel = current
            .linked_note
            .clone()
            .unwrap_or_else(|| format!("{}-notes.md", session.rel_path.trim_end_matches(".pdf")));
        let abs = root.join(&rel);
        if !abs.exists() {
            let seed = format!("# Notes — {}\n", session.rel_path);
            if let Err(error) = md_vault::atomic_save(&abs, seed.as_bytes()) {
                self.status = format!("linked note failed: {error}");
                return;
            }
            if let Some(index) = self.index.as_mut() {
                let _ = index.sync_paths(&root, &[abs]);
            }
        }
        match store.set_linked_note(id, &rel) {
            Ok(()) => {
                refresh_annotations(store, session);
                let _ = self.open_document(&rel);
                self.status = format!("linked note {rel}");
            }
            Err(error) => self.status = format!("linked note failed: {error}"),
        }
    }

    pub(super) fn orphan_report(&mut self) {
        self.ensure_annotations();
        let Some(store) = self.annotations.as_ref() else {
            self.status = "annotation store unavailable".to_string();
            return;
        };
        let known = match store.known_documents() {
            Ok(known) => known,
            Err(error) => {
                self.status = format!("orphan report failed: {error}");
                return;
            }
        };
        let live: std::collections::HashSet<String> = scan_vault(&self.vault_root)
            .iter()
            .filter(|rel| rel.ends_with(".pdf"))
            .filter_map(|rel| md_vault::document_hash(&self.vault_root.join(rel)).ok())
            .collect();
        let rows: Vec<(String, String)> = known
            .iter()
            .filter(|document| document.annotation_count > 0 && !live.contains(&document.doc_hash))
            .map(|document| {
                (
                    document.last_path.clone(),
                    format!("{} annotations", document.annotation_count),
                )
            })
            .collect();
        if rows.is_empty() {
            self.status = "no orphaned annotations".to_string();
            return;
        }
        self.open_overlay(Overlay::OrphanReport { rows });
    }

    pub(super) fn set_annotation_note(&mut self, note: &str) {
        let Some(doc) = self.ws.focused_tab().and_then(|tab| self.tab_document(tab)) else {
            return;
        };
        let (Some(store), Some(session)) =
            (self.annotations.as_mut(), self.sessions.pdf.get_mut(&doc))
        else {
            return;
        };
        let Some(id) = session.selected_annotation else {
            return;
        };
        match store.update_note(id, note) {
            Ok(()) => {
                refresh_annotations(store, session);
                self.status = "note saved".to_string();
            }
            Err(error) => self.status = format!("note failed: {error}"),
        }
    }

    pub(super) fn remove_selected_annotation(&mut self) {
        let Some(doc) = self.ws.focused_tab().and_then(|tab| self.tab_document(tab)) else {
            return;
        };
        let (Some(store), Some(session)) =
            (self.annotations.as_mut(), self.sessions.pdf.get_mut(&doc))
        else {
            return;
        };
        let Some(id) = session.selected_annotation.take() else {
            return;
        };
        match store.remove(id) {
            Ok(()) => {
                refresh_annotations(store, session);
                self.status = "highlight removed".to_string();
            }
            Err(error) => self.status = format!("remove failed: {error}"),
        }
    }
}

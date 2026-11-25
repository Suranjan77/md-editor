//! PDF selection and study handlers: text selection, copy, highlights, quick notes and linked notes.

use super::*;
use crate::messages::PdfMessage;

/// Seconds since the Unix epoch, for annotation timestamps.
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

impl MdEditor {
    /// `PdfMessage::SelectionChanged` / `SelectionFinished`.
    pub(super) fn on_pdf_selection(
        &mut self,
        page: u16,
        anchor: usize,
        focus: usize,
    ) -> Task<Message> {
        self.active_panel = ActivePanel::Pdf;
        self.pdf.selection = Some(views::interactive_pdf::PdfSelection {
            page_index: page,
            anchor_idx: anchor,
            focus_idx: focus,
        });
        Task::none()
    }

    /// `PdfMessage::CopySelection`: copy the selected PDF text to the clipboard.
    pub(super) fn on_pdf_copy_selection(&mut self) -> Task<Message> {
        if !self.pdf_copy_shortcut_is_active() {
            return Task::none();
        }
        if let Some(sel) = &self.pdf.selection
            && let Some(page_text) = self.pdf.page_text.get(&sel.page_index)
        {
            let start = sel.anchor_idx.min(sel.focus_idx);
            let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);
            let selected = text_by_char_range(&page_text.text, start, end);
            if !selected.is_empty() {
                return iced::clipboard::write(selected);
            }
        }
        Task::none()
    }

    /// `PdfMessage::QuickHighlight`: highlight with the next colour in the cycle.
    pub(super) fn on_pdf_quick_highlight(&mut self) -> Task<Message> {
        // Only consume a palette color when there is actually a selection to
        // highlight, so the cycle doesn't silently advance on empty clicks.
        if self.pdf.selection.is_none() {
            return Task::none();
        }
        let color = self.pdf.next_highlight_color;
        self.pdf.next_highlight_color = color.next();
        self.create_highlight(color)
    }

    /// `PdfMessage::CopyAnnotationText`.
    pub(super) fn on_pdf_copy_annotation_text(&mut self, id: String) -> Task<Message> {
        let Some(text) = self
            .pdf
            .annotations
            .values()
            .flatten()
            .find(|a| a.id == id)
            .map(|a| a.selected_text.clone())
        else {
            return Task::none();
        };
        self.ui.toast = Some("Annotation text copied".to_string());
        iced::clipboard::write(text)
    }

    /// Recompute the backlinks panel for the open PDF after its highlights changed.
    fn refresh_pdf_backlinks(&mut self) {
        if let Some(ref path) = self.pdf.active_path {
            self.vault.backlinks =
                md_editor_core::vault::get_mixed_backlinks(&self.state, path).unwrap_or_default();
        }
    }

    /// `PdfMessage::DeleteHighlight`: delete it, unfocus it and close its note modal.
    pub(super) fn on_pdf_delete_highlight(&mut self, id: String) -> Task<Message> {
        if let Err(e) = self.state.delete_pdf_annotation(&id) {
            self.ui.toast = Some(format!("Failed to delete highlight: {}", e));
            return Task::none();
        }
        for page_anns in self.pdf.annotations.values_mut() {
            page_anns.retain(|a| a.id != id);
        }
        if self.pdf.focused_annotation_id.as_ref() == Some(&id) {
            self.pdf.focused_annotation_id = None;
        }
        if let Some(views::modals::ModalType::QuickNote(ref mid)) = self.ui.active_modal
            && mid == &id
        {
            self.ui.active_modal = None;
            self.ui.modal_input.clear();
        }
        self.refresh_pdf_backlinks();
        self.invalidate_graph();
        Task::none()
    }

    /// `PdfMessage::AddQuickNote`: set a highlight's note text and persist it.
    pub(super) fn on_pdf_add_quick_note(&mut self, id: String, note: String) -> Task<Message> {
        let Some(ann) = self
            .pdf
            .annotations
            .values_mut()
            .flatten()
            .find(|a| a.id == id)
        else {
            return Task::none();
        };
        ann.note = Some(note);
        ann.updated_at = unix_now();
        let ann = ann.clone();
        if let Err(e) = self.state.save_pdf_annotation(&ann) {
            self.ui.toast = Some(format!("Failed to save note: {}", e));
        } else {
            self.refresh_pdf_backlinks();
        }
        Task::none()
    }

    /// `PdfMessage::LinkNote`: with an empty path, ask for one; otherwise create or
    /// extend that note with the highlight, link it and open it beside the PDF.
    pub(super) fn on_pdf_link_note(
        &mut self,
        annotation_id: String,
        note_path: String,
    ) -> Task<Message> {
        let Some(mut ann) = self
            .pdf
            .annotations
            .values()
            .flatten()
            .find(|a| a.id == annotation_id)
            .cloned()
        else {
            return Task::none();
        };
        if note_path.is_empty() {
            self.ui.modal_input = self.default_pdf_note_path(&ann);
            self.ui.link_note_picker_search.clear();
            self.ui.active_modal = Some(views::modals::ModalType::LinkNote(annotation_id));
            return Task::none();
        }

        let note_path = normalize_note_path(&note_path);
        let Some(ref pdf_path) = self.pdf.active_path else {
            return Task::none();
        };
        let pdf_path = pdf_path.clone();
        if let Err(e) = self.write_linked_pdf_note(&note_path, &pdf_path, &ann) {
            self.ui.toast = Some(format!("Failed to create linked note: {}", e));
            return Task::none();
        }

        ann.linked_note_path = Some(note_path.clone());
        ann.updated_at = unix_now();
        if let Err(e) = self.state.save_pdf_annotation(&ann) {
            self.ui.toast = Some(format!("Failed to link note: {}", e));
            return Task::none();
        }
        if let Some(a) = self
            .pdf
            .annotations
            .values_mut()
            .flatten()
            .find(|a| a.id == annotation_id)
        {
            a.linked_note_path = Some(note_path.clone());
            a.updated_at = ann.updated_at;
        }
        self.vault.entries = md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
        self.ui.toast = Some(format!("Linked note: {}", note_path));
        self.invalidate_graph();
        Task::done(Message::Pdf(PdfMessage::OpenLinkedNote(note_path)))
    }

    /// Add the highlight's section to `note_path`. A note open in the editor is
    /// edited in its buffer (one undo step) and flushed, so unsaved typing and
    /// undo history survive; a parked note gets the same edit, so its history
    /// still matches disk when it is resumed.
    fn write_linked_pdf_note(
        &mut self,
        note_path: &str,
        pdf_path: &str,
        ann: &md_editor_core::pdf::PdfAnnotation,
    ) -> Result<(), String> {
        if self.active_path.as_deref() == Some(note_path) {
            let current = self.editor.buffer.text();
            let updated = append_linked_pdf_note_section(&current, pdf_path, ann);
            if updated != current {
                self.editor.buffer.replace_all_text(&updated);
                self.editor.buffer_revision = self.editor.buffer_revision.wrapping_add(1);
                self.editor.mark_dirty_now();
                self.editor.toc_entries = views::toc::get_toc(&updated);
            }
            return match self.save_active_document() {
                SaveOutcome::Failed(err) => Err(err.to_string()),
                SaveOutcome::Saved | SaveOutcome::NothingToDo => Ok(()),
            };
        }
        let content = self.linked_pdf_note_file_content(note_path, pdf_path, ann);
        md_editor_core::vault::save_file(&self.state, note_path, &content)
            .map_err(|e| e.to_string())?;
        self.note_self_write(note_path);
        self.editor.update_retained(note_path, |buffer| {
            buffer.replace_all_text(&content);
            buffer.mark_saved();
        });
        Ok(())
    }

    pub(super) fn default_pdf_note_path(&self, ann: &md_editor_core::pdf::PdfAnnotation) -> String {
        let pdf_filename = self
            .pdf
            .active_path
            .as_deref()
            .and_then(|pdf_path| std::path::Path::new(pdf_path).file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("document");
        let clean_pdf_name = slug_fragment(pdf_filename);
        format!(
            "pdf-notes/{}-p{}-{}.md",
            clean_pdf_name,
            ann.page_index + 1,
            &ann.id[..8.min(ann.id.len())]
        )
    }

    pub(super) fn linked_pdf_note_file_content(
        &self,
        note_path: &str,
        pdf_path: &str,
        ann: &md_editor_core::pdf::PdfAnnotation,
    ) -> String {
        match md_editor_core::vault::open_file(&self.state, note_path)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
        {
            Some(existing) => append_linked_pdf_note_section(&existing, pdf_path, ann),
            None => new_linked_pdf_note_content(note_path, pdf_path, ann),
        }
    }

    pub(super) fn pdf_copy_shortcut_is_active(&self) -> bool {
        self.pdf.selection.is_some()
            && self.pdf.active_path.is_some()
            && (self.showing_pdf
                || (self.ui.split_view_active
                    && self.active_path.is_some()
                    && self.active_panel == ActivePanel::Pdf))
    }

    /// Create a highlight annotation over the current PDF text selection.
    /// Shared by the explicit color buttons and the cycling quick highlight.
    pub(super) fn create_highlight(
        &mut self,
        color: md_editor_core::pdf::PdfAnnotationColor,
    ) -> Task<Message> {
        if self.pdf.selection.is_some() && self.pdf.document_id.is_none() {
            // Highlights are keyed by the document id, computed right after
            // opening. Say so rather than dropping the click.
            self.ui.toast = Some("The PDF is still opening; highlight again in a moment".into());
            return Task::none();
        }
        if let (Some(sel), Some(doc_id)) = (&self.pdf.selection, &self.pdf.document_id)
            && let Some(page_text) = self.pdf.page_text.get(&sel.page_index)
        {
            let start = sel.anchor_idx.min(sel.focus_idx);
            let end = sel.anchor_idx.max(sel.focus_idx).saturating_add(1);

            let mut selected_chars = Vec::new();
            for c in &page_text.chars {
                if c.text_index >= start && c.text_index < end {
                    selected_chars.push(c.clone());
                }
            }

            let selected_text = text_by_char_range(&page_text.text, start, end);

            let rects = md_editor_core::pdf::merge_char_rects(&selected_chars);

            let id = uuid::Uuid::new_v4().to_string();
            let now = unix_now();

            let ann = md_editor_core::pdf::PdfAnnotation {
                id: id.clone(),
                document_id: doc_id.clone(),
                page_index: sel.page_index,
                kind: md_editor_core::pdf::PdfAnnotationKind::Highlight,
                color,
                selected_text,
                ranges: vec![md_editor_core::pdf::PdfTextRange {
                    start_text_index: start,
                    end_text_index: end,
                }],
                rects,
                note: None,
                linked_note_path: None,
                markdown_anchor: None,
                created_at: now,
                updated_at: now,
            };

            if let Err(e) = self.state.save_pdf_annotation(&ann) {
                self.ui.toast = Some(format!("Failed to save highlight: {}", e));
            } else {
                self.pdf
                    .annotations
                    .entry(sel.page_index)
                    .or_default()
                    .push(ann);
                self.pdf.selection = None;
                self.refresh_pdf_backlinks();
                self.invalidate_graph();
            }
        }
        Task::none()
    }

    /// Build a short report of "orphan" annotations: highlights whose stored
    /// `selected_text` no longer appears in the current text of their page,
    /// which means the underlying PDF text has drifted away from the anchor.
    pub(super) fn pdf_orphan_report(&self) -> String {
        let mut total = 0usize;
        let mut orphans = 0usize;
        let mut checked = 0usize;
        for (page_index, anns) in &self.pdf.annotations {
            let page_text = self.pdf.page_text.get(page_index);
            for ann in anns {
                total += 1;
                let needle = ann.selected_text.trim();
                if needle.is_empty() {
                    continue;
                }
                // Only pages whose text has been loaded can be checked.
                if let Some(pt) = page_text {
                    checked += 1;
                    if !pt.text.contains(needle) {
                        orphans += 1;
                    }
                }
            }
        }
        if checked == 0 {
            format!(
                "Orphan report: 0 of {total} annotations checkable (scroll pages to load text first)"
            )
        } else if orphans == 0 {
            format!("Orphan report: no drift found ({checked} of {total} checked)")
        } else {
            format!("Orphan report: {orphans} drifted of {checked} checked ({total} total)")
        }
    }
}

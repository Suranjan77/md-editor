//! PDF message router: dispatches each `PdfMessage` to its handler in `pdf_render.rs`, `pdf_nav.rs`, `pdf_select.rs` or `PdfPane::update`.

use super::*;
use crate::messages::PdfMessage;

impl MdEditor {
    /// Handle a message routed here by [`MdEditor::update_inner`].
    pub(super) fn update_pdf(&mut self, message: PdfMessage) -> Task<Message> {
        match message {
            // Layout and rendering (pdf_render.rs).
            PdfMessage::Loaded(generation, result) => self.on_pdf_loaded(generation, result),
            PdfMessage::ZoomChanged(zoom) => self.on_pdf_zoom_changed(zoom),
            PdfMessage::FitToWidth => self.on_pdf_fit_to_width(),
            PdfMessage::Rendered(generation, page, crate::messages::PageImage(img)) => {
                self.on_pdf_page_rendered(generation, page, img)
            }
            PdfMessage::RenderFailed(generation, page) => {
                self.on_pdf_render_failed(generation, page)
            }
            PdfMessage::Scrolled { y, viewport_height } => self.on_pdf_scrolled(y, viewport_height),
            PdfMessage::ScrollBy(delta) => self.on_pdf_scroll_by(delta),
            PdfMessage::PageLinksLoaded(generation, page, links) => {
                self.on_pdf_page_links_loaded(generation, page, links)
            }
            PdfMessage::TocLoaded(path, entries, synthetic) => {
                self.on_pdf_toc_loaded(path, entries, synthetic)
            }
            // Arms that mutate only `self.pdf` (pdf_pane.rs).
            m @ (PdfMessage::PageSizesLoaded(..)
            | PdfMessage::RenderSkipped(..)
            | PdfMessage::ClosePdfLinkPreview
            | PdfMessage::PageTextLoaded(..)
            | PdfMessage::SelectionCleared) => self.pdf.update(Message::Pdf(m)),
            // Links, search, document identity (pdf_nav.rs).
            PdfMessage::LeftClicked(page, x, y, modifiers) => {
                self.on_pdf_left_clicked(page, x, y, modifiers)
            }
            PdfMessage::RightClicked(page, x, y) => self.on_pdf_right_clicked(page, x, y),
            PdfMessage::LinkPreviewResult(result) => self.on_pdf_link_preview(result),
            PdfMessage::SearchLooseToggled(value) => self.on_pdf_search_loose_toggled(value),
            PdfMessage::SearchResult(generation, result) => {
                self.on_pdf_search_result(generation, result)
            }
            PdfMessage::SearchResultClicked(page) => self.on_pdf_search_result_clicked(page),
            PdfMessage::DocumentIdComputed(generation, Some((path, hash, len, mtime))) => {
                self.on_pdf_document_id(generation, path, hash, len, mtime)
            }
            PdfMessage::DocumentIdComputed(_, None) => Task::none(),
            PdfMessage::ReferencesLoaded(doc_id, links) => {
                self.on_pdf_references_loaded(doc_id, links)
            }
            PdfMessage::OpenLinkedNote(note_path) => self.on_pdf_open_linked_note(note_path),
            PdfMessage::AnnotationFocused {
                document_path,
                annotation_id,
                page,
            } => self.on_pdf_annotation_focused(document_path, annotation_id, page),
            // Selection, highlights and notes (pdf_select.rs).
            PdfMessage::SelectionChanged(page, anchor, focus)
            | PdfMessage::SelectionFinished(page, anchor, focus) => {
                self.on_pdf_selection(page, anchor, focus)
            }
            PdfMessage::CopySelection => self.on_pdf_copy_selection(),
            PdfMessage::CreateHighlight(color) => self.create_highlight(color),
            PdfMessage::QuickHighlight => self.on_pdf_quick_highlight(),
            PdfMessage::CopyAnnotationText(id) => self.on_pdf_copy_annotation_text(id),
            PdfMessage::OrphanReport => {
                self.ui.toast = Some(self.pdf_orphan_report());
                Task::none()
            }
            PdfMessage::DeleteHighlight(id) => self.on_pdf_delete_highlight(id),
            PdfMessage::AddQuickNote(id, note) => self.on_pdf_add_quick_note(id, note),
            PdfMessage::LinkNote(annotation_id, note_path) => {
                self.on_pdf_link_note(annotation_id, note_path)
            }
        }
    }
}

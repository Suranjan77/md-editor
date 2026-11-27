//! PDF message router: dispatches each `PdfMessage` to its handler in `pdf_render.rs`, `pdf_nav.rs`, `pdf_select.rs`, `pdf_ink.rs`, `pdf_attachments.rs` or `PdfPane::update`.

use super::*;
use crate::messages::PdfMessage;

impl MdEditor {
    /// Handle a message routed here by [`MdEditor::update_inner`].
    pub(super) fn update_pdf(&mut self, message: PdfMessage) -> Task<Message> {
        match message {
            // Layout and rendering (pdf_render.rs).
            PdfMessage::Loaded(generation, result) => self.on_pdf_loaded(generation, result),
            PdfMessage::ZoomChanged(zoom) => self.on_pdf_zoom_changed(zoom),
            PdfMessage::ZoomInputChanged(raw) => {
                self.ui.pdf_zoom_input = Some(raw);
                Task::none()
            }
            PdfMessage::ZoomInputSubmit => {
                if let Some(raw) = self.ui.pdf_zoom_input.take()
                    && let Ok(pct) = raw.trim().trim_end_matches('%').parse::<f32>()
                {
                    let zoom = (pct / 100.0).clamp(0.25, 4.0);
                    return self.on_pdf_zoom_changed(zoom);
                }
                Task::none()
            }
            PdfMessage::PageInputChanged(raw) => {
                self.ui.pdf_page_input = Some(raw);
                Task::none()
            }
            PdfMessage::PageInputSubmit => {
                if let Some(raw) = self.ui.pdf_page_input.take()
                    && let Ok(page_1) = raw.trim().parse::<u16>()
                    && self.pdf.total_pages > 0
                {
                    let target = page_1
                        .saturating_sub(1)
                        .min(self.pdf.total_pages.saturating_sub(1));
                    return self.navigate_pdf_page(target);
                }
                Task::none()
            }
            PdfMessage::FitToWidth => self.on_pdf_fit_to_width(),
            PdfMessage::FitToWidthClicked => {
                self.pdf.fit_fills_pane = true;
                self.on_pdf_fit_to_width()
            }
            PdfMessage::ViewportMeasured(size) => self.on_pdf_viewport_measured(size),
            PdfMessage::FitToPage => self.on_pdf_fit_to_page(),
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
            PdfMessage::RegionSelected(page, region) => self.on_pdf_region_selected(page, region),
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
            // Non-destructive margin ink (pdf_ink.rs).
            PdfMessage::InkToolToggled(tool) => self.on_pdf_ink_tool_toggled(tool),
            PdfMessage::InkColorSelected(color) => self.on_pdf_ink_color_selected(color),
            PdfMessage::InkLiveUpdated(live) => self.on_pdf_ink_live_updated(live),
            PdfMessage::InkStrokeCommitted { page, points } => {
                self.on_pdf_ink_stroke_committed(page, points)
            }
            PdfMessage::InkEraseAt {
                page,
                x_frac,
                y_frac,
            } => self.on_pdf_ink_erase_at(page, x_frac, y_frac),
            PdfMessage::UndoInkStroke => self.on_pdf_undo_ink_stroke(),
            // Embedded files (pdf_attachments.rs).
            PdfMessage::AttachmentsLoaded(path, listed) => {
                self.on_pdf_attachments_loaded(path, listed)
            }
            PdfMessage::OpenAttachment(path, attachment) => {
                self.on_pdf_open_attachment(path, attachment)
            }
            PdfMessage::SaveAttachment(path, attachment) => {
                self.on_pdf_save_attachment(path, attachment)
            }
            PdfMessage::AttachmentSaved(stored) => self.on_pdf_attachment_saved(stored),
            PdfMessage::AttachmentFailed(err) => self.on_pdf_attachment_failed(err),
        }
    }
}

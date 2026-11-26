//! PDF navigation handlers: link clicks and previews, PDF search results, document identity and references, jumping to annotations.

use super::*;
use crate::messages::PdfMessage;

impl MdEditor {
    /// `PdfMessage::LeftClicked`: follow an internal link, report an external one
    /// (opened with Ctrl), or focus the highlight under the pointer.
    pub(super) fn on_pdf_left_clicked(
        &mut self,
        page_idx: u16,
        x: f32,
        y: f32,
        modifiers: iced::keyboard::Modifiers,
    ) -> Task<Message> {
        self.active_panel = ActivePanel::Pdf;
        if let Some(link) = self.pdf.link_at(page_idx, x, y) {
            if let Some(dest_page) = link.dest_page {
                self.pdf.current_page =
                    dest_page.min(u32::from(self.pdf.total_pages.saturating_sub(1))) as u16;
                return self.navigate_pdf_page(self.pdf.current_page);
            }
            if let Some(uri) = link.uri {
                if modifiers.control() || modifiers.command() {
                    crate::platform::open_external(&uri);
                    self.ui.toast = Some(format!("Opening: {}", uri));
                } else {
                    self.ui.toast = Some(format!("External link (Ctrl+click to open): {}", uri));
                }
            }
            return Task::none();
        }
        let Some(ann) = self.pdf.annotation_at(page_idx, x, y) else {
            self.pdf.focused_annotation_id = None;
            return Task::none();
        };
        self.pdf.focused_annotation_id = Some(ann.id.clone());
        match ann.linked_note_path {
            Some(path) if !path.is_empty() => {
                Task::done(Message::Pdf(PdfMessage::OpenLinkedNote(path)))
            }
            _ => Task::none(),
        }
    }

    /// `PdfMessage::RightClicked`: edit the note of the highlight under the pointer
    /// (or of the focused one, for keyboard-invoked clicks at negative coordinates),
    /// otherwise preview an internal link's destination.
    pub(super) fn on_pdf_right_clicked(&mut self, page_idx: u16, x: f32, y: f32) -> Task<Message> {
        self.active_panel = ActivePanel::Pdf;
        let target_ann = if x < 0.0 || y < 0.0 {
            self.pdf.focused_annotation_id.as_ref().and_then(|ann_id| {
                self.pdf
                    .annotations
                    .values()
                    .flatten()
                    .find(|a| a.id == *ann_id)
                    .cloned()
            })
        } else {
            self.pdf.annotation_at(page_idx, x, y)
        };

        if let Some(ann) = target_ann {
            self.ui.active_modal = Some(views::modals::ModalType::QuickNote(ann.id));
            self.ui.modal_input = ann.note.unwrap_or_default();
            return Task::none();
        }
        let Some(link) = self.pdf.link_at(page_idx, x, y) else {
            return Task::none();
        };
        let Some(dest_page) = link.dest_page else {
            return Task::none();
        };
        let dest_y = link.dest_y;
        let Some(path) = self.pdf.active_path.clone() else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(&path) else {
            return Task::none();
        };
        let abs_path = abs_path.to_string_lossy().to_string();
        let state = self.state.clone();
        let target_px = self.preview_target_px();

        Task::perform(
            super::blocking(move || {
                let renderer = state.pdf_renderer.as_ref()?;
                renderer
                    .render_link_preview(&abs_path, dest_page, dest_y, target_px)
                    .ok()
            }),
            |res| {
                Message::Pdf(PdfMessage::LinkPreviewResult(
                    res.ok_or_else(|| "Failed to preview".into()),
                ))
            },
        )
    }

    /// `PdfMessage::LinkPreviewResult`.
    pub(super) fn on_pdf_link_preview(
        &mut self,
        result: Result<md_editor_core::pdf::LinkPreviewResult, String>,
    ) -> Task<Message> {
        match result {
            Ok(res) => {
                if let Ok(img) = image::load_from_memory(&res.image_data) {
                    let (width, height) = img.dimensions();
                    self.pdf.link_preview_size = Some((width as f32, height as f32));
                    self.pdf.link_preview = Some(iced::widget::image::Handle::from_rgba(
                        width,
                        height,
                        img.into_rgba8().into_raw(),
                    ));
                }
            }
            Err(e) => self.ui.toast = Some(format!("Preview Error: {}", e)),
        }
        Task::none()
    }

    /// `PdfMessage::SearchLooseToggled`: re-run the PDF search with the new mode.
    pub(super) fn on_pdf_search_loose_toggled(&mut self, value: bool) -> Task<Message> {
        self.search.loose = value;
        self.search.match_index = None;
        if (self.search.visible || self.pdf_search_is_active())
            && self.pdf.active_path.is_some()
            && self.search.query.len() > 1
        {
            self.search_pdf()
        } else {
            Task::none()
        }
    }

    /// `PdfMessage::SearchResult`.
    pub(super) fn on_pdf_search_result(
        &mut self,
        generation: u64,
        result: Result<Vec<md_editor_core::pdf::PdfSearchMatch>, String>,
    ) -> Task<Message> {
        if generation != self.search.pdf_search_seq {
            return Task::none();
        }
        let results = match result {
            Ok(results) => results,
            Err(err) => {
                self.search.pdf_results.clear();
                self.search.pdf_indices_by_page.clear();
                self.search.pdf_error = Some(err);
                return Task::none();
            }
        };
        self.search.pdf_error = None;
        self.search.pdf_results = results;
        self.search.rebuild_pdf_page_index();
        if self
            .search
            .match_index
            .is_some_and(|index| index >= self.search.pdf_results.len())
        {
            self.search.match_index = None;
        }
        match self.search.match_index {
            Some(index) if self.pdf_search_is_active() && !self.search.pdf_results.is_empty() => {
                self.navigate_pdf_search_to_index(index)
            }
            _ => Task::none(),
        }
    }

    /// `PdfMessage::SearchResultClicked`: a page picked from the global search list.
    pub(super) fn on_pdf_search_result_clicked(&mut self, page: u16) -> Task<Message> {
        self.search.visible = false;
        self.search.file_visible = true;
        self.active_panel = ActivePanel::Pdf;
        self.search.match_index = self
            .search
            .pdf_results
            .iter()
            .position(|result| result.page_index == page);
        if let Some(index) = self.search.match_index {
            self.navigate_pdf_search_to_index(index)
        } else {
            self.pdf.current_page = page.min(self.pdf.total_pages.saturating_sub(1));
            self.navigate_pdf_page(self.pdf.current_page)
        }
    }

    /// `PdfMessage::DocumentIdComputed`: the content hash is known, so load the
    /// document's highlights and references and resolve a pending target page.
    pub(super) fn on_pdf_document_id(
        &mut self,
        generation: u64,
        path: String,
        hash: String,
        len: u64,
        mtime: Option<i64>,
    ) -> Task<Message> {
        // Gated on the document generation (bumped per open), not the render
        // generation. The render generation moves with the zoom, and the
        // fit-to-width right after opening bumps it whenever the first fit
        // crosses a zoom bucket: the id was dropped at random, so highlights
        // could not be saved, saved ones never loaded and references never
        // resolved. A result from an earlier open of the same path (the file
        // may have changed since) is still rejected.
        if generation != self.pdf.document_generation
            || self.pdf.active_path.as_deref() != Some(path.as_str())
        {
            return Task::none();
        }
        if let Err(e) = self.state.save_pdf_document(&hash, &path, len, mtime) {
            log::warn!("could not record PDF document {path}: {e}");
        }
        self.pdf.document_id = Some(hash.clone());
        // The vault's copy of this PDF's highlights may have changed since the
        // vault opened (a pull, another machine through Syncthing).
        if let Err(e) = self.state.sync_pdf_sidecar(&hash) {
            log::warn!("could not sync highlights of {path}: {e}");
        }

        let annotations = self
            .state
            .get_pdf_annotations(&hash, None)
            .unwrap_or_default();
        self.pdf.annotations.clear();
        for ann in annotations {
            self.pdf
                .annotations
                .entry(ann.page_index)
                .or_default()
                .push(ann);
        }

        let scroll_task = self.resolve_pdf_open_target();
        let refs_task = self.load_pdf_document_scan(&path, hash);
        Task::batch(vec![scroll_task, refs_task])
    }

    /// Scroll to the annotation or page the document was opened for, or stash the
    /// page until the page count is known.
    fn resolve_pdf_open_target(&mut self) -> Task<Message> {
        let mut target_page = None;
        if let Some(ref target_id) = self.pdf.initial_target_annotation {
            for (page_idx, page_anns) in &self.pdf.annotations {
                if page_anns.iter().any(|a| &a.id == target_id) {
                    target_page = Some(*page_idx);
                    self.pdf.focused_annotation_id = Some(target_id.clone());
                    break;
                }
            }
        }

        let resolved_target = if let Some(page) = target_page {
            self.pdf.initial_target_annotation = None;
            Some(page)
        } else {
            self.pdf.initial_target_page
        };
        match resolved_target {
            Some(page) if self.pdf.total_pages > 0 => {
                self.pdf.initial_target_page = None;
                self.navigate_pdf_page(page)
            }
            Some(page) => {
                // The hash task won the race against Loaded, so the page count
                // isn't known yet and navigating now would clamp the target to
                // 0. Stash it; once the document is applied it is consumed by
                // FitToWidth's is_initial path (the normal fit-on-open flow) or
                // by Loaded directly when fit-to-width is off.
                self.pdf.initial_target_page = Some(page);
                Task::none()
            }
            None => Task::none(),
        }
    }

    /// One background pass over the document's text, shared by the two
    /// things that need all of it: the recovered outline of a bookmark-less
    /// PDF and the resolved cross-references. Both are cached by document id,
    /// so a reopened document skips the scan entirely.
    ///
    /// Path-scoped rather than generation-scoped: the results are
    /// document-level and outlive the fit-to-width zoom change that bumps the
    /// generation. Text is extracted a page at a time in the worker's
    /// background lane, so every render queued meanwhile goes first.
    fn load_pdf_document_scan(&mut self, path: &str, hash: String) -> Task<Message> {
        let cached_references = self.state.get_pdf_references(&hash);
        let cached_outline = self.state.get_pdf_outline(&hash);
        let Some(abs) = self.resolve_active_path(path) else {
            return Task::none();
        };
        let abs = abs.to_string_lossy().to_string();
        let state = self.state.clone();
        let doc_id = hash.clone();
        let path = path.to_string();

        Task::perform(
            super::blocking(move || {
                let renderer = state.pdf_renderer.as_ref()?;
                let embedded = renderer.get_embedded_toc(&abs).unwrap_or_default();
                // `Some` only for a bookmark-less document.
                let mut outline = embedded.is_empty().then_some(cached_outline).flatten();
                let needs_outline = embedded.is_empty() && outline.is_none();
                if !needs_outline && let Some(references) = cached_references {
                    return Some((references, outline));
                }

                let pages = renderer.page_count(&abs).ok()?;
                let mut texts = Vec::with_capacity(pages as usize);
                for index in 0..pages {
                    if let Ok(text) = renderer.get_page_text_background(&abs, index) {
                        texts.push(text);
                    }
                }
                let texts = std::sync::Arc::new(texts);

                if needs_outline {
                    let recovered = renderer
                        .recover_toc(&abs, texts.clone())
                        .unwrap_or_default();
                    state.put_pdf_outline(&doc_id, &recovered);
                    outline = Some(recovered);
                }
                let references = match cached_references {
                    Some(references) => references,
                    None => {
                        let toc = if embedded.is_empty() {
                            outline.as_deref().unwrap_or_default()
                        } else {
                            &embedded
                        };
                        let links = md_editor_core::references::resolve_references(&texts, toc);
                        state.put_pdf_references(&doc_id, &links);
                        links
                    }
                };
                Some((references, outline))
            }),
            |res| res,
        )
        .then(move |res| {
            let (references, outline) = res.unwrap_or_default();
            let mut tasks = vec![Task::done(Message::Pdf(PdfMessage::ReferencesLoaded(
                hash.clone(),
                references,
            )))];
            if let Some(outline) = outline {
                tasks.push(Task::done(Message::Pdf(PdfMessage::TocLoaded(
                    path.clone(),
                    outline,
                    true,
                ))));
            }
            Task::batch(tasks)
        })
    }

    /// `PdfMessage::ReferencesLoaded`.
    pub(super) fn on_pdf_references_loaded(
        &mut self,
        doc_id: String,
        links: Vec<md_editor_core::references::ReferenceLink>,
    ) -> Task<Message> {
        // Apply only if still on the same document (the scan is async and may
        // finish after the user has navigated away).
        if self.pdf.document_id.as_deref() == Some(doc_id.as_str()) {
            self.pdf.set_references(links);
        }
        Task::none()
    }

    /// `PdfMessage::OpenLinkedNote`: open a highlight's note beside the PDF.
    pub(super) fn on_pdf_open_linked_note(&mut self, note_path: String) -> Task<Message> {
        self.ui.split_view_active = true;
        let open_task = self.open_file_extended(&note_path, false);
        if self.pdf.fit_to_width {
            Task::batch(vec![
                open_task,
                Task::done(Message::Pdf(PdfMessage::FitToWidth)),
            ])
        } else {
            Task::batch(vec![open_task, self.restore_scroll_positions()])
        }
    }

    /// `PdfMessage::AnnotationFocused`: a highlight link clicked in a note; show the
    /// PDF in split view at that highlight, opening the document if needed.
    pub(super) fn on_pdf_annotation_focused(
        &mut self,
        document_path: String,
        annotation_id: String,
        page: u16,
    ) -> Task<Message> {
        let resolved_pdf_path = resolve_relative_link_path(
            self.vault.root.as_deref(),
            self.active_path.as_deref(),
            &document_path,
        );

        let split_changed = !self.ui.split_view_active;
        self.ui.split_view_active = true;
        self.showing_pdf = true;

        if self.pdf.active_path.as_deref() == Some(&resolved_pdf_path) {
            self.pdf.focused_annotation_id = Some(annotation_id);
            let nav_task = self.navigate_pdf_page(page.saturating_sub(1));
            if split_changed {
                Task::batch(vec![self.refit_pdf_if_needed(), nav_task])
            } else {
                nav_task
            }
        } else {
            self.pdf.initial_target_page = Some(page.saturating_sub(1));
            self.pdf.initial_target_annotation = Some(annotation_id);
            self.open_pdf(&resolved_pdf_path)
        }
    }
}

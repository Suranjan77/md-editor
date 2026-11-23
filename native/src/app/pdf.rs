//! PDF viewing: loading, rendering, zoom, selection, highlights, links and PDF search.

use super::*;

impl MdEditor {
    /// Handle a message routed here by [`MdEditor::update_inner`].
    pub(super) fn update_pdf(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PdfLoaded(generation, result) => {
                if generation != self.pdf.render_generation {
                    return Task::none();
                }
                let pages = match result {
                    Ok(pages) if pages > 0 => {
                        self.pdf.load_error = None;
                        pages
                    }
                    Ok(_) => {
                        let file = self.pdf.active_path.as_deref().unwrap_or("PDF");
                        self.pdf.load_error = Some(format!(
                            "Could not open '{file}': the document has no pages"
                        ));
                        0
                    }
                    Err(error) => {
                        let file = self.pdf.active_path.as_deref().unwrap_or("PDF");
                        self.pdf.load_error = Some(format!("Could not open '{file}': {error}"));
                        0
                    }
                };
                self.pdf.total_pages = pages;
                self.pdf.pages = vec![None; pages as usize];
                self.pdf.dimensions = vec![None; pages as usize];
                if self.pdf.page_sizes.len() != pages as usize {
                    self.pdf.page_sizes = vec![None; pages as usize];
                }
                self.pdf.pending_pages.clear();
                self.pdf.pending_links.clear();
                self.pdf.programmatic_scroll = false;
                self.pdf.toc_target_page = None;
                if let Some(error) = self.pdf.load_error.clone() {
                    self.ui.toast = Some(error);
                }
                // A target page stashed by PdfDocumentIdComputed while the page
                // count was still unknown (see that handler) is resolved here.
                let target_task = if pages == 0 {
                    // Failed load: drop the target so it can't leak into the
                    // next document opened.
                    self.pdf.initial_target_page = None;
                    self.pdf.initial_target_annotation = None;
                    Task::none()
                } else if self.pdf.fit_to_width {
                    // Leave the stash for the PdfFitToWidth handler, whose
                    // `is_initial` path scrolls to the target at the post-fit
                    // zoom. Navigating here would use pre-fit page offsets and
                    // then be re-scrolled to page 0 by the fit pass.
                    Task::none()
                } else if let Some(page) = self.pdf.initial_target_page.take() {
                    self.navigate_pdf_page(page)
                } else {
                    Task::none()
                };
                let layout_task = if self.pdf.fit_to_width
                    && self
                        .pdf
                        .page_sizes
                        .iter()
                        .take(pages as usize)
                        .any(Option::is_some)
                {
                    Task::done(Message::PdfFitToWidth)
                } else if self.pdf.fit_to_width {
                    Task::none()
                } else {
                    self.render_all_pdf_pages()
                };
                Task::batch(vec![layout_task, target_task])
            }
            Message::PdfZoomChanged(zoom) => {
                let current_page = self.pdf.page_at_scroll(self.pdf.scroll_y);
                let page_start_offset = self.pdf.page_offset(current_page);
                let relative_ratio = if self.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                    0.0
                } else {
                    let page_height_old = self.pdf.page_height(current_page);
                    if page_height_old > 0.0 {
                        ((self.pdf.scroll_y - page_start_offset).max(0.0)) / page_height_old
                    } else {
                        0.0
                    }
                };

                // Page bitmaps are rendered at a quantized zoom bucket, so a
                // zoom change only needs a re-render when it crosses a bucket
                // boundary. Within a bucket the cached bitmaps are reused and
                // iced rescales them to the new layout box — making zoom feel
                // instant instead of re-rasterizing every page.
                let bucket_changed = md_editor_core::pdf::pdf_render_bucket(self.pdf.zoom)
                    != md_editor_core::pdf::pdf_render_bucket(zoom);

                self.pdf.fit_to_width = false;
                self.pdf.zoom = zoom;
                if bucket_changed {
                    self.pdf.pages = vec![None; self.pdf.total_pages as usize];
                    self.pdf.dimensions = vec![None; self.pdf.total_pages as usize];
                    self.pdf.placeholder_page_size = self.pdf.first_page_size();
                    self.pdf.pending_pages.clear();
                    self.pdf.pending_links.clear();
                    self.pdf.render_generation = self.pdf.render_generation.wrapping_add(1);
                }
                self.pdf.toc_target_page = Some(current_page);
                self.pdf.programmatic_scroll = true;

                let new_scroll_y = if self.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                    self.pdf.scroll_y
                } else {
                    self.pdf.page_offset(current_page)
                        + relative_ratio * self.pdf.page_height(current_page)
                };
                self.pdf.scroll_y = new_scroll_y;

                Task::batch(vec![
                    self.render_visible_pdf_pages(),
                    operation::scroll_to(
                        iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                        AbsoluteOffset {
                            x: 0.0,
                            y: new_scroll_y,
                        },
                    ),
                ])
            }
            Message::PdfFitToWidth => {
                let is_initial = self.pdf.initial_target_page.is_some();
                let current_page = if let Some(target_page) = self.pdf.initial_target_page.take() {
                    target_page.min(self.pdf.total_pages.saturating_sub(1))
                } else {
                    self.pdf.page_at_scroll(self.pdf.scroll_y)
                };
                let page_start_offset = self.pdf.page_offset(current_page);
                let relative_ratio = if is_initial {
                    0.0
                } else if self.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                    0.0
                } else {
                    let page_height_old = self.pdf.page_height(current_page);
                    if page_height_old > 0.0 {
                        ((self.pdf.scroll_y - page_start_offset).max(0.0)) / page_height_old
                    } else {
                        0.0
                    }
                };

                self.pdf.fit_to_width = true;
                let available_width = self.pdf_available_width();
                let page_width = self
                    .pdf
                    .page_sizes
                    .iter()
                    .flatten()
                    .next()
                    .map(|(w, _)| (*w).max(1.0))
                    .or_else(|| {
                        self.pdf
                            .dimensions
                            .iter()
                            .flatten()
                            .next()
                            .map(|(w, _)| (*w as f32 / self.pdf.zoom).max(1.0))
                    })
                    .unwrap_or(612.0);
                let next_zoom =
                    ((available_width - 48.0).max(240.0) / page_width).clamp(0.5, PDF_FIT_MAX_ZOOM);
                let next_zoom = (next_zoom * 100.0).round() / 100.0;
                // Only re-render when fit-to-width crosses a zoom bucket;
                // otherwise reuse cached bitmaps (see PdfZoomChanged).
                let bucket_changed = md_editor_core::pdf::pdf_render_bucket(self.pdf.zoom)
                    != md_editor_core::pdf::pdf_render_bucket(next_zoom);
                self.pdf.zoom = next_zoom;
                if bucket_changed {
                    self.pdf.pages = vec![None; self.pdf.total_pages as usize];
                    self.pdf.dimensions = vec![None; self.pdf.total_pages as usize];
                    self.pdf.placeholder_page_size = self.pdf.first_page_size();
                    self.pdf.pending_pages.clear();
                    self.pdf.pending_links.clear();
                    self.pdf.render_generation = self.pdf.render_generation.wrapping_add(1);
                }
                self.pdf.toc_target_page = Some(current_page);
                self.pdf.programmatic_scroll = true;

                let new_scroll_y = if is_initial {
                    self.pdf.page_offset(current_page)
                } else if self.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
                    self.pdf.scroll_y
                } else {
                    self.pdf.page_offset(current_page)
                        + relative_ratio * self.pdf.page_height(current_page)
                };
                self.pdf.scroll_y = new_scroll_y;
                if is_initial {
                    self.pdf.current_page = current_page;
                }

                Task::batch(vec![
                    self.render_visible_pdf_pages(),
                    operation::scroll_to(
                        iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                        AbsoluteOffset {
                            x: 0.0,
                            y: new_scroll_y,
                        },
                    ),
                ])
            }
            // PDF arms that mutate only `self.pdf` are routed to
            // `PdfPane::update`; see pdf_pane.rs.
            m @ (Message::PdfPageSizesLoaded(..)
            | Message::PdfRenderSkipped(..)
            | Message::ClosePdfLinkPreview
            | Message::PdfPageTextLoaded(..)
            | Message::PdfSelectionCleared) => self.pdf.update(m),
            Message::PdfRendered(generation, page, img) => {
                self.pdf.pending_pages.remove(&page);
                if generation != self.pdf.render_generation {
                    return Task::none();
                }
                let (width, height) = img.dimensions();
                let handle = iced::widget::image::Handle::from_rgba(
                    width,
                    height,
                    img.into_rgba8().into_raw(),
                );
                let supersample = self.pdf_supersample();
                let logical_width = (width as f32 / supersample).round() as u32;
                let logical_height = (height as f32 / supersample).round() as u32;
                if (page as usize) < self.pdf.pages.len() {
                    self.pdf.pages[page as usize] = Some(handle);
                    self.pdf.dimensions[page as usize] = Some((logical_width, logical_height));
                }
                if self.pdf.placeholder_page_size.is_none() || page == 0 {
                    self.pdf.placeholder_page_size = Some((
                        logical_width as f32 / self.pdf.zoom,
                        logical_height as f32 / self.pdf.zoom,
                    ));
                }
                let mut tasks = vec![self.load_pdf_page_links(page)];
                if !self.pdf.page_text.contains_key(&page) && !self.pdf.pending_text.contains(&page)
                {
                    tasks.push(self.load_pdf_page_text(page));
                }
                if self.pdf.toc_target_page == Some(page) {
                    self.pdf.programmatic_scroll = true;
                    let scroll_y = self.pdf.page_offset(page);
                    tasks.push(operation::scroll_to(
                        iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                        AbsoluteOffset {
                            x: 0.0,
                            y: scroll_y,
                        },
                    ));
                }
                Task::batch(tasks)
            }
            Message::PdfRenderFailed(generation, page) => {
                self.pdf.pending_pages.remove(&page);
                if generation != self.pdf.render_generation {
                    return Task::none();
                }
                if self.pdf.toc_target_page == Some(page) {
                    self.pdf.toc_target_page = None;
                    self.pdf.programmatic_scroll = false;
                }
                self.ui.toast = Some(format!("Could not render PDF page {}", page + 1));
                Task::none()
            }
            Message::PdfScrolled { y, viewport_height } => {
                self.active_panel = ActivePanel::Pdf;
                self.pdf.scroll_y = y;
                let new_page = self.pdf.page_at_scroll(y + viewport_height * 0.33);
                if self.pdf.programmatic_scroll {
                    self.pdf.programmatic_scroll = false;
                    let target_page = self.pdf.toc_target_page.take().unwrap_or(new_page);
                    self.pdf.current_page = target_page.min(self.pdf.total_pages.saturating_sub(1));
                    let start = self.pdf.current_page.saturating_sub(2);
                    let end =
                        (self.pdf.current_page + 2).min(self.pdf.total_pages.saturating_sub(1));
                    return self.render_pdf_page_range(start, end);
                }
                if new_page != self.pdf.current_page && new_page < self.pdf.total_pages {
                    if new_page.abs_diff(self.pdf.current_page) > 8 {
                        self.pdf.pending_pages.clear();
                        self.pdf.pending_links.clear();
                    }
                    self.pdf.current_page = new_page;
                    self.render_pdf_pages_for_viewport(y, viewport_height)
                } else {
                    self.render_pdf_pages_for_viewport(y, viewport_height)
                }
            }
            Message::PdfLeftClicked(page_idx, x, y, modifiers) => {
                self.active_panel = ActivePanel::Pdf;
                if let Some(link) = self.pdf.link_at(page_idx, x, y) {
                    if let Some(dest_page) = link.dest_page {
                        self.pdf.current_page =
                            dest_page.min(u32::from(self.pdf.total_pages.saturating_sub(1))) as u16;
                        self.navigate_pdf_page(self.pdf.current_page)
                    } else if let Some(uri) = link.uri {
                        if modifiers.control() || modifiers.command() {
                            crate::platform::open_external(&uri);
                            self.ui.toast = Some(format!("Opening: {}", uri));
                        } else {
                            self.ui.toast =
                                Some(format!("External link (Ctrl+click to open): {}", uri));
                        }
                        Task::none()
                    } else {
                        Task::none()
                    }
                } else if let Some(ann) = self.pdf.annotation_at(page_idx, x, y) {
                    self.pdf.focused_annotation_id = Some(ann.id.clone());
                    if let Some(ref path) = ann.linked_note_path {
                        if !path.is_empty() {
                            Task::done(Message::PdfOpenLinkedNote(path.clone()))
                        } else {
                            Task::none()
                        }
                    } else {
                        Task::none()
                    }
                } else {
                    self.pdf.focused_annotation_id = None;
                    Task::none()
                }
            }
            Message::PdfRightClicked(page_idx, x, y) => {
                self.active_panel = ActivePanel::Pdf;
                let mut target_ann = None;
                if x < 0.0 || y < 0.0 {
                    if let Some(ref ann_id) = self.pdf.focused_annotation_id {
                        for page_anns in self.pdf.annotations.values() {
                            if let Some(ann) = page_anns.iter().find(|a| a.id == *ann_id) {
                                target_ann = Some(ann.clone());
                                break;
                            }
                        }
                    }
                } else {
                    target_ann = self.pdf.annotation_at(page_idx, x, y);
                }

                if let Some(ann) = target_ann {
                    self.ui.active_modal = Some(views::modals::ModalType::QuickNote(ann.id));
                    self.ui.modal_input = ann.note.unwrap_or_default();
                    Task::none()
                } else if let Some(link) = self
                    .pdf
                    .link_at(page_idx, x, y)
                    .filter(|link| link.dest_page.is_some())
                {
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
                    let _state = self.state.clone();
                    let target_px = self.preview_target_px();

                    Task::perform(
                        async move {
                            let renderer = _state.pdf_renderer.as_ref()?;
                            renderer
                                .render_link_preview(&abs_path, dest_page, dest_y, target_px)
                                .ok()
                        },
                        |res| {
                            Message::PdfLinkPreviewResult(
                                res.ok_or_else(|| "Failed to preview".into()),
                            )
                        },
                    )
                } else {
                    Task::none()
                }
            }
            Message::PdfLinkPreviewResult(Ok(res)) => {
                if let Ok(img) = image::load_from_memory(&res.image_data) {
                    let (width, height) = img.dimensions();
                    self.pdf.link_preview_size = Some((width as f32, height as f32));
                    self.pdf.link_preview = Some(iced::widget::image::Handle::from_rgba(
                        width,
                        height,
                        img.into_rgba8().into_raw(),
                    ));
                }
                Task::none()
            }
            Message::PdfLinkPreviewResult(Err(e)) => {
                self.ui.toast = Some(format!("Preview Error: {}", e));
                Task::none()
            }
            Message::PdfTocLoaded(path, entries, synthetic) => {
                // Path-gated, not generation-gated: fit-to-width bumps the
                // render generation right after open, and the TOC scan for
                // bookmark-less PDFs regularly finishes after that. Only a
                // result for a different document is stale.
                if self.pdf.active_path.as_deref() != Some(path.as_str()) {
                    return Task::none();
                }
                fn flatten_pdf_toc(
                    entries: &[md_editor_core::pdf::TocEntry],
                    level: u8,
                    out: &mut Vec<views::toc::TocEntry>,
                ) {
                    for entry in entries {
                        if let Some(page_index) = entry.page_index {
                            out.push(views::toc::TocEntry {
                                level,
                                text: entry.title.clone(),
                                line: page_index as usize,
                            });
                        }
                        flatten_pdf_toc(&entry.children, (level + 1).min(6), out);
                    }
                }

                let mut mapped = Vec::new();
                flatten_pdf_toc(&entries, 1, &mut mapped);
                self.pdf.toc_entries = mapped;
                self.pdf.toc_is_synthetic = synthetic;
                Task::none()
            }
            Message::PdfPageLinksLoaded(generation, page, links) => {
                self.pdf.pending_links.remove(&page);
                if generation != self.pdf.render_generation {
                    return Task::none();
                }
                self.pdf.page_links.insert(page, links);
                Task::none()
            }
            Message::PdfSearchLooseToggled(value) => {
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
            Message::PdfSearchResult(generation, Ok(results)) => {
                if !pdf_search_generation_is_current(self.pdf.render_generation, generation) {
                    return Task::none();
                }
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
                if self.pdf_search_is_active() && !self.search.pdf_results.is_empty() {
                    if let Some(index) = self.search.match_index {
                        self.navigate_pdf_search_to_index(index)
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
            Message::PdfSearchResult(generation, Err(err)) => {
                if !pdf_search_generation_is_current(self.pdf.render_generation, generation) {
                    return Task::none();
                }
                self.search.pdf_results.clear();
                self.search.pdf_indices_by_page.clear();
                self.search.pdf_error = Some(err);
                Task::none()
            }
            Message::PdfSearchResultClicked(page) => {
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
            Message::PdfScrollBy(delta) => {
                // The palette owns the arrow keys while it is open.
                if self.ui.command_palette_visible {
                    let count = views::command_palette::result_count(
                        &self.ui.command_palette_query,
                        &self.ui.commands,
                        &self.vault.entries,
                    );
                    if count > 0 {
                        let last = count - 1;
                        self.ui.palette_selected = if delta > 0.0 {
                            (self.ui.palette_selected + 1).min(last)
                        } else {
                            self.ui.palette_selected.saturating_sub(1)
                        };
                    }
                    return Task::none();
                }
                if self.pptx.active_path.is_some() {
                    if self.search.visible || self.ui.active_modal.is_some() {
                        return Task::none();
                    }
                    let width = self.pdf_available_width();
                    // Page Up and Page Down move a whole slide; the arrows scroll.
                    return if delta.abs() > ARROW_SCROLL {
                        self.pptx
                            .update(Message::PptxStep(delta.signum() as i32), width)
                    } else {
                        self.pptx.scroll_by(delta, width)
                    };
                }
                if self.pdf.active_path.is_none()
                    || (!self.showing_pdf
                        && !(self.ui.split_view_active && self.active_path.is_some()))
                    || (self.ui.split_view_active
                        && self.active_path.is_some()
                        && self.active_panel != ActivePanel::Pdf)
                    || self.search.visible
                    || self.search.file_visible
                    || self.ui.active_modal.is_some()
                    || self.ui.command_palette_visible
                {
                    return Task::none();
                }
                let max_y = self.pdf.total_height().max(0.0);
                let y = (self.pdf.scroll_y + delta).clamp(0.0, max_y);
                operation::scroll_to(
                    iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                    AbsoluteOffset { x: 0.0, y },
                )
            }
            Message::PdfDocumentIdComputed(generation, Some((path, hash, len, mtime))) => {
                if generation != self.pdf.render_generation
                    || self.pdf.active_path.as_deref() != Some(path.as_str())
                {
                    return Task::none();
                }
                let _ = self.state.save_pdf_document(&hash, &path, len, mtime);
                self.pdf.document_id = Some(hash.clone());

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
                let scroll_task = match resolved_target {
                    Some(page) if self.pdf.total_pages > 0 => {
                        self.pdf.initial_target_page = None;
                        self.navigate_pdf_page(page)
                    }
                    Some(page) => {
                        // The hash task won the race against PdfLoaded, so the
                        // page count isn't known yet and navigating now would
                        // clamp the target to 0. Stash it; once the document is
                        // applied it is consumed by PdfFitToWidth's is_initial
                        // path (the normal fit-on-open flow) or by PdfLoaded
                        // directly when fit-to-width is off.
                        self.pdf.initial_target_page = Some(page);
                        Task::none()
                    }
                    None => Task::none(),
                };

                // Internal references: load the cached target map if this
                // document has already been scanned, otherwise resolve once in
                // the background and cache it. Gated on document_id (not render
                // generation): references are a document-level artifact, and the
                // scan outlives the fit-to-width zoom change that bumps the
                // generation.
                //
                // The scan extracts text one page at a time so the single
                // pdfium worker drains queued page renders *between* commands —
                // avoiding the ~0.5–1s stall a monolithic full-document command
                // would cause on first open of a bookmarked PDF.
                let refs_task = if let Some(links) = self.state.get_pdf_references(&hash) {
                    self.pdf.set_references(links);
                    Task::none()
                } else if let Some(abs) = self.resolve_active_path(&path) {
                    let abs = abs.to_string_lossy().to_string();
                    let state_refs = self.state.clone();
                    let doc_id = hash.clone();
                    let doc_id_msg = hash.clone();
                    Task::perform(
                        async move {
                            let renderer = state_refs.pdf_renderer.as_ref()?;
                            // Embedded outline first (cheap, no scan).
                            let mut toc = renderer.get_embedded_toc(&abs).unwrap_or_default();
                            let n = renderer.page_count(&abs).ok()?;
                            let mut pages = Vec::with_capacity(n as usize);
                            for i in 0..n {
                                if let Ok(pt) = renderer.get_page_text(&abs, i) {
                                    pages.push(pt);
                                }
                            }
                            // No bookmarks → recover section structure from the
                            // text just collected (no second scan).
                            if toc.is_empty() {
                                toc = md_editor_core::pdf::recover_toc_from_texts(&pages);
                            }
                            let links =
                                md_editor_core::references::resolve_references(&pages, &toc);
                            state_refs.put_pdf_references(&doc_id, &links);
                            Some(links)
                        },
                        move |res| {
                            Message::PdfReferencesLoaded(
                                doc_id_msg.clone(),
                                res.unwrap_or_default(),
                            )
                        },
                    )
                } else {
                    Task::none()
                };

                Task::batch(vec![scroll_task, refs_task])
            }
            Message::PdfReferencesLoaded(doc_id, links) => {
                // Apply only if still on the same document (the scan is async and
                // may finish after the user has navigated away).
                if self.pdf.document_id.as_deref() == Some(doc_id.as_str()) {
                    self.pdf.set_references(links);
                }
                Task::none()
            }
            Message::PdfDocumentIdComputed(_, None) => Task::none(),
            Message::PdfSelectionChanged(page, anchor, focus) => {
                self.active_panel = ActivePanel::Pdf;
                self.pdf.selection = Some(views::interactive_pdf::PdfSelection {
                    page_index: page,
                    anchor_idx: anchor,
                    focus_idx: focus,
                });
                Task::none()
            }
            Message::PdfSelectionFinished(page, anchor, focus) => {
                self.active_panel = ActivePanel::Pdf;
                self.pdf.selection = Some(views::interactive_pdf::PdfSelection {
                    page_index: page,
                    anchor_idx: anchor,
                    focus_idx: focus,
                });
                Task::none()
            }
            Message::PdfCopySelection => {
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
            Message::PdfCreateHighlight(color) => self.create_highlight(color),
            Message::PdfQuickHighlight => {
                // Only consume a palette color when there is actually a
                // selection to highlight, so the cycle doesn't silently
                // advance on empty clicks.
                if self.pdf.selection.is_none() {
                    Task::none()
                } else {
                    let color = self.pdf.next_highlight_color;
                    self.pdf.next_highlight_color = color.next();
                    self.create_highlight(color)
                }
            }
            Message::PdfCopyAnnotationText(id) => {
                if let Some(text) = self
                    .pdf
                    .annotations
                    .values()
                    .flatten()
                    .find(|a| a.id == id)
                    .map(|a| a.selected_text.clone())
                {
                    self.ui.toast = Some("Annotation text copied".to_string());
                    iced::clipboard::write(text)
                } else {
                    Task::none()
                }
            }
            Message::PdfOrphanReport => {
                let report = self.pdf_orphan_report();
                self.ui.toast = Some(report);
                Task::none()
            }
            Message::PdfDeleteHighlight(id) => {
                if let Err(e) = self.state.delete_pdf_annotation(&id) {
                    self.ui.toast = Some(format!("Failed to delete highlight: {}", e));
                } else {
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
                    if let Some(ref path) = self.pdf.active_path {
                        self.vault.backlinks =
                            md_editor_core::vault::get_mixed_backlinks(&self.state, path)
                                .unwrap_or_default();
                    }
                    self.invalidate_graph();
                }
                Task::none()
            }
            Message::PdfAddQuickNote(id, note_content) => {
                let mut found_ann = None;
                for page_anns in self.pdf.annotations.values_mut() {
                    if let Some(ann) = page_anns.iter_mut().find(|a| a.id == id) {
                        ann.note = Some(note_content.clone());
                        ann.updated_at = std::time::SystemTime::now()
                            .duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64;
                        found_ann = Some(ann.clone());
                        break;
                    }
                }
                if let Some(ann) = found_ann {
                    if let Err(e) = self.state.save_pdf_annotation(&ann) {
                        self.ui.toast = Some(format!("Failed to save note: {}", e));
                    } else {
                        if let Some(ref path) = self.pdf.active_path {
                            self.vault.backlinks =
                                md_editor_core::vault::get_mixed_backlinks(&self.state, path)
                                    .unwrap_or_default();
                        }
                    }
                }
                Task::none()
            }
            Message::PdfLinkNote(annotation_id, mut note_path) => {
                let mut annotation = None;
                for page_anns in self.pdf.annotations.values() {
                    if let Some(ann) = page_anns.iter().find(|a| a.id == annotation_id) {
                        annotation = Some(ann.clone());
                        break;
                    }
                }
                if let Some(mut ann) = annotation {
                    if note_path.is_empty() {
                        self.ui.modal_input = self.default_pdf_note_path(&ann);
                        self.ui.link_note_picker_search.clear();
                        self.ui.active_modal =
                            Some(views::modals::ModalType::LinkNote(annotation_id));
                        return Task::none();
                    }

                    note_path = normalize_note_path(&note_path);
                    if let Some(ref pdf_path) = self.pdf.active_path {
                        let content = self.linked_pdf_note_file_content(&note_path, pdf_path, &ann);

                        if let Err(e) =
                            md_editor_core::vault::save_file(&self.state, &note_path, &content)
                        {
                            self.ui.toast = Some(format!("Failed to create linked note: {}", e));
                            return Task::none();
                        }
                    } else {
                        return Task::none();
                    }

                    ann.linked_note_path = Some(note_path.clone());
                    ann.updated_at = std::time::SystemTime::now()
                        .duration_since(std::time::SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;

                    if let Err(e) = self.state.save_pdf_annotation(&ann) {
                        self.ui.toast = Some(format!("Failed to link note: {}", e));
                    } else {
                        for page_anns in self.pdf.annotations.values_mut() {
                            if let Some(a) = page_anns.iter_mut().find(|a| a.id == annotation_id) {
                                a.linked_note_path = Some(note_path.clone());
                                a.updated_at = ann.updated_at;
                                break;
                            }
                        }
                        self.vault.entries =
                            md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                        self.ui.toast = Some(format!("Linked note: {}", note_path));
                        self.invalidate_graph();
                        return Task::done(Message::PdfOpenLinkedNote(note_path));
                    }
                }
                Task::none()
            }
            Message::PdfOpenLinkedNote(note_path) => {
                self.ui.split_view_active = true;
                let open_task = self.open_file_extended(&note_path, false);
                if self.pdf.fit_to_width {
                    Task::batch(vec![open_task, Task::done(Message::PdfFitToWidth)])
                } else {
                    Task::batch(vec![open_task, self.restore_scroll_positions()])
                }
            }
            Message::PdfAnnotationFocused {
                document_path,
                annotation_id,
                page,
            } => {
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
            // Variants routed here whose remaining shapes need no work.
            _ => Task::none(),
        }
    }

    pub(super) fn open_pdf(&mut self, path: &str) -> Task<Message> {
        let Some(abs_path) = self.resolve_active_path(path) else {
            self.ui.toast = Some("Open a vault before opening a PDF".to_string());
            return Task::none();
        };
        let path_str = abs_path.to_string_lossy().to_string();
        self.pdf.active_path = Some(path.to_string());
        self.graph.set_active_path(Some(path.to_string()));
        self.pdf.load_error = None;
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
        self.active_image_path = None;
        self.active_image = None;
        self.pptx.close();
        self.showing_pdf = true;
        self.active_panel = ActivePanel::Pdf;
        self.pdf.current_page = 0;
        self.pdf.fit_to_width = true;
        self.pdf.pages = Vec::new();
        self.pdf.dimensions = Vec::new();
        self.pdf.page_sizes = Vec::new();
        self.pdf.placeholder_page_size = None;
        self.pdf.pending_pages.clear();
        self.pdf.pending_links.clear();
        self.pdf.page_links.clear();
        self.pdf.references.clear();
        self.search.pdf_results.clear();
        self.search.pdf_indices_by_page.clear();
        self.search.pdf_error = None;
        self.pdf.programmatic_scroll = false;
        self.pdf.toc_target_page = None;
        self.pdf.render_generation = self.pdf.render_generation.wrapping_add(1);
        let generation = self.pdf.render_generation;

        // Reset PDF study state
        self.pdf.document_id = None;
        self.pdf.page_text.clear();
        self.pdf.selection = None;
        self.pdf.annotations.clear();
        self.pdf.focused_annotation_id = None;
        self.pdf.pending_text.clear();
        self.pdf.text_lru.clear();
        self.pdf.toc_entries.clear();
        self.pdf.toc_is_synthetic = false;
        self.vault.backlinks =
            md_editor_core::vault::get_mixed_backlinks(&self.state, path).unwrap_or_default();

        let path_for_hash = path.to_string();
        let abs_path_for_hash = abs_path.clone();
        let hash_task = Task::perform(
            async move {
                match md_editor_core::pdf::compute_provisional_id(&abs_path_for_hash) {
                    Ok((hash, len, mtime)) => Some((path_for_hash, hash, len, mtime)),
                    Err(_) => None,
                }
            },
            move |result| Message::PdfDocumentIdComputed(generation, result),
        );

        let _state = self.state.clone();
        let _state_toc = self.state.clone();
        let _state_sizes = self.state.clone();
        let path_clone = path_str.clone();
        let path_str_toc = path_str.clone();
        let path_for_toc = path.to_string();
        let path_for_sizes = path.to_string();
        let path_str_sizes = path_str.clone();

        Task::batch(vec![
            hash_task,
            Task::perform(
                async move {
                    let renderer = _state
                        .pdf_renderer
                        .as_ref()
                        .ok_or_else(|| "PDF renderer is unavailable".to_string())?;
                    renderer.page_count(&path_clone)
                },
                move |res| Message::PdfLoaded(generation, res),
            ),
            Task::perform(
                async move {
                    let renderer = _state_sizes.pdf_renderer.as_ref()?;
                    renderer.page_sizes(&path_str_sizes).ok()
                },
                move |res| {
                    Message::PdfPageSizesLoaded(
                        generation,
                        path_for_sizes.clone(),
                        res.unwrap_or_default(),
                    )
                },
            ),
            Task::perform(
                async move {
                    let renderer = _state_toc.pdf_renderer.as_ref()?;
                    renderer.get_toc(&path_str_toc).ok()
                },
                move |res| {
                    let (entries, synthetic) = res.unwrap_or_default();
                    Message::PdfTocLoaded(path_for_toc.clone(), entries, synthetic)
                },
            ),
        ])
    }

    pub(super) fn render_pdf_page(&self, page: u16) -> Task<Message> {
        let Some(path) = &self.pdf.active_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        let path_str = abs_path.to_string_lossy().to_string();
        let zoom = md_editor_core::pdf::pdf_render_bucket(self.pdf.zoom) * self.pdf_supersample();
        let generation = self.pdf.render_generation;
        let _state = self.state.clone();

        Task::perform(
            async move {
                let renderer = _state.pdf_renderer.as_ref()?;
                let res = renderer.render_page(&path_str, page, zoom);
                Some((page, res))
            },
            move |res| {
                if let Some((p, Ok(img))) = res {
                    Message::PdfRendered(generation, p, img)
                } else if let Some((p, Err(err))) = res {
                    if err == "Skipped" {
                        Message::PdfRenderSkipped(generation, p)
                    } else {
                        Message::PdfRenderFailed(generation, p)
                    }
                } else {
                    Message::PdfRenderFailed(generation, page)
                }
            },
        )
    }

    pub(super) fn render_pdf_page_direct(&mut self, page: u16) -> Task<Message> {
        let Some(path) = &self.pdf.active_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        let path_str = abs_path.to_string_lossy().to_string();
        let zoom = md_editor_core::pdf::pdf_render_bucket(self.pdf.zoom) * self.pdf_supersample();
        let generation = self.pdf.render_generation;
        let _state = self.state.clone();
        if self
            .pdf
            .pages
            .get(page as usize)
            .is_none_or(|p| p.is_none())
        {
            self.pdf.pending_pages.insert(page);
        }

        Task::perform(
            async move {
                let renderer = _state.pdf_renderer.as_ref()?;
                renderer
                    .render_page_priority(&path_str, page, zoom)
                    .map_err(|e| println!("PDF PRIORITY RENDER ERROR (Page {}): {}", page, e))
                    .ok()
                    .map(|img| (page, img))
            },
            move |res| {
                if let Some((p, img)) = res {
                    Message::PdfRendered(generation, p, img)
                } else {
                    Message::PdfRenderFailed(generation, page)
                }
            },
        )
    }

    pub(super) fn load_pdf_page_links(&mut self, page: u16) -> Task<Message> {
        if self.pdf.page_links.contains_key(&page) || self.pdf.pending_links.contains(&page) {
            return Task::none();
        }
        let Some(path) = &self.pdf.active_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        self.pdf.pending_links.insert(page);
        let path_str = abs_path.to_string_lossy().to_string();
        let generation = self.pdf.render_generation;
        let _state = self.state.clone();

        Task::perform(
            async move {
                let renderer = _state.pdf_renderer.as_ref()?;
                renderer.get_page_links(&path_str, page).ok()
            },
            move |res| Message::PdfPageLinksLoaded(generation, page, res.unwrap_or_default()),
        )
    }

    pub(super) fn load_pdf_page_text(&mut self, page: u16) -> Task<Message> {
        if self.pdf.page_text.contains_key(&page) || self.pdf.pending_text.contains(&page) {
            return Task::none();
        }
        let Some(path) = &self.pdf.active_path else {
            return Task::none();
        };
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        self.pdf.pending_text.insert(page);
        let path_str = abs_path.to_string_lossy().to_string();
        let generation = self.pdf.render_generation;
        let _state = self.state.clone();

        Task::perform(
            async move {
                let renderer = _state
                    .pdf_renderer
                    .as_ref()
                    .ok_or_else(|| "No PDF renderer".to_string())?;
                renderer.get_page_text(&path_str, page)
            },
            move |res| Message::PdfPageTextLoaded(generation, page, res),
        )
    }

    pub(super) fn render_all_pdf_pages(&mut self) -> Task<Message> {
        self.render_visible_pdf_pages()
    }

    pub(super) fn render_visible_pdf_pages(&mut self) -> Task<Message> {
        if self.pdf.total_pages == 0 {
            return Task::none();
        }
        // Estimate visible range using viewport height and page height
        let page_h = self.pdf.estimated_page_height().max(100.0);
        let viewport_h = self.ui.window_height.max(400.0);
        let pages_in_view = (viewport_h / page_h).ceil() as u16;
        let first_visible = self.pdf.current_page;
        let last_visible =
            (first_visible + pages_in_view).min(self.pdf.total_pages.saturating_sub(1));

        if let Some(path) = &self.pdf.active_path
            && let Some(abs_path) = self.resolve_active_path(path)
        {
            let path_str = abs_path.to_string_lossy().to_string();
            if let Some(renderer) = self.state.pdf_renderer.as_ref() {
                renderer.set_visible_range(first_visible, last_visible, &path_str);
            }
        }

        let start = self.pdf.current_page.saturating_sub(3);
        let end =
            (self.pdf.current_page + pages_in_view + 3).min(self.pdf.total_pages.saturating_sub(1));
        self.render_pdf_page_range(start, end)
    }

    pub(super) fn render_pdf_pages_for_viewport(
        &mut self,
        scroll_y: f32,
        viewport_height: f32,
    ) -> Task<Message> {
        if self.pdf.total_pages == 0 {
            return Task::none();
        }

        let first_visible = self.pdf.page_at_scroll(scroll_y);
        let last_visible = self.pdf.page_at_scroll(scroll_y + viewport_height);

        if let Some(path) = &self.pdf.active_path
            && let Some(abs_path) = self.resolve_active_path(path)
        {
            let path_str = abs_path.to_string_lossy().to_string();
            if let Some(renderer) = self.state.pdf_renderer.as_ref() {
                renderer.set_visible_range(first_visible, last_visible, &path_str);
            }
        }

        let first = self
            .pdf
            .page_at_scroll((scroll_y - self.pdf.estimated_page_height()).max(0.0));
        let last = self
            .pdf
            .page_at_scroll(scroll_y + viewport_height + self.pdf.estimated_page_height());
        self.render_pdf_page_range(
            first.saturating_sub(2),
            (last + 2).min(self.pdf.total_pages.saturating_sub(1)),
        )
    }

    pub(super) fn render_pdf_page_range(&mut self, start: u16, end: u16) -> Task<Message> {
        let mut tasks = Vec::new();
        for page_idx in start..=end {
            if self
                .pdf
                .pages
                .get(page_idx as usize)
                .is_none_or(|p| p.is_none())
                && !self.pdf.pending_pages.contains(&page_idx)
            {
                self.pdf.pending_pages.insert(page_idx);
                tasks.push(self.render_pdf_page(page_idx));
            }
            if !self.pdf.page_text.contains_key(&page_idx)
                && !self.pdf.pending_text.contains(&page_idx)
            {
                tasks.push(self.load_pdf_page_text(page_idx));
            }
        }

        // Free bitmaps well outside the rendered window so a long PDF doesn't
        // keep every page resident in memory (each page is a full RGBA bitmap,
        // and supersampling multiplies that cost). Layout metadata in
        // `dimensions`/`page_sizes` is cheap and kept, so geometry/link
        // hit-testing still work; an evicted page simply re-renders when it
        // scrolls back into view.
        self.evict_distant_page_bitmaps(start, end);

        Task::batch(tasks)
    }

    /// Drop cached page bitmaps further than [`PDF_PAGE_BITMAP_EVICT_MARGIN`]
    /// from the `[start, end]` render window. The margin gives scroll
    /// hysteresis so small back-and-forth movements reuse cached pages.
    pub(super) fn evict_distant_page_bitmaps(&mut self, start: u16, end: u16) {
        let keep_lo = start.saturating_sub(PDF_PAGE_BITMAP_EVICT_MARGIN);
        let keep_hi = end.saturating_add(PDF_PAGE_BITMAP_EVICT_MARGIN);
        for (idx, slot) in self.pdf.pages.iter_mut().enumerate() {
            let idx = idx as u16;
            if (idx < keep_lo || idx > keep_hi) && slot.is_some() {
                *slot = None;
            }
        }
    }

    /// Supersampling factor for PDF rasterization. Renders at the display's
    /// device-pixel ratio so pages are pixel-sharp on HiDPI/fractional displays
    /// while staying 1:1 (no wasted work) on standard 1× screens. With zoom
    /// bucketing always rounding up, this guarantees the rasterized bitmap is
    /// never undersampled relative to the physical pixels it lands on.
    pub(super) fn pdf_supersample(&self) -> f32 {
        self.ui.scale_factor.clamp(1.0, PDF_SUPERSAMPLE_MAX)
    }

    /// Re-fit the PDF to the available width when fit-to-width is active and a
    /// PDF is open. Call after any change that alters the PDF pane's width (TOC,
    /// sidebar, split view) so pages re-render at the correct scale instead of
    /// being stretched from a stale zoom.
    /// Logical width the link-preview modal box is sized to (width-limited for
    /// the typical landscape crop). Shared by the render request and the view so
    /// the rendered bitmap matches the on-screen box.
    pub(super) fn preview_max_box_width(&self) -> f32 {
        (self.ui.window_width * 0.85).clamp(480.0, 1400.0)
    }

    /// Physical pixel width to rasterize the link preview at: the logical box
    /// width times the display scale factor, so the enlarged preview renders
    /// pixel-sharp. Capped to bound the render cost.
    pub(super) fn preview_target_px(&self) -> u32 {
        (self.preview_max_box_width() * self.ui.scale_factor).clamp(600.0, 4000.0) as u32
    }

    pub(super) fn refit_pdf_if_needed(&self) -> Task<Message> {
        if self.pdf.fit_to_width && self.pdf.active_path.is_some() {
            Task::done(Message::PdfFitToWidth)
        } else {
            Task::none()
        }
    }

    pub(super) fn pdf_available_width(&self) -> f32 {
        let sidebar_width = if self.vault.sidebar_visible {
            260.0
        } else {
            0.0
        };
        let toc_width = if self.editor.toc_visible { 260.0 } else { 0.0 };
        let backlinks_width = if self.vault.backlinks_visible {
            260.0
        } else {
            0.0
        };
        let chrome_width = sidebar_width + toc_width + backlinks_width;
        let content_width = (self.ui.window_width - chrome_width).max(320.0);

        if self.ui.split_view_active && self.active_path.is_some() && self.pdf.active_path.is_some()
        {
            (content_width * (1.0 - self.ui.split_ratio)).max(280.0)
        } else {
            content_width
        }
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

    pub(super) fn navigate_pdf_page(&mut self, page: u16) -> Task<Message> {
        let target_page = page.min(self.pdf.total_pages.saturating_sub(1));
        self.pdf.current_page = target_page;
        self.pdf.pending_pages.clear();
        self.pdf.pending_links.clear();
        // No generation bump here: the document and zoom bucket are unchanged,
        // so in-flight results are still valid. Bumping would race the initial
        // PdfLoaded/PdfTocLoaded tasks (tagged with the open generation) and
        // silently drop them when a PDF is opened with a target page.
        self.pdf.toc_target_page = Some(target_page);

        let target_dimensions_ready = self
            .pdf
            .dimensions
            .get(target_page as usize)
            .and_then(|d| *d)
            .is_some();
        let target_image_ready = self
            .pdf
            .pages
            .get(target_page as usize)
            .is_some_and(|page| page.is_some());

        let mut tasks = Vec::new();
        if target_image_ready && target_dimensions_ready {
            tasks.push(self.load_pdf_page_links(target_page));
        } else {
            tasks.push(self.render_pdf_page_direct(target_page));
        }

        self.pdf.programmatic_scroll = true;
        let scroll_y = self.pdf.page_offset(target_page);
        tasks.push(operation::scroll_to(
            iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
            AbsoluteOffset {
                x: 0.0,
                y: scroll_y,
            },
        ));
        Task::batch(tasks)
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
            let now = std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;

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
                if let Some(ref path) = self.pdf.active_path {
                    self.vault.backlinks =
                        md_editor_core::vault::get_mixed_backlinks(&self.state, path)
                            .unwrap_or_default();
                }
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

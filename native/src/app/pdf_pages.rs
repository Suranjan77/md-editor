//! Opening PDFs and managing their pages: rendering, links, text, fit, navigation and highlights.

use super::*;
use crate::messages::PdfMessage;

impl MdEditor {
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
            move |result| Message::Pdf(PdfMessage::DocumentIdComputed(generation, result)),
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
                move |res| Message::Pdf(PdfMessage::Loaded(generation, res.map_err(String::from))),
            ),
            Task::perform(
                async move {
                    let renderer = _state_sizes.pdf_renderer.as_ref()?;
                    renderer.page_sizes(&path_str_sizes).ok()
                },
                move |res| {
                    Message::Pdf(PdfMessage::PageSizesLoaded(
                        generation,
                        path_for_sizes.clone(),
                        res.unwrap_or_default(),
                    ))
                },
            ),
            Task::perform(
                async move {
                    let renderer = _state_toc.pdf_renderer.as_ref()?;
                    renderer.get_toc(&path_str_toc).ok()
                },
                move |res| {
                    let (entries, synthetic) = res.unwrap_or_default();
                    Message::Pdf(PdfMessage::TocLoaded(
                        path_for_toc.clone(),
                        entries,
                        synthetic,
                    ))
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
                    Message::Pdf(PdfMessage::Rendered(
                        generation,
                        p,
                        crate::messages::PageImage(img),
                    ))
                } else if let Some((p, Err(err))) = res {
                    if err == md_editor_core::pdf::PdfError::Skipped {
                        Message::Pdf(PdfMessage::RenderSkipped(generation, p))
                    } else {
                        Message::Pdf(PdfMessage::RenderFailed(generation, p))
                    }
                } else {
                    Message::Pdf(PdfMessage::RenderFailed(generation, page))
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
                    .map_err(|e| log::warn!("Priority render of page {page} failed: {e}"))
                    .ok()
                    .map(|img| (page, img))
            },
            move |res| {
                if let Some((p, img)) = res {
                    Message::Pdf(PdfMessage::Rendered(
                        generation,
                        p,
                        crate::messages::PageImage(img),
                    ))
                } else {
                    Message::Pdf(PdfMessage::RenderFailed(generation, page))
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
            move |res| {
                Message::Pdf(PdfMessage::PageLinksLoaded(
                    generation,
                    page,
                    res.unwrap_or_default(),
                ))
            },
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
            move |res| {
                Message::Pdf(PdfMessage::PageTextLoaded(
                    generation,
                    page,
                    res.map_err(String::from),
                ))
            },
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
            Task::done(Message::Pdf(PdfMessage::FitToWidth))
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

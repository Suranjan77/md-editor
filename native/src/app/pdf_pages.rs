//! Opening PDFs and managing their pages: rendering, links, text, fit, navigation and highlights.

use super::pdf_render::scroll_pdf_to;
use super::*;
use crate::messages::PdfMessage;

impl MdEditor {
    pub(super) fn open_pdf(&mut self, path: &str) -> Task<Message> {
        let Some(abs_path) = self.resolve_active_path(path) else {
            self.ui.toast = Some("Open a vault before opening a PDF".to_string());
            return Task::none();
        };
        let path_str = abs_path.to_string_lossy().to_string();
        self.save_current_tab_view();
        self.pdf.active_path = Some(path.to_string());
        self.tabs.open(path, crate::tabs::TabKind::Pdf, false);
        self.persist_tabs();
        self.ui.pdf_page_input = None;
        self.ui.pdf_zoom_input = None;
        self.graph.set_active_path(Some(path.to_string()));
        self.pdf.load_error = None;
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
        self.active_image_path = None;
        self.active_image = None;
        self.showing_pdf = true;
        self.active_panel = ActivePanel::Pdf;
        self.pdf.current_page = 0;
        self.pdf.fit_to_width = true;
        self.pdf.fit_fills_pane = false;
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
        self.search.match_index = None;
        // A search still running is for the previous document.
        self.search.pdf_search_seq = self.search.pdf_search_seq.wrapping_add(1);
        self.pdf.programmatic_scroll = false;
        self.pdf.toc_target_page = None;
        self.pdf.render_generation = self.pdf.render_generation.wrapping_add(1);
        self.pdf.document_generation = self.pdf.document_generation.wrapping_add(1);
        let generation = self.pdf.render_generation;
        let document_generation = self.pdf.document_generation;

        // Reset PDF study state
        self.pdf.document_id = None;
        self.pdf.page_text.clear();
        self.pdf.selection = None;
        self.pdf.live_ink = None;
        self.pdf.annotations.clear();
        self.pdf.focused_annotation_id = None;
        self.pdf.pending_text.clear();
        self.pdf.text_lru.clear();
        self.pdf.toc_entries.clear();
        self.pdf.toc_is_synthetic = false;
        self.pdf.attachments.clear();
        self.vault.backlinks =
            md_editor_core::vault::get_mixed_backlinks(&self.state, path).unwrap_or_default();

        let path_for_hash = path.to_string();
        let abs_path_for_hash = abs_path.clone();
        let hash_task = Task::perform(
            super::blocking(move || {
                match md_editor_core::pdf::compute_provisional_id(&abs_path_for_hash) {
                    Ok((hash, len, mtime)) => Some((path_for_hash, hash, len, mtime)),
                    Err(_) => None,
                }
            }),
            move |result| Message::Pdf(PdfMessage::DocumentIdComputed(document_generation, result)),
        );

        let _state = self.state.clone();
        let _state_toc = self.state.clone();
        let _state_sizes = self.state.clone();
        let path_clone = path_str.clone();
        let path_str_toc = path_str.clone();
        let path_for_toc = path.to_string();
        let path_for_sizes = path.to_string();
        let path_str_sizes = path_str.clone();

        let attachments_task = self.load_pdf_attachments(path, path_str.clone());

        Task::batch(vec![
            hash_task,
            attachments_task,
            Task::perform(
                super::blocking(move || {
                    let renderer = _state
                        .pdf_renderer
                        .as_ref()
                        .ok_or_else(|| "PDF renderer is unavailable".to_string())?;
                    renderer.page_count(&path_clone)
                }),
                // Gated on the document, not the render generation: a zoom
                // or refit while the PDF loads must not drop its page count.
                move |res| {
                    Message::Pdf(PdfMessage::Loaded(
                        document_generation,
                        res.map_err(String::from),
                    ))
                },
            ),
            Task::perform(
                super::blocking(move || {
                    let renderer = _state_sizes.pdf_renderer.as_ref()?;
                    renderer.page_sizes(&path_str_sizes).ok()
                }),
                move |res| {
                    Message::Pdf(PdfMessage::PageSizesLoaded(
                        generation,
                        path_for_sizes.clone(),
                        res.unwrap_or_default(),
                    ))
                },
            ),
            // Bookmarks only: cheap, and in the interactive lane. A
            // bookmark-less PDF gets its outline from the background document
            // scan (`load_pdf_document_scan`) instead of a full-text pass that
            // used to sit in front of the first page render.
            Task::perform(
                super::blocking(move || {
                    let renderer = _state_toc.pdf_renderer.as_ref()?;
                    renderer.get_embedded_toc(&path_str_toc).ok()
                }),
                |res| res,
            )
            .then(move |res| match res {
                Some(entries) if !entries.is_empty() => Task::done(Message::Pdf(
                    PdfMessage::TocLoaded(path_for_toc.clone(), entries, false),
                )),
                _ => Task::none(),
            }),
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
            super::blocking(move || {
                let renderer = _state.pdf_renderer.as_ref()?;
                let res = renderer.render_page(&path_str, page, zoom);
                Some((page, res))
            }),
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
            super::blocking(move || {
                let renderer = _state.pdf_renderer.as_ref()?;
                Some(renderer.render_page_priority(&path_str, page, zoom))
            }),
            move |res| match res {
                Some(Ok(img)) => Message::Pdf(PdfMessage::Rendered(
                    generation,
                    page,
                    crate::messages::PageImage(img),
                )),
                // A newer priority request replaced this one (fast navigation):
                // nothing failed, so no error toast.
                Some(Err(md_editor_core::pdf::PdfError::Skipped)) => {
                    Message::Pdf(PdfMessage::RenderSkipped(generation, page))
                }
                Some(Err(e)) => {
                    log::warn!("Priority render of page {page} failed: {e}");
                    Message::Pdf(PdfMessage::RenderFailed(generation, page))
                }
                None => Message::Pdf(PdfMessage::RenderFailed(generation, page)),
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
        let generation = self.pdf.document_generation;
        let _state = self.state.clone();

        Task::perform(
            super::blocking(move || {
                let renderer = _state.pdf_renderer.as_ref()?;
                renderer.get_page_links(&path_str, page).ok()
            }),
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
        let generation = self.pdf.document_generation;
        let _state = self.state.clone();

        Task::perform(
            super::blocking(move || {
                let renderer = _state
                    .pdf_renderer
                    .as_ref()
                    .ok_or_else(|| "No PDF renderer".to_string())?;
                renderer.get_page_text(&path_str, page)
            }),
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
        // Before the page count is known the fit has nothing to measure; the
        // load fits the pages itself.
        if self.pdf.fit_to_width && self.pdf.active_path.is_some() && self.pdf.total_pages > 0 {
            Task::done(Message::Pdf(PdfMessage::FitToWidth))
        } else {
            Task::none()
        }
    }

    pub(super) fn pdf_available_width(&self) -> f32 {
        // The measured pane, once the PDF has been laid out at least once.
        if self.showing_pdf
            && let Some(width) = self.pdf.viewport_width
        {
            return width;
        }
        let sidebar_width = if self.vault.sidebar_visible {
            self.ui.sidebar_width
        } else {
            0.0
        };
        let toc_width = if self.editor.toc_visible {
            self.ui.side_panel_width
        } else {
            0.0
        };
        let backlinks_width = if self.vault.backlinks_visible {
            self.ui.side_panel_width
        } else {
            0.0
        };
        // Before the first measurement only: the Folio chrome around the sheet
        // (activity rail, right gutter, sheet border).
        let folio_chrome = crate::design::metrics::RAIL_WIDTH
            + crate::design::metrics::SHEET_GUTTER
            + 2.0 * crate::design::metrics::BORDER_WIDTH;
        let chrome_width = sidebar_width + toc_width + backlinks_width + folio_chrome;
        let content_width = (self.ui.window_width - chrome_width).max(320.0);

        // This pane's share, side by side.
        let share = match (self.workspace.is_split(), self.workspace.axis) {
            (true, super::workspace::SplitAxis::Vertical) => match self.workspace.current {
                super::workspace::PaneId::A => self.ui.split_ratio,
                super::workspace::PaneId::B => 1.0 - self.ui.split_ratio,
            },
            _ => 1.0,
        };
        (content_width * share).max(280.0)
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
        tasks.push(scroll_pdf_to(self.pdf.page_offset(target_page)));
        Task::batch(tasks)
    }
}

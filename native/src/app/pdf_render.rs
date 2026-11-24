//! PDF layout and rendering handlers: load results, zoom and fit, rendered pages, scrolling.

use super::*;
use crate::messages::{PdfMessage, ShellMessage};

impl MdEditor {
    /// `PdfMessage::Loaded`: apply the page count (or the load error) and start layout.
    pub(super) fn on_pdf_loaded(
        &mut self,
        generation: u64,
        result: Result<u16, String>,
    ) -> Task<Message> {
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
        // A target page stashed by DocumentIdComputed while the page count was
        // still unknown (see that handler) is resolved here.
        let target_task = if pages == 0 {
            // Failed load: drop the target so it can't leak into the next
            // document opened.
            self.pdf.initial_target_page = None;
            self.pdf.initial_target_annotation = None;
            Task::none()
        } else if self.pdf.fit_to_width {
            // Leave the stash for the FitToWidth handler, whose `is_initial`
            // path scrolls to the target at the post-fit zoom. Navigating here
            // would use pre-fit page offsets and then be re-scrolled to page 0
            // by the fit pass.
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
            Task::done(Message::Pdf(PdfMessage::FitToWidth))
        } else if self.pdf.fit_to_width {
            Task::none()
        } else {
            self.render_all_pdf_pages()
        };
        Task::batch(vec![layout_task, target_task])
    }

    /// `PdfMessage::ZoomChanged`: an explicit zoom, which turns fit-to-width off.
    pub(super) fn on_pdf_zoom_changed(&mut self, zoom: f32) -> Task<Message> {
        let current_page = self.pdf.page_at_scroll(self.pdf.scroll_y);
        let relative_ratio = self.pdf_scroll_ratio_in_page(current_page);
        self.pdf.fit_to_width = false;
        self.apply_pdf_zoom(zoom, current_page, relative_ratio, false)
    }

    /// `PdfMessage::FitToWidth`: zoom so the first page fills the available width.
    /// On the first fit after opening, scrolls to the stashed target page.
    pub(super) fn on_pdf_fit_to_width(&mut self) -> Task<Message> {
        let is_initial = self.pdf.initial_target_page.is_some();
        let current_page = if let Some(target_page) = self.pdf.initial_target_page.take() {
            target_page.min(self.pdf.total_pages.saturating_sub(1))
        } else {
            self.pdf.page_at_scroll(self.pdf.scroll_y)
        };
        let relative_ratio = if is_initial {
            0.0
        } else {
            self.pdf_scroll_ratio_in_page(current_page)
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
        self.apply_pdf_zoom(next_zoom, current_page, relative_ratio, is_initial)
    }

    /// How far into `page` the viewport top sits, as a fraction of the page height.
    fn pdf_scroll_ratio_in_page(&self, page: u16) -> f32 {
        if self.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
            return 0.0;
        }
        let page_height = self.pdf.page_height(page);
        if page_height > 0.0 {
            ((self.pdf.scroll_y - self.pdf.page_offset(page)).max(0.0)) / page_height
        } else {
            0.0
        }
    }

    /// Set the zoom, drop bitmaps only if the render bucket changed, and keep the
    /// viewport on the same spot of `current_page` (its top when `is_initial`).
    fn apply_pdf_zoom(
        &mut self,
        zoom: f32,
        current_page: u16,
        relative_ratio: f32,
        is_initial: bool,
    ) -> Task<Message> {
        // Page bitmaps are rendered at a quantized zoom bucket, so a zoom change
        // only needs a re-render when it crosses a bucket boundary. Within a
        // bucket the cached bitmaps are reused and iced rescales them to the
        // new layout box — making zoom feel instant instead of re-rasterizing
        // every page.
        let bucket_changed = md_editor_core::pdf::pdf_render_bucket(self.pdf.zoom)
            != md_editor_core::pdf::pdf_render_bucket(zoom);
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

        let new_scroll_y = if is_initial {
            self.pdf.page_offset(current_page)
        } else if self.pdf.scroll_y < PDF_PAGE_LIST_PADDING {
            self.pdf.scroll_y
        } else {
            self.pdf.page_offset(current_page) + relative_ratio * self.pdf.page_height(current_page)
        };
        self.pdf.scroll_y = new_scroll_y;
        if is_initial {
            self.pdf.current_page = current_page;
        }

        Task::batch(vec![
            self.render_visible_pdf_pages(),
            scroll_pdf_to(new_scroll_y),
        ])
    }

    /// `PdfMessage::Rendered`: store the bitmap, then fetch the page's links and text.
    pub(super) fn on_pdf_page_rendered(
        &mut self,
        generation: u64,
        page: u16,
        img: image::DynamicImage,
    ) -> Task<Message> {
        self.pdf.pending_pages.remove(&page);
        if generation != self.pdf.render_generation {
            return Task::none();
        }
        let (width, height) = img.dimensions();
        let handle =
            iced::widget::image::Handle::from_rgba(width, height, img.into_rgba8().into_raw());
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
        if !self.pdf.page_text.contains_key(&page) && !self.pdf.pending_text.contains(&page) {
            tasks.push(self.load_pdf_page_text(page));
        }
        if self.pdf.toc_target_page == Some(page) {
            self.pdf.programmatic_scroll = true;
            tasks.push(scroll_pdf_to(self.pdf.page_offset(page)));
        }
        Task::batch(tasks)
    }

    /// `PdfMessage::RenderFailed`: clear the pending state and tell the user.
    pub(super) fn on_pdf_render_failed(&mut self, generation: u64, page: u16) -> Task<Message> {
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

    /// `PdfMessage::Scrolled`: track the current page and render what came into view.
    pub(super) fn on_pdf_scrolled(&mut self, y: f32, viewport_height: f32) -> Task<Message> {
        self.active_panel = ActivePanel::Pdf;
        self.pdf.scroll_y = y;
        let new_page = self.pdf.page_at_scroll(y + viewport_height * 0.33);
        if self.pdf.programmatic_scroll {
            self.pdf.programmatic_scroll = false;
            let target_page = self.pdf.toc_target_page.take().unwrap_or(new_page);
            self.pdf.current_page = target_page.min(self.pdf.total_pages.saturating_sub(1));
            let start = self.pdf.current_page.saturating_sub(2);
            let end = (self.pdf.current_page + 2).min(self.pdf.total_pages.saturating_sub(1));
            return self.render_pdf_page_range(start, end);
        }
        if new_page != self.pdf.current_page && new_page < self.pdf.total_pages {
            if new_page.abs_diff(self.pdf.current_page) > 8 {
                self.pdf.pending_pages.clear();
                self.pdf.pending_links.clear();
            }
            self.pdf.current_page = new_page;
        }
        self.render_pdf_pages_for_viewport(y, viewport_height)
    }

    /// `PdfMessage::ScrollBy`: arrow and page keys. The command palette and an open
    /// presentation take them first; otherwise they scroll the PDF when it has focus.
    pub(super) fn on_pdf_scroll_by(&mut self, delta: f32) -> Task<Message> {
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
                self.pptx.update(
                    Message::Shell(ShellMessage::PptxStep(delta.signum() as i32)),
                    width,
                )
            } else {
                self.pptx.scroll_by(delta, width)
            };
        }
        if self.pdf.active_path.is_none()
            || (!self.showing_pdf && !(self.ui.split_view_active && self.active_path.is_some()))
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
        scroll_pdf_to((self.pdf.scroll_y + delta).clamp(0.0, max_y))
    }

    /// `PdfMessage::PageLinksLoaded`.
    pub(super) fn on_pdf_page_links_loaded(
        &mut self,
        generation: u64,
        page: u16,
        links: Vec<md_editor_core::pdf::LinkInfo>,
    ) -> Task<Message> {
        self.pdf.pending_links.remove(&page);
        if generation != self.pdf.render_generation {
            return Task::none();
        }
        self.pdf.page_links.insert(page, links);
        Task::none()
    }

    /// `PdfMessage::TocLoaded`: flatten the outline into the TOC panel's entries.
    pub(super) fn on_pdf_toc_loaded(
        &mut self,
        path: String,
        entries: Vec<md_editor_core::pdf::TocEntry>,
        synthetic: bool,
    ) -> Task<Message> {
        // Path-gated, not generation-gated: fit-to-width bumps the render
        // generation right after open, and the TOC scan for bookmark-less PDFs
        // regularly finishes after that. Only a result for a different document
        // is stale.
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
}

/// Scroll the PDF page list to an absolute vertical offset.
pub(super) fn scroll_pdf_to(y: f32) -> Task<Message> {
    operation::scroll_to(
        iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
        AbsoluteOffset { x: 0.0, y },
    )
}

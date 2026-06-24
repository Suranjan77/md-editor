//! PDF viewer sub-state.
//!
//! Owns the PDF document fields that used to be `pdf_*`/`active_pdf_path`/
//! `focused_annotation_id` members of `MdEditor`: rendered pages, page
//! geometry, scroll position, annotations, selection, text, and the
//! render/scroll bookkeeping. For now this is a field container — the PDF
//! geometry/render/navigation methods still live on the shell and read through
//! `self.pdf`; moving them here is a worthwhile follow-up.
//!
//! Part of the `MdEditor` decomposition; see
//! `docs/refactor-mdeditor-decomposition.md`.

use std::collections::{HashMap, HashSet, VecDeque};

use iced::widget::image::Handle;
use iced::Task;

use md_editor_core::pdf::{
    LinkInfo, PdfAnnotation, PdfAnnotationColor, PdfPageText, PdfSearchMatch,
};

use crate::messages::Message;
use crate::views::interactive_pdf::PdfSelection;
use crate::views::pdf_viewer::{PDF_PAGE_LIST_PADDING, PDF_PAGE_SPACING};

pub struct PdfPane {
    pub active_path: Option<String>,
    pub current_page: u16,
    pub total_pages: u16,
    pub zoom: f32,
    pub fit_to_width: bool,
    pub scroll_y: f32,

    pub pages: Vec<Option<Handle>>,
    pub dimensions: Vec<Option<(u32, u32)>>,
    pub page_sizes: Vec<Option<(f32, f32)>>,
    pub placeholder_page_size: Option<(f32, f32)>,

    pub page_links: HashMap<u16, Vec<LinkInfo>>,
    pub link_preview: Option<Handle>,

    pub document_id: Option<String>,
    pub page_text: HashMap<u16, PdfPageText>,
    pub selection: Option<PdfSelection>,
    pub annotations: HashMap<u16, Vec<PdfAnnotation>>,
    pub focused_annotation_id: Option<String>,

    pub initial_target_page: Option<u16>,
    pub initial_target_annotation: Option<String>,

    pub pending_text: HashSet<u16>,
    pub text_lru: VecDeque<u16>,
    pub pending_pages: HashSet<u16>,
    pub pending_links: HashSet<u16>,

    pub render_generation: u64,
    pub programmatic_scroll: bool,
    pub toc_target_page: Option<u16>,

    /// Color used by the next quick highlight; advances through the palette on
    /// each quick highlight so successive highlights are visually distinct.
    pub next_highlight_color: PdfAnnotationColor,
}

impl PdfPane {
    pub fn new() -> Self {
        Self {
            active_path: None,
            current_page: 0,
            total_pages: 0,
            zoom: 1.5,
            fit_to_width: true,
            scroll_y: 0.0,
            pages: Vec::new(),
            dimensions: Vec::new(),
            page_sizes: Vec::new(),
            placeholder_page_size: None,
            page_links: HashMap::new(),
            link_preview: None,
            document_id: None,
            page_text: HashMap::new(),
            selection: None,
            annotations: HashMap::new(),
            focused_annotation_id: None,
            initial_target_page: None,
            initial_target_annotation: None,
            pending_text: HashSet::new(),
            text_lru: VecDeque::new(),
            pending_pages: HashSet::new(),
            pending_links: HashSet::new(),
            render_generation: 0,
            programmatic_scroll: false,
            toc_target_page: None,
            next_highlight_color: PdfAnnotationColor::Yellow,
        }
    }

    /// Handle messages that mutate only this pane's own state: render
    /// bookkeeping (page sizes/skips), cached page text, the link preview,
    /// and selection clearing. Arms that need the shared `AppState`, resolve
    /// vault paths, or render/navigate (which the shell coordinates) stay on
    /// the shell.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PdfPageSizesLoaded(generation, path, sizes) => {
                if generation != self.render_generation
                    && self.active_path.as_deref() != Some(path.as_str())
                {
                    return Task::none();
                }
                self.page_sizes = sizes.into_iter().map(Some).collect();
                if self.page_sizes.len() < self.total_pages as usize {
                    self.page_sizes.resize(self.total_pages as usize, None);
                }
                if self.placeholder_page_size.is_none() {
                    self.placeholder_page_size = self.first_page_size();
                }
                if self.fit_to_width && self.total_pages > 0 {
                    Task::done(Message::PdfFitToWidth)
                } else {
                    Task::none()
                }
            }
            Message::PdfRenderSkipped(generation, page) => {
                self.pending_pages.remove(&page);
                if generation != self.render_generation {
                    return Task::none();
                }
                if self.toc_target_page == Some(page) {
                    self.toc_target_page = None;
                    self.programmatic_scroll = false;
                }
                Task::none()
            }
            Message::ClosePdfLinkPreview => {
                self.link_preview = None;
                Task::none()
            }
            Message::PdfPageTextLoaded(generation, page, res) => {
                self.pending_text.remove(&page);
                if generation == self.render_generation {
                    if let Ok(page_text) = res {
                        self.page_text.insert(page, page_text);
                        self.text_lru.push_back(page);
                        if self.text_lru.len() > 12 {
                            if let Some(oldest) = self.text_lru.pop_front() {
                                self.page_text.remove(&oldest);
                            }
                        }
                    }
                }
                Task::none()
            }
            Message::PdfSelectionCleared => {
                self.selection = None;
                Task::none()
            }
            _ => Task::none(),
        }
    }

    // ── Page geometry ────────────────────────────────────────────────
    //
    // Pure functions of the PDF state (page sizes/dimensions/zoom/total). The
    // layout-dependent available-width calculation stays on the shell, since it
    // needs sidebar/TOC/split/window state.

    pub fn estimated_page_height(&self) -> f32 {
        self.placeholder_display_size().1
    }

    pub fn first_page_size(&self) -> Option<(f32, f32)> {
        self.page_sizes.first().and_then(|s| *s).or_else(|| {
            self.dimensions
                .first()
                .and_then(|d| d.map(|(w, h)| (w as f32 / self.zoom, h as f32 / self.zoom)))
        })
    }

    pub fn placeholder_display_size(&self) -> (f32, f32) {
        placeholder_display_size_from(
            self.placeholder_page_size,
            self.page_sizes.first().and_then(|s| *s),
            self.dimensions.first().and_then(|d| *d),
            self.zoom,
        )
    }

    pub fn page_display_size(&self, page: u16) -> (f32, f32) {
        if let Some(Some((w, h))) = self.page_sizes.get(page as usize) {
            (*w * self.zoom, *h * self.zoom)
        } else {
            self.placeholder_display_size()
        }
    }

    pub fn page_height(&self, page: u16) -> f32 {
        if (page as usize) < self.total_pages as usize {
            self.page_display_size(page).1
        } else {
            self.estimated_page_height()
        }
    }

    pub fn page_offset(&self, page: u16) -> f32 {
        let mut offset = PDF_PAGE_LIST_PADDING;
        let limit = page.min(self.total_pages);
        for i in 0..limit {
            offset += self.page_height(i) + PDF_PAGE_SPACING;
        }
        offset
    }

    pub fn total_height(&self) -> f32 {
        if self.total_pages == 0 {
            return PDF_PAGE_LIST_PADDING;
        }
        let mut total = PDF_PAGE_LIST_PADDING;
        for i in 0..self.total_pages {
            total += self.page_height(i) + PDF_PAGE_SPACING;
        }
        total
    }

    pub fn page_at_scroll(&self, scroll_y: f32) -> u16 {
        if self.total_pages == 0 {
            return 0;
        }
        let mut offset = PDF_PAGE_LIST_PADDING;
        for i in 0..self.total_pages {
            let page_h = self.page_height(i);
            if scroll_y < offset + page_h + PDF_PAGE_SPACING {
                return i;
            }
            offset += page_h + PDF_PAGE_SPACING;
        }
        self.total_pages.saturating_sub(1)
    }

    pub fn search_match_scroll_y(&self, result: &PdfSearchMatch) -> f32 {
        let rect = result.rects.first();
        let page_height = self
            .page_sizes
            .get(result.page_index as usize)
            .and_then(|size| *size)
            .map(|(_, h)| h)
            .unwrap_or_else(|| self.page_height(result.page_index) / self.zoom.max(0.01));
        search_match_scroll_y_from(
            self.page_offset(result.page_index),
            rect.map(|rect| rect.y),
            rect.map(|rect| rect.height).unwrap_or(0.0),
            page_height,
            self.zoom,
            self.total_height(),
        )
    }

    pub fn annotation_at(&self, page_idx: u16, x: f32, y: f32) -> Option<PdfAnnotation> {
        let page_text = self.page_text.get(&page_idx)?;
        let px = x * page_text.page_width;
        let py = y * page_text.page_height;

        let page_anns = self.annotations.get(&page_idx)?;
        for ann in page_anns {
            for rect in &ann.rects {
                let view_y = page_text.page_height - rect.y - rect.height;
                if px >= rect.x
                    && px <= rect.x + rect.width
                    && py >= view_y
                    && py <= view_y + rect.height
                {
                    return Some(ann.clone());
                }
            }
        }
        None
    }

    pub fn link_at(&self, page_idx: u16, x: f32, y: f32) -> Option<LinkInfo> {
        let links = self.page_links.get(&page_idx)?;
        let dim = self.dimensions.get(page_idx as usize).and_then(|d| *d)?;
        let real_x = (x * dim.0 as f32) / self.zoom;
        let real_y = (y * dim.1 as f32) / self.zoom;

        links
            .iter()
            .find(|link| {
                let lx = link.bbox.x;
                let ly = link.bbox.y;
                let lw = link.bbox.width;
                let lh = link.bbox.height;
                real_x >= lx && real_x <= lx + lw && real_y >= ly && real_y <= ly + lh
            })
            .cloned()
    }
}

/// Resolve the on-screen placeholder page size from the best available source,
/// scaled by `zoom`. Falls back to US-Letter dimensions.
pub(crate) fn placeholder_display_size_from(
    placeholder_page_size: Option<(f32, f32)>,
    first_page_size: Option<(f32, f32)>,
    first_dimensions: Option<(u32, u32)>,
    zoom: f32,
) -> (f32, f32) {
    placeholder_page_size
        .or(first_page_size)
        .or_else(|| first_dimensions.map(|(w, h)| (w as f32 / zoom, h as f32 / zoom)))
        .map(|(w, h)| (w * zoom, h * zoom))
        .unwrap_or((612.0 * zoom, 792.0 * zoom))
}

/// Scroll offset that brings a search-match rect into view, clamped to the
/// scrollable range.
pub(crate) fn search_match_scroll_y_from(
    page_offset: f32,
    rect_y: Option<f32>,
    rect_height: f32,
    page_height: f32,
    zoom: f32,
    max_y: f32,
) -> f32 {
    let match_top = rect_y
        .map(|y| (page_height - y - rect_height).max(0.0) * zoom)
        .unwrap_or(0.0);
    (page_offset + match_top - 96.0).clamp(0.0, max_y.max(0.0))
}

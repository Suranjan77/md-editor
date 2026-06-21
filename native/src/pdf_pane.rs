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

use md_editor_core::pdf::{LinkInfo, PdfAnnotation, PdfPageText};

use crate::views::interactive_pdf::PdfSelection;

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
        }
    }
}

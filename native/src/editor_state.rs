//! Editor pane sub-state.
//!
//! Owns the document buffer, the highlighted/rendered lines, the debounced
//! highlight pipeline bookkeeping, the buffer revision (used by SearchState to
//! invalidate its match cache), the table of contents, the editor
//! scroll/viewport geometry, and the image/math resource caches used when
//! rendering markdown content.
//!
//! The editing/highlighting methods still live on the shell and read through
//! `self.editor`; moving them here is a sensible follow-up.
//!
//! Final domain in the `MdEditor` decomposition; see
//! `docs/refactor-mdeditor-decomposition.md`.

use std::collections::HashMap;
use std::time::Instant;

use iced::widget::image::Handle;

use crate::editor::buffer::DocBuffer;
use crate::editor::highlight::StyledLine;
use crate::views;

pub struct EditorPane {
    pub buffer: DocBuffer,
    pub highlighted_lines: Vec<StyledLine>,

    pub highlight_generation: u64,
    pub pending_highlight_generation: Option<u64>,
    pub pending_highlight_requested_at: Option<Instant>,
    pub pending_highlight_text: Option<String>,

    /// Bumped on every text change; read by SearchState to invalidate its
    /// in-document match cache.
    pub buffer_revision: u64,

    pub toc_visible: bool,
    pub toc_entries: Vec<views::toc::TocEntry>,

    pub scroll_y: f32,
    pub viewport_width: f32,
    pub viewport_height: f32,

    pub image_cache: HashMap<String, (Handle, f32, f32)>,
    pub math_cache: HashMap<String, (Handle, f32, f32)>,
}

impl EditorPane {
    pub fn new() -> Self {
        Self {
            buffer: DocBuffer::new(),
            highlighted_lines: Vec::new(),
            highlight_generation: 0,
            pending_highlight_generation: None,
            pending_highlight_requested_at: None,
            pending_highlight_text: None,
            buffer_revision: 0,
            toc_visible: false,
            toc_entries: Vec::new(),
            scroll_y: 0.0,
            viewport_width: 900.0,
            viewport_height: 720.0,
            image_cache: HashMap::new(),
            math_cache: HashMap::new(),
        }
    }
}

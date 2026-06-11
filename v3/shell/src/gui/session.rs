//! Per-document shell state. The kernel's `DocumentStore` owns identity;
//! these sessions own the *content* state the GUI needs — the editor engine
//! instance for markdown, page/zoom/frame for PDFs.

use std::collections::HashMap;

use md3_editor::buffer::Command;
use md3_editor::document::EditorDocument;
use md3_editor::layout::Damage;
use md3_kernel::pane::DocumentId;

use super::editor_canvas::{LINE_HEIGHT, MonoMeasurer};

pub struct MdSession {
    pub doc: EditorDocument<MonoMeasurer>,
    /// Vault-relative path (the kernel document path).
    pub rel_path: String,
    pub scroll: f32,
    /// Last viewport height a canvas event reported; used to keep the caret
    /// visible after edits. Refined on every mouse interaction.
    pub viewport_h: f32,
}

impl MdSession {
    pub fn new(rel_path: &str, text: &str) -> MdSession {
        MdSession {
            // Wrap is effectively off in the M1 shell (MonoMeasurer returns
            // one row regardless), so the width only has to be finite.
            doc: EditorDocument::new(MonoMeasurer, 1e9, text),
            rel_path: rel_path.to_string(),
            scroll: 0.0,
            viewport_h: 600.0,
        }
    }

    /// Apply an editor command and keep the caret on screen.
    pub fn apply(&mut self, command: Command) -> Damage {
        let (_, damage) = self.doc.apply(command);
        self.scroll_caret_into_view();
        damage
    }

    pub fn scroll_caret_into_view(&mut self) {
        let head = self.doc.buffer().primary().head;
        let (line, _) = self.doc.buffer().offset_to_line_col(head);
        let Ok(top) = self.doc.layout().offset_of(line) else {
            return;
        };
        let top = top as f32;
        let bottom = top + LINE_HEIGHT;
        if top < self.scroll {
            self.scroll = top;
        } else if bottom > self.scroll + self.viewport_h {
            self.scroll = bottom - self.viewport_h;
        }
        self.clamp_scroll();
    }

    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll += dy;
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let max = (self.doc.layout().total_height() as f32 - self.viewport_h + LINE_HEIGHT)
            .max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
    }
}

#[derive(Debug, Default)]
pub struct PdfSession {
    pub rel_path: String,
    pub page: u32,
    pub page_count: u32,
    pub zoom: f32,
    /// Rendered current page, when the `pdfium` feature is on and the
    /// library bound: (width, height, rgba image handle).
    pub frame: Option<(u32, u32, iced::widget::image::Handle)>,
    pub status: String,
}

impl PdfSession {
    pub fn new(rel_path: &str) -> PdfSession {
        PdfSession {
            rel_path: rel_path.to_string(),
            page: 0,
            page_count: 0,
            zoom: 1.0,
            frame: None,
            status: String::new(),
        }
    }
}

/// All open sessions, keyed by kernel document id. Dropped when the kernel
/// garbage-collects the document (last tab closed).
#[derive(Default)]
pub struct Sessions {
    pub md: HashMap<DocumentId, MdSession>,
    pub pdf: HashMap<DocumentId, PdfSession>,
}

impl Sessions {
    /// Drop sessions whose documents the kernel no longer knows.
    pub fn gc(&mut self, docs: &md3_kernel::pane::DocumentStore) {
        self.md.retain(|id, _| docs.get(*id).is_some());
        self.pdf.retain(|id, _| docs.get(*id).is_some());
    }
}

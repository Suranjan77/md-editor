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
        let max =
            (self.doc.layout().total_height() as f32 - self.viewport_h + LINE_HEIGHT).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
    }
}

/// Gap between page sheets in display px (zoom-independent).
pub const PAGE_GAP: f32 = 16.0;
/// Tile pixmap budget per document (bytes).
const TILE_BUDGET: usize = 192 * 1024 * 1024;

/// An in-progress or finished text selection, anchored to one page.
/// Geometry lives in page points (top-left origin) so it survives scroll
/// and zoom unchanged; the canvas projects it per frame.
#[derive(Debug, Clone)]
pub struct PdfSelection {
    pub page: u32,
    /// Where the drag started, page points.
    pub anchor: (f32, f32),
    pub quads: Vec<md3_pdf::SelRect>,
    pub text: String,
}

pub struct PdfSession {
    pub rel_path: String,
    /// Continuous-scroll geometry; `None` until page sizes load (no pdfium,
    /// or the file failed to open) — the placeholder view shows `status`.
    pub layout: Option<md3_pdf::DocLayout>,
    pub scroll: f32,
    pub zoom: f32,
    /// Last viewport a canvas event reported (px); used by tile requests.
    pub viewport: (f32, f32),
    /// Rendered tile pixmaps, owned here; the engine cache owns the budget
    /// accounting and tells us what to drop.
    pub tiles: HashMap<md3_pdf::TileKey, iced::widget::image::Handle>,
    pub cache: md3_pdf::TileCache,
    pub queue: md3_pdf::RenderQueue,
    pub status: String,
    /// SHA-256 of the file's bytes — the annotation identity (vault
    /// convention). Present whenever the file was readable on open.
    pub doc_hash: Option<String>,
    /// Glyph geometry per page, loaded on first selection touch. Empty vec
    /// = page has no selectable text (or no pdfium).
    pub chars: HashMap<u32, Vec<md3_pdf::CharBox>>,
    pub selection: Option<PdfSelection>,
    /// Flattened bookmark tree (empty: none, or no pdfium); loaded with the
    /// page geometry on open.
    pub outline: Vec<md3_pdf::OutlineEntry>,
    /// This document's stored annotations, refreshed after every mutation.
    pub annotations: Vec<md3_vault::Annotation>,
    /// Annotation picked by clicking one of its quads; note edits and
    /// deletion target it.
    pub selected_annotation: Option<i64>,
    /// Jump-list history (plan §3.3 back/forward): scroll positions in
    /// *points* (display px ÷ zoom) so entries survive zoom changes.
    back: Vec<f32>,
    forward: Vec<f32>,
}

impl PdfSession {
    pub fn new(rel_path: &str) -> PdfSession {
        PdfSession {
            rel_path: rel_path.to_string(),
            layout: None,
            scroll: 0.0,
            zoom: 1.0,
            viewport: (1000.0, 750.0),
            tiles: HashMap::new(),
            cache: md3_pdf::TileCache::new(TILE_BUDGET),
            queue: md3_pdf::RenderQueue::new(),
            status: String::new(),
            doc_hash: None,
            chars: HashMap::new(),
            selection: None,
            outline: Vec::new(),
            annotations: Vec::new(),
            selected_annotation: None,
            back: Vec::new(),
            forward: Vec::new(),
        }
    }

    /// Call *before* a jump (go-to-page, TOC, find): the position being
    /// left becomes reachable with `pdf.back`, and the forward branch is
    /// dropped (same grammar as every jump list).
    pub fn record_jump(&mut self) {
        const CAP: usize = 64;
        self.forward.clear();
        self.back.push(self.scroll / self.zoom);
        if self.back.len() > CAP {
            self.back.remove(0);
        }
    }

    /// Pop the jump history. `false` when there is nowhere to go.
    pub fn nav_back(&mut self) -> bool {
        let Some(pos) = self.back.pop() else {
            return false;
        };
        self.forward.push(self.scroll / self.zoom);
        self.scroll_to_points(pos);
        true
    }

    pub fn nav_forward(&mut self) -> bool {
        let Some(pos) = self.forward.pop() else {
            return false;
        };
        self.back.push(self.scroll / self.zoom);
        self.scroll_to_points(pos);
        true
    }

    fn scroll_to_points(&mut self, points: f32) {
        let max = self
            .layout
            .as_ref()
            .map_or(0.0, |l| l.max_scroll(self.viewport.1));
        self.scroll = (points * self.zoom).clamp(0.0, max);
    }

    /// Title of the outline section the viewport is in, for the status
    /// pill. `None` without an outline or above the first section.
    pub fn current_section(&self) -> Option<&str> {
        let i = md3_pdf::section_at(&self.outline, self.current_page() as u32)?;
        Some(self.outline[i].title.as_str())
    }

    /// The stored annotation whose quads contain the page point, topmost
    /// (most recent) first.
    pub fn annotation_at(&self, page: u32, pt: (f32, f32)) -> Option<&md3_vault::Annotation> {
        self.annotations.iter().rev().find(|a| {
            a.page == page
                && a.quads.iter().any(|q| {
                    f64::from(pt.0) >= q.x0
                        && f64::from(pt.0) <= q.x1
                        && f64::from(pt.1) >= q.y0
                        && f64::from(pt.1) <= q.y1
                })
        })
    }

    pub fn selected_annotation(&self) -> Option<&md3_vault::Annotation> {
        let id = self.selected_annotation?;
        self.annotations.iter().find(|a| a.id == id)
    }

    /// 0-based page the viewport is "on" — what the page pill shows and
    /// what zoom changes re-anchor to (the page a third down the screen).
    pub fn current_page(&self) -> usize {
        match &self.layout {
            Some(layout) => layout.page_at(self.scroll + self.viewport.1 / 3.0),
            None => 0,
        }
    }

    pub fn page_count(&self) -> usize {
        self.layout
            .as_ref()
            .map_or(0, md3_pdf::DocLayout::page_count)
    }

    pub fn scroll_by(&mut self, dy: f32) {
        let max = self
            .layout
            .as_ref()
            .map_or(0.0, |l| l.max_scroll(self.viewport.1));
        self.scroll = (self.scroll + dy).clamp(0.0, max);
    }

    pub fn go_to_page(&mut self, page: usize) {
        if let Some(layout) = &self.layout {
            let max = layout.max_scroll(self.viewport.1);
            self.scroll = layout.page_top(page).clamp(0.0, max);
        }
    }

    /// Change zoom, keeping the current page anchored at the top of the
    /// viewport. Tiles from the old bucket stay cached (zoom wiggles within
    /// a bucket cost nothing); newly needed ones render on the next ensure.
    pub fn set_zoom(&mut self, zoom: f32) {
        let zoom = zoom.clamp(0.25, 6.0);
        let anchor = self.current_page();
        self.zoom = zoom;
        if let Some(layout) = &mut self.layout {
            layout.set_zoom(zoom);
            let max = layout.max_scroll(self.viewport.1);
            self.scroll = layout.page_top(anchor).clamp(0.0, max);
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

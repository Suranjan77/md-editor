//! Per-document shell state. The kernel's `DocumentStore` owns identity;
//! these sessions own the *content* state the GUI needs — the editor engine
//! instance for markdown, page/zoom/frame for PDFs.

use std::collections::{HashMap, HashSet};

use md3_editor::buffer::Command;
use md3_editor::document::EditorDocument;
use md3_editor::layout::Damage;
use md3_editor::style::SpanKind;
use md3_kernel::pane::DocumentId;

use super::editor_canvas::{LINE_HEIGHT, MonoMeasurer};

pub struct MdSession {
    pub doc: EditorDocument<MonoMeasurer>,
    measurer: MonoMeasurer,
    /// Vault-relative path (the kernel document path).
    pub rel_path: String,
    pub scroll: f32,
    /// Last viewport height a canvas event reported; used to keep the caret
    /// visible after edits. Refined on every mouse interaction.
    pub viewport_h: f32,
    pub outline_open: bool,
    pub outline_width: f32,
    pub find_open: bool,
    pub find_query: String,
    pub replace_text: String,
    pub image_cache: HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    pub math_cache: HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    math_blocks: HashMap<usize, String>,
    math_block_continuations: HashSet<usize>,
}

impl MdSession {
    pub fn new(rel_path: &str, text: &str) -> MdSession {
        let measurer = MonoMeasurer::default();
        MdSession {
            doc: EditorDocument::new(measurer.clone(), 976.0, text),
            measurer,
            rel_path: rel_path.to_string(),
            scroll: 0.0,
            viewport_h: 600.0,
            outline_open: false,
            outline_width: 250.0,
            find_open: false,
            find_query: String::new(),
            replace_text: String::new(),
            image_cache: HashMap::new(),
            math_cache: HashMap::new(),
            math_blocks: HashMap::new(),
            math_block_continuations: HashSet::new(),
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
        let bottom = top
            + self
                .doc
                .layout()
                .height_of(line)
                .unwrap_or(f64::from(LINE_HEIGHT)) as f32;
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

    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport_h = height.max(LINE_HEIGHT);
        self.doc.set_wrap_width((width - 32.0).max(10.0) as f64);
        self.scroll_caret_into_view();
    }

    pub fn load_visual_assets(&mut self, abs_path: &std::path::Path) {
        let Some(base_path) = abs_path.parent() else {
            return;
        };
        let block_lines = self.load_math_blocks();
        for line in 0..self.doc.line_count() {
            if block_lines.contains(&line) {
                continue;
            }
            let Some(styled) = self.doc.styled_line(line) else {
                continue;
            };
            for span in styled.spans {
                match span.kind {
                    SpanKind::Image { url } if !self.image_cache.contains_key(&url) => {
                        if let Ok(image) = image::open(base_path.join(&url)) {
                            let width = image.width();
                            let height = image.height();
                            let handle = iced::widget::image::Handle::from_rgba(
                                width,
                                height,
                                image.into_rgba8().into_raw(),
                            );
                            self.image_cache
                                .insert(url.clone(), (handle, width as f32, height as f32));
                            self.measurer
                                .set_image_size(url, width as f32, height as f32);
                        }
                    }
                    SpanKind::Math | SpanKind::MathContent => {
                        let chars = styled.display.chars().collect::<Vec<_>>();
                        let tex = chars[span.range]
                            .iter()
                            .collect::<String>()
                            .trim_matches('$')
                            .trim()
                            .to_string();
                        if !tex.is_empty()
                            && !self.math_cache.contains_key(&tex)
                            && let Ok(rendered) = render_math(&tex)
                        {
                            self.measurer
                                .set_math_size(tex.clone(), rendered.1, rendered.2);
                            self.math_cache.insert(tex, rendered);
                        }
                    }
                    _ => {}
                }
            }
        }
        self.doc.remeasure();
        self.clamp_scroll();
    }

    pub fn math_block_at(&self, line: usize) -> Option<&str> {
        self.math_blocks.get(&line).map(String::as_str)
    }

    pub fn is_math_block_continuation(&self, line: usize) -> bool {
        self.math_block_continuations.contains(&line)
    }

    fn load_math_blocks(&mut self) -> HashSet<usize> {
        self.math_blocks.clear();
        self.math_block_continuations.clear();
        let mut covered = HashSet::new();
        let mut line = 0;
        while line < self.doc.line_count() {
            let Some(styled) = self.doc.styled_line(line) else {
                line += 1;
                continue;
            };
            if !matches!(styled.kind, md3_editor::parse::LineKind::MathOpen) {
                line += 1;
                continue;
            }

            let mut cursor = line + 1;
            let mut content = Vec::new();
            let mut content_lines = Vec::new();
            while cursor < self.doc.line_count() {
                let Some(styled) = self.doc.styled_line(cursor) else {
                    break;
                };
                match styled.kind {
                    md3_editor::parse::LineKind::MathContent => {
                        content.push(styled.display);
                        content_lines.push(cursor);
                        covered.insert(cursor);
                    }
                    md3_editor::parse::LineKind::MathClose => break,
                    _ => break,
                }
                cursor += 1;
            }

            if let Some(&leader) = content_lines.first() {
                let tex = content.join("\n");
                if !tex.is_empty() {
                    if !self.math_cache.contains_key(&tex)
                        && let Ok(rendered) = render_math(&tex)
                    {
                        self.math_cache.insert(tex.clone(), rendered);
                    }
                    if let Some((_, width, height)) = self.math_cache.get(&tex) {
                        self.measurer.set_math_block_size(
                            self.doc.buffer().line_text(leader),
                            *width,
                            *height,
                        );
                        self.math_blocks.insert(leader, tex);
                        self.math_block_continuations
                            .extend(content_lines.into_iter().skip(1));
                    }
                }
            }
            line = cursor.saturating_add(1);
        }
        covered
    }

    fn clamp_scroll(&mut self) {
        let max =
            (self.doc.layout().total_height() as f32 - self.viewport_h + LINE_HEIGHT).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
    }
}

fn render_math(tex: &str) -> Result<(iced::widget::image::Handle, f32, f32), String> {
    use ratex_layout::{LayoutOptions, layout, to_display_list};
    use ratex_parser::parser::parse;
    use ratex_render::{RenderOptions, render_to_png};
    use ratex_types::color::Color as RatexColor;
    use ratex_types::math_style::MathStyle;

    let render_options = RenderOptions {
        font_size: 24.0,
        padding: 4.0,
        background_color: RatexColor {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.0,
        },
        font_dir: String::new(),
        device_pixel_ratio: 2.0,
    };
    let layout_options = LayoutOptions::default()
        .with_style(MathStyle::Display)
        .with_color(RatexColor {
            r: 0.89,
            g: 0.90,
            b: 0.93,
            a: 1.0,
        });
    let ast = parse(tex).map_err(|error| format!("math parse: {error}"))?;
    let layout_box = layout(&ast, &layout_options);
    let bytes = render_to_png(&to_display_list(&layout_box), &render_options)
        .map_err(|error| format!("math render: {error:?}"))?;
    let image = image::load_from_memory(&bytes).map_err(|error| error.to_string())?;
    Ok((
        iced::widget::image::Handle::from_bytes(bytes),
        image.width() as f32 / 2.0,
        image.height() as f32 / 2.0,
    ))
}

/// Gap between page sheets in display px (zoom-independent).
pub const PAGE_GAP: f32 = 16.0;
/// Tile pixmap budget per document (bytes).
const TILE_BUDGET: usize = 192 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfFitMode {
    Width,
    Page,
}

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
    pub fit_mode: Option<PdfFitMode>,
    /// Last viewport a canvas event reported (px); used by tile requests.
    pub viewport: (f32, f32),
    /// Rendered tile pixmaps, owned here; the engine cache owns the budget
    /// accounting and tells us what to drop.
    pub tiles: HashMap<md3_pdf::TileKey, iced::widget::image::Handle>,
    pub cache: md3_pdf::TileCache,
    pub queue: md3_pdf::RenderQueue,
    /// Tiles handed to the async worker and not yet returned — never
    /// re-scheduled while here (the worker cannot cancel in-flight work;
    /// stale results land in the LRU cache harmlessly).
    pub tiles_in_flight: std::collections::HashSet<md3_pdf::TileKey>,
    /// Pages whose glyph/link loads are at the worker; absence from the map
    /// + absence here = not yet requested.
    pub chars_pending: std::collections::HashSet<u32>,
    pub links_pending: std::collections::HashSet<u32>,
    pub status: String,
    /// SHA-256 of the file's bytes — the annotation identity (vault
    /// convention). Present whenever the file was readable on open.
    pub doc_hash: Option<String>,
    /// Glyph geometry per page, loaded on first selection touch. Empty vec
    /// = page has no selectable text (or no pdfium).
    pub chars: HashMap<u32, Vec<md3_pdf::CharBox>>,
    pub links: HashMap<u32, Vec<md3_pdf::LinkBox>>,
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
    pub toc_open: bool,
    pub toc_width: f32,
    pub annotations_open: bool,
    pub annotations_width: f32,
}

impl PdfSession {
    pub fn new(rel_path: &str) -> PdfSession {
        PdfSession {
            rel_path: rel_path.to_string(),
            layout: None,
            scroll: 0.0,
            zoom: 1.0,
            fit_mode: None,
            viewport: (1000.0, 750.0),
            tiles: HashMap::new(),
            cache: md3_pdf::TileCache::new(TILE_BUDGET),
            queue: md3_pdf::RenderQueue::new(),
            tiles_in_flight: std::collections::HashSet::new(),
            chars_pending: std::collections::HashSet::new(),
            links_pending: std::collections::HashSet::new(),
            status: String::new(),
            doc_hash: None,
            chars: HashMap::new(),
            links: HashMap::new(),
            selection: None,
            outline: Vec::new(),
            annotations: Vec::new(),
            selected_annotation: None,
            back: Vec::new(),
            forward: Vec::new(),
            toc_open: false,
            toc_width: 240.0,
            annotations_open: false,
            annotations_width: 240.0,
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

    /// Return the index of the outline section the viewport is currently viewing.
    pub fn current_section_index(&self) -> Option<usize> {
        md3_pdf::section_at(&self.outline, self.current_page() as u32)
    }

    /// Extract highlight text for a given annotation by checking which character boxes' centers lie within the annotation's quads.
    pub fn annotation_text(&self, a: &md3_vault::Annotation) -> String {
        let Some(chars) = self.chars.get(&a.page) else {
            return String::new();
        };
        let mut text = String::new();
        for c in chars {
            let cx = (c.x0 + c.x1) / 2.0;
            let cy = (c.y0 + c.y1) / 2.0;
            let inside = a.quads.iter().any(|q| {
                f64::from(cx) >= q.x0
                    && f64::from(cx) <= q.x1
                    && f64::from(cy) >= q.y0
                    && f64::from(cy) <= q.y1
            });
            if inside {
                text.push(c.ch);
            }
        }
        text.trim().to_string()
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

    /// Find link at the given point, topmost-last.
    pub fn link_at(&self, page: u32, pt: (f32, f32)) -> Option<&md3_pdf::LinkBox> {
        self.links.get(&page)?.iter().rev().find(|l| {
            pt.0 >= l.rect.x0 && pt.0 <= l.rect.x1 && pt.1 >= l.rect.y0 && pt.1 <= l.rect.y1
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
        self.fit_mode = None;
        self.apply_zoom(zoom);
    }

    pub fn set_fit_mode(&mut self, mode: PdfFitMode) {
        self.fit_mode = Some(mode);
        self.refit();
    }

    pub fn set_viewport(&mut self, viewport: (f32, f32)) {
        if viewport.0 <= 0.0 || viewport.1 <= 0.0 {
            return;
        }
        self.viewport = viewport;
        if self.fit_mode.is_some() {
            self.refit();
        } else if let Some(layout) = &self.layout {
            self.scroll = self.scroll.clamp(0.0, layout.max_scroll(viewport.1));
        }
    }

    fn refit(&mut self) {
        let Some(mode) = self.fit_mode else {
            return;
        };
        let Some(layout) = &self.layout else {
            return;
        };
        let page = self.current_page();
        let zoom = match mode {
            PdfFitMode::Width => layout.zoom_for_fit_width(page, self.viewport.0),
            PdfFitMode::Page => layout.zoom_for_fit_page(page, self.viewport),
        };
        self.apply_zoom(zoom);
    }

    fn apply_zoom(&mut self, zoom: f32) {
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

#[cfg(test)]
mod tests {
    use super::{MdSession, PdfFitMode, PdfSession};

    #[test]
    fn fit_width_tracks_viewport_resize_until_manual_zoom() {
        let mut session = PdfSession::new("paper.pdf");
        session.layout = Some(md3_pdf::DocLayout::new(vec![(600.0, 800.0)], 1.0, 16.0));
        session.set_viewport((632.0, 700.0));
        session.set_fit_mode(PdfFitMode::Width);
        assert!((session.zoom - 1.0).abs() < 0.001);

        session.set_viewport((332.0, 700.0));
        assert!((session.zoom - 0.5).abs() < 0.001);

        session.set_zoom(1.25);
        session.set_viewport((632.0, 700.0));
        assert!((session.zoom - 1.25).abs() < 0.001);
        assert_eq!(session.fit_mode, None);
    }

    #[test]
    fn fit_page_uses_both_resized_dimensions() {
        let mut session = PdfSession::new("paper.pdf");
        session.layout = Some(md3_pdf::DocLayout::new(vec![(600.0, 800.0)], 1.0, 16.0));
        session.set_viewport((632.0, 432.0));
        session.set_fit_mode(PdfFitMode::Page);
        assert!((session.zoom - 0.5).abs() < 0.001);
    }

    #[test]
    fn markdown_visual_assets_load_from_note_directory() {
        let dir = match tempfile::tempdir() {
            Ok(dir) => dir,
            Err(error) => panic!("tempdir: {error}"),
        };
        let image_path = dir.path().join("plot.png");
        let image = image::RgbaImage::from_pixel(8, 6, image::Rgba([10, 20, 30, 255]));
        if let Err(error) = image.save(&image_path) {
            panic!("save image: {error}");
        }
        let note_path = dir.path().join("note.md");
        let mut session = MdSession::new("note.md", "![plot](plot.png)\n$x^2$");
        session.load_visual_assets(&note_path);

        assert!(session.image_cache.contains_key("plot.png"));
        assert!(session.math_cache.contains_key("x^2"));
    }

    #[test]
    fn multiline_math_environment_is_rendered_as_one_block() {
        let dir = match tempfile::tempdir() {
            Ok(dir) => dir,
            Err(error) => panic!("tempdir: {error}"),
        };
        let note_path = dir.path().join("note.md");
        let text = "$$\n\\begin{align}\nx &= a + b \\\\\ny &= c + d\n\\end{align}\n$$";
        let mut session = MdSession::new("note.md", text);
        session.load_visual_assets(&note_path);

        assert!(session.math_block_at(1).is_some());
        assert!(session.is_math_block_continuation(2));
        assert!(session.is_math_block_continuation(3));
        assert!(session.is_math_block_continuation(4));
    }
}

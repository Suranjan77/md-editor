//! pdfium wiring for the tile renderer (plan §3.3, ADR-0002 re-affirmed):
//! the impure half of the engine. Everything here is behind the `pdfium`
//! cargo feature so the crate builds and tests on machines without
//! `libpdfium` — CI runs the pure tile logic always and this module when
//! the library is present.
//!
//! Strategy (M1): render the whole page at the tile's zoom-bucket scale,
//! then slice the requested tile out of the RGBA buffer. Correct first;
//! pdfium clip-rect rendering is the optimization path once profiles say
//! page-sized renders dominate (they won't until ~4× zoom on A0 pages).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use pdfium_render::prelude::*;

use crate::tile::{TILE_PX, TileKey, zoom_bucket_scale};

// pdfium is single-threaded. pdfium-render 0.9's `thread_safe` feature only
// makes handles Send+Sync — it does NOT serialize FFI calls (v2 serialized
// by funneling everything through one worker thread). The engine serializes
// here so any caller topology is safe; concurrent calls otherwise SIGSEGV.
static PDFIUM_CALLS: Mutex<()> = Mutex::new(());

fn pdfium_lock() -> MutexGuard<'static, ()> {
    PDFIUM_CALLS.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("could not bind pdfium library: {0}")]
    Bind(String),
    #[error("could not open {path}: {source}")]
    Open { path: PathBuf, source: PdfiumError },
    #[error("page {page} out of bounds (document has {count})")]
    PageOutOfBounds { page: u32, count: u16 },
    #[error("render failed: {0}")]
    Render(PdfiumError),
    #[error("tile {0:?} lies outside the rendered page")]
    TileOutOfPage(TileKey),
}

/// Owns the pdfium binding (a process-wide singleton underneath — the
/// first binding wins). All methods serialize through [`PDFIUM_CALLS`],
/// so the renderer is safe to share or duplicate across threads.
pub struct PdfRenderer {
    pdfium: Pdfium,
}

/// One rendered tile: tightly-packed RGBA8.
#[derive(Clone)]
pub struct RenderedTile {
    pub key: TileKey,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for RenderedTile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderedTile")
            .field("key", &self.key)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.rgba.len())
            .finish()
    }
}

impl RenderedTile {
    pub fn byte_size(&self) -> usize {
        self.rgba.len()
    }
}

/// A whole page rendered at some scale: tightly-packed RGBA8.
#[derive(Clone)]
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl PdfRenderer {
    /// Bind to `libpdfium` next to the executable, in `lib_dir` if given,
    /// or on the system library path.
    pub fn new(lib_dir: Option<&Path>) -> Result<PdfRenderer, PdfError> {
        let _calls = pdfium_lock();
        let bindings = match lib_dir {
            Some(dir) => Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(dir)),
            None => Pdfium::bind_to_system_library(),
        }
        .map_err(|e| PdfError::Bind(format!("{e:?}")))?;
        Ok(PdfRenderer {
            pdfium: Pdfium::new(bindings),
        })
    }

    pub fn page_count(&self, path: &Path) -> Result<u16, PdfError> {
        let _calls = pdfium_lock();
        let doc = self.open(path)?;
        Ok(doc.pages().len().max(0) as u16)
    }

    /// Page size in PDF points at scale 1.0.
    pub fn page_size(&self, path: &Path, page: u32) -> Result<(f32, f32), PdfError> {
        let _calls = pdfium_lock();
        let doc = self.open(path)?;
        let page = get_page(&doc, page)?;
        Ok((page.width().value, page.height().value))
    }

    /// Render one tile: page at the key's bucket scale, sliced to the
    /// `TILE_PX` grid cell `(col, row)`. Edge tiles are smaller.
    pub fn render_tile(&self, path: &Path, key: TileKey) -> Result<RenderedTile, PdfError> {
        let _calls = pdfium_lock();
        let doc = self.open(path)?;
        let page = get_page(&doc, key.page)?;
        let scale = zoom_bucket_scale(key.bucket);
        let page_w = (page.width().value * scale).round().max(1.0) as u32;
        let page_h = (page.height().value * scale).round().max(1.0) as u32;

        let x0 = key.col as u32 * TILE_PX;
        let y0 = key.row as u32 * TILE_PX;
        if x0 >= page_w || y0 >= page_h {
            return Err(PdfError::TileOutOfPage(key));
        }
        let tile_w = TILE_PX.min(page_w - x0);
        let tile_h = TILE_PX.min(page_h - y0);

        let config = PdfRenderConfig::new()
            .set_target_width(page_w as i32)
            .set_target_height(page_h as i32);
        let bitmap = page.render_with_config(&config).map_err(PdfError::Render)?;
        let full = bitmap.as_rgba_bytes();
        let full_w = bitmap.width() as u32;
        let full_h = bitmap.height() as u32;

        // Slice the tile rows out of the full-page buffer.
        let mut rgba = Vec::with_capacity((tile_w * tile_h * 4) as usize);
        for row in y0..(y0 + tile_h).min(full_h) {
            let line_start = ((row * full_w + x0.min(full_w.saturating_sub(1))) * 4) as usize;
            let line_end = line_start + (tile_w.min(full_w - x0) * 4) as usize;
            if line_end <= full.len() {
                rgba.extend_from_slice(&full[line_start..line_end]);
            }
        }
        Ok(RenderedTile {
            key,
            width: tile_w,
            height: tile_h,
            rgba,
        })
    }

    /// Render a whole page at `scale` (1.0 = 72 dpi points). The shell's
    /// simple path for single-page views; tiled rendering remains the
    /// scrolling/zooming path.
    pub fn render_page(
        &self,
        path: &Path,
        page: u32,
        scale: f32,
    ) -> Result<RenderedPage, PdfError> {
        let _calls = pdfium_lock();
        let doc = self.open(path)?;
        let page = get_page(&doc, page)?;
        let page_w = (page.width().value * scale).round().max(1.0) as u32;
        let page_h = (page.height().value * scale).round().max(1.0) as u32;
        let config = PdfRenderConfig::new()
            .set_target_width(page_w as i32)
            .set_target_height(page_h as i32);
        let bitmap = page.render_with_config(&config).map_err(PdfError::Render)?;
        Ok(RenderedPage {
            width: bitmap.width() as u32,
            height: bitmap.height() as u32,
            rgba: bitmap.as_rgba_bytes(),
        })
    }

    /// Render a compact internal-link preview centered on destination content.
    ///
    /// PDF destinations usually expose page + y, not an object rectangle.
    /// Nearby glyph bounds provide the tightest reliable content box.
    pub fn render_link_preview(
        &self,
        path: &Path,
        page: u32,
        dest_y: Option<f32>,
    ) -> Result<RenderedPage, PdfError> {
        const SCALE: f32 = 2.0;
        let rendered = self.render_page(path, page, SCALE)?;
        let (page_w, page_h) = self.page_size(path, page)?;
        let chars = self.page_chars(path, page)?;
        let target_y = dest_y.unwrap_or(page_h / 2.0);
        let nearby = chars.iter().filter(|c| {
            let center_y = (c.y0 + c.y1) / 2.0;
            (center_y - target_y).abs() <= 110.0
        });
        let mut content: Option<(f32, f32, f32, f32)> = None;
        for c in nearby {
            content = Some(match content {
                Some((x0, y0, x1, y1)) => (x0.min(c.x0), y0.min(c.y0), x1.max(c.x1), y1.max(c.y1)),
                None => (c.x0, c.y0, c.x1, c.y1),
            });
        }
        let (crop_x, crop_y, crop_w, crop_h) = preview_crop(
            rendered.width,
            rendered.height,
            page_w,
            target_y,
            SCALE,
            content,
        );
        Ok(crop_rgba(rendered, crop_x, crop_y, crop_w, crop_h))
    }

    /// Per-character glyph geometry in reading order, page points with a
    /// **top-left** origin (pdfium's bottom-left rects flipped) — the input
    /// [`crate::select`] works on. Control characters carry no usable box
    /// and are dropped; line structure is recovered geometrically.
    pub fn page_chars(&self, path: &Path, page: u32) -> Result<Vec<crate::CharBox>, PdfError> {
        let _calls = pdfium_lock();
        let doc = self.open(path)?;
        let page = get_page(&doc, page)?;
        let page_h = page.height().value;
        let text = page.text().map_err(PdfError::Render)?;
        let mut out = Vec::new();
        for c in text.chars().iter() {
            let Some(ch) = c.unicode_char() else {
                continue;
            };
            if ch.is_control() {
                continue;
            }
            let Ok(bounds) = c.loose_bounds() else {
                continue;
            };
            out.push(crate::CharBox {
                ch,
                x0: bounds.left().value,
                y0: page_h - bounds.top().value,
                x1: bounds.right().value,
                y1: page_h - bounds.bottom().value,
            });
        }
        Ok(out)
    }

    pub fn page_links(&self, path: &Path, page: u32) -> Result<Vec<crate::LinkBox>, PdfError> {
        let _calls = pdfium_lock();
        let doc = self.open(path)?;
        let page_obj = get_page(&doc, page)?;
        let page_h = page_obj.height().value;
        let mut out = Vec::new();
        for link in page_obj.links().iter() {
            let Ok(rect) = link.rect() else {
                continue;
            };
            let sel_rect = crate::SelRect {
                x0: rect.left().value,
                y0: page_h - rect.top().value,
                x1: rect.right().value,
                y1: page_h - rect.bottom().value,
            };

            let mut dest_page = None;
            let mut dest_y = None;
            let mut uri = None;

            let extract_dest = |dest: &PdfDestination| {
                let p = dest.page_index().ok().map(|i| i as u32);
                let y = p.and_then(|dp| {
                    let dest_page_obj = doc.pages().get(dp as i32).ok()?;
                    let dest_page_h = dest_page_obj.height().value;
                    match dest.view_settings() {
                        Ok(PdfDestinationViewSettings::SpecificCoordinatesAndZoom(
                            _,
                            Some(y_pts),
                            _,
                        )) => Some(dest_page_h - y_pts.value),
                        Ok(PdfDestinationViewSettings::FitPageHorizontallyToWindow(Some(
                            y_pts,
                        ))) => Some(dest_page_h - y_pts.value),
                        Ok(PdfDestinationViewSettings::FitBoundsHorizontallyToWindow(Some(
                            y_pts,
                        ))) => Some(dest_page_h - y_pts.value),
                        _ => None,
                    }
                });
                (p, y)
            };

            if let Some(action) = link.action() {
                match action {
                    PdfAction::Uri(ref uri_action) => {
                        uri = uri_action.uri().ok();
                    }
                    PdfAction::LocalDestination(ref local) => {
                        if let Ok(dest) = local.destination() {
                            let (p, y) = extract_dest(&dest);
                            dest_page = p;
                            dest_y = y;
                        }
                    }
                    _ => {}
                }
            }

            if dest_page.is_none()
                && uri.is_none()
                && let Some(dest) = link.destination()
            {
                let (p, y) = extract_dest(&dest);
                dest_page = p;
                dest_y = y;
            }

            if dest_page.is_none() && uri.is_none() {
                continue;
            }

            let dest = dest_page.map(|p| (p, dest_y));

            out.push(crate::LinkBox {
                rect: sel_rect,
                dest,
                uri,
            });
        }
        Ok(out)
    }

    /// The document outline (bookmark tree) flattened to prefix order with
    /// depths — [`crate::outline`]'s input. Entries without a title or a
    /// page destination are skipped (their subtrees are still walked).
    /// Empty when the document has no outline.
    pub fn outline(&self, path: &Path) -> Result<Vec<crate::OutlineEntry>, PdfError> {
        // Defensive caps: pdfium documents can carry cyclic or absurd
        // bookmark graphs; a TOC deeper or larger than this is garbage.
        const MAX_DEPTH: u8 = 32;
        const MAX_ENTRIES: usize = 4096;
        let _calls = pdfium_lock();
        let doc = self.open(path)?;
        let mut out = Vec::new();
        let mut stack: Vec<(PdfBookmark<'_>, u8)> = doc
            .bookmarks()
            .root()
            .map(|root| vec![(root, 0)])
            .unwrap_or_default();
        while let Some((node, depth)) = stack.pop() {
            if out.len() >= MAX_ENTRIES {
                break;
            }
            // Sibling first so it pops after this node's subtree (prefix
            // order from a LIFO stack).
            if let Some(sibling) = node.next_sibling() {
                stack.push((sibling, depth));
            }
            if depth < MAX_DEPTH
                && let Some(child) = node.first_child()
            {
                stack.push((child, depth + 1));
            }
            let page = node
                .destination()
                .and_then(|d| d.page_index().ok())
                .and_then(|i| u32::try_from(i).ok());
            if let (Some(title), Some(page)) = (node.title(), page)
                && !title.trim().is_empty()
            {
                out.push(crate::OutlineEntry { title, page, depth });
            }
        }
        Ok(out)
    }

    /// Plain-text extraction per page (feeds the vault's FTS index).
    pub fn extract_text(&self, path: &Path, page: u32) -> Result<String, PdfError> {
        let _calls = pdfium_lock();
        let doc = self.open(path)?;
        let page = get_page(&doc, page)?;
        let text = page.text().map_err(PdfError::Render)?;
        Ok(text.all())
    }

    fn open(&self, path: &Path) -> Result<PdfDocument<'_>, PdfError> {
        self.pdfium
            .load_pdf_from_file(path, None)
            .map_err(|source| PdfError::Open {
                path: path.to_path_buf(),
                source,
            })
    }
}

fn preview_crop(
    full_width: u32,
    full_height: u32,
    page_width: f32,
    target_y: f32,
    scale: f32,
    content: Option<(f32, f32, f32, f32)>,
) -> (u32, u32, u32, u32) {
    let (left, right) = content.map_or((page_width * 0.09, page_width * 0.91), |bounds| {
        (
            (bounds.0 - 28.0).max(0.0),
            (bounds.2 + 28.0).min(page_width),
        )
    });
    let crop_x = (left * scale).round().clamp(0.0, full_width as f32) as u32;
    let desired_w = ((right - left) * scale).round().max(1.0) as u32;
    let crop_w = desired_w.min(full_width.saturating_sub(crop_x).max(1));

    let content_height = content.map_or(180.0, |bounds| (bounds.3 - bounds.1) + 80.0);
    let desired_h = (content_height.clamp(180.0, 360.0) * scale).round() as u32;
    let crop_h = desired_h.min(full_height.max(1));
    let target_px = target_y * scale;
    let max_y = full_height.saturating_sub(crop_h);
    let crop_y = (target_px - crop_h as f32 * 0.32)
        .round()
        .clamp(0.0, max_y as f32) as u32;
    (crop_x, crop_y, crop_w, crop_h)
}

fn crop_rgba(rendered: RenderedPage, x: u32, y: u32, width: u32, height: u32) -> RenderedPage {
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    let stride = rendered.width as usize * 4;
    let row_bytes = width as usize * 4;
    for row in y..y + height {
        let start = row as usize * stride + x as usize * 4;
        let end = start + row_bytes;
        if let Some(slice) = rendered.rgba.get(start..end) {
            rgba.extend_from_slice(slice);
        }
    }
    RenderedPage {
        width,
        height,
        rgba,
    }
}

#[cfg(test)]
mod preview_tests {
    use super::{RenderedPage, crop_rgba, preview_crop};

    #[test]
    fn preview_crop_centers_destination_and_uses_content_width() {
        let crop = preview_crop(
            1200,
            1600,
            600.0,
            400.0,
            2.0,
            Some((100.0, 380.0, 400.0, 420.0)),
        );
        assert_eq!(crop.0, 144);
        assert!(crop.1 < 800 && crop.1 + crop.3 > 800);
        assert_eq!(crop.2, 712);
        assert!(crop.3 < 1600);
    }

    #[test]
    fn rgba_crop_keeps_requested_pixels() {
        let rendered = RenderedPage {
            width: 2,
            height: 2,
            rgba: vec![1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255],
        };
        let cropped = crop_rgba(rendered, 1, 0, 1, 2);
        assert_eq!(cropped.width, 1);
        assert_eq!(cropped.height, 2);
        assert_eq!(cropped.rgba, vec![2, 0, 0, 255, 4, 0, 0, 255]);
    }
}

// The returned page carries the binding lifetime `'a`, not the document
// borrow — unifying them (`&'a PdfDocument<'a>`) would require a local
// document to outlive itself.
fn get_page<'a>(doc: &PdfDocument<'a>, page: u32) -> Result<PdfPage<'a>, PdfError> {
    let count = doc.pages().len().max(0) as u16;
    if page >= count as u32 {
        return Err(PdfError::PageOutOfBounds { page, count });
    }
    doc.pages()
        .get(page as i32)
        .map_err(|_| PdfError::PageOutOfBounds { page, count })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tile::zoom_bucket;

    /// Shared PDF fixture corpus (see tests-fixtures/pdf/README.md).
    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests-fixtures/pdf")
            .join(name)
    }

    fn renderer() -> Option<&'static PdfRenderer> {
        // One shared binding (re-binding per test is wasted work). Dev machines
        // can point at a local libpdfium via PDFIUM_LIB_DIR; otherwise bind the
        // system library, or skip (not fail) when neither exists so the suite
        // stays green on machines without pdfium.
        static RENDERER: std::sync::OnceLock<Option<PdfRenderer>> = std::sync::OnceLock::new();
        RENDERER
            .get_or_init(|| {
                let lib_dir = std::env::var_os("PDFIUM_LIB_DIR").map(PathBuf::from);
                PdfRenderer::new(lib_dir.as_deref())
                    .or_else(|_| PdfRenderer::new(None))
                    .ok()
            })
            .as_ref()
    }

    fn ok<T>(r: Result<T, PdfError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("{e}"),
        }
    }

    #[test]
    fn renders_a_real_tile_from_the_fixture_corpus() {
        let Some(renderer) = renderer() else {
            eprintln!("skipping: libpdfium not available");
            return;
        };
        let path = fixture("single-page.pdf");
        assert_eq!(ok(renderer.page_count(&path)), 1);
        let (w, h) = ok(renderer.page_size(&path, 0));
        assert!(w > 0.0 && h > 0.0);

        let key = TileKey {
            page: 0,
            bucket: zoom_bucket(1.0),
            col: 0,
            row: 0,
        };
        let tile = ok(renderer.render_tile(&path, key));
        assert!(tile.width > 0 && tile.height > 0);
        assert_eq!(
            tile.rgba.len(),
            (tile.width * tile.height * 4) as usize,
            "tightly packed RGBA"
        );
        assert!(
            tile.rgba.iter().any(|&b| b != 0),
            "tile is not all-zero pixels"
        );
    }

    #[test]
    fn renders_a_whole_page() {
        let Some(renderer) = renderer() else {
            eprintln!("skipping: libpdfium not available");
            return;
        };
        let path = fixture("single-page.pdf");
        let page = ok(renderer.render_page(&path, 0, 1.5));
        assert_eq!(
            page.rgba.len(),
            (page.width * page.height * 4) as usize,
            "tightly packed RGBA"
        );
        let (w, _) = ok(renderer.page_size(&path, 0));
        assert_eq!(page.width, (w * 1.5).round() as u32, "scale honored");
    }

    #[test]
    fn corrupt_pdf_is_a_typed_error_not_a_panic() {
        let Some(renderer) = renderer() else {
            eprintln!("skipping: libpdfium not available");
            return;
        };
        let result = renderer.page_count(&fixture("corrupt.pdf"));
        assert!(matches!(result, Err(PdfError::Open { .. })));
    }

    #[test]
    fn out_of_bounds_requests_are_typed_errors() {
        let Some(renderer) = renderer() else {
            eprintln!("skipping: libpdfium not available");
            return;
        };
        let path = fixture("single-page.pdf");
        assert!(matches!(
            renderer.page_size(&path, 99),
            Err(PdfError::PageOutOfBounds { .. })
        ));
        let far_tile = TileKey {
            page: 0,
            bucket: 0,
            col: 200,
            row: 200,
        };
        assert!(matches!(
            renderer.render_tile(&path, far_tile),
            Err(PdfError::TileOutOfPage(_))
        ));
    }

    #[test]
    fn page_chars_yield_selectable_geometry() {
        let Some(renderer) = renderer() else {
            eprintln!("skipping: libpdfium not available");
            return;
        };
        let path = fixture("single-page.pdf");
        let chars = ok(renderer.page_chars(&path, 0));
        assert!(!chars.is_empty(), "fixture page has text");
        let (page_w, page_h) = ok(renderer.page_size(&path, 0));
        for c in &chars {
            assert!(c.x0 < c.x1 && c.y0 < c.y1, "degenerate box: {c:?}");
            assert!(
                c.x0 >= -1.0 && c.x1 <= page_w + 1.0 && c.y0 >= -1.0 && c.y1 <= page_h + 1.0,
                "box outside the page: {c:?}"
            );
            assert!(!c.ch.is_control(), "control char leaked: {c:?}");
        }
        // A whole-page drag through the pure selector recovers real text.
        let selection = crate::select::select(&chars, (0.0, 0.0), (page_w, page_h));
        let Some(selection) = selection else {
            panic!("whole-page drag selected nothing");
        };
        assert!(!selection.quads.is_empty());
        let extracted = ok(renderer.extract_text(&path, 0));
        let word = selection.text.split_whitespace().next();
        let Some(word) = word else {
            panic!("selection has no words: {:?}", selection.text);
        };
        assert!(
            extracted.contains(word),
            "selected `{word}` not in extracted text"
        );
    }

    #[test]
    fn outline_flattens_the_fixture_bookmark_tree() {
        let Some(renderer) = renderer() else {
            eprintln!("skipping: libpdfium not available");
            return;
        };
        let toc = ok(renderer.outline(&fixture("multipage-outline.pdf")));
        assert!(!toc.is_empty(), "fixture has a bookmark tree");
        let pages = ok(renderer.page_count(&fixture("multipage-outline.pdf")));
        for e in &toc {
            assert!(!e.title.trim().is_empty());
            assert!(e.page < u32::from(pages), "destination in range: {e:?}");
        }
        assert!(
            toc.windows(2).any(|w| w[0].page <= w[1].page),
            "document order"
        );
        // Section tracking composes with the pure half.
        let last = toc.last().map(|e| e.page).unwrap_or(0);
        assert!(crate::outline::section_at(&toc, last).is_some());
        // No outline is an empty list, not an error.
        let none = ok(renderer.outline(&fixture("single-page.pdf")));
        assert!(none.is_empty());
    }

    #[test]
    fn cjk_text_extraction_works() {
        let Some(renderer) = renderer() else {
            eprintln!("skipping: libpdfium not available");
            return;
        };
        let text = ok(renderer.extract_text(&fixture("cjk-text.pdf"), 0));
        assert!(!text.trim().is_empty(), "extracted some text");
    }

    #[test]
    fn internal_links_fixture_yields_a_page_two_destination() {
        let Some(renderer) = renderer() else {
            eprintln!("skipping: libpdfium not available");
            return;
        };
        let path = fixture("internal-links.pdf");
        let links = ok(renderer.page_links(&path, 0));
        assert!(!links.is_empty(), "internal-links.pdf should have links");

        let link = links.iter().find(|l| {
            if let Some((dest_page, _)) = l.dest {
                dest_page == 1
            } else {
                false
            }
        });
        assert!(link.is_some(), "should find a link dest on page 1");

        let (page_w, page_h) = ok(renderer.page_size(&path, 0));
        if let Some(l) = link {
            assert!(l.rect.x0 >= -1.0 && l.rect.x1 <= page_w + 1.0);
            assert!(l.rect.y0 >= -1.0 && l.rect.y1 <= page_h + 1.0);
        }

        let empty_links = ok(renderer.page_links(&fixture("single-page.pdf"), 0));
        assert!(empty_links.is_empty(), "single-page.pdf has no links");
    }
}

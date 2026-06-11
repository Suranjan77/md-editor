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

    /// Fixture corpus from the v2 quarry (plan M0 "port fixtures").
    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests-fixtures/pdf")
            .join(name)
    }

    fn renderer() -> Option<&'static PdfRenderer> {
        // One shared binding (re-binding per test is wasted work).
        // Prefer the repo-local library; fall back to system. Skip (not
        // fail) when neither exists so the suite stays green on machines
        // without pdfium.
        static RENDERER: std::sync::OnceLock<Option<PdfRenderer>> = std::sync::OnceLock::new();
        RENDERER
            .get_or_init(|| {
                let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/pdfium");
                PdfRenderer::new(Some(&local))
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
    fn cjk_text_extraction_works() {
        let Some(renderer) = renderer() else {
            eprintln!("skipping: libpdfium not available");
            return;
        };
        let text = ok(renderer.extract_text(&fixture("cjk-text.pdf"), 0));
        assert!(!text.trim().is_empty(), "extracted some text");
    }
}

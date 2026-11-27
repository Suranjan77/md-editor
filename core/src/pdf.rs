//! PDF engine: shared data types, document identity, and the PDFium binding.

mod error;
pub use error::{PdfError, PdfResult};
use image::DynamicImage;
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

mod attachments;
mod geometry;
mod renderer;
mod toc;
mod toc_synth;
mod worker;

pub use attachments::PdfAttachmentInfo;
pub use renderer::*;
pub use toc::*;
pub use toc_synth::synthesize_toc;
pub(crate) use toc_synth::{is_caps_heading, is_plausible_title, numbered_heading_depth};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PdfTextChar {
    pub char_index: u32,
    pub text_index: usize,
    pub ch: char,
    pub bbox: PdfRect,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PdfTextLine {
    pub start_text_index: usize,
    pub end_text_index: usize,
    pub bbox: PdfRect,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PdfPageText {
    pub page_index: u16,
    pub page_width: f32,
    pub page_height: f32,
    pub text: String,
    pub chars: Vec<PdfTextChar>,
    pub lines: Vec<PdfTextLine>,
    /// Byte offset of each char of `text`, built on first use; see
    /// [`PdfPageText::text_range`].
    #[serde(skip)]
    pub offsets: CharOffsets,
}

/// Lazily built char → byte offsets of a page's text.
#[derive(Clone, Debug, Default)]
pub struct CharOffsets(std::sync::OnceLock<Vec<usize>>);

impl PdfPageText {
    /// The text of chars `[start, end)` (clamped), in O(1) once the page's
    /// offsets are built, instead of walking the text from its start.
    pub fn text_range(&self, start: usize, end: usize) -> &str {
        let offsets = self.offsets.0.get_or_init(|| {
            let mut offsets: Vec<usize> = self.text.char_indices().map(|(i, _)| i).collect();
            offsets.push(self.text.len());
            offsets
        });
        let last = offsets.len() - 1;
        let (start, end) = (start.min(last), end.min(last));
        if start >= end {
            return "";
        }
        &self.text[offsets[start]..offsets[end]]
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum PdfAnnotationKind {
    Highlight,
    Note,
    Ink,
}

impl PdfAnnotationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Highlight => "Highlight",
            Self::Note => "Note",
            Self::Ink => "Ink",
        }
    }

    pub fn parse(s: &str) -> PdfResult<Self> {
        match s {
            "Highlight" => Ok(Self::Highlight),
            "Note" => Ok(Self::Note),
            "Ink" => Ok(Self::Ink),
            _ => Err(PdfError::Other(format!("Unknown annotation kind: {s}"))),
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
pub enum PdfAnnotationColor {
    Yellow,
    Green,
    Blue,
    Pink,
    Orange,
}

impl PdfAnnotationColor {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Pink => "Pink",
            Self::Orange => "Orange",
        }
    }

    pub fn parse(s: &str) -> PdfResult<Self> {
        match s {
            "Yellow" => Ok(Self::Yellow),
            "Green" => Ok(Self::Green),
            "Blue" => Ok(Self::Blue),
            "Pink" => Ok(Self::Pink),
            "Orange" => Ok(Self::Orange),
            _ => Err(PdfError::Other(format!("Unknown annotation color: {s}"))),
        }
    }

    /// Next color in the highlight palette, wrapping around. Used to cycle
    /// colors across successive quick highlights.
    pub fn next(self) -> Self {
        match self {
            Self::Yellow => Self::Green,
            Self::Green => Self::Blue,
            Self::Blue => Self::Pink,
            Self::Pink => Self::Orange,
            Self::Orange => Self::Yellow,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PdfTextRange {
    pub start_text_index: usize,
    pub end_text_index: usize,
}

/// Non-destructive stylus/pointer stroke on a PDF page, stored in normalized
/// page fraction space (`[x_frac, y_frac, pressure]`, `0.0..=1.0`, top-left origin)
/// so strokes zoom and resize cleanly without altering the PDF file.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct PdfInkStroke {
    pub size: f32,
    pub points: Vec<[f32; 3]>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PdfAnnotation {
    pub id: String,
    pub document_id: String,
    pub page_index: u16,
    pub kind: PdfAnnotationKind,
    pub color: PdfAnnotationColor,
    pub selected_text: String,
    pub ranges: Vec<PdfTextRange>,
    pub rects: Vec<PdfRect>,
    pub note: Option<String>,
    pub linked_note_path: Option<String>,
    pub markdown_anchor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ink_stroke: Option<PdfInkStroke>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl PdfAnnotation {
    /// Construct a non-destructive margin ink annotation with a PDF-point bounding box.
    pub fn new_ink(
        id: String,
        document_id: String,
        page_index: u16,
        color: PdfAnnotationColor,
        stroke: PdfInkStroke,
        page_size: (f32, f32),
        now: i64,
    ) -> Self {
        let mut min_x = 1.0f32;
        let mut min_y = 1.0f32;
        let mut max_x = 0.0f32;
        let mut max_y = 0.0f32;
        for pt in &stroke.points {
            min_x = min_x.min(pt[0]);
            min_y = min_y.min(pt[1]);
            max_x = max_x.max(pt[0]);
            max_y = max_y.max(pt[1]);
        }
        let (pw, ph) = (page_size.0.max(1.0), page_size.1.max(1.0));
        let rects = if stroke.points.is_empty() {
            Vec::new()
        } else {
            let x = min_x * pw;
            let width = ((max_x - min_x) * pw).max(1.0);
            let height = ((max_y - min_y) * ph).max(1.0);
            let y = (1.0 - max_y) * ph;
            vec![PdfRect {
                x,
                y,
                width,
                height,
            }]
        };
        Self {
            id,
            document_id,
            page_index,
            kind: PdfAnnotationKind::Ink,
            color,
            selected_text: String::new(),
            ranges: Vec::new(),
            rects,
            note: None,
            linked_note_path: None,
            markdown_anchor: None,
            ink_stroke: Some(stroke),
            created_at: now,
            updated_at: now,
        }
    }
}

pub fn compute_provisional_id(
    path: &std::path::Path,
) -> Result<(String, u64, Option<i64>), String> {
    use sha2::{Digest, Sha256};
    use std::fs::File;
    use std::io::Read;

    let metadata =
        std::fs::metadata(path).map_err(|e| format!("Failed to read file metadata: {e}"))?;
    let file_len = metadata.len();
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);

    let mut file = File::open(path).map_err(|e| format!("Failed to open file: {e}"))?;

    // Read up to 1 MiB
    let chunk_size = 1024 * 1024;
    let mut buffer = vec![0u8; chunk_size];
    let bytes_read = file
        .read(&mut buffer)
        .map_err(|e| format!("Failed to read file: {e}"))?;
    buffer.truncate(bytes_read);

    // Deliberately content-only (first 1 MiB + length): the id keys all
    // annotations, and including mtime would orphan them whenever the file is
    // copied, restored from backup, or re-downloaded. mtime is still returned
    // for bookkeeping in `pdf_documents`.
    let mut hasher = Sha256::new();
    hasher.update(&buffer);
    hasher.update(file_len.to_be_bytes());

    let hash_result = hasher.finalize();
    let id = format!("{:x}", hash_result);

    Ok((id, file_len, modified))
}

pub(super) fn pdf_load_error(error: PdfiumError) -> PdfError {
    if matches!(
        error,
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError)
    ) {
        PdfError::PasswordRequired
    } else {
        PdfError::Other(format!("Failed to load PDF: {error:?}"))
    }
}

pub fn merge_char_rects(chars: &[PdfTextChar]) -> Vec<PdfRect> {
    let chars = chars
        .iter()
        .filter(|c| c.bbox.width > 0.0 && c.bbox.height > 0.0)
        .collect::<Vec<_>>();
    if chars.is_empty() {
        return Vec::new();
    }
    let mut rects = Vec::new();
    let mut current: Option<PdfRect> = None;
    for c in chars {
        match &mut current {
            Some(r) => {
                let r_y_min = r.y;
                let r_y_max = r.y + r.height;
                let c_y_min = c.bbox.y;
                let c_y_max = c.bbox.y + c.bbox.height;

                let overlap = r_y_max.min(c_y_max) - r_y_min.max(c_y_min);
                let min_h = r.height.min(c.bbox.height);
                let horizontal_gap = c.bbox.x - (r.x + r.width);

                if overlap > 0.0
                    && overlap > 0.3 * min_h
                    && c.bbox.x >= r.x
                    && horizontal_gap < 3.0 * min_h.max(4.0)
                {
                    let x_min = r.x;
                    let x_max = (r.x + r.width).max(c.bbox.x + c.bbox.width);
                    let y_min = r.y.min(c.bbox.y);
                    let y_max = (r.y + r.height).max(c.bbox.y + c.bbox.height);
                    r.x = x_min;
                    r.y = y_min;
                    r.width = x_max - x_min;
                    r.height = y_max - y_min;
                } else {
                    let prev = std::mem::replace(r, c.bbox.clone());
                    rects.push(prev);
                }
            }
            None => {
                current = Some(c.bbox.clone());
            }
        }
    }
    if let Some(r) = current {
        rects.push(r);
    }
    rects
}

/// Most matches one PDF search returns. A search that reaches it stops early,
/// so callers should present the count as "at least". It was 250 — and silent —
/// so a match past roughly the 250th hit in a long book could never be found.
pub const PDF_SEARCH_MATCH_LIMIT: usize = 10_000;

/// Version of the outline-recovery heuristics (`pdf/toc.rs`, `pdf/toc_synth.rs`).
/// Bump it when they change, so outlines cached in `pdf_outline` are recovered
/// again rather than served stale.
pub const OUTLINE_VERSION: u32 = 1;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct TocEntry {
    pub title: String,
    pub page_index: Option<u32>,
    pub children: Vec<TocEntry>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PdfRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LinkInfo {
    pub bbox: PdfRect,
    pub dest_page: Option<u32>,
    pub dest_y: Option<f32>,
    pub uri: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LinkPreviewResult {
    pub image_data: Vec<u8>,
    pub center_ratio: f32,
}

/// A region of a page, in points of the displayed page box measured from its
/// top-left corner (the box the page is drawn in: rotation and crop box
/// applied). This is what a `pdf://…&crop=x,y,w,h` link names.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageRegion {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A rendered region: straight RGBA, `width × height × 4` bytes.
#[derive(Clone, Debug)]
pub struct RegionImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PdfSearchMatch {
    pub page_index: u16,
    pub context: String,
    pub rects: Vec<PdfRect>,
}

/// Base for the zoom-to-render-scale quantization. A bitmap rendered at a
/// bucket's scale is never displayed at more than this factor of
/// magnification, so it stays sharp without re-rendering on every zoom step.
pub const PDF_RENDER_BUCKET_BASE: f32 = 1.4;

/// Quantize a continuous zoom level to a discrete render-scale bucket
/// (the smallest power of [`PDF_RENDER_BUCKET_BASE`] that is >= `zoom`).
///
/// Page bitmaps are rendered at the bucket scale rather than the exact zoom,
/// so zooming within a bucket reuses the cached bitmap (iced rescales it to
/// the layout box) instead of re-rasterizing every page. Crossing a bucket
/// boundary is the only event that forces a re-render.
pub fn pdf_render_bucket(zoom: f32) -> f32 {
    let zoom = zoom.clamp(0.05, 64.0);
    let mut scale = 1.0_f32;
    if zoom > 1.0 {
        while scale < zoom {
            scale *= PDF_RENDER_BUCKET_BASE;
        }
    } else {
        while scale / PDF_RENDER_BUCKET_BASE >= zoom {
            scale /= PDF_RENDER_BUCKET_BASE;
        }
    }
    scale
}

fn bind_pdfium() -> Result<Pdfium, String> {
    static BIND_RESULT: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();

    let res = BIND_RESULT.get_or_init(|| {
        let lib_name = Pdfium::pdfium_platform_library_name();
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf));
        let candidates = pdfium_candidates(exe_dir.as_deref(), Path::new(&lib_name));

        let mut bound = false;
        for candidate in candidates {
            if candidate.exists() {
                log::debug!("Binding PDFium from {}", candidate.display());
                match Pdfium::bind_to_library(candidate) {
                    Ok(bindings) => {
                        let _ = Pdfium::new(bindings);
                        bound = true;
                        break;
                    }
                    Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => {
                        bound = true;
                        break;
                    }
                    Err(_) => {}
                }
            }
        }

        if !bound {
            match Pdfium::bind_to_library(lib_name) {
                Ok(bindings) => {
                    let _ = Pdfium::new(bindings);
                }
                Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => {}
                Err(e) => {
                    return Err(format!("{e:?}"));
                }
            }
        }
        Ok(())
    });

    match res {
        Ok(()) => Ok(Pdfium::default()),
        Err(e) => Err(e.clone()),
    }
}

/// Where to look for the PDFium library, in order, before the system loader
/// path:
///
/// 1. `<exe>/../lib/md-editor/` — the installed layout
///    (`/usr/bin/md-editor` + `/usr/lib/md-editor/libpdfium.so`), which also
///    works under `/usr/local` or a packaging root;
/// 2. `<exe>/` and `<exe>/resources/` — development builds, where the build
///    script copies the library next to the binary;
/// 3. the crate's own `pdfium/` folder (tests run from the source tree).
fn pdfium_candidates(exe_dir: Option<&Path>, lib_name: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = exe_dir {
        if let Some(prefix) = dir.parent() {
            candidates.push(prefix.join("lib").join("md-editor").join(lib_name));
        }
        candidates.push(dir.join(lib_name));
        candidates.push(dir.join("resources").join(lib_name));
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("pdfium")
            .join(lib_name),
    );
    candidates
}

#[cfg(test)]
mod tests;

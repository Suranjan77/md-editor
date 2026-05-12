use std::collections::HashMap;
use std::ffi::c_int;
use std::fs;
use std::io::Cursor;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use image::ImageFormat;
use lru::LruCache;
use pdfium_render::prelude::*;
use serde::Serialize;
use tauri::{Manager, State};

use crate::commands::AppState;

// ── Shared Types ────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TocEntry {
    pub title: String,
    pub page_index: Option<u32>,
    pub children: Vec<TocEntry>,
}

#[derive(Serialize)]
pub struct PdfMetadata {
    pub total_pages: u32,
    pub page_width: f32,
    pub page_height: f32,
    pub title: Option<String>,
    pub author: Option<String>,
    pub toc: Vec<TocEntry>,
    pub render_generation: u64,
}

#[derive(Clone, Serialize)]
pub struct PdfRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Serialize)]
pub struct LinkInfo {
    pub bbox: PdfRect,
    pub dest_page: Option<u32>,
    pub dest_y: Option<f32>,
    pub uri: Option<String>,
}

#[derive(Serialize)]
pub struct SearchHit {
    pub page_index: u32,
    pub text: String,
}

#[derive(Serialize)]
pub struct LinkPreviewResult {
    pub image: String,
    pub center_ratio: f32,
}

#[derive(Serialize)]
pub struct PdfiumDiagnostics {
    pub available: bool,
    pub selected_path: Option<String>,
    pub attempted_paths: Vec<String>,
    pub error: Option<String>,
}

// ── Cache Key ───────────────────────────────────────────────────────

/// Key for the rendered page cache: (file_path, page_index, scale_percent)
type PageCacheKey = (String, u32, u32);

// ── PDF State held in AppState ──────────────────────────────────────

pub struct PdfState {
    pub current_path: Option<String>,
    pub current_bytes: Option<Arc<Vec<u8>>>,
    pub page_cache: LruCache<PageCacheKey, Vec<u8>>,
    pub link_cache: HashMap<u32, Vec<LinkInfo>>,
    pub render_generation: u64,
}

impl PdfState {
    pub fn new() -> Self {
        Self {
            current_path: None,
            current_bytes: None,
            page_cache: LruCache::new(NonZeroUsize::new(50).unwrap()),
            link_cache: HashMap::new(),
            render_generation: 0,
        }
    }
}

// ── Thread-Local Pdfium Engine ──────────────────────────────────────

thread_local! {
    static LOCAL_PDFIUM: std::cell::RefCell<Option<Pdfium>> = std::cell::RefCell::new(None);
}

// PDFium has process-global state and can return opaque internal errors when
// multiple documents/pages are opened or rendered concurrently.
static PDFIUM_MUTEX: Mutex<()> = Mutex::new(());

fn with_pdfium<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&Pdfium) -> Result<R, String>,
{
    LOCAL_PDFIUM.with(|cell| {
        let _guard = PDFIUM_MUTEX.lock().unwrap();
        let mut pdfium_opt = cell.borrow_mut();
        if pdfium_opt.is_none() {
            *pdfium_opt = Some(init_pdfium()?);
        }
        f(pdfium_opt.as_ref().unwrap())
    })
}

// ── Helper: Initialize Pdfium ───────────────────────────────────────

fn init_pdfium() -> Result<Pdfium, String> {
    let bind_paths = get_pdfium_bind_paths();

    let mut errors = Vec::new();
    for path in bind_paths {
        if !path.exists() {
            errors.push(format!("{}: not found", path.display()));
            continue;
        }

        match Pdfium::bind_to_library(&path) {
            Ok(bindings) => return Ok(Pdfium::new(bindings)),
            Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => {
                return Ok(Pdfium::default());
            }
            Err(err) => errors.push(format!("{}: {}", path.display(), err)),
        }
    }

    Err(format!(
        "Could not bind to Pdfium library. Tried: {}",
        errors.join("; ")
    ))
}

fn get_pdfium_bind_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    let lib_name = if cfg!(target_os = "windows") {
        "pdfium.dll"
    } else if cfg!(target_os = "macos") {
        "libpdfium.dylib"
    } else {
        "libpdfium.so"
    };

    // 1. Next to the executable (for bundled apps)
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            paths.push(exe_dir.join(lib_name));
            paths.push(exe_dir.join("pdfium").join(lib_name));

            // Tauri bundle resource layouts can place resources in a sibling
            // `resources/` directory (Windows/Linux) or inside the app bundle.
            paths.push(exe_dir.join("resources").join(lib_name));
            paths.push(exe_dir.join("resources").join("pdfium").join(lib_name));
            paths.push(
                exe_dir
                    .join("..")
                    .join("Resources")
                    .join(lib_name)
                    .canonicalize()
                    .unwrap_or_else(|_| exe_dir.join("..").join("Resources").join(lib_name)),
            );
            paths.push(
                exe_dir
                    .join("..")
                    .join("Resources")
                    .join("pdfium")
                    .join(lib_name)
                    .canonicalize()
                    .unwrap_or_else(|_| {
                        exe_dir
                            .join("..")
                            .join("Resources")
                            .join("pdfium")
                            .join(lib_name)
                    }),
            );
        }
    }

    if let Some(resource_dir) = std::env::var_os("PDFIUM_RESOURCE_DIR") {
        let resource_dir = PathBuf::from(resource_dir);
        paths.push(resource_dir.join(lib_name));
        paths.push(resource_dir.join("pdfium").join(lib_name));
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // 2. In the src-tauri/pdfium/ directory (development mode). Use Cargo's
    // compile-time manifest path; CARGO_MANIFEST_DIR is not guaranteed at runtime.
    paths.push(manifest_dir.join("pdfium").join(lib_name));

    // 3. In Cargo target output directories used by the build script.
    if let Some(target_profile_dir) = option_env!("PDFIUM_TARGET_PROFILE_DIR") {
        paths.push(PathBuf::from(target_profile_dir).join(lib_name));
    }
    if let Some(target_dir) = option_env!("PDFIUM_TARGET_DIR") {
        paths.push(PathBuf::from(target_dir).join("debug").join(lib_name));
        paths.push(PathBuf::from(target_dir).join("release").join(lib_name));
    }

    // 4. Current directory
    paths.push(PathBuf::from(lib_name));

    paths
}

fn resolve_pdfium_library_path() -> Result<PathBuf, String> {
    let mut errors = Vec::new();

    for path in get_pdfium_bind_paths() {
        if !path.exists() {
            errors.push(format!("{}: not found", path.display()));
            continue;
        }

        return Ok(path);
    }

    Err(errors.join("; "))
}

fn path_display(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

// ── Helper: Render a page to PNG bytes ──────────────────────────────

pub fn render_page_to_png(
    pdfium: &Pdfium,
    bytes: &[u8],
    page_index: u32,
    scale: f32,
) -> Result<Vec<u8>, String> {
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|e| format!("Failed to open PDF: {}", e))?;

    let page = document
        .pages()
        .get(page_index as c_int)
        .map_err(|e| format!("Failed to get page {}: {}", page_index, e))?;

    let width = (page.width().value * scale) as u32;
    let height = (page.height().value * scale) as u32;

    let bitmap = page
        .render_with_config(
            &PdfRenderConfig::new()
                .set_target_width(width as Pixels)
                .set_target_height(height as Pixels)
                .render_form_data(true)
                .render_annotations(true),
        )
        .map_err(|e| format!("Failed to render page: {}", e))?;

    let dynamic_image = bitmap.as_image();

    match dynamic_image {
        Ok(dynamic_image) => {
            let mut buf = Cursor::new(Vec::new());
            dynamic_image
                .write_to(&mut buf, ImageFormat::Png)
                .map_err(|e| format!("Failed to encode page as PNG: {}", e))?;

            Ok(buf.into_inner())
        }
        Err(e) => Err(format!("Failed to render page: {}", e)),
    }
}

// ── Helper: Extract bookmarks as TOC ────────────────────────────────
pub fn extract_bookmarks(document: &PdfDocument) -> Vec<TocEntry> {
    let bookmarks: Vec<PdfBookmark> = document.bookmarks().iter().collect();
    parse_bookmarks(&bookmarks)
}

fn parse_bookmarks(bookmarks: &Vec<PdfBookmark>) -> Vec<TocEntry> {
    let mut entries = Vec::new();

    for bookmark in bookmarks.iter() {
        let title = bookmark.title().unwrap_or_default();

        let page_index = bookmark
            .destination()
            .and_then(|dest| dest.page_index().ok())
            .map(|idx| idx as u32);

        let child_bookmarks = bookmark.iter_direct_children().collect();
        let children = parse_bookmarks(&child_bookmarks);

        entries.push(TocEntry {
            title,
            page_index,
            children,
        });
    }

    entries
}

// ── Tauri Commands ──────────────────────────────────────────────────

#[tauri::command]
pub fn open_pdf(path: String, state: State<'_, AppState>) -> Result<PdfMetadata, String> {
    let vault_root = {
        let vr = state.vault_root.lock().map_err(|e| e.to_string())?;
        vr.clone()
    };

    // Pre-resolve path to ensure it exists
    let abs_path = if path.starts_with("file://") {
        PathBuf::from(path.strip_prefix("file://").unwrap())
    } else if PathBuf::from(&path).is_absolute() {
        PathBuf::from(&path)
    } else if let Some(vr) = vault_root {
        vr.join(&path)
    } else {
        PathBuf::from(&path)
    };

    if !abs_path.exists() {
        return Err(format!("File does not exist: {:?}", abs_path));
    }

    let abs_path_str = abs_path.to_string_lossy().to_string();
    let pdf_bytes = Arc::new(
        fs::read(&abs_path).map_err(|e| format!("Failed to read PDF {}: {}", abs_path_str, e))?,
    );

    // Verify it's a valid PDF by opening it and extract metadata
    let (total_pages, page_width, page_height, title, author, toc) = with_pdfium(|pdfium| {
        let document = pdfium
            .load_pdf_from_byte_slice(pdf_bytes.as_slice(), None)
            .map_err(|e| format!("Failed to open PDF: {}", e))?;

        let total_pages = document.pages().len() as u32;

        // Get page dimensions from the first page (default to A4 if no pages)
        let (pw, ph) = if total_pages > 0 {
            let first_page = document
                .pages()
                .get(0)
                .map_err(|e| format!("Failed to get first page: {}", e))?;
            (first_page.width().value, first_page.height().value)
        } else {
            (612.0, 792.0) // A4 default in points
        };

        // Extract TOC from bookmarks
        let toc = extract_bookmarks(&document);

        Ok((total_pages, pw, ph, None, None, toc))
    })?;

    // Update PDF state
    let mut pdf_state = state.pdf_state.lock().map_err(|e| e.to_string())?;
    pdf_state.render_generation = pdf_state.render_generation.wrapping_add(1);
    let render_generation = pdf_state.render_generation;
    pdf_state.current_path = Some(abs_path_str);
    pdf_state.current_bytes = Some(pdf_bytes);
    pdf_state.page_cache.clear();
    pdf_state.link_cache.clear();

    Ok(PdfMetadata {
        total_pages,
        page_width,
        page_height,
        title,
        author,
        toc,
        render_generation,
    })
}

#[tauri::command]
pub fn close_pdf(state: State<'_, AppState>) -> Result<(), String> {
    let mut pdf_state = state.pdf_state.lock().map_err(|e| e.to_string())?;
    pdf_state.current_path = None;
    pdf_state.current_bytes = None;
    pdf_state.page_cache.clear();
    pdf_state.link_cache.clear();
    pdf_state.render_generation = pdf_state.render_generation.wrapping_add(1);
    Ok(())
}

#[tauri::command]
pub fn set_pdf_render_generation(
    generation: u64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut pdf_state = state.pdf_state.lock().map_err(|e| e.to_string())?;
    if generation > pdf_state.render_generation {
        pdf_state.render_generation = generation;
    }
    Ok(())
}

/// Gets a rendered page as PNG bytes, using the cache if possible.
/// This is meant to be called by the custom URI scheme handler.
pub fn get_pdf_page_bytes(
    app_handle: &tauri::AppHandle,
    page_index: u32,
    scale: f32,
    generation: Option<u64>,
) -> Result<Vec<u8>, String> {
    let state = app_handle.state::<AppState>();

    let (bytes, cache_key) = {
        let mut pdf_state = state.pdf_state.lock().map_err(|e| e.to_string())?;

        let path = pdf_state
            .current_path
            .as_ref()
            .ok_or("No PDF is currently open")?
            .clone();
        if let Some(generation) = generation {
            if generation != pdf_state.render_generation {
                return Err("Stale PDF render request".to_string());
            }
        }
        let bytes = pdf_state
            .current_bytes
            .as_ref()
            .ok_or("No PDF is currently open")?
            .clone();

        let scale_key = (scale * 100.0) as u32;
        let cache_key = (path.clone(), page_index, scale_key);

        if let Some(cached) = pdf_state.page_cache.get(&cache_key) {
            return Ok(cached.clone());
        }

        (bytes, cache_key)
    }; // Lock is dropped here so other threads can process requests!

    // Render the page
    let png_bytes =
        with_pdfium(|pdfium| render_page_to_png(pdfium, bytes.as_slice(), page_index, scale))?;

    // Re-acquire lock to store in cache
    if let Ok(mut pdf_state) = state.pdf_state.lock() {
        if let Some(generation) = generation {
            if generation != pdf_state.render_generation {
                return Ok(png_bytes);
            }
        }
        let cache_capacity = if scale >= 3.0 { 8 } else { 50 };
        pdf_state
            .page_cache
            .resize(NonZeroUsize::new(cache_capacity).unwrap());
        pdf_state.page_cache.put(cache_key, png_bytes.clone());
    }

    Ok(png_bytes)
}

#[tauri::command]
pub fn get_pdf_page_image(
    app_handle: tauri::AppHandle,
    page_index: u32,
    scale: f32,
    generation: Option<u64>,
) -> Result<String, String> {
    let bytes = get_pdf_page_bytes(&app_handle, page_index, scale, generation)?;
    Ok(format!("data:image/png;base64,{}", BASE64.encode(bytes)))
}

#[tauri::command]
pub fn get_page_links(
    page_index: u32,
    state: State<'_, AppState>,
) -> Result<Vec<LinkInfo>, String> {
    let bytes = {
        let pdf_state = state.pdf_state.lock().map_err(|e| e.to_string())?;
        if let Some(cached) = pdf_state.link_cache.get(&page_index) {
            return Ok(cached.clone());
        }
        pdf_state
            .current_bytes
            .as_ref()
            .ok_or("No PDF is currently open")?
            .clone()
    }; // Lock dropped here

    let links = with_pdfium(|pdfium| {
        let document = pdfium
            .load_pdf_from_byte_slice(bytes.as_slice(), None)
            .map_err(|e| format!("Failed to open PDF: {}", e))?;

        let page = document
            .pages()
            .get(page_index as c_int)
            .map_err(|e| format!("Failed to get page {}: {}", page_index, e))?;

        let page_height = page.height().value;

        let mut links = Vec::new();
        for link in page.links().iter() {
            // Get the link's bounding rectangle
            let rect = match link.rect() {
                Ok(r) => r,
                Err(_) => continue,
            };

            let bbox = PdfRect {
                x: rect.left().value,
                y: page_height - rect.top().value,
                width: rect.width().value,
                height: rect.height().value,
            };

            // Determine link destination
            let mut dest_page: Option<u32> = None;
            let mut dest_y: Option<f32> = None;
            let mut uri: Option<String> = None;

            // Helper: extract page index and y-coordinate from a PdfDestination
            let extract_dest = |dest: &PdfDestination, page_h: f32| -> (Option<u32>, Option<f32>) {
                let page = dest.page_index().ok().map(|i| i as u32);
                let y = match dest.view_settings() {
                    Ok(PdfDestinationViewSettings::SpecificCoordinatesAndZoom(
                        _,
                        Some(y_pts),
                        _,
                    )) => Some(page_h - y_pts.value),
                    Ok(PdfDestinationViewSettings::FitPageHorizontallyToWindow(Some(y_pts))) => {
                        Some(page_h - y_pts.value)
                    }
                    Ok(PdfDestinationViewSettings::FitBoundsHorizontallyToWindow(Some(y_pts))) => {
                        Some(page_h - y_pts.value)
                    }
                    _ => None,
                };
                (page, y)
            };

            // Check action first
            if let Some(action) = link.action() {
                match action {
                    PdfAction::Uri(ref uri_action) => {
                        uri = uri_action.uri().ok();
                    }
                    PdfAction::LocalDestination(ref local) => {
                        if let Ok(dest) = local.destination() {
                            let (p, y) = extract_dest(&dest, page_height);
                            dest_page = p;
                            dest_y = y;
                        }
                    }
                    _ => {}
                }
            }

            // Fallback: check destination directly
            if dest_page.is_none() && uri.is_none() {
                if let Some(dest) = link.destination() {
                    let (p, y) = extract_dest(&dest, page_height);
                    dest_page = p;
                    if dest_y.is_none() {
                        dest_y = y;
                    }
                }
            }

            links.push(LinkInfo {
                bbox,
                dest_page,
                dest_y,
                uri,
            });
        }

        Ok(links)
    })?;

    if let Ok(mut pdf_state) = state.pdf_state.lock() {
        pdf_state.link_cache.insert(page_index, links.clone());
    }

    Ok(links)
}

#[tauri::command]
pub fn get_link_preview(
    dest_page: u32,
    dest_y: Option<f32>,
    state: State<'_, AppState>,
) -> Result<LinkPreviewResult, String> {
    let bytes = {
        let pdf_state = state.pdf_state.lock().map_err(|e| e.to_string())?;
        pdf_state
            .current_bytes
            .as_ref()
            .ok_or("No PDF is currently open")?
            .clone()
    }; // Lock dropped here

    let result = with_pdfium(|pdfium| {
        let document = pdfium
            .load_pdf_from_byte_slice(bytes.as_slice(), None)
            .map_err(|e| format!("Failed to open PDF: {}", e))?;

        let page = document
            .pages()
            .get(dest_page as c_int)
            .map_err(|e| format!("Failed to get page {}: {}", dest_page, e))?;

        // Render the full page at 2x scale for a crisp preview
        let scale = 2.0_f32;
        let full_width = (page.width().value * scale) as u32;
        let full_height = (page.height().value * scale) as u32;

        let bitmap = page
            .render_with_config(
                &PdfRenderConfig::new()
                    .set_target_width(full_width as Pixels)
                    .set_target_height(full_height as Pixels)
                    .render_form_data(true)
                    .render_annotations(true),
            )
            .map_err(|e| format!("Failed to render page: {}", e))?;

        let dynamic_image = bitmap.as_image();

        match dynamic_image {
            Ok(dynamic_image) => {
                // Use the destination y-coordinate (in screen coords) if available,
                // otherwise default to the top of the page
                let target_y = dest_y.unwrap_or(0.0);

                // Crop full page width, centered vertically on the destination position
                let v_padding = 150.0 * scale;
                let crop_x = 0_u32;
                let center_y_scaled = target_y * scale;
                let crop_y = (center_y_scaled - v_padding).max(0.0) as u32;
                let crop_h = (v_padding * 2.0).min((full_height - crop_y) as f32) as u32;
                let crop_w = full_width;

                // Compute where the target center falls within the crop (0.0 to 1.0)
                let target_center_in_crop = center_y_scaled - crop_y as f32;
                let center_ratio = if crop_h > 0 {
                    (target_center_in_crop / crop_h as f32).clamp(0.0, 1.0)
                } else {
                    0.5
                };

                let cropped = dynamic_image.crop_imm(crop_x, crop_y, crop_w.max(1), crop_h.max(1));

                let mut buf = Cursor::new(Vec::new());
                cropped
                    .write_to(&mut buf, ImageFormat::Png)
                    .map_err(|e| format!("Failed to encode preview as PNG: {}", e))?;

                Ok(LinkPreviewResult {
                    image: BASE64.encode(buf.into_inner()),
                    center_ratio,
                })
            }
            Err(e) => Err(e.to_string()),
        }
    })?;

    Ok(result)
}

#[tauri::command]
pub fn search_pdf(query: String, state: State<'_, AppState>) -> Result<Vec<SearchHit>, String> {
    let bytes = {
        let pdf_state = state.pdf_state.lock().map_err(|e| e.to_string())?;
        pdf_state
            .current_bytes
            .as_ref()
            .ok_or("No PDF is currently open")?
            .clone()
    }; // Lock dropped here

    let hits = with_pdfium(|pdfium| {
        let document = pdfium
            .load_pdf_from_byte_slice(bytes.as_slice(), None)
            .map_err(|e| format!("Failed to open PDF: {}", e))?;

        let query_lower = query.to_lowercase();
        let mut hits = Vec::new();

        for (page_idx, page) in document.pages().iter().enumerate() {
            let text = page
                .text()
                .map_err(|e| format!("Failed to extract text from page {}: {}", page_idx, e))?;
            let page_text = text.all();

            if page_text.to_lowercase().contains(&query_lower) {
                // Find the matching line/context
                for line in page_text.lines() {
                    if line.to_lowercase().contains(&query_lower) {
                        hits.push(SearchHit {
                            page_index: page_idx as u32,
                            text: line.trim().to_string(),
                        });
                    }
                }
            }
        }

        Ok(hits)
    })?;

    Ok(hits)
}

#[tauri::command]
pub fn get_pdfium_diagnostics() -> PdfiumDiagnostics {
    let attempted_paths: Vec<String> = get_pdfium_bind_paths()
        .iter()
        .map(|path| path_display(path))
        .collect();

    match resolve_pdfium_library_path() {
        Ok(path) => PdfiumDiagnostics {
            available: true,
            selected_path: Some(path_display(&path)),
            attempted_paths,
            error: None,
        },
        Err(error) => PdfiumDiagnostics {
            available: false,
            selected_path: None,
            attempted_paths,
            error: Some(error),
        },
    }
}

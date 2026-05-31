use image::DynamicImage;
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

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
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum PdfAnnotationKind {
    Highlight,
    Note,
}

impl PdfAnnotationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Highlight => "Highlight",
            Self::Note => "Note",
        }
    }
}

impl std::str::FromStr for PdfAnnotationKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Highlight" => Ok(Self::Highlight),
            "Note" => Ok(Self::Note),
            _ => Err(format!("Unknown annotation kind: {s}")),
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
}

impl std::str::FromStr for PdfAnnotationColor {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Yellow" => Ok(Self::Yellow),
            "Green" => Ok(Self::Green),
            "Blue" => Ok(Self::Blue),
            "Pink" => Ok(Self::Pink),
            "Orange" => Ok(Self::Orange),
            _ => Err(format!("Unknown annotation color: {s}")),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PdfTextRange {
    pub start_text_index: usize,
    pub end_text_index: usize,
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
    pub created_at: i64,
    pub updated_at: i64,
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

    let mut hasher = Sha256::new();
    hasher.update(&buffer);
    hasher.update(file_len.to_be_bytes());
    if let Some(mtime) = modified {
        hasher.update(mtime.to_be_bytes());
    } else {
        hasher.update([0u8; 8]);
    }

    let hash_result = hasher.finalize();
    let id = format!("{:x}", hash_result);

    Ok((id, file_len, modified))
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

                // Characters should be moving left-to-right (c.bbox.x >= r.x)
                // and horizontal gap should not be too large
                let horizontal_gap = c.bbox.x - (r.x + r.width);

                if overlap > 0.0
                    && overlap > 0.3 * min_h
                    && c.bbox.x >= r.x
                    && horizontal_gap < 3.0 * min_h.max(4.0)
                {
                    // Merge
                    let x_min = r.x;
                    let x_max = (r.x + r.width).max(c.bbox.x + c.bbox.width);
                    let y_min = r.y.min(c.bbox.y);
                    let y_max = (r.y + r.height).max(c.bbox.y + c.bbox.height);
                    r.x = x_min;
                    r.y = y_min;
                    r.width = x_max - x_min;
                    r.height = y_max - y_min;
                } else {
                    rects.push(current.take().unwrap());
                    current = Some(c.bbox);
                }
            }
            None => {
                current = Some(c.bbox);
            }
        }
    }
    if let Some(r) = current {
        rects.push(r);
    }
    rects
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TocEntry {
    pub title: String,
    pub page_index: Option<u32>,
    pub children: Vec<TocEntry>,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct PdfRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
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

fn link_preview_crop(
    full_width: u32,
    full_height: u32,
    target_y: Option<f32>,
    scale: f32,
) -> (u32, u32, u32, u32, f32) {
    if full_height == 0 {
        return (0, 0, full_width.max(1), 1, 0.5);
    }

    let desired_w = (full_width as f32 * 0.82).round().max(1.0) as u32;
    let crop_w = desired_w.min(full_width.max(1));
    let crop_x = full_width.saturating_sub(crop_w) / 2;

    let desired_h = (720.0 * scale).round().max(1.0) as u32;
    let crop_h = desired_h.min(full_height);
    let target = target_y.unwrap_or(full_height as f32 / (2.0 * scale)) * scale;
    let max_y = full_height.saturating_sub(crop_h);
    let crop_y = (target - crop_h as f32 * 0.42)
        .round()
        .clamp(0.0, max_y as f32) as u32;
    let center_ratio = ((target - crop_y as f32) / crop_h as f32).clamp(0.0, 1.0);

    (crop_x, crop_y, crop_w.max(1), crop_h.max(1), center_ratio)
}

fn link_preview_content_crop(
    full_width: u32,
    full_height: u32,
    page_width: f32,
    page_height: f32,
    target_y: Option<f32>,
    scale: f32,
    lines: &[PdfTextLine],
) -> (u32, u32, u32, u32, f32) {
    let target = target_y.unwrap_or(page_height / 2.0);
    let mut sorted = lines
        .iter()
        .filter(|line| line.bbox.width > 0.0 && line.bbox.height > 0.0)
        .collect::<Vec<_>>();
    sorted.sort_by(|a, b| {
        let a_top = page_height - a.bbox.y - a.bbox.height;
        let b_top = page_height - b.bbox.y - b.bbox.height;
        a_top
            .partial_cmp(&b_top)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if sorted.is_empty() {
        return link_preview_crop(full_width, full_height, target_y, scale);
    }

    let start_idx = sorted
        .iter()
        .position(|line| {
            let line_bottom = page_height - line.bbox.y;
            line_bottom >= target - 8.0
        })
        .unwrap_or_else(|| sorted.len().saturating_sub(1));

    let mut selected = Vec::new();
    let mut block_bottom = page_height - sorted[start_idx].bbox.y;
    let block_top = page_height - sorted[start_idx].bbox.y - sorted[start_idx].bbox.height;

    for line in sorted.iter().skip(start_idx) {
        let line_top = page_height - line.bbox.y - line.bbox.height;
        let line_bottom = page_height - line.bbox.y;
        if !selected.is_empty() {
            let gap = line_top - block_bottom;
            if gap > 34.0 || line_bottom - block_top > 220.0 || selected.len() >= 10 {
                break;
            }
        }
        block_bottom = block_bottom.max(line_bottom);
        selected.push(*line);
    }

    let x_min = selected
        .iter()
        .map(|line| line.bbox.x)
        .fold(page_width, f32::min);
    let x_max = selected
        .iter()
        .map(|line| line.bbox.x + line.bbox.width)
        .fold(0.0, f32::max);
    let y_top = selected
        .iter()
        .map(|line| page_height - line.bbox.y - line.bbox.height)
        .fold(page_height, f32::min);
    let y_bottom = selected
        .iter()
        .map(|line| page_height - line.bbox.y)
        .fold(0.0, f32::max);

    let x_pad = 36.0;
    let y_pad = 42.0;
    let crop_x = ((x_min - x_pad).max(0.0) * scale).round() as u32;
    let right = ((x_max + x_pad).min(page_width) * scale).round() as u32;
    let min_w = (360.0 * scale).round() as u32;
    let crop_w = right
        .saturating_sub(crop_x)
        .max(min_w)
        .min(full_width.saturating_sub(crop_x).max(1));

    let content_top = ((y_top - y_pad).max(0.0) * scale).round();
    let content_bottom = ((y_bottom + y_pad).min(page_height) * scale).round();
    let content_h = (content_bottom - content_top).max(1.0) as u32;
    let min_h = (180.0 * scale).round() as u32;
    let max_h = (330.0 * scale).round() as u32;
    let crop_h = content_h.max(min_h).min(max_h).min(full_height.max(1));
    let content_center = (content_top + content_bottom) / 2.0;
    let max_y = full_height.saturating_sub(crop_h);
    let crop_y = (content_center - crop_h as f32 / 2.0)
        .round()
        .clamp(0.0, max_y as f32) as u32;
    let target_scaled = target * scale;
    let center_ratio = ((target_scaled - crop_y as f32) / crop_h as f32).clamp(0.0, 1.0);

    (crop_x, crop_y, crop_w, crop_h, center_ratio)
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PdfSearchMatch {
    pub page_index: u16,
    pub context: String,
    pub rects: Vec<PdfRect>,
}

pub struct PdfState {
    pub current_page: u16,
    pub total_pages: u16,
    pub scale: f32,
    pub path: Option<String>,
}

impl PdfState {
    pub fn new() -> Self {
        Self {
            current_page: 0,
            total_pages: 0,
            scale: 1.5,
            path: None,
        }
    }
}

impl Default for PdfState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct PdfRenderer {
    render_sender: std::sync::mpsc::Sender<RenderCommand>,
    query_sender: std::sync::mpsc::Sender<QueryCommand>,
    priority_sender: std::sync::mpsc::Sender<PriorityRender>,
    visible_range: std::sync::Arc<std::sync::Mutex<Option<(u16, u16, String)>>>,
}

struct PriorityRender {
    path: String,
    page_index: u16,
    scale: f32,
    resp: std::sync::mpsc::SyncSender<Result<DynamicImage, String>>,
}

pub enum RenderCommand {
    Wake,
    PageCount(String, std::sync::mpsc::SyncSender<Result<u16, String>>),
    PageSizes(
        String,
        std::sync::mpsc::SyncSender<Result<Vec<(f32, f32)>, String>>,
    ),
    RenderPage(
        String,
        u16,
        f32,
        std::sync::mpsc::SyncSender<Result<DynamicImage, String>>,
    ),
    GetToc(
        String,
        std::sync::mpsc::SyncSender<Result<Vec<TocEntry>, String>>,
    ),
    GetLinks(
        String,
        u16,
        std::sync::mpsc::SyncSender<Result<Vec<LinkInfo>, String>>,
    ),
    RenderLinkPreview(
        String,
        u32,
        Option<f32>,
        std::sync::mpsc::SyncSender<Result<LinkPreviewResult, String>>,
    ),
}

pub enum QueryCommand {
    SearchText {
        path: String,
        query: String,
        regex: bool,
        match_case: bool,
        result_sender: std::sync::mpsc::Sender<PdfSearchMatch>,
        done_sender: std::sync::mpsc::Sender<Result<(), String>>,
        search_id: u64,
    },
    CancelSearch {
        search_id: u64,
    },
    GetPageText(
        String,
        u16,
        std::sync::mpsc::SyncSender<Result<PdfPageText, String>>,
    ),
}

fn ensure_document<'a>(
    pdfium: &'a Pdfium,
    current_document: &mut Option<(String, PdfDocument<'a>)>,
    path: &str,
) -> Result<(), String> {
    if current_document
        .as_ref()
        .map(|(p, _)| p != path)
        .unwrap_or(true)
    {
        let doc = pdfium
            .load_pdf_from_file(path, None)
            .map_err(|e| format!("Failed to load PDF: {:?}", e))?;
        *current_document = Some((path.to_string(), doc));
    }
    Ok(())
}

fn get_page_text_impl<'a>(
    pdfium: &'a Pdfium,
    current_document: &mut Option<(String, PdfDocument<'a>)>,
    path: &str,
    index: u16,
) -> Result<PdfPageText, String> {
    ensure_document(pdfium, current_document, path)?;
    let Some((_, doc)) = current_document.as_ref() else {
        return Err("PDF document was not loaded".to_string());
    };
    let pages = doc.pages();
    if i32::from(index) >= pages.len() {
        return Err("Page index out of bounds".to_string());
    }
    let page = pages.get(index as i32).map_err(|e| e.to_string())?;
    let text_page = page.text().map_err(|e| e.to_string())?;

    let page_width = page.width().value;
    let page_height = page.height().value;

    let mut text = String::new();
    let mut chars = Vec::new();
    let mut text_index = 0usize;

    for c in text_page.chars().iter() {
        if let Some(ch) = c.unicode_char() {
            let char_index = c.index() as u32;
            text.push(ch);

            let bbox = match c.loose_bounds() {
                Ok(rect) => PdfRect {
                    x: rect.left().value,
                    y: rect.bottom().value,
                    width: rect.width().value,
                    height: rect.height().value,
                },
                Err(_) => PdfRect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                },
            };

            chars.push(PdfTextChar {
                char_index,
                text_index,
                ch,
                bbox,
            });
            text_index += 1;
        }
    }

    let mut lines = Vec::new();
    let mut current_line: Option<PdfTextLine> = None;

    for c in chars
        .iter()
        .filter(|c| c.bbox.width > 0.0 && c.bbox.height > 0.0)
    {
        match &mut current_line {
            Some(line) => {
                let line_y_min = line.bbox.y;
                let line_y_max = line.bbox.y + line.bbox.height;
                let c_y_min = c.bbox.y;
                let c_y_max = c.bbox.y + c.bbox.height;

                let overlap =
                    line_y_max.min(c_y_max) - line_y_min.max(c_y_min);
                let min_h = line.bbox.height.min(c.bbox.height);

                if overlap > 0.0 && overlap > 0.3 * min_h {
                    let x_min = line.bbox.x.min(c.bbox.x);
                    let x_max = (line.bbox.x + line.bbox.width)
                        .max(c.bbox.x + c.bbox.width);
                    let y_min = line.bbox.y.min(c.bbox.y);
                    let y_max = (line.bbox.y + line.bbox.height)
                        .max(c.bbox.y + c.bbox.height);

                    line.bbox.x = x_min;
                    line.bbox.y = y_min;
                    line.bbox.width = x_max - x_min;
                    line.bbox.height = y_max - y_min;
                    line.end_text_index = c.text_index + 1;
                } else {
                    lines.push(current_line.take().unwrap());
                    current_line = Some(PdfTextLine {
                        start_text_index: c.text_index,
                        end_text_index: c.text_index + 1,
                        bbox: c.bbox,
                    });
                }
            }
            None => {
                current_line = Some(PdfTextLine {
                    start_text_index: c.text_index,
                    end_text_index: c.text_index + 1,
                    bbox: c.bbox,
                });
            }
        }
    }
    if let Some(line) = current_line {
        lines.push(line);
    }

    Ok(PdfPageText {
        page_index: index,
        page_width,
        page_height,
        text,
        chars,
        lines,
    })
}

fn scan_page_for_search<'a>(
    pdfium: &'a Pdfium,
    current_document: &mut Option<(String, PdfDocument<'a>)>,
    path: &str,
    index: u16,
    query: &str,
    regex: bool,
    match_case: bool,
) -> Vec<PdfSearchMatch> {
    let Ok(page_layer) = get_page_text_impl(pdfium, current_document, path, index) else {
        return Vec::new();
    };
    let page_text = page_layer.text.as_str();

    let re = {
        let pattern = if regex {
            query.to_string()
        } else {
            regex::escape(query)
        };
        match regex::RegexBuilder::new(&pattern)
            .case_insensitive(!match_case)
            .build()
        {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        }
    };

    let page_matches: Vec<(usize, usize, Vec<PdfRect>)> = re
        .find_iter(page_text)
        .filter_map(|found| {
            let match_char_idx_in_text = page_text[..found.start()].chars().count();
            let match_char_count = found.as_str().chars().count();

            if match_char_count > 0 {
                let match_end = match_char_idx_in_text + match_char_count;
                let chars = page_layer
                    .chars
                    .iter()
                    .filter(|c| {
                        c.text_index >= match_char_idx_in_text && c.text_index < match_end
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                let rects = merge_char_rects(&chars);
                Some((match_char_idx_in_text, match_char_count, rects))
            } else {
                None
            }
        })
        .collect();

    let mut matches = Vec::new();
    for (pos, match_len, rects) in page_matches {
        if rects.is_empty() {
            continue;
        }
        let start = pos.saturating_sub(48);
        let take = match_len + 96;
        let context = page_text
            .chars()
            .skip(start)
            .take(take)
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        matches.push(PdfSearchMatch {
            page_index: index,
            context,
            rects,
        });
    }
    matches
}

impl PdfRenderer {
    pub fn new() -> Result<Self, String> {
        bind_pdfium().map_err(|e| format!("Failed to initialize PDF engine: {e}"))?;

        let (render_sender, render_receiver) = std::sync::mpsc::channel();
        let (query_sender, query_receiver) = std::sync::mpsc::channel();
        let (priority_sender, priority_receiver) = std::sync::mpsc::channel::<PriorityRender>();
        let visible_range: std::sync::Arc<std::sync::Mutex<Option<(u16, u16, String)>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let visible_range_clone = visible_range.clone();

        // 1. Spawn Render Worker Thread
        std::thread::spawn(move || {
            let pdfium = match bind_pdfium() {
                Ok(pdfium) => pdfium,
                Err(err) => {
                    eprintln!("Failed to bind PDFium in render thread: {err}");
                    return;
                }
            };

            let mut current_document: Option<(String, PdfDocument)> = None;
            let mut pending_commands = VecDeque::new();

            loop {
                let cmd = if let Some(cmd) = pending_commands.pop_front() {
                    cmd
                } else {
                    match render_receiver.recv() {
                        Ok(cmd) => {
                            pending_commands.push_back(cmd);
                            while let Ok(cmd) = render_receiver.try_recv() {
                                pending_commands.push_back(cmd);
                            }
                            continue;
                        }
                        Err(_) => break,
                    }
                };

                while let Ok(mut priority) = priority_receiver.try_recv() {
                    while let Ok(newer_priority) = priority_receiver.try_recv() {
                        priority = newer_priority;
                    }

                    let res = render_page_from_cache(
                        &pdfium,
                        &mut current_document,
                        &priority.path,
                        priority.page_index,
                        priority.scale,
                    );
                    let _ = priority.resp.send(res);
                }

                match cmd {
                    RenderCommand::Wake => {}
                    RenderCommand::PageCount(path, resp) => {
                        let res = (|| {
                            ensure_document(&pdfium, &mut current_document, &path)?;
                            let Some((_, doc)) = current_document.as_ref() else {
                                return Err("PDF document was not loaded".to_string());
                            };
                            Ok(doc.pages().len() as u16)
                        })();
                        let _ = resp.send(res);
                    }
                    RenderCommand::PageSizes(path, resp) => {
                        let res = (|| {
                            ensure_document(&pdfium, &mut current_document, &path)?;
                            let Some((_, doc)) = current_document.as_ref() else {
                                return Err("PDF document was not loaded".to_string());
                            };
                            let mut sizes = Vec::with_capacity(doc.pages().len() as usize);
                            for index in 0..doc.pages().len() {
                                let page = doc.pages().get(index).map_err(|e| e.to_string())?;
                                sizes.push((page.width().value, page.height().value));
                            }
                            Ok(sizes)
                        })();
                        let _ = resp.send(res);
                    }
                    RenderCommand::RenderPage(path, index, scale, resp) => {
                        let skipped = {
                            if let Ok(range_lock) = visible_range_clone.lock() {
                                if let Some((start, end, ref range_path)) = *range_lock {
                                    if range_path == &path {
                                        let buffered_start = start.saturating_sub(2);
                                        let buffered_end = end.saturating_add(2);
                                        index < buffered_start || index > buffered_end
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        };

                        if skipped {
                            let _ = resp.send(Err("Skipped".to_string()));
                        } else {
                            let res = render_page_from_cache(
                                &pdfium,
                                &mut current_document,
                                &path,
                                index,
                                scale,
                            );
                            let _ = resp.send(res);
                        }
                    }
                    RenderCommand::GetToc(path, resp) => {
                        let res = (|| {
                            ensure_document(&pdfium, &mut current_document, &path)?;
                            let Some((_, doc)) = current_document.as_ref() else {
                                return Err("PDF document was not loaded".to_string());
                            };
                            let mut bookmarks = Vec::new();
                            let mut current = doc.bookmarks().root();
                            while let Some(bookmark) = current {
                                current = bookmark.next_sibling();
                                bookmarks.push(bookmark);
                            }
                            Ok(Self::parse_bookmarks(&bookmarks))
                        })();
                        let _ = resp.send(res);
                    }
                    RenderCommand::GetLinks(path, index, resp) => {
                        let res = (|| {
                            ensure_document(&pdfium, &mut current_document, &path)?;
                            let Some((_, doc)) = current_document.as_ref() else {
                                return Err("PDF document was not loaded".to_string());
                            };
                            let pages = doc.pages();
                            if i32::from(index) >= pages.len() {
                                return Err("Page out of bounds".to_string());
                            }
                            let page = pages.get(index as i32).map_err(|e| e.to_string())?;
                            let page_height = page.height().value;
                            let mut links = Vec::new();
                            for link in page.links().iter() {
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
                                let mut dest_page = None;
                                let mut dest_y = None;
                                let mut uri = None;
                                let extract_dest = |dest: &PdfDestination, page_h: f32| -> (Option<u32>, Option<f32>) {
                                    let p = dest.page_index().ok().map(|i| i as u32);
                                    let y = match dest.view_settings() {
                                        Ok(PdfDestinationViewSettings::SpecificCoordinatesAndZoom(_, Some(y_pts), _)) => Some(page_h - y_pts.value),
                                        Ok(PdfDestinationViewSettings::FitPageHorizontallyToWindow(Some(y_pts))) => Some(page_h - y_pts.value),
                                        Ok(PdfDestinationViewSettings::FitBoundsHorizontallyToWindow(Some(y_pts))) => Some(page_h - y_pts.value),
                                        _ => None,
                                    };
                                    (p, y)
                                };
                                if let Some(action) = link.action() {
                                    match action {
                                        PdfAction::Uri(ref uri_action) => {
                                            uri = uri_action.uri().ok()
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
                                if dest_page.is_none()
                                    && uri.is_none()
                                    && let Some(dest) = link.destination()
                                {
                                    let (p, y) = extract_dest(&dest, page_height);
                                    dest_page = p;
                                    if dest_y.is_none() {
                                        dest_y = y;
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
                        })();
                        let _ = resp.send(res);
                    }
                    RenderCommand::RenderLinkPreview(path, index, dest_y, resp) => {
                        let res = (|| {
                            ensure_document(&pdfium, &mut current_document, &path)?;
                            let page_text =
                                get_page_text_impl(&pdfium, &mut current_document, &path, index as u16)
                                    .ok();
                            let Some((_, doc)) = current_document.as_ref() else {
                                return Err("PDF document was not loaded".to_string());
                            };
                            let page = doc.pages().get(index as i32).map_err(|e| e.to_string())?;
                            let scale = 2.0;
                            let full_width = (page.width().value * scale) as i32;
                            let full_height = (page.height().value * scale) as i32;
                            let render_config = PdfRenderConfig::new()
                                .set_target_width(full_width)
                                .set_target_height(full_height);
                            let bitmap = page
                                .render_with_config(&render_config)
                                .map_err(|e| e.to_string())?;
                            let dynamic_image = bitmap.as_image().map_err(|e| e.to_string())?;
                            let (crop_x, crop_y, crop_w, crop_h, center_ratio) = if let Some(page_text) = page_text {
                                link_preview_content_crop(
                                    full_width as u32,
                                    full_height as u32,
                                    page.width().value,
                                    page.height().value,
                                    dest_y,
                                    scale,
                                    &page_text.lines,
                                )
                            } else {
                                link_preview_crop(full_width as u32, full_height as u32, dest_y, scale)
                            };
                            let cropped = dynamic_image.crop_imm(crop_x, crop_y, crop_w, crop_h);
                            let mut buf = std::io::Cursor::new(Vec::new());
                            cropped
                                .write_to(&mut buf, image::ImageFormat::Png)
                                .map_err(|e| e.to_string())?;
                            Ok(LinkPreviewResult {
                                image_data: buf.into_inner(),
                                center_ratio,
                            })
                        })();
                        let _ = resp.send(res);
                    }
                }
            }
        });

        // 2. Spawn Query Worker Thread
        std::thread::spawn(move || {
            let pdfium = match bind_pdfium() {
                Ok(pdfium) => pdfium,
                Err(err) => {
                    eprintln!("Failed to bind PDFium in query thread: {err}");
                    return;
                }
            };

            let mut current_document: Option<(String, PdfDocument)> = None;
            let mut active_search: Option<(u64, String, String, bool, bool, std::sync::mpsc::Sender<PdfSearchMatch>, std::sync::mpsc::Sender<Result<(), String>>, u16, u16)> = None;

            loop {
                let cmd = if active_search.is_some() {
                    match query_receiver.try_recv() {
                        Ok(c) => Some(c),
                        Err(std::sync::mpsc::TryRecvError::Empty) => None,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    }
                } else {
                    match query_receiver.recv() {
                        Ok(c) => Some(c),
                        Err(_) => break,
                    }
                };

                if let Some(cmd) = cmd {
                    match cmd {
                        QueryCommand::CancelSearch { search_id } => {
                            if let Some((active_id, _, _, _, _, _, _, _, _)) = &active_search {
                                if *active_id == search_id {
                                    active_search = None;
                                }
                            }
                        }
                        QueryCommand::SearchText {
                            path,
                            query,
                            regex,
                            match_case,
                            result_sender,
                            done_sender,
                            search_id,
                        } => {
                            if query.trim().is_empty() {
                                let _ = done_sender.send(Ok(()));
                                active_search = None;
                            } else {
                                let total_pages = (|| {
                                    ensure_document(&pdfium, &mut current_document, &path)?;
                                    let Some((_, doc)) = current_document.as_ref() else {
                                        return Err("PDF document was not loaded".to_string());
                                    };
                                    Ok::<u16, String>(doc.pages().len() as u16)
                                })();
                                match total_pages {
                                    Ok(total) => {
                                        active_search = Some((
                                            search_id,
                                            path,
                                            query,
                                            regex,
                                            match_case,
                                            result_sender,
                                            done_sender,
                                            0,
                                            total,
                                        ));
                                    }
                                    Err(err) => {
                                        let _ = done_sender.send(Err(err));
                                        active_search = None;
                                    }
                                }
                            }
                        }
                        QueryCommand::GetPageText(path, index, resp) => {
                            let res = get_page_text_impl(&pdfium, &mut current_document, &path, index);
                            let _ = resp.send(res);
                        }
                    }
                }

                if let Some((search_id, path, query, regex, match_case, result_sender, done_sender, page_idx, total_pages)) = active_search.clone() {
                    if page_idx >= total_pages {
                        let _ = done_sender.send(Ok(()));
                        active_search = None;
                    } else {
                        let matches = scan_page_for_search(&pdfium, &mut current_document, &path, page_idx, &query, regex, match_case);
                        let mut send_err = false;
                        for m in matches {
                            if result_sender.send(m).is_err() {
                                send_err = true;
                                break;
                            }
                        }
                        if send_err {
                            active_search = None;
                        } else {
                            if let Some((active_id, _, _, _, _, _, _, ref mut cur_page, _)) = active_search {
                                if active_id == search_id {
                                    *cur_page += 1;
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            render_sender,
            query_sender,
            priority_sender,
            visible_range,
        })
    }

    pub fn set_visible_range(&self, start: u16, end: u16, path: &str) {
        if let Ok(mut range_lock) = self.visible_range.lock() {
            *range_lock = Some((start, end, path.to_string()));
        }
    }

    pub fn render_page(
        &self,
        path: &str,
        page_index: u16,
        scale: f32,
    ) -> Result<DynamicImage, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.render_sender
            .send(RenderCommand::RenderPage(
                path.to_string(),
                page_index,
                scale,
                tx,
            ))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    pub fn render_page_priority(
        &self,
        path: &str,
        page_index: u16,
        scale: f32,
    ) -> Result<DynamicImage, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.priority_sender
            .send(PriorityRender {
                path: path.to_string(),
                page_index,
                scale,
                resp: tx,
            })
            .map_err(|e| e.to_string())?;
        self.render_sender
            .send(RenderCommand::Wake)
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    pub fn page_count(&self, path: &str) -> Result<u16, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.render_sender
            .send(RenderCommand::PageCount(path.to_string(), tx))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    pub fn page_sizes(&self, path: &str) -> Result<Vec<(f32, f32)>, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.render_sender
            .send(RenderCommand::PageSizes(path.to_string(), tx))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    pub fn get_toc(&self, path: &str) -> Result<Vec<TocEntry>, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.render_sender
            .send(RenderCommand::GetToc(path.to_string(), tx))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    pub fn get_page_links(&self, path: &str, page_index: u16) -> Result<Vec<LinkInfo>, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.render_sender
            .send(RenderCommand::GetLinks(path.to_string(), page_index, tx))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    pub fn search_text_stream(
        &self,
        path: String,
        query: String,
        regex: bool,
        match_case: bool,
        search_id: u64,
    ) -> Result<(std::sync::mpsc::Receiver<PdfSearchMatch>, std::sync::mpsc::Receiver<Result<(), String>>), String> {
        let (res_tx, res_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        self.query_sender
            .send(QueryCommand::SearchText {
                path,
                query,
                regex,
                match_case,
                result_sender: res_tx,
                done_sender: done_tx,
                search_id,
            })
            .map_err(|e| e.to_string())?;
        Ok((res_rx, done_rx))
    }

    pub fn cancel_search(&self, search_id: u64) -> Result<(), String> {
        self.query_sender
            .send(QueryCommand::CancelSearch { search_id })
            .map_err(|e| e.to_string())
    }

    pub fn search_text(
        &self,
        path: &str,
        query: &str,
        regex: bool,
        match_case: bool,
    ) -> Result<Vec<PdfSearchMatch>, String> {
        let (res_rx, done_rx) = self.search_text_stream(
            path.to_string(),
            query.to_string(),
            regex,
            match_case,
            9999, // dummy search_id
        )?;
        let mut results = Vec::new();
        while let Ok(m) = res_rx.recv() {
            results.push(m);
        }
        let _ = done_rx.recv();
        Ok(results)
    }

    pub fn render_link_preview(
        &self,
        path: &str,
        page_index: u32,
        dest_y: Option<f32>,
    ) -> Result<LinkPreviewResult, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.render_sender
            .send(RenderCommand::RenderLinkPreview(
                path.to_string(),
                page_index,
                dest_y,
                tx,
            ))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    pub fn get_page_text(&self, path: &str, page_index: u16) -> Result<PdfPageText, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.query_sender
            .send(QueryCommand::GetPageText(path.to_string(), page_index, tx))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    fn parse_bookmarks(bookmarks: &[PdfBookmark]) -> Vec<TocEntry> {
        let mut entries = Vec::new();
        for bookmark in bookmarks.iter() {
            let title = bookmark.title().unwrap_or_default();
            let page_index = bookmark
                .destination()
                .and_then(|dest| dest.page_index().ok())
                .map(|idx| idx as u32);
            let child_bookmarks: Vec<PdfBookmark> = bookmark.iter_direct_children().collect();
            let children = Self::parse_bookmarks(&child_bookmarks);
            entries.push(TocEntry {
                title,
                page_index,
                children,
            });
        }
        entries
    }
}

fn render_page_from_cache<'a>(
    pdfium: &'a Pdfium,
    current_document: &mut Option<(String, PdfDocument<'a>)>,
    path: &str,
    index: u16,
    scale: f32,
) -> Result<DynamicImage, String> {
    if current_document
        .as_ref()
        .map(|(p, _)| p != path)
        .unwrap_or(true)
    {
        let doc = pdfium
            .load_pdf_from_file(path, None)
            .map_err(|e| format!("Failed to load PDF: {:?}", e))?;
        *current_document = Some((path.to_string(), doc));
    }
    let Some((_, doc)) = current_document.as_ref() else {
        return Err("PDF document was not loaded".to_string());
    };
    let pages = doc.pages();
    if i32::from(index) >= pages.len() {
        return Err("Page index out of bounds".to_string());
    }
    let page = pages
        .get(index as i32)
        .map_err(|e| format!("Failed to get page: {:?}", e))?;

    let render_config = PdfRenderConfig::new()
        .set_target_width((page.width().value * scale) as i32)
        .set_target_height((page.height().value * scale) as i32);

    let bitmap = page
        .render_with_config(&render_config)
        .map_err(|e| format!("Failed to render page: {:?}", e))?;

    bitmap
        .as_image()
        .map_err(|e| format!("Failed to convert to image: {:?}", e))
}

fn bind_pdfium() -> Result<Pdfium, String> {
    static BIND_RESULT: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();

    let res = BIND_RESULT.get_or_init(|| {
        let lib_name = Pdfium::pdfium_platform_library_name();
        let mut candidates = Vec::new();

        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            candidates.push(dir.join("resources").join(&lib_name));
            candidates.push(dir.join(&lib_name));
        }
        candidates.push(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("pdfium")
                .join(&lib_name),
        );

        let mut bound = false;
        for candidate in candidates {
            if candidate.exists() {
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

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn pdf_rect_is_copy_for_overlay_and_annotation_hot_paths() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<PdfRect>();

        let rect = PdfRect {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        };
        let copied = rect;

        assert_eq!(rect.x, copied.x);
        assert_eq!(rect.y, copied.y);
        assert_eq!(rect.width, copied.width);
        assert_eq!(rect.height, copied.height);
    }

    #[test]
    fn test_pdf_search() {
        let _guard = TEST_LOCK.lock().unwrap();
        let pdfium = bind_pdfium().unwrap();
        let doc = pdfium.load_pdf_from_file("../dummy.pdf", None).unwrap();
        let page = doc.pages().get(0).unwrap();
        let text_page = page.text().unwrap();
        let page_height = page.height().value;
        let page_width = page.width().value;
        println!("Page size: {} x {}", page_width, page_height);
        let options = PdfSearchOptions::new();
        let search = text_page.search("dummy", &options).unwrap();
        let mut count = 0;
        for segments in search.iter(PdfSearchDirection::SearchForward) {
            count += 1;
            println!("Match {}:", count);
            for segment in segments.iter() {
                let bounds = segment.bounds();
                let ry = bounds.bottom().value;
                let rh = bounds.height().value;
                let view_y = page_height - ry - rh;
                println!(
                    "  Segment bounds: left={}, bottom={}, width={}, height={}",
                    bounds.left().value,
                    ry,
                    bounds.width().value,
                    rh
                );
                println!("  Expected view y (zoom=1.0): {}", view_y);
            }
        }
        assert!(count > 0, "No matches found!");
    }

    #[test]
    fn test_pdf_renderer_search() {
        let _guard = TEST_LOCK.lock().unwrap();
        let renderer = PdfRenderer::new().unwrap();
        let path = "../dummy.pdf";

        // Test non-regex search
        let results = renderer.search_text(path, "dummy", false, false).unwrap();
        println!("Non-regex results: {results:?}");
        assert!(
            !results.is_empty(),
            "Non-regex search for 'dummy' should return matches"
        );
        for match_info in &results {
            assert!(match_info.context.to_lowercase().contains("dummy"));
            assert!(!match_info.rects.is_empty());
        }

        // Test regex search
        let regex_results = renderer
            .search_text(path, "dum[a-z]+y", true, false)
            .unwrap();
        println!("Regex results: {regex_results:?}");
        assert!(
            !regex_results.is_empty(),
            "Regex search for 'dum[a-z]+y' should return matches"
        );
        assert_eq!(
            results.len(),
            regex_results.len(),
            "Regex and non-regex search count should match"
        );
    }

    #[test]
    fn link_preview_crop_is_tall_and_clamped_to_page_bounds() {
        let (top_x, top_y, top_w, top_h, top_ratio) =
            link_preview_crop(1000, 1600, Some(20.0), 2.0);
        assert_eq!(top_x, 90);
        assert_eq!(top_y, 0);
        assert_eq!(top_w, 820);
        assert_eq!(top_h, 1440);
        assert!(top_ratio > 0.0 && top_ratio < 0.1);

        let (middle_x, middle_y, middle_w, middle_h, middle_ratio) =
            link_preview_crop(1000, 2000, Some(450.0), 2.0);
        assert_eq!(middle_x, 90);
        assert_eq!(middle_w, 820);
        assert_eq!(middle_h, 1440);
        assert!(middle_y > 0);
        assert!(middle_ratio > 0.35 && middle_ratio < 0.5);

        let (_, bottom_y, _, bottom_h, bottom_ratio) =
            link_preview_crop(1000, 1600, Some(2000.0), 2.0);
        assert_eq!(bottom_h, 1440);
        assert_eq!(bottom_y + bottom_h, 1600);
        assert_eq!(bottom_ratio, 1.0);
    }

    #[test]
    fn link_preview_content_crop_uses_nearby_text_bounds_without_clipping_width() {
        let lines = vec![
            PdfTextLine {
                start_text_index: 0,
                end_text_index: 10,
                bbox: PdfRect {
                    x: 70.0,
                    y: 600.0,
                    width: 470.0,
                    height: 14.0,
                },
            },
            PdfTextLine {
                start_text_index: 10,
                end_text_index: 20,
                bbox: PdfRect {
                    x: 72.0,
                    y: 575.0,
                    width: 430.0,
                    height: 14.0,
                },
            },
        ];

        let (x, y, w, h, ratio) =
            link_preview_content_crop(1224, 1584, 612.0, 792.0, Some(185.0), 2.0, &lines);

        assert!(x <= 68, "left content edge must include padding");
        assert!(x + w >= 1152, "wide content must not be clipped");
        assert!(h >= 360);
        assert!(h <= 660, "preview should show content block, not whole page");
        assert!(y < 360);
        assert!(ratio > 0.0 && ratio < 1.0);
    }

    #[test]
    fn ladr_reference_preview_renders_content_crop_not_full_page() {
        let _guard = TEST_LOCK.lock().unwrap();
        let path = "/home/sur/repo/study-tracker/Y1/books/LADR4e.pdf";
        if !std::path::Path::new(path).exists() {
            eprintln!("Skipping LADR preview test; fixture not present: {path}");
            return;
        }

        let renderer = PdfRenderer::new().unwrap();
        let mut target = None;
        for page in 0..20 {
            let Ok(links) = renderer.get_page_links(path, page) else {
                continue;
            };
            if let Some(link) = links
                .into_iter()
                .find(|link| link.dest_page.is_some() && link.dest_y.is_some())
            {
                target = Some(link);
                break;
            }
        }

        let link = target.expect("LADR fixture should contain internal PDF references");
        let preview = renderer
            .render_link_preview(path, link.dest_page.unwrap(), link.dest_y)
            .expect("LADR link preview should render");
        let img = image::load_from_memory(&preview.image_data).unwrap();

        assert!(img.height() <= 700, "preview should not render full page");
        assert!(img.width() <= 1300, "preview should not render full page width at 2x");
        assert!(img.height() >= 260, "preview should remain comfortably readable");
    }

    #[test]
    fn test_pdf_text_extraction_and_hashing() {
        let _guard = TEST_LOCK.lock().unwrap();
        let path = "../dummy.pdf";
        let (id, size, modified) = compute_provisional_id(std::path::Path::new(path)).unwrap();
        assert!(!id.is_empty(), "Document hash must not be empty");
        assert!(size > 0, "Document size must be > 0");
        assert!(modified.is_some(), "Modified time should be available");

        let renderer = PdfRenderer::new().unwrap();
        let text_layer = renderer.get_page_text(path, 0).unwrap();
        println!("Extracted text: {}", text_layer.text);
        assert!(
            text_layer.text.to_lowercase().contains("dummy"),
            "Extracted text should contain 'dummy'"
        );
        assert!(
            !text_layer.chars.is_empty(),
            "Chars list should not be empty"
        );
        assert!(
            !text_layer.lines.is_empty(),
            "Lines list should not be empty"
        );
        for line in &text_layer.lines {
            assert!(
                line.end_text_index > line.start_text_index,
                "PDF text lines must use non-empty exclusive ranges"
            );
            assert!(
                line.bbox.width > 0.0 && line.bbox.height > 0.0,
                "PDF text lines must be backed by visible glyph bounds"
            );
            assert!(
                line.end_text_index <= text_layer.text.chars().count(),
                "PDF text line range must stay within extracted text"
            );
            let line_chars = text_layer
                .chars
                .iter()
                .filter(|c| {
                    c.text_index >= line.start_text_index
                        && c.text_index < line.end_text_index
                        && c.bbox.width > 0.0
                        && c.bbox.height > 0.0
                })
                .count();
            assert!(line_chars > 0, "Every PDF text line must be selectable");
        }

        // Verify bounding boxes coordinates
        for c in &text_layer.chars {
            assert!(c.bbox.width >= 0.0);
            assert!(c.bbox.height >= 0.0);
        }

        // Test character merging
        let merged = merge_char_rects(&text_layer.chars[..5]);
        assert!(!merged.is_empty(), "Merged rects list should not be empty");
    }
}

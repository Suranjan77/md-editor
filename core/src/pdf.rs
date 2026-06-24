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

    pub fn from_str(s: &str) -> Result<Self, String> {
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

    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "Yellow" => Ok(Self::Yellow),
            "Green" => Ok(Self::Green),
            "Blue" => Ok(Self::Blue),
            "Pink" => Ok(Self::Pink),
            "Orange" => Ok(Self::Orange),
            _ => Err(format!("Unknown annotation color: {s}")),
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
    hasher.update(&file_len.to_be_bytes());
    if let Some(mtime) = modified {
        hasher.update(&mtime.to_be_bytes());
    } else {
        hasher.update(&[0u8; 8]);
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
                    current = Some(c.bbox.clone());
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

#[derive(Clone, Serialize, Deserialize, Debug)]
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

pub struct PdfRenderer {
    sender: std::sync::mpsc::Sender<PdfCommand>,
    priority_sender: std::sync::mpsc::Sender<PriorityRender>,
    visible_range: std::sync::Arc<std::sync::Mutex<Option<(u16, u16, String)>>>,
}

struct PriorityRender {
    path: String,
    page_index: u16,
    scale: f32,
    resp: std::sync::mpsc::SyncSender<Result<DynamicImage, String>>,
}

enum PdfCommand {
    Wake,
    PageCount(String, std::sync::mpsc::SyncSender<Result<u16, String>>),
    ExtractText(String, std::sync::mpsc::SyncSender<Result<String, String>>),
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
        std::sync::mpsc::SyncSender<Result<(Vec<TocEntry>, bool), String>>,
    ),
    GetReferences(
        String,
        std::sync::mpsc::SyncSender<
            Result<Vec<crate::references::ReferenceLink>, String>,
        >,
    ),
    GetEmbeddedToc(
        String,
        std::sync::mpsc::SyncSender<Result<Vec<TocEntry>, String>>,
    ),
    GetLinks(
        String,
        u16,
        std::sync::mpsc::SyncSender<Result<Vec<LinkInfo>, String>>,
    ),
    SearchText(
        String,
        String,
        bool,
        bool,
        std::sync::mpsc::SyncSender<Result<Vec<PdfSearchMatch>, String>>,
    ),
    RenderLinkPreview(
        String,
        u32,
        Option<f32>,
        u32,
        std::sync::mpsc::SyncSender<Result<LinkPreviewResult, String>>,
    ),
    GetPageText(
        String,
        u16,
        std::sync::mpsc::SyncSender<Result<PdfPageText, String>>,
    ),
}

impl PdfRenderer {
    pub fn new() -> Result<Self, String> {
        let (sender, receiver) = std::sync::mpsc::channel();
        let (priority_sender, priority_receiver) = std::sync::mpsc::channel::<PriorityRender>();
        let visible_range: std::sync::Arc<std::sync::Mutex<Option<(u16, u16, String)>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let visible_range_clone = visible_range.clone();

        std::thread::spawn(move || {
            let pdfium = match bind_pdfium() {
                Ok(pdfium) => pdfium,
                Err(err) => {
                    eprintln!("Failed to bind PDFium: {err}");
                    return;
                }
            };

            let mut current_document: Option<(String, PdfDocument)> = None;
            let mut pending_commands = VecDeque::new();

            loop {
                let cmd = if let Some(cmd) = pending_commands.pop_front() {
                    cmd
                } else {
                    match receiver.recv() {
                        Ok(cmd) => {
                            pending_commands.push_back(cmd);
                            while let Ok(cmd) = receiver.try_recv() {
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
                    PdfCommand::Wake => {}
                    PdfCommand::PageCount(path, resp) => {
                        let res = (|| {
                            if current_document
                                .as_ref()
                                .map(|(p, _)| p != &path)
                                .unwrap_or(true)
                            {
                                let doc = pdfium
                                    .load_pdf_from_file(&path, None)
                                    .map_err(|e| format!("Failed to load PDF: {:?}", e))?;
                                current_document = Some((path.clone(), doc));
                            }
                            let Some((_, doc)) = current_document.as_ref() else {
                                return Err("PDF document was not loaded".to_string());
                            };
                            Ok(doc.pages().len() as u16)
                        })();
                        let _ = resp.send(res);
                    }
                    PdfCommand::ExtractText(path, resp) => {
                        let res = (|| {
                            if current_document
                                .as_ref()
                                .map(|(p, _)| p != &path)
                                .unwrap_or(true)
                            {
                                let doc = pdfium
                                    .load_pdf_from_file(&path, None)
                                    .map_err(|e| format!("Failed to load PDF: {:?}", e))?;
                                current_document = Some((path.clone(), doc));
                            }
                            let Some((_, doc)) = current_document.as_ref() else {
                                return Err("PDF document was not loaded".to_string());
                            };
                            let mut text = String::new();
                            for page in doc.pages().iter() {
                                if let Ok(tp) = page.text() {
                                    text.push_str(&tp.all());
                                    text.push('\n');
                                }
                            }
                            Ok(text)
                        })();
                        let _ = resp.send(res);
                    }
                    PdfCommand::PageSizes(path, resp) => {
                        let res = (|| {
                            if current_document
                                .as_ref()
                                .map(|(p, _)| p != &path)
                                .unwrap_or(true)
                            {
                                let doc = pdfium
                                    .load_pdf_from_file(&path, None)
                                    .map_err(|e| format!("Failed to load PDF: {:?}", e))?;
                                current_document = Some((path.clone(), doc));
                            }
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
                    PdfCommand::RenderPage(path, index, scale, resp) => {
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
                    PdfCommand::GetToc(path, resp) => {
                        let res = (|| {
                            if current_document
                                .as_ref()
                                .map(|(p, _)| p != &path)
                                .unwrap_or(true)
                            {
                                let doc = pdfium
                                    .load_pdf_from_file(&path, None)
                                    .map_err(|e| format!("Failed to load PDF: {:?}", e))?;
                                current_document = Some((path.clone(), doc));
                            }
                            let Some((_, doc)) = current_document.as_ref() else {
                                return Err("PDF document was not loaded".to_string());
                            };
                            let embedded = embedded_toc(doc);
                            if !embedded.is_empty() {
                                return Ok((embedded, false));
                            }
                            // No embedded bookmarks. Recover an outline locally
                            // from the page text (best source first).
                            let page_texts = build_all_page_texts(doc);
                            Ok((recover_toc(doc, &page_texts), true))
                        })();
                        let _ = resp.send(res);
                    }
                    PdfCommand::GetReferences(path, resp) => {
                        let res = (|| {
                            if current_document
                                .as_ref()
                                .map(|(p, _)| p != &path)
                                .unwrap_or(true)
                            {
                                let doc = pdfium
                                    .load_pdf_from_file(&path, None)
                                    .map_err(|e| format!("Failed to load PDF: {:?}", e))?;
                                current_document = Some((path.clone(), doc));
                            }
                            let Some((_, doc)) = current_document.as_ref() else {
                                return Err("PDF document was not loaded".to_string());
                            };
                            // References need the full text layer (for equation
                            // labels and captions) and the outline (for section
                            // targets). The text scan is the one-time cost the
                            // caller caches; see `pdf-text-scan-costs`.
                            let page_texts = build_all_page_texts(doc);
                            let embedded = embedded_toc(doc);
                            let toc = if embedded.is_empty() {
                                recover_toc(doc, &page_texts)
                            } else {
                                embedded
                            };
                            Ok(crate::references::resolve_references(&page_texts, &toc))
                        })();
                        let _ = resp.send(res);
                    }
                    PdfCommand::GetEmbeddedToc(path, resp) => {
                        let res = (|| {
                            if current_document
                                .as_ref()
                                .map(|(p, _)| p != &path)
                                .unwrap_or(true)
                            {
                                let doc = pdfium
                                    .load_pdf_from_file(&path, None)
                                    .map_err(|e| format!("Failed to load PDF: {:?}", e))?;
                                current_document = Some((path.clone(), doc));
                            }
                            let Some((_, doc)) = current_document.as_ref() else {
                                return Err("PDF document was not loaded".to_string());
                            };
                            // Embedded bookmarks only — no text scan. Cheap; used
                            // by the chunked reference resolver so it never issues
                            // a monolithic full-document command.
                            Ok(embedded_toc(doc))
                        })();
                        let _ = resp.send(res);
                    }
                    PdfCommand::GetLinks(path, index, resp) => {
                        let res = (|| {
                            if current_document
                                .as_ref()
                                .map(|(p, _)| p != &path)
                                .unwrap_or(true)
                            {
                                let doc = pdfium
                                    .load_pdf_from_file(&path, None)
                                    .map_err(|e| format!("Failed to load PDF: {:?}", e))?;
                                current_document = Some((path.clone(), doc));
                            }
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
                        })();
                        let _ = resp.send(res);
                    }
                    PdfCommand::SearchText(path, query, regex, match_case, resp) => {
                        let res = (|| {
                            if query.trim().is_empty() {
                                return Ok(Vec::new());
                            }
                            if current_document
                                .as_ref()
                                .map(|(p, _)| p != &path)
                                .unwrap_or(true)
                            {
                                let doc = pdfium
                                    .load_pdf_from_file(&path, None)
                                    .map_err(|e| format!("Failed to load PDF: {:?}", e))?;
                                current_document = Some((path.clone(), doc));
                            }
                            let Some((_, doc)) = current_document.as_ref() else {
                                return Err("PDF document was not loaded".to_string());
                            };

                            let re = if regex {
                                Some(
                                    regex::RegexBuilder::new(&query)
                                        .case_insensitive(!match_case)
                                        .build()
                                        .map_err(|err| {
                                            format!("Invalid PDF search regex: {err}")
                                        })?,
                                )
                            } else {
                                None
                            };
                            let mut matches = Vec::new();
                            for index in 0..doc.pages().len() {
                                let page = doc.pages().get(index).map_err(|e| e.to_string())?;
                                let text_page = page.text().map_err(|e| e.to_string())?;
                                let mut page_text = String::new();
                                let mut char_indices = Vec::new();
                                for c in text_page.chars().iter() {
                                    if let Some(ch) = c.unicode_char() {
                                        page_text.push(ch);
                                        char_indices.push(c.index());
                                    }
                                }
                                let page_matches: Vec<(usize, usize, Vec<PdfRect>)> =
                                    if let Some(re) = &re {
                                        re.find_iter(&page_text)
                                            .filter_map(|found| {
                                                let match_char_idx_in_text =
                                                    page_text[..found.start()].chars().count();
                                                let match_char_count =
                                                    found.as_str().chars().count();
                                                if match_char_idx_in_text < char_indices.len()
                                                    && match_char_count > 0
                                                {
                                                    let char_start =
                                                        char_indices[match_char_idx_in_text];
                                                    let char_end_idx = (match_char_idx_in_text
                                                        + match_char_count
                                                        - 1)
                                                    .min(char_indices.len() - 1);
                                                    let char_count =
                                                        char_indices[char_end_idx] - char_start + 1;
                                                    let rects = text_page
                                                        .segments_subset(char_start, char_count)
                                                        .iter()
                                                        .map(|segment| {
                                                            let bounds = segment.bounds();
                                                            PdfRect {
                                                                x: bounds.left().value,
                                                                y: bounds.bottom().value,
                                                                width: bounds.width().value,
                                                                height: bounds.height().value,
                                                            }
                                                        })
                                                        .collect::<Vec<_>>();
                                                    Some((
                                                        match_char_idx_in_text,
                                                        match_char_count,
                                                        rects,
                                                    ))
                                                } else {
                                                    None
                                                }
                                            })
                                            .collect()
                                    } else {
                                        let options =
                                            PdfSearchOptions::new().match_case(match_case);
                                        match text_page.search(&query, &options) {
                                            Ok(search) => search
                                                .iter(PdfSearchDirection::SearchForward)
                                                .map(|segments| {
                                                    let rects = segments
                                                        .iter()
                                                        .map(|segment| {
                                                            let bounds = segment.bounds();
                                                            PdfRect {
                                                                x: bounds.left().value,
                                                                y: bounds.bottom().value,
                                                                width: bounds.width().value,
                                                                height: bounds.height().value,
                                                            }
                                                        })
                                                        .collect::<Vec<_>>();
                                                    let mut min_char_index = None;
                                                    let mut max_char_index = None;
                                                    for segment in segments.iter() {
                                                        if let Ok(chars) = segment.chars() {
                                                            for char in chars.iter() {
                                                                let idx = char.index();
                                                                if min_char_index
                                                                    .map_or(true, |min| idx < min)
                                                                {
                                                                    min_char_index = Some(idx);
                                                                }
                                                                if max_char_index
                                                                    .map_or(true, |max| idx > max)
                                                                {
                                                                    max_char_index = Some(idx);
                                                                }
                                                            }
                                                        }
                                                    }
                                                    let char_start = min_char_index.unwrap_or(0);
                                                    let char_count = max_char_index
                                                        .map(|max| max - char_start + 1)
                                                        .unwrap_or(0);
                                                    let page_text_idx = char_indices
                                                        .binary_search(&char_start)
                                                        .unwrap_or_else(|x| x);
                                                    let match_char_count = if char_count > 0 {
                                                        let page_text_end_idx = char_indices
                                                            .binary_search(
                                                                &(char_start + char_count - 1),
                                                            )
                                                            .unwrap_or_else(|x| x);
                                                        if page_text_end_idx >= page_text_idx {
                                                            page_text_end_idx - page_text_idx + 1
                                                        } else {
                                                            0
                                                        }
                                                    } else {
                                                        0
                                                    };
                                                    (page_text_idx, match_char_count, rects)
                                                })
                                                .collect(),
                                            Err(_) => Vec::new(),
                                        }
                                    };

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
                                        page_index: index as u16,
                                        context,
                                        rects,
                                    });
                                    if matches.len() >= 250 {
                                        break;
                                    }
                                }
                                if matches.len() >= 250 {
                                    break;
                                }
                            }
                            Ok(matches)
                        })();
                        let _ = resp.send(res);
                    }
                    PdfCommand::RenderLinkPreview(path, index, dest_y, target_width_px, resp) => {
                        let res = (|| {
                            if current_document
                                .as_ref()
                                .map(|(p, _)| p != &path)
                                .unwrap_or(true)
                            {
                                let doc = pdfium
                                    .load_pdf_from_file(&path, None)
                                    .map_err(|e| format!("Failed to load PDF: {:?}", e))?;
                                current_document = Some((path.clone(), doc));
                            }
                            let Some((_, doc)) = current_document.as_ref() else {
                                return Err("PDF document was not loaded".to_string());
                            };
                            let page = doc.pages().get(index as i32).map_err(|e| e.to_string())?;
                            // Render so the page width hits the caller's target
                            // *physical* pixel width (the size the preview is
                            // actually displayed at, accounting for the modal box
                            // and the display's scale factor). This keeps the
                            // enlarged preview pixel-sharp instead of upscaling a
                            // page-points-sized bitmap. Clamped to a sane range.
                            let page_w = page.width().value.max(1.0);
                            let scale = (target_width_px as f32 / page_w).clamp(1.5, 6.0);
                            let full_width = (page.width().value * scale) as i32;
                            let full_height = (page.height().value * scale) as i32;
                            let render_config = PdfRenderConfig::new()
                                .set_target_width(full_width)
                                .set_target_height(full_height);
                            let bitmap = page
                                .render_with_config(&render_config)
                                .map_err(|e| e.to_string())?;
                            let dynamic_image = bitmap.as_image().map_err(|e| e.to_string())?;

                            // When the link names a destination Y, crop a fixed
                            // vertical window of `PREVIEW_WINDOW_PT` points with
                            // the target line exactly at the centre — padding with
                            // white when the target is near a page edge so it is
                            // always centred. Without a Y, the whole page is shown.
                            let (result_image, center_ratio) = match dest_y {
                                Some(y) => {
                                    let window_h_px =
                                        ((PREVIEW_WINDOW_PT * scale).round() as u32).max(1);
                                    let center_px = y * scale;
                                    let win_top = center_px - window_h_px as f32 / 2.0;
                                    let page_rgba = dynamic_image.to_rgba8();
                                    let mut canvas = image::RgbaImage::from_pixel(
                                        full_width as u32,
                                        window_h_px,
                                        image::Rgba([255, 255, 255, 255]),
                                    );
                                    // Negative offset clips the top of the page;
                                    // positive shifts it down, leaving white above.
                                    image::imageops::overlay(
                                        &mut canvas,
                                        &page_rgba,
                                        0,
                                        (-win_top).round() as i64,
                                    );
                                    (image::DynamicImage::ImageRgba8(canvas), 0.5)
                                }
                                None => (dynamic_image, 0.0),
                            };

                            let mut buf = std::io::Cursor::new(Vec::new());
                            result_image
                                .write_to(&mut buf, image::ImageFormat::Png)
                                .map_err(|e| e.to_string())?;
                            Ok(LinkPreviewResult {
                                image_data: buf.into_inner(),
                                center_ratio,
                            })
                        })();
                        let _ = resp.send(res);
                    }
                    PdfCommand::GetPageText(path, index, resp) => {
                        let res = (|| {
                            if current_document
                                .as_ref()
                                .map(|(p, _)| p != &path)
                                .unwrap_or(true)
                            {
                                let doc = pdfium
                                    .load_pdf_from_file(&path, None)
                                    .map_err(|e| format!("Failed to load PDF: {:?}", e))?;
                                current_document = Some((path.clone(), doc));
                            }
                            let Some((_, doc)) = current_document.as_ref() else {
                                return Err("PDF document was not loaded".to_string());
                            };
                            let pages = doc.pages();
                            if i32::from(index) >= pages.len() {
                                return Err("Page index out of bounds".to_string());
                            }
                            let page = pages.get(index as i32).map_err(|e| e.to_string())?;
                            build_page_text(&page, index)
                        })();
                        let _ = resp.send(res);
                    }
                }
            }
        });

        Ok(Self {
            sender,
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
        self.sender
            .send(PdfCommand::RenderPage(
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
        self.sender
            .send(PdfCommand::Wake)
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    pub fn page_count(&self, path: &str) -> Result<u16, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::PageCount(path.to_string(), tx))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    /// Extract the full plain text of a PDF (all pages, newline-separated).
    /// Used to feed PDF content into the vault's full-text search index.
    pub fn extract_document_text(&self, path: &str) -> Result<String, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::ExtractText(path.to_string(), tx))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    pub fn page_sizes(&self, path: &str) -> Result<Vec<(f32, f32)>, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::PageSizes(path.to_string(), tx))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    /// Returns the document's table of contents along with a flag that is
    /// `true` when the outline was synthesized from page text because the PDF
    /// has no embedded bookmarks.
    pub fn get_toc(&self, path: &str) -> Result<(Vec<TocEntry>, bool), String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::GetToc(path.to_string(), tx))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    /// Resolve internal cross-references (numbered equations, figures, tables,
    /// sections) for the whole document. This performs a one-time full text
    /// scan; callers should cache the result by document id (the result is
    /// stable for a given file). See [`crate::references`].
    pub fn get_references(
        &self,
        path: &str,
    ) -> Result<Vec<crate::references::ReferenceLink>, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::GetReferences(path.to_string(), tx))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    /// The embedded outline (bookmarks) only — no text scan, so cheap on every
    /// document. The chunked reference resolver uses this plus per-page text
    /// extraction to avoid a monolithic full-document command.
    pub fn get_embedded_toc(&self, path: &str) -> Result<Vec<TocEntry>, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::GetEmbeddedToc(path.to_string(), tx))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    pub fn get_page_links(&self, path: &str, page_index: u16) -> Result<Vec<LinkInfo>, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::GetLinks(path.to_string(), page_index, tx))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    pub fn search_text(
        &self,
        path: &str,
        query: &str,
        regex: bool,
        match_case: bool,
    ) -> Result<Vec<PdfSearchMatch>, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::SearchText(
                path.to_string(),
                query.to_string(),
                regex,
                match_case,
                tx,
            ))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    pub fn render_link_preview(
        &self,
        path: &str,
        page_index: u32,
        dest_y: Option<f32>,
        target_width_px: u32,
    ) -> Result<LinkPreviewResult, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::RenderLinkPreview(
                path.to_string(),
                page_index,
                dest_y,
                target_width_px,
                tx,
            ))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    pub fn get_page_text(&self, path: &str, page_index: u16) -> Result<PdfPageText, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::GetPageText(path.to_string(), page_index, tx))
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

/// The embedded outline (bookmarks) of a document, empty when there are none.
fn embedded_toc(doc: &PdfDocument) -> Vec<TocEntry> {
    let mut bookmarks = Vec::new();
    let mut current = doc.bookmarks().root();
    while let Some(bookmark) = current {
        current = bookmark.next_sibling();
        bookmarks.push(bookmark);
    }
    PdfRenderer::parse_bookmarks(&bookmarks)
}

/// Extract the text layer of every page, in page order. This is the one-time
/// full-document scan shared by TOC recovery and reference resolution.
fn build_all_page_texts(doc: &PdfDocument) -> Vec<PdfPageText> {
    let pages_handle = doc.pages();
    let page_count = pages_handle.len();
    let mut page_texts = Vec::new();
    for i in 0..page_count {
        let page = match pages_handle.get(i) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if let Ok(pt) = build_page_text(&page, i as u16) {
            page_texts.push(pt);
        }
    }
    page_texts
}

/// Recover an outline for a bookmark-less document from its page text, best
/// source first: a printed contents page via its link annotations, then its
/// dot-leader text, and only failing that a typographic heuristic.
fn recover_toc(doc: &PdfDocument, page_texts: &[PdfPageText]) -> Vec<TocEntry> {
    let toc_pages = detect_toc_pages(page_texts);
    if !toc_pages.is_empty() {
        let linked = harvest_linked_toc(doc, page_texts, &toc_pages);
        if linked.len() >= 2 {
            return linked;
        }
    }
    let printed = parse_printed_toc(page_texts);
    if printed.len() >= 2 {
        return printed;
    }
    synthesize_toc(page_texts)
}

/// Recover an outline from page text alone (no `PdfDocument`), for callers that
/// have already extracted the text and want section structure without a second
/// scan or the doc handle. Skips the link-annotation path (which needs the doc);
/// uses the printed-contents text parser, then the typographic heuristic.
pub fn recover_toc_from_texts(page_texts: &[PdfPageText]) -> Vec<TocEntry> {
    let printed = parse_printed_toc(page_texts);
    if printed.len() >= 2 {
        return printed;
    }
    synthesize_toc(page_texts)
}

/// Extract a page's text, per-character bounding boxes, and grouped text lines.
///
/// Shared by the `GetPageText` command and the auto-TOC synthesis path so the
/// line-grouping heuristic lives in exactly one place.
fn build_page_text(page: &PdfPage, index: u16) -> Result<PdfPageText, String> {
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

    // Group into lines
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

                let overlap = line_y_max.min(c_y_max) - line_y_min.max(c_y_min);
                let min_h = line.bbox.height.min(c.bbox.height);

                if overlap > 0.0 && overlap > 0.3 * min_h {
                    // Merge bbox
                    let x_min = line.bbox.x.min(c.bbox.x);
                    let x_max = (line.bbox.x + line.bbox.width).max(c.bbox.x + c.bbox.width);
                    let y_min = line.bbox.y.min(c.bbox.y);
                    let y_max = (line.bbox.y + line.bbox.height).max(c.bbox.y + c.bbox.height);

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
                        bbox: c.bbox.clone(),
                    });
                }
            }
            None => {
                current_line = Some(PdfTextLine {
                    start_text_index: c.text_index,
                    end_text_index: c.text_index + 1,
                    bbox: c.bbox.clone(),
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

/// Text of a single line, sliced out of the page's character buffer.
fn line_text(page: &PdfPageText, line: &PdfTextLine) -> String {
    page.text
        .chars()
        .skip(line.start_text_index)
        .take(line.end_text_index.saturating_sub(line.start_text_index))
        .collect::<String>()
        .trim()
        .to_string()
}

/// Nesting depth of a numbered-heading prefix, or `None` when the line does not
/// begin with a recognizable section number.
///
/// Recognizes dotted numeric prefixes (`1.`, `1.2`, `1.2.3`) and keyworded
/// headings (`Chapter 4`, `Section 2`, `Part III`, `Appendix B`). A bare number
/// with no dot (e.g. `2020 was a year`) is intentionally rejected to avoid
/// matching years and quantities. The returned depth drives heading nesting:
/// `1.` → 1, `1.2` → 2, `Chapter 4` → 1.
fn numbered_heading_depth(title: &str) -> Option<usize> {
    let t = title.trim_start();

    // Keyworded headings.
    let lower = t.to_lowercase();
    for kw in ["chapter ", "section ", "part ", "appendix "] {
        if lower.starts_with(kw) {
            let rest = t[kw.len()..].trim_start();
            if rest.chars().next().is_some_and(|c| c.is_alphanumeric()) {
                return Some(1);
            }
        }
    }

    // Dotted numeric prefix: runs of ASCII digits separated by '.'.
    let mut depth = 0usize;
    let mut saw_digit = false;
    let mut consumed = 0usize;
    for c in t.chars() {
        if c.is_ascii_digit() {
            saw_digit = true;
            consumed += 1;
        } else if c == '.' && saw_digit {
            depth += 1;
            saw_digit = false;
            consumed += 1;
        } else {
            break;
        }
    }
    if saw_digit {
        depth += 1; // trailing segment with no dot, e.g. the "2" in "1.2"
    }
    if depth == 0 {
        return None;
    }
    // The number must be followed by whitespace and real heading text.
    let after = &t[consumed..];
    let has_text = matches!(after.chars().next(), Some(c) if c.is_whitespace())
        && !after.trim().is_empty();
    if !has_text {
        return None;
    }
    if t[..consumed].contains('.') {
        // Dotted section number: "1." -> 1, "1.2" -> 2.
        return Some(depth);
    }
    // Bare integer with no dot. Accept as a top-level heading only when it is a
    // plausible section index (<= 2 digits, so years like "2020" are rejected)
    // followed by a capitalized word ("1 Introduction"), which separates real
    // headings from quantities ("1 apple") and sentences ("2020 was a year").
    if consumed <= 2 && after.trim_start().chars().next().is_some_and(|c| c.is_uppercase()) {
        return Some(1);
    }
    None
}

/// True when a line reads like a heading title rather than mathematics or
/// decorative glyphs. Rejects strings dominated by symbols and any containing
/// Private Use Area code points (font-specific math brackets, ligatures, icons),
/// which are a common source of junk entries on equation-heavy pages.
fn is_plausible_title(title: &str) -> bool {
    // A heading is a single line; embedded newlines/control chars mark captured
    // multi-line content such as a display equation, not a title. Private Use
    // Area code points are font-specific math brackets, ligatures, or icons.
    if title
        .chars()
        .any(|c| c.is_control() || ('\u{E000}'..='\u{F8FF}').contains(&c))
    {
        return false;
    }
    let letters = title.chars().filter(|c| c.is_alphabetic()).count();
    let non_space = title.chars().filter(|c| !c.is_whitespace()).count();
    // At least three letters, and letters must form the majority of glyphs.
    letters >= 3 && letters * 2 >= non_space
}

/// True for a short line set entirely in uppercase letters (e.g. `INTRODUCTION`),
/// a common heading style that carries no extra glyph height.
fn is_caps_heading(title: &str) -> bool {
    let letters = title.chars().filter(|c| c.is_alphabetic()).count();
    letters >= 3 && title.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase())
}

/// Vertical window, in PDF points, shown around a reference's destination in the
/// right-click link preview. ~⅓ of a US-Letter page: enough context to read the
/// referenced line/figure while keeping it comfortably zoomed.
const PREVIEW_WINDOW_PT: f32 = 300.0;

/// Maximum heading-nesting depth used across every TOC-recovery strategy.
const MAX_TOC_LEVELS: usize = 4;
/// How many leading pages to scan when looking for a printed contents page.
const MAX_TOC_SCAN: usize = 24;
/// Characters that act as TOC leaders between a title and its page number.
const LEADER_CHARS: [char; 5] = ['.', ' ', '\u{00b7}', '\u{2026}', '\t'];

/// Whitespace-collapsed, lowercased form of a heading for cross-page matching.
fn normalize_heading(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// True for a line that is itself the "Contents" / "Table of Contents" title.
fn is_contents_heading(text: &str) -> bool {
    let low = normalize_heading(text);
    low == "contents" || low == "table of contents" || low.starts_with("table of contents")
}

/// Strip a trailing dot-leader and page number from a heading-like string,
/// e.g. `"Introduction ........ 12"` -> `"Introduction"`. A trailing number is
/// only removed when preceded by a real leader (>= 2 dots), so titles that
/// legitimately end in a number (`"Chapter 5"`) are preserved.
fn strip_leader_tail(text: &str) -> String {
    let t = text.trim();
    let digits = t.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return t.to_string();
    }
    let head = &t[..t.len() - digits];
    let dots = head
        .chars()
        .rev()
        .take_while(|c| LEADER_CHARS.contains(c))
        .filter(|c| *c != ' ' && *c != '\t')
        .count();
    if dots >= 2 {
        head.trim_end_matches(|c: char| LEADER_CHARS.contains(&c))
            .trim()
            .to_string()
    } else {
        t.to_string()
    }
}

/// Clean the text under a TOC link annotation into a title. The link rect often
/// spans the whole row, so the captured text wraps across visual lines (joined
/// here with spaces) and ends with the right-column page number (dropped, since
/// the link's destination already gives the exact page).
fn clean_link_title(raw: &str) -> String {
    let mut parts: Vec<&str> = raw
        .split(['\n', '\r'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if parts
        .last()
        .is_some_and(|s| s.chars().all(|c| c.is_ascii_digit()))
    {
        parts.pop();
    }
    strip_leader_tail(&parts.join(" "))
}

/// Parse a printed table-of-contents line into `(title, printed_page_number)`.
///
/// Matches `Some Section Title .......... 23` and, with section numbering,
/// `1.2 Some Section .... 23`. A trailing integer is required. When
/// `require_leader` is true a run of >= 2 leader dots must separate the title
/// from the number, so ordinary prose ending in a number is not misread; this
/// is relaxed on pages already identified as a TOC (where space-only leaders
/// are common).
fn parse_leader_line(text: &str, require_leader: bool) -> Option<(String, u32)> {
    let t = text.trim();
    let digit_count = t.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digit_count == 0 || digit_count > 6 {
        return None;
    }
    let split = t.len() - digit_count; // digits are ASCII, so byte == char here
    let page: u32 = t[split..].parse().ok()?;
    let head = &t[..split];
    let dot_count = head
        .chars()
        .rev()
        .take_while(|c| LEADER_CHARS.contains(c))
        .filter(|c| *c != ' ' && *c != '\t')
        .count();
    if require_leader && dot_count < 2 {
        return None;
    }
    let title = head
        .trim_end_matches(|c: char| LEADER_CHARS.contains(&c))
        .trim();
    // A real entry has a title with letters (rejects dotted number-only rows).
    if title.is_empty() || !title.chars().any(|c| c.is_alphabetic()) {
        return None;
    }
    Some((title.to_string(), page))
}

/// Assemble a flat list of `(title, page_index, level)` entries (in document
/// order, `level` 0 = top) into a nested tree, attaching each entry under the
/// most recent entry at a strictly shallower level.
fn build_toc_tree(items: Vec<(String, Option<u32>, usize)>) -> Vec<TocEntry> {
    let mut roots: Vec<TocEntry> = Vec::new();
    // Path of child-indices from a root down to the last entry at each level.
    let mut path: Vec<(usize, Vec<usize>)> = Vec::new();
    for (title, page_index, level) in items {
        let entry = TocEntry {
            title,
            page_index,
            children: Vec::new(),
        };
        while path.last().map(|(l, _)| *l >= level).unwrap_or(false) {
            path.pop();
        }
        let new_path = if let Some((_, parent_path)) = path.last().cloned() {
            let mut node = &mut roots;
            for &idx in &parent_path {
                node = &mut node[idx].children;
            }
            node.push(entry);
            let mut p = parent_path;
            p.push(node.len() - 1);
            p
        } else {
            roots.push(entry);
            vec![roots.len() - 1]
        };
        path.push((level, new_path));
    }
    roots
}

/// Distinct left-edge indents (ascending, merged within 2pt, capped at
/// [`MAX_TOC_LEVELS`]) used to map an entry's x position to a nesting level.
fn indent_bands(indents: &[f32]) -> Vec<f32> {
    let mut v: Vec<f32> = indents.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v.dedup_by(|a, b| (*a - *b).abs() < 2.0);
    v.truncate(MAX_TOC_LEVELS);
    v
}

/// Nesting level (0 = outermost) for an entry at left edge `x`.
fn level_for_indent(bands: &[f32], x: f32) -> usize {
    bands
        .iter()
        .rposition(|&i| x >= i - 2.0)
        .unwrap_or(0)
        .min(bands.len().saturating_sub(1))
}

/// Level for a flat entry: its section-number depth when numbered, else its
/// indent band. Shared by the link- and text-based printed-TOC strategies.
fn entry_level(title: &str, indent: f32, bands: &[f32]) -> usize {
    numbered_heading_depth(title)
        .map(|d| (d - 1).min(MAX_TOC_LEVELS - 1))
        .unwrap_or_else(|| level_for_indent(bands, indent))
}

/// One recovered contents-page entry. `real` is true when the page number came
/// from an actual pairing (embedded leader or number column) rather than being
/// inherited by a standalone heading whose number failed to pair.
struct TocRow {
    title: String,
    page: u32,
    indent: f32,
    real: bool,
}

/// Extract entries from one contents page.
///
/// Handles two layouts that defeat a naive per-line parse:
///  * **Dot leaders** — `Some Title .......... 23` on a single line.
///  * **Two-column** — the page number sits in a separate right-hand column on
///    the *same row* as its title (no leader at all), which is how most
///    professionally typeset documents lay out their contents.
///
/// Title lines whose row carries no page number are treated as wrapped
/// continuations and merged into the preceding entry.
fn extract_toc_entries(page: &PdfPageText) -> Vec<TocRow> {
    struct Line {
        text: String,
        x: f32,
        y: f32,
        h: f32,
        num: Option<u32>,
    }
    // A page-number column lives in the right portion of the page; section
    // numbers and footers do not, so left-half numerics are ignored.
    let right_min = page.page_width * 0.5;
    let mut lines: Vec<Line> = Vec::new();
    let mut num_col: Vec<(f32, u32, f32)> = Vec::new(); // (y, value, height)
    for line in &page.lines {
        let t = line_text(page, line);
        if t.is_empty() || is_contents_heading(&t) {
            continue;
        }
        if let Ok(v) = t.parse::<u32>() {
            if t.len() <= 4 && line.bbox.x > right_min {
                num_col.push((line.bbox.y, v, line.bbox.height));
            }
            continue; // a numeric-only line is never a title
        }
        lines.push(Line {
            text: t,
            x: line.bbox.x,
            y: line.bbox.y,
            h: line.bbox.height,
            num: None,
        });
    }
    let has_column = !num_col.is_empty();

    // Resolve each line's title and (when present) its page number: from an
    // embedded leader, else from the number column at the same row. An embedded
    // number always requires a real dot leader — without one, a trailing integer
    // is far more likely a year, equation, or reference number (as in a paper's
    // bibliography) than a page reference.
    let mut titled: Vec<Line> = Vec::with_capacity(lines.len());
    for mut l in lines {
        if let Some((title, n)) = parse_leader_line(&l.text, true) {
            l.text = title;
            l.num = Some(n);
        } else if has_column {
            let tol = (l.h.max(1.0) * 0.7).max(2.0);
            l.num = num_col
                .iter()
                .find(|(ny, _, nh)| (ny - l.y).abs() <= tol.max(nh * 0.7))
                .map(|(_, v, _)| *v);
        }
        titled.push(l);
    }

    // Walk rows top-to-bottom; a numbered row starts an entry, an unnumbered row
    // continues the previous one.
    titled.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<TocRow> = Vec::new();
    let mut pending: Option<TocRow> = None;
    for l in titled {
        match l.num {
            Some(n) => {
                if let Some(p) = pending.take() {
                    out.push(p);
                }
                pending = Some(TocRow {
                    title: l.text.trim().to_string(),
                    page: n,
                    indent: l.x,
                    real: true,
                });
            }
            None => {
                // A row with no page number is normally a wrapped continuation
                // of the entry above it. But if it is plainly a heading in its
                // own right (numbered or ALL-CAPS), keep it as its own entry —
                // its number simply failed to pair — inheriting the previous
                // entry's page so it stays navigable. Inherited entries are
                // flagged `real = false` so they don't count toward TOC-page
                // detection (front-matter prose has such subheadings too).
                let standalone = numbered_heading_depth(&l.text).is_some()
                    || is_caps_heading(l.text.trim());
                if standalone {
                    let page = pending.as_ref().map(|p| p.page).unwrap_or(0);
                    if let Some(p) = pending.take() {
                        out.push(p);
                    }
                    pending = Some(TocRow {
                        title: l.text.trim().to_string(),
                        page,
                        indent: l.x,
                        real: false,
                    });
                } else if let Some(p) = pending.as_mut() {
                    p.title.push(' ');
                    p.title.push_str(l.text.trim());
                }
            }
        }
    }
    if let Some(p) = pending.take() {
        out.push(p);
    }
    out
}

/// Indices of leading pages that look like a printed table of contents: a page
/// carrying a "Contents" heading alongside entries, or one with a strong run of
/// page-numbered entries (covers multi-page TOCs with no repeated heading).
fn detect_toc_pages(pages: &[PdfPageText]) -> Vec<usize> {
    let scan = pages.len().min(MAX_TOC_SCAN);
    let mut candidates: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut heading_candidate: Option<usize> = None;
    for (i, page) in pages.iter().enumerate().take(scan) {
        let has_heading = page
            .lines
            .iter()
            .any(|l| is_contents_heading(&line_text(page, l)));
        // Count only entries with a genuinely paired page number; inherited
        // standalone headings (common in front-matter prose) don't qualify.
        let real = extract_toc_entries(page)
            .iter()
            .filter(|e| e.real)
            .count();
        if (has_heading && real >= 2) || real >= 5 {
            candidates.insert(i);
            if has_heading && heading_candidate.is_none() {
                heading_candidate = Some(i);
            }
        }
    }
    // A real TOC occupies a contiguous run of pages; isolated later matches are
    // body pages that merely contain numbered lists/figures. Anchor on the first
    // "Contents" page when present (else the first candidate) and extend forward
    // only while pages stay TOC-like.
    let Some(anchor) = heading_candidate.or_else(|| candidates.iter().min().copied()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut p = anchor;
    while candidates.contains(&p) {
        out.push(p);
        p += 1;
    }
    out
}

/// Learn the constant offset `physical_index - printed_page_number` by matching
/// a few TOC titles to headings on the body pages they point at. Returns `None`
/// unless at least two entries agree, so a single coincidental match cannot
/// misalign the whole outline.
fn learn_page_delta(pages: &[PdfPageText], last_toc: usize, entries: &[(&str, u32)]) -> Option<i64> {
    let mut votes: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
    for (title, printed) in entries.iter().take(40) {
        let norm = normalize_heading(title);
        if norm.len() < 4 {
            continue;
        }
        for page in pages
            .iter()
            .filter(|p| (p.page_index as usize) > last_toc)
        {
            let matched = page
                .lines
                .iter()
                .any(|l| normalize_heading(&line_text(page, l)) == norm);
            if matched {
                let delta = page.page_index as i64 - *printed as i64;
                *votes.entry(delta).or_default() += 1;
                break; // first matching page wins for this entry
            }
        }
    }
    votes
        .into_iter()
        .filter(|(_, c)| *c >= 2)
        .max_by_key(|(_, c)| *c)
        .map(|(d, _)| d)
}

/// Recover a printed table of contents from its link annotations: for each link
/// on a detected TOC page, the anchored text becomes the title and the link's
/// destination gives the exact page index. The most reliable strategy when it
/// applies, since the page mapping is taken from the document, not guessed.
fn harvest_linked_toc(
    doc: &PdfDocument,
    page_texts: &[PdfPageText],
    toc_pages: &[usize],
) -> Vec<TocEntry> {
    let toc_set: std::collections::HashSet<usize> = toc_pages.iter().copied().collect();
    let pages = doc.pages();
    struct Raw {
        title: String,
        dest: u32,
        indent: f32,
        page_rank: usize, // position of the TOC page in reading order
        top: f32,         // top edge, for top-to-bottom ordering within a page
    }
    let mut raws: Vec<Raw> = Vec::new();
    let mut seen: std::collections::HashSet<(String, u32)> = std::collections::HashSet::new();
    for (rank, &pi) in toc_pages.iter().enumerate() {
        let page = match pages.get(pi as i32) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let ptext = page_texts.iter().find(|p| p.page_index as usize == pi);
        for link in page.links().iter() {
            let rect = match link.rect() {
                Ok(r) => r,
                Err(_) => continue,
            };
            let mut dest_page: Option<u32> = None;
            if let Some(PdfAction::LocalDestination(ref local)) = link.action() {
                if let Ok(dest) = local.destination() {
                    dest_page = dest.page_index().ok().map(|i| i as u32);
                }
            }
            if dest_page.is_none() {
                if let Some(dest) = link.destination() {
                    dest_page = dest.page_index().ok().map(|i| i as u32);
                }
            }
            let Some(dest) = dest_page else {
                continue;
            };
            if toc_set.contains(&(dest as usize)) {
                continue; // intra-TOC navigation link, not an entry
            }
            let Some(pt) = ptext else { continue };
            let title = clean_link_title(&text_in_rect(
                pt,
                rect.left().value,
                rect.bottom().value,
                rect.right().value,
                rect.top().value,
            ));
            if !is_plausible_title(&title) {
                continue;
            }
            if !seen.insert((title.clone(), dest)) {
                continue;
            }
            raws.push(Raw {
                title,
                dest,
                indent: rect.left().value,
                page_rank: rank,
                top: rect.top().value,
            });
        }
    }
    if raws.len() < 2 {
        return Vec::new();
    }
    // Link annotations are not necessarily stored in reading order, so sort into
    // it (page, then top-to-bottom) before nesting by indent.
    raws.sort_by(|a, b| {
        a.page_rank.cmp(&b.page_rank).then(
            b.top
                .partial_cmp(&a.top)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    let bands = indent_bands(&raws.iter().map(|r| r.indent).collect::<Vec<_>>());
    let flat = raws
        .into_iter()
        .map(|r| {
            let level = entry_level(&r.title, r.indent, &bands);
            (r.title, Some(r.dest), level)
        })
        .collect();
    build_toc_tree(flat)
}

/// Recover a printed table of contents from its text: parse dot-leader lines on
/// detected TOC pages into `(title, printed page number)`, then map printed
/// numbers to physical indices via a learned offset ([`learn_page_delta`]).
/// Pure and windowless so it can be unit-tested without pdfium.
pub fn parse_printed_toc(pages: &[PdfPageText]) -> Vec<TocEntry> {
    let toc_pages = detect_toc_pages(pages);
    let Some(&last_toc) = toc_pages.iter().max() else {
        return Vec::new();
    };
    let mut raws: Vec<TocRow> = Vec::new();
    for &pi in &toc_pages {
        raws.extend(extract_toc_entries(&pages[pi]));
    }
    raws.retain(|r| is_plausible_title(&r.title));
    if raws.len() < 2 {
        return Vec::new();
    }
    // Map printed page numbers to physical indices. Prefer an offset learned by
    // matching titles to the body headings they point at; otherwise assume the
    // content begins on the page right after the contents (a sound default for
    // documents whose body headings don't match their TOC text verbatim).
    let entries: Vec<(&str, u32)> = raws.iter().map(|r| (r.title.as_str(), r.page)).collect();
    let min_printed = raws.iter().map(|r| r.page).min().unwrap_or(1);
    let delta = learn_page_delta(pages, last_toc, &entries)
        .unwrap_or_else(|| last_toc as i64 + 1 - min_printed as i64);

    let bands = indent_bands(&raws.iter().map(|r| r.indent).collect::<Vec<_>>());
    let total_pages = pages.len() as i64;
    let flat: Vec<(String, Option<u32>, usize)> = raws
        .into_iter()
        .map(|r| {
            let phys = (r.page as i64 + delta).clamp(0, total_pages - 1) as u32;
            let level = entry_level(&r.title, r.indent, &bands);
            (r.title, Some(phys), level)
        })
        .collect();
    // A genuine table of contents references at least a few distinct places in
    // the document. When the recovered targets collapse onto one or two pages,
    // the "TOC" is really a bibliography/index whose numbers are years or
    // citation indices (not pages), or a misdetected body page — discard it.
    let distinct: std::collections::HashSet<Option<u32>> =
        flat.iter().map(|(_, p, _)| *p).collect();
    if distinct.len() < 3 {
        return Vec::new();
    }
    build_toc_tree(flat)
}

/// Concatenate, in reading order, the text of every character whose center
/// falls inside the given rect (PDF points, bottom-left origin). Used to read
/// the title a TOC link annotation sits on top of.
fn text_in_rect(page: &PdfPageText, left: f32, bottom: f32, right: f32, top: f32) -> String {
    let mut s = String::new();
    for c in &page.chars {
        let cx = c.bbox.x + c.bbox.width * 0.5;
        let cy = c.bbox.y + c.bbox.height * 0.5;
        if cx >= left && cx <= right && cy >= bottom && cy <= top {
            s.push(c.ch);
        }
    }
    s.trim().to_string()
}

/// Synthesize a table of contents from page text when a PDF has no embedded
/// bookmark tree.
///
/// A line is treated as a heading when any signal fires: it is meaningfully
/// taller than the document's body text, it begins with a section number
/// (`1.2`, `Chapter 4`), or it is a short ALL-CAPS line. Numbered headings nest
/// by their numbering depth; the rest nest by relative font size. Running
/// headers/footers (identical text repeated on many pages) are dropped.
///
/// When no headings are found at all, a multi-page document falls back to a
/// per-page outline (each page labeled by its most prominent line) so the
/// reader always gets something faster than scrolling. The result flows through
/// the same downstream pipeline as embedded bookmarks.
///
/// Pure and windowless so it can be unit-tested without pdfium.
pub fn synthesize_toc(pages: &[PdfPageText]) -> Vec<TocEntry> {
    const HEADING_RATIO: f32 = 1.15;
    const MAX_HEADING_WORDS: usize = 12;
    const HEADER_REPEAT_FRACTION: f32 = 0.25;
    const MAX_LEVELS: usize = 4;
    const MAX_ENTRIES: usize = 500;
    const MAX_LABEL_CHARS: usize = 80;

    if pages.is_empty() {
        return Vec::new();
    }

    // Collect every line's height to find the document body size.
    let mut heights: Vec<f32> = Vec::new();
    for page in pages {
        for line in &page.lines {
            if line.bbox.height > 0.0 {
                heights.push(line.bbox.height);
            }
        }
    }
    if heights.is_empty() {
        return Vec::new();
    }

    // Body size = median line height (robust against a few large headings).
    let body_size = {
        let mut sorted = heights.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        sorted[sorted.len() / 2]
    };
    let heading_threshold = body_size * HEADING_RATIO;

    // Count identical text occurrences across pages to spot running
    // headers/footers, which should never become TOC entries.
    let mut text_page_counts: std::collections::HashMap<String, std::collections::HashSet<u16>> =
        std::collections::HashMap::new();
    for page in pages {
        for line in &page.lines {
            let t = line_text(page, line);
            if !t.is_empty() {
                text_page_counts
                    .entry(t)
                    .or_default()
                    .insert(page.page_index);
            }
        }
    }
    let repeat_page_threshold =
        ((pages.len() as f32) * HEADER_REPEAT_FRACTION).ceil().max(2.0) as usize;
    let is_running_header = |title: &str| -> bool {
        text_page_counts
            .get(title)
            .map(|set| set.len() >= repeat_page_threshold)
            .unwrap_or(false)
    };

    // Gather candidate headings. `level` is `Some` when known from numbering,
    // otherwise resolved from font size after the sweep.
    struct Candidate {
        title: String,
        page_index: u32,
        size: f32,
        level: Option<usize>,
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    'outer: for page in pages {
        for line in &page.lines {
            let title = line_text(page, line);
            if title.is_empty() || title.split_whitespace().count() > MAX_HEADING_WORDS {
                continue;
            }
            if is_running_header(&title) || !is_plausible_title(&title) {
                continue;
            }
            let size = line.bbox.height;
            let num_depth = numbered_heading_depth(&title);
            let by_size = size >= heading_threshold;
            let by_caps = is_caps_heading(&title) && size >= body_size * 0.95;
            // A section number alone isn't enough: enumerated body lists ("1.
            // Zero-order approximation…", "2. The remainder…") share the form of
            // a numbered heading. Accept numbering only for a short, non-prose
            // line (real headings don't run to a sentence ending in a period).
            let by_number = num_depth.is_some()
                && title.split_whitespace().count() <= 8
                && !title.trim_end().ends_with('.');
            if !by_size && !by_caps && !by_number {
                continue;
            }
            candidates.push(Candidate {
                title,
                page_index: page.page_index as u32,
                size,
                level: num_depth.map(|d| (d - 1).min(MAX_LEVELS - 1)),
            });
            if candidates.len() >= MAX_ENTRIES {
                break 'outer;
            }
        }
    }

    // A genuine document outline is spread across the document. When many
    // "headings" instead cluster on just a page or two, they are really an
    // enumerated list — a reference section, index, or glossary — or scan/math
    // noise, not a table of contents. Fabricating an outline from those is worse
    // than offering none, so bail rather than emit a page-long bogus TOC.
    let distinct_pages: std::collections::HashSet<u32> =
        candidates.iter().map(|c| c.page_index).collect();
    if candidates.len() >= 6 && distinct_pages.len() <= 2 {
        return Vec::new();
    }
    // Conversely, a real outline is sparse — at most a heading or two per page.
    // When candidates are far denser than that, the heuristic is misfiring on an
    // equation-, theorem-, or caption-heavy document (typical of papers with no
    // real contents page); an empty outline beats a hundreds-of-entries dump.
    if candidates.len() > 2 * pages.len() + 4 {
        return Vec::new();
    }

    if candidates.is_empty() {
        // Fallback: a per-page outline, but only when there are several pages to
        // navigate (a one-entry TOC for a single page is pointless).
        if pages.len() < 2 {
            return Vec::new();
        }
        let mut out: Vec<TocEntry> = Vec::new();
        for page in pages {
            let label = page
                .lines
                .iter()
                .map(|line| (line.bbox.height, line_text(page, line)))
                .filter(|(_, t)| !t.is_empty() && !is_running_header(t) && is_plausible_title(t))
                .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(_, t)| t)
                .unwrap_or_else(|| format!("Page {}", page.page_index + 1));
            let label: String = label.chars().take(MAX_LABEL_CHARS).collect();
            out.push(TocEntry {
                title: label,
                page_index: Some(page.page_index as u32),
                children: Vec::new(),
            });
            if out.len() >= MAX_ENTRIES {
                break;
            }
        }
        return out;
    }

    // Cluster distinct heading sizes (descending) into <= MAX_LEVELS levels.
    let mut distinct_sizes: Vec<f32> = candidates.iter().map(|c| c.size).collect();
    distinct_sizes.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    distinct_sizes.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    distinct_sizes.truncate(MAX_LEVELS);

    // Map a heading size to a level (0 = largest/top). Larger or equal to the
    // n-th distinct size means level n.
    let level_for = |size: f32| -> usize {
        for (i, s) in distinct_sizes.iter().enumerate() {
            if size >= *s - 0.5 {
                return i;
            }
        }
        distinct_sizes.len().saturating_sub(1)
    };

    // Numbered headings nest by their numbering depth; everything else by
    // relative font size. Build the tree with the shared stack-based assembler.
    let flat = candidates
        .into_iter()
        .map(|c| {
            let level = c.level.unwrap_or_else(|| level_for(c.size));
            (c.title, Some(c.page_index), level)
        })
        .collect();
    build_toc_tree(flat)
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
        let mut candidates = Vec::new();

        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("resources").join(&lib_name));
                candidates.push(dir.join(&lib_name));
            }
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
    fn test_pdf_render_bucket() {
        // Exact powers of 1.4 land on their own bucket.
        assert_eq!(pdf_render_bucket(1.0), 1.0);
        assert!((pdf_render_bucket(1.4) - 1.4).abs() < 1e-4);

        // Zoom levels round up to the next bucket (never upscaled > 1.4x).
        assert!((pdf_render_bucket(1.1) - 1.4).abs() < 1e-4);
        assert!((pdf_render_bucket(1.5) - 1.4 * 1.4).abs() < 1e-3);

        // A whole band of nearby zooms shares one bucket, so they reuse the
        // same cached bitmap instead of forcing re-renders.
        let b = pdf_render_bucket(1.05);
        assert_eq!(b, pdf_render_bucket(1.2));
        assert_eq!(b, pdf_render_bucket(1.4));

        // Buckets stay >= zoom (so the bitmap is never upscaled), within
        // 1.4x (so it is never wastefully oversampled).
        for &z in &[0.5_f32, 0.75, 0.9, 1.0, 1.3, 2.0, 3.3, 4.0] {
            let s = pdf_render_bucket(z);
            assert!(s >= z - 1e-4, "bucket {s} below zoom {z}");
            assert!(s < z * 1.4 + 1e-4, "bucket {s} too far above zoom {z}");
        }
    }

    // --- synthesize_toc (windowless: no pdfium) -------------------------

    /// Build a `PdfPageText` from `(text, height)` line specs. The page `text`
    /// is the lines joined by '\n', and each `PdfTextLine` spans its slice with
    /// a bbox of the given height.
    fn page_fixture(page_index: u16, lines: &[(&str, f32)]) -> PdfPageText {
        let mut text = String::new();
        let mut text_lines = Vec::new();
        for (i, (s, h)) in lines.iter().enumerate() {
            if i > 0 {
                text.push('\n');
            }
            let start = text.chars().count();
            text.push_str(s);
            let end = text.chars().count();
            text_lines.push(PdfTextLine {
                start_text_index: start,
                end_text_index: end,
                bbox: PdfRect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: *h,
                },
            });
        }
        PdfPageText {
            page_index,
            page_width: 600.0,
            page_height: 800.0,
            text,
            chars: Vec::new(),
            lines: text_lines,
        }
    }

    /// Build a `PdfPageText` from `(text, x, y, height)` line specs, for tests
    /// that need real geometry (column layouts, row pairing).
    fn page_fixture_xy(page_index: u16, lines: &[(&str, f32, f32, f32)]) -> PdfPageText {
        let mut text = String::new();
        let mut text_lines = Vec::new();
        for (i, (s, x, y, h)) in lines.iter().enumerate() {
            if i > 0 {
                text.push('\n');
            }
            let start = text.chars().count();
            text.push_str(s);
            let end = text.chars().count();
            text_lines.push(PdfTextLine {
                start_text_index: start,
                end_text_index: end,
                bbox: PdfRect {
                    x: *x,
                    y: *y,
                    width: 100.0,
                    height: *h,
                },
            });
        }
        PdfPageText {
            page_index,
            page_width: 595.0,
            page_height: 842.0,
            text,
            chars: Vec::new(),
            lines: text_lines,
        }
    }

    fn flatten_titles(entries: &[TocEntry], out: &mut Vec<String>) {
        for e in entries {
            out.push(e.title.clone());
            flatten_titles(&e.children, out);
        }
    }

    #[test]
    fn test_synthesize_toc_picks_large_lines() {
        // Body text at 10pt, two headings at 16pt.
        let pages = vec![
            page_fixture(
                0,
                &[
                    ("Big Heading One", 16.0),
                    ("some body text here", 10.0),
                    ("more body text here", 10.0),
                ],
            ),
            page_fixture(
                1,
                &[
                    ("Big Heading Two", 16.0),
                    ("yet more body text", 10.0),
                ],
            ),
        ];
        let toc = synthesize_toc(&pages);
        let mut titles = Vec::new();
        flatten_titles(&toc, &mut titles);
        assert_eq!(titles, vec!["Big Heading One", "Big Heading Two"]);
        assert_eq!(toc[0].page_index, Some(0));
        assert_eq!(toc[1].page_index, Some(1));
    }

    #[test]
    fn test_synthesize_toc_drops_running_header() {
        // A 16pt running header repeats on every page and must not appear,
        // while a genuine 16pt heading on page 1 does.
        let mut pages = Vec::new();
        for p in 0..4u16 {
            let mut lines = vec![
                ("My Document Title", 16.0), // running header on every page
                ("body line one here", 10.0),
                ("body line two here", 10.0),
            ];
            if p == 1 {
                lines.insert(1, ("Introduction", 16.0));
            }
            pages.push(page_fixture(p, &lines));
        }
        let toc = synthesize_toc(&pages);
        let mut titles = Vec::new();
        flatten_titles(&toc, &mut titles);
        assert!(
            !titles.iter().any(|t| t == "My Document Title"),
            "running header should be dropped, got {titles:?}"
        );
        assert!(titles.iter().any(|t| t == "Introduction"));
    }

    #[test]
    fn test_synthesize_toc_levels_nest() {
        // 18pt > 14pt > 10pt body: the 14pt heading nests under the 18pt one.
        let pages = vec![page_fixture(
            0,
            &[
                ("Chapter", 18.0),
                ("Section", 14.0),
                ("body text content", 10.0),
                ("more body content", 10.0),
                ("even more content", 10.0),
                ("still more content", 10.0),
            ],
        )];
        let toc = synthesize_toc(&pages);
        assert_eq!(toc.len(), 1);
        assert_eq!(toc[0].title, "Chapter");
        assert_eq!(toc[0].children.len(), 1);
        assert_eq!(toc[0].children[0].title, "Section");
    }

    #[test]
    fn test_synthesize_toc_empty_when_uniform() {
        // All lines the same size => no headings.
        let pages = vec![page_fixture(
            0,
            &[
                ("line one here", 10.0),
                ("line two here", 10.0),
                ("line three here", 10.0),
            ],
        )];
        assert!(synthesize_toc(&pages).is_empty());
    }

    #[test]
    fn test_numbered_heading_depth() {
        assert_eq!(numbered_heading_depth("1. Introduction"), Some(1));
        assert_eq!(numbered_heading_depth("1.2 Background"), Some(2));
        assert_eq!(numbered_heading_depth("1.2.3 Details"), Some(3));
        assert_eq!(numbered_heading_depth("Chapter 4 Results"), Some(1));
        assert_eq!(numbered_heading_depth("Appendix B"), Some(1));
        // Bare integer + capitalized word is a common chapter style.
        assert_eq!(numbered_heading_depth("1 Introduction"), Some(1));
        assert_eq!(numbered_heading_depth("12 Conclusions"), Some(1));
        // Bare numbers / years / quantities must NOT count as headings.
        assert_eq!(numbered_heading_depth("2020 was a year"), None);
        assert_eq!(numbered_heading_depth("1 apple"), None);
        assert_eq!(numbered_heading_depth("1.5kg of flour"), None);
        assert_eq!(numbered_heading_depth("just text"), None);
    }

    // --- printed-TOC recovery (windowless) -----------------------------

    #[test]
    fn test_parse_leader_line() {
        // Dotted leader with a page number.
        assert_eq!(
            parse_leader_line("Introduction ......... 12", true),
            Some(("Introduction".to_string(), 12))
        );
        // Space-only leader is accepted only when leaders are not required.
        assert_eq!(
            parse_leader_line("Methods            34", false),
            Some(("Methods".to_string(), 34))
        );
        assert_eq!(parse_leader_line("Methods            34", true), None);
        // Prose ending in a number is rejected when a leader is required.
        assert_eq!(parse_leader_line("we saw 3 cats", true), None);
        // A title needs letters, so a number-only row is rejected.
        assert_eq!(parse_leader_line("12 ..... 34", true), None);
    }

    #[test]
    fn test_strip_leader_tail() {
        assert_eq!(strip_leader_tail("Introduction ...... 12"), "Introduction");
        // No real leader: a title that ends in a number is preserved.
        assert_eq!(strip_leader_tail("Chapter 5"), "Chapter 5");
        assert_eq!(strip_leader_tail("Conclusion"), "Conclusion");
    }

    #[test]
    fn test_detect_toc_pages() {
        let pages = vec![
            page_fixture(
                0,
                &[
                    ("Contents", 16.0),
                    ("Introduction ...... 1", 10.0),
                    ("Methods ...... 3", 10.0),
                ],
            ),
            page_fixture(1, &[("Introduction", 14.0), ("body here", 10.0)]),
        ];
        assert_eq!(detect_toc_pages(&pages), vec![0]);
    }

    #[test]
    fn test_parse_printed_toc_maps_pages_via_learned_delta() {
        // A "Contents" page lists two entries by their printed page numbers; the
        // body pages carry matching headings, so the printed->physical offset is
        // learned (here delta = 0) and applied.
        let pages = vec![
            page_fixture(
                0,
                &[
                    ("Contents", 16.0),
                    ("Introduction ......... 1", 10.0),
                    ("Methods ......... 3", 10.0),
                    ("Results ......... 5", 10.0),
                ],
            ),
            page_fixture(1, &[("Introduction", 14.0), ("body text here", 10.0)]),
            page_fixture(2, &[("filler page text", 10.0)]),
            page_fixture(3, &[("Methods", 14.0), ("more body text", 10.0)]),
            page_fixture(4, &[("more filler text", 10.0)]),
            page_fixture(5, &[("Results", 14.0), ("yet more body", 10.0)]),
        ];
        let toc = parse_printed_toc(&pages);
        let mut titles = Vec::new();
        flatten_titles(&toc, &mut titles);
        assert_eq!(titles, vec!["Introduction", "Methods", "Results"]);
        assert_eq!(toc[0].page_index, Some(1));
        assert_eq!(toc[1].page_index, Some(3));
        assert_eq!(toc[2].page_index, Some(5));
    }

    #[test]
    fn test_parse_printed_toc_two_column_layout() {
        // The real-world failure: a contents page whose page numbers sit in a
        // separate right-hand column on the same row as the title (no leaders),
        // including a wrapped title that spans two rows.
        let pages = vec![
            page_fixture_xy(
                0,
                &[
                    ("Contents", 51.0, 744.0, 46.0),
                    ("Introduction", 51.0, 704.0, 14.0),
                    ("1", 539.0, 704.0, 14.0),
                    ("Methods", 51.0, 663.0, 14.0),
                    ("3", 538.0, 663.0, 14.0),
                    ("Advanced topics in", 51.0, 642.0, 14.0),
                    ("5", 538.0, 642.0, 14.0),
                    ("computing", 87.0, 629.0, 14.0), // wrapped continuation, no number
                ],
            ),
            page_fixture_xy(1, &[("Introduction", 51.0, 700.0, 20.0)]),
            page_fixture_xy(2, &[("filler", 51.0, 700.0, 10.0)]),
            page_fixture_xy(3, &[("Methods", 51.0, 700.0, 20.0)]),
            page_fixture_xy(4, &[("filler", 51.0, 700.0, 10.0)]),
            page_fixture_xy(5, &[("Advanced topics in computing", 51.0, 700.0, 20.0)]),
        ];
        let toc = parse_printed_toc(&pages);
        let mut titles = Vec::new();
        flatten_titles(&toc, &mut titles);
        assert_eq!(
            titles,
            vec!["Introduction", "Methods", "Advanced topics in computing"]
        );
        assert_eq!(toc[0].page_index, Some(1));
        assert_eq!(toc[1].page_index, Some(3));
        assert_eq!(toc[2].page_index, Some(5));
    }

    #[test]
    fn test_parse_printed_toc_positional_fallback() {
        // No body heading matches the entries, so the offset can't be learned;
        // the parser falls back to "content starts right after the contents
        // page" (here delta 0) rather than discarding the recovered titles.
        let pages = vec![
            page_fixture(
                0,
                &[
                    ("Contents", 16.0),
                    ("Alpha ......... 1", 10.0),
                    ("Bravo ......... 3", 10.0),
                    ("Charlie ......... 5", 10.0),
                ],
            ),
            page_fixture(1, &[("unrelated text", 10.0)]),
            page_fixture(2, &[("more unrelated text", 10.0)]),
            page_fixture(3, &[("still unrelated", 10.0)]),
            page_fixture(4, &[("yet more text", 10.0)]),
            page_fixture(5, &[("final unrelated", 10.0)]),
        ];
        let toc = parse_printed_toc(&pages);
        let mut titles = Vec::new();
        flatten_titles(&toc, &mut titles);
        assert_eq!(titles, vec!["Alpha", "Bravo", "Charlie"]);
        assert_eq!(toc[0].page_index, Some(1));
        assert_eq!(toc[1].page_index, Some(3));
        assert_eq!(toc[2].page_index, Some(5));
    }

    #[test]
    fn test_synthesize_toc_numbered_same_size() {
        // Headings share the body font size but are detected by numbering, and
        // nest by numbering depth.
        let pages = vec![page_fixture(
            0,
            &[
                ("1. Introduction", 10.0),
                ("1.1 Background here", 10.0),
                ("body text content", 10.0),
                ("more body content", 10.0),
                ("2. Methods", 10.0),
            ],
        )];
        let toc = synthesize_toc(&pages);
        assert_eq!(toc.len(), 2, "two top-level numbered headings");
        assert_eq!(toc[0].title, "1. Introduction");
        assert_eq!(toc[0].children.len(), 1);
        assert_eq!(toc[0].children[0].title, "1.1 Background here");
        assert_eq!(toc[1].title, "2. Methods");
    }

    #[test]
    fn test_synthesize_toc_caps_heading_same_size() {
        // ALL-CAPS short line at body size is treated as a heading.
        let pages = vec![page_fixture(
            0,
            &[
                ("INTRODUCTION", 10.0),
                ("the body text here", 10.0),
                ("still more body text", 10.0),
            ],
        )];
        let toc = synthesize_toc(&pages);
        let mut titles = Vec::new();
        flatten_titles(&toc, &mut titles);
        assert_eq!(titles, vec!["INTRODUCTION"]);
    }

    #[test]
    fn test_synthesize_toc_per_page_fallback() {
        // No size/number/caps signal across multiple pages => per-page outline,
        // one entry per page labeled by its most prominent (tallest) line, with
        // the running header excluded.
        let pages = vec![
            page_fixture(
                0,
                &[("Running Header", 10.0), ("alpha content line", 12.0)],
            ),
            page_fixture(
                1,
                &[("Running Header", 10.0), ("bravo content line", 12.0)],
            ),
        ];
        let toc = synthesize_toc(&pages);
        assert_eq!(toc.len(), 2);
        assert_eq!(toc[0].title, "alpha content line");
        assert_eq!(toc[0].page_index, Some(0));
        assert_eq!(toc[1].title, "bravo content line");
        assert_eq!(toc[1].page_index, Some(1));
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

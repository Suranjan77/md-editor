use image::DynamicImage;
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

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
        std::sync::mpsc::SyncSender<Result<LinkPreviewResult, String>>,
    ),
}

impl PdfRenderer {
    pub fn new() -> Result<Self, String> {
        let (sender, receiver) = std::sync::mpsc::channel();
        let (priority_sender, priority_receiver) = std::sync::mpsc::channel::<PriorityRender>();

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
                        let res = render_page_from_cache(
                            &pdfium,
                            &mut current_document,
                            &path,
                            index,
                            scale,
                        );
                        let _ = resp.send(res);
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
                                                 let match_char_idx_in_text = page_text[..found.start()].chars().count();
                                                 let match_char_count = found.as_str().chars().count();
                                                 if match_char_idx_in_text < char_indices.len() && match_char_count > 0 {
                                                     let char_start = char_indices[match_char_idx_in_text];
                                                     let char_end_idx = (match_char_idx_in_text + match_char_count - 1).min(char_indices.len() - 1);
                                                     let char_count = char_indices[char_end_idx] - char_start + 1;
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
                                                     Some((match_char_idx_in_text, match_char_count, rects))
                                                 } else {
                                                     None
                                                 }
                                             })
                                             .collect()
                                     } else {
                                         let options =
                                             PdfSearchOptions::new().match_case(match_case);
                                         match text_page.search(&query, &options) {
                                             Ok(search) => {
                                                 search
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
                                                                     if min_char_index.map_or(true, |min| idx < min) {
                                                                         min_char_index = Some(idx);
                                                                     }
                                                                     if max_char_index.map_or(true, |max| idx > max) {
                                                                         max_char_index = Some(idx);
                                                                     }
                                                                 }
                                                             }
                                                         }
                                                         let char_start = min_char_index.unwrap_or(0);
                                                         let char_count = max_char_index.map(|max| max - char_start + 1).unwrap_or(0);
                                                         let page_text_idx = char_indices.binary_search(&char_start).unwrap_or_else(|x| x);
                                                         let match_char_count = if char_count > 0 {
                                                             let page_text_end_idx = char_indices
                                                                 .binary_search(&(char_start + char_count - 1))
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
                                                     .collect()
                                             }
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
                    PdfCommand::RenderLinkPreview(path, index, dest_y, resp) => {
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
                            let target_y = dest_y.unwrap_or(0.0);
                            let v_padding = 150.0 * scale;
                            let center_y_scaled = target_y * scale;
                            let crop_y = (center_y_scaled - v_padding).max(0.0) as u32;
                            let crop_h =
                                (v_padding * 2.0).min((full_height as u32 - crop_y) as f32) as u32;
                            let center_ratio = if crop_h > 0 {
                                ((center_y_scaled - crop_y as f32) / crop_h as f32).clamp(0.0, 1.0)
                            } else {
                                0.5
                            };
                            let cropped =
                                dynamic_image.crop_imm(0, crop_y, full_width as u32, crop_h.max(1));
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

        Ok(Self {
            sender,
            priority_sender,
        })
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

    pub fn page_sizes(&self, path: &str) -> Result<Vec<(f32, f32)>, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::PageSizes(path.to_string(), tx))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    pub fn get_toc(&self, path: &str) -> Result<Vec<TocEntry>, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::GetToc(path.to_string(), tx))
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
    ) -> Result<LinkPreviewResult, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::RenderLinkPreview(
                path.to_string(),
                page_index,
                dest_y,
                tx,
            ))
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
    fn test_pdf_search() {
        let _guard = TEST_LOCK.lock().unwrap();
        let pdfium = bind_pdfium().unwrap();
        let doc = pdfium.load_pdf_from_file("/home/sur/.local/share/sioyek/tutorial.pdf", None).unwrap();
        let page = doc.pages().get(0).unwrap();
        let text_page = page.text().unwrap();
        let page_height = page.height().value;
        let page_width = page.width().value;
        println!("Page size: {} x {}", page_width, page_height);
        let options = PdfSearchOptions::new();
        let search = text_page.search("sioyek", &options).unwrap();
        let mut count = 0;
        for segments in search.iter(PdfSearchDirection::SearchForward) {
            count += 1;
            println!("Match {}:", count);
            for segment in segments.iter() {
                let bounds = segment.bounds();
                let ry = bounds.bottom().value;
                let rh = bounds.height().value;
                let view_y = page_height - ry - rh;
                println!("  Segment bounds: left={}, bottom={}, width={}, height={}",
                         bounds.left().value, ry, bounds.width().value, rh);
                println!("  Expected view y (zoom=1.0): {}", view_y);
            }
        }
        assert!(count > 0, "No matches found!");
    }

    #[test]
    fn test_pdf_renderer_search() {
        let _guard = TEST_LOCK.lock().unwrap();
        let renderer = PdfRenderer::new().unwrap();
        let path = "/home/sur/.local/share/sioyek/tutorial.pdf";
        
        // Test non-regex search
        let results = renderer.search_text(path, "sioyek", false, false).unwrap();
        println!("Non-regex results: {results:?}");
        assert!(!results.is_empty(), "Non-regex search for 'sioyek' should return matches");
        for match_info in &results {
            assert!(match_info.context.to_lowercase().contains("sioyek"));
            assert!(!match_info.rects.is_empty());
        }

        // Test regex search
        let regex_results = renderer.search_text(path, "sio[a-z]+k", true, false).unwrap();
        println!("Regex results: {regex_results:?}");
        assert!(!regex_results.is_empty(), "Regex search for 'sio[a-z]+k' should return matches");
        assert_eq!(results.len(), regex_results.len(), "Regex and non-regex search count should match");
    }


}


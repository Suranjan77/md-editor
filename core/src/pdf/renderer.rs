//! The PDFium worker: one background thread owns the library and documents; `PdfRenderer` sends it commands (render, text, search, links, TOC) and receives results.

use super::*;

#[derive(Debug)]
pub struct PdfRenderer {
    pub(super) sender: std::sync::mpsc::Sender<PdfCommand>,
    pub(super) priority_sender: std::sync::mpsc::Sender<PriorityRender>,
    pub(super) visible_range: std::sync::Arc<std::sync::Mutex<Option<(u16, u16, String)>>>,
}

pub(super) struct PriorityRender {
    pub(super) path: String,
    pub(super) page_index: u16,
    pub(super) scale: f32,
    pub(super) resp: std::sync::mpsc::SyncSender<Result<DynamicImage, String>>,
}

pub(super) enum PdfCommand {
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
        std::sync::mpsc::SyncSender<Result<Vec<crate::references::ReferenceLink>, String>>,
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
                Ok(pdfium) => {
                    log::info!("PDFium bound");
                    pdfium
                }
                Err(err) => {
                    log::error!("Failed to bind PDFium: {err}");
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
                                    .map_err(format_pdf_load_error)?;
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
                                    .map_err(format_pdf_load_error)?;
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
                                    .map_err(format_pdf_load_error)?;
                                current_document = Some((path.clone(), doc));
                            }
                            let Some((_, doc)) = current_document.as_ref() else {
                                return Err("PDF document was not loaded".to_string());
                            };
                            let mut sizes = Vec::with_capacity(doc.pages().len() as usize);
                            let mut fallback = (612.0, 792.0);
                            for index in 0..doc.pages().len() {
                                if let Ok(page) = doc.pages().get(index) {
                                    fallback = (page.width().value, page.height().value);
                                }
                                sizes.push(fallback);
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
                                    .map_err(format_pdf_load_error)?;
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
                                    .map_err(format_pdf_load_error)?;
                                current_document = Some((path.clone(), doc));
                            }
                            let Some((_, doc)) = current_document.as_ref() else {
                                return Err("PDF document was not loaded".to_string());
                            };
                            // References need the full text layer (for equation
                            // labels and captions) and the outline (for section
                            // targets). The text scan is the one-time cost the
                            // caller caches; see the `PDF-Engine-and-Sidecars` wiki page.
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
                                    .map_err(format_pdf_load_error)?;
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
                                    .map_err(format_pdf_load_error)?;
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
                                    .map_err(format_pdf_load_error)?;
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
                                                                    .is_none_or(|min| idx < min)
                                                                {
                                                                    min_char_index = Some(idx);
                                                                }
                                                                if max_char_index
                                                                    .is_none_or(|max| idx > max)
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

                                // PDFium is single-threaded, but search need
                                // not monopolize it: yield to the newest
                                // visible-page render after each small chunk.
                                if index % 8 == 7 || index + 1 == doc.pages().len() {
                                    while let Ok(mut priority) = priority_receiver.try_recv() {
                                        while let Ok(newer) = priority_receiver.try_recv() {
                                            priority = newer;
                                        }
                                        let rendered = if priority.path == path {
                                            render_page_from_document(
                                                doc,
                                                priority.page_index,
                                                priority.scale,
                                            )
                                        } else {
                                            pdfium
                                                .load_pdf_from_file(&priority.path, None)
                                                .map_err(format_pdf_load_error)
                                                .and_then(|other| {
                                                    render_page_from_document(
                                                        &other,
                                                        priority.page_index,
                                                        priority.scale,
                                                    )
                                                })
                                        };
                                        let _ = priority.resp.send(rendered);
                                    }
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
                                    .map_err(format_pdf_load_error)?;
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
                                    .map_err(format_pdf_load_error)?;
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

    pub(super) fn parse_bookmarks(bookmarks: &[PdfBookmark]) -> Vec<TocEntry> {
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

pub(super) fn render_page_from_cache<'a>(
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
            .map_err(format_pdf_load_error)?;
        *current_document = Some((path.to_string(), doc));
    }
    let Some((_, doc)) = current_document.as_ref() else {
        return Err("PDF document was not loaded".to_string());
    };
    render_page_from_document(doc, index, scale)
}

pub(super) fn render_page_from_document(
    doc: &PdfDocument<'_>,
    index: u16,
    scale: f32,
) -> Result<DynamicImage, String> {
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

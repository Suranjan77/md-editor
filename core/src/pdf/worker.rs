//! The PDFium background worker: 3-document LRU cache, priority render queue, interactive and background command lanes, and command handlers.

use super::*;
use std::sync::mpsc::{Receiver, TryRecvError};

/// Largest render scale for a page region (pixels per point, ~576 dpi).
const REGION_MAX_SCALE: f32 = 8.0;

/// `region` intersected with a `page_w × page_h` page, or `None` when nothing
/// of it is on the page (or it is degenerate).
pub(super) fn clamp_region(
    region: PageRegion,
    page_w: f32,
    page_h: f32,
) -> Option<(f32, f32, f32, f32)> {
    let finite = [region.x, region.y, region.width, region.height]
        .iter()
        .all(|v| v.is_finite());
    if !finite {
        return None;
    }
    let left = region.x.max(0.0);
    let top = region.y.max(0.0);
    let right = (region.x + region.width).min(page_w);
    let bottom = (region.y + region.height).min(page_h);
    (right - left >= 1.0 && bottom - top >= 1.0).then_some((left, top, right - left, bottom - top))
}

/// Maximum number of parsed `PdfDocument` handles retained by the worker so
/// switching between open PDF tabs does not re-parse documents.
pub(super) const DOC_CACHE_CAPACITY: usize = 3;

/// Access or insert an entry in a bounded LRU `VecDeque` (`front` = oldest,
/// `back` = most recently used). Promotes hits to the back and evicts the
/// oldest entry when inserting into a full cache.
pub(super) fn lru_get_or_insert_with<'c, T, F>(
    cache: &'c mut VecDeque<(String, T)>,
    capacity: usize,
    key: &str,
    load: F,
) -> PdfResult<&'c T>
where
    F: FnOnce() -> PdfResult<T>,
{
    if let Some(pos) = cache.iter().position(|(k, _)| k == key) {
        if let Some(entry) = cache.remove(pos) {
            cache.push_back(entry);
        }
    } else {
        let value = load()?;
        while cache.len() >= capacity.max(1) {
            cache.pop_front();
        }
        cache.push_back((key.to_string(), value));
    }
    cache
        .back()
        .map(|(_, value)| value)
        .ok_or_else(|| PdfError::Other("PDF document was not loaded".to_string()))
}

/// What identifies one version of a file on disk, cheaply.
type FileStamp = (u64, Option<std::time::SystemTime>);

fn file_stamp(path: &str) -> Option<FileStamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()))
}

pub(super) struct Worker<'a> {
    pub(super) pdfium: &'a Pdfium,
    pub(super) documents: VecDeque<(String, PdfDocument<'a>)>,
    /// Size and modification time of each cached document's file when it was
    /// loaded: a file replaced on disk is loaded again, not served stale.
    stamps: std::collections::HashMap<String, FileStamp>,
    /// Commands someone is waiting on to draw the screen: renders, sizes,
    /// links, the visible pages' text. Always served before `background`.
    pub(super) interactive: VecDeque<PdfCommand>,
    /// Whole-document work (outline recovery, reference scans, search). Kept
    /// behind interactive work so a render never waits for a scan.
    pub(super) background: VecDeque<PdfCommand>,
    pub(super) receiver: Receiver<PdfCommand>,
    pub(super) priority_receiver: Receiver<PriorityRender>,
    pub(super) visible_range: std::sync::Arc<std::sync::Mutex<Option<(u16, u16, String)>>>,
}

impl<'a> Worker<'a> {
    pub(super) fn new(
        pdfium: &'a Pdfium,
        receiver: Receiver<PdfCommand>,
        priority_receiver: Receiver<PriorityRender>,
        visible_range: std::sync::Arc<std::sync::Mutex<Option<(u16, u16, String)>>>,
    ) -> Self {
        Self {
            pdfium,
            documents: VecDeque::with_capacity(DOC_CACHE_CAPACITY),
            stamps: std::collections::HashMap::new(),
            interactive: VecDeque::new(),
            background: VecDeque::new(),
            receiver,
            priority_receiver,
            visible_range,
        }
    }

    pub(super) fn run(&mut self) {
        loop {
            // Take everything already sent, so a render that arrived behind a
            // background job still goes first.
            while let Ok(queued) = self.receiver.try_recv() {
                self.enqueue(queued);
            }
            let next = self
                .interactive
                .pop_front()
                .or_else(|| self.background.pop_front());
            let cmd = match next {
                Some(cmd) => cmd,
                None => match self.receiver.recv() {
                    Ok(cmd) => {
                        self.enqueue(cmd);
                        continue;
                    }
                    Err(_) => break,
                },
            };

            self.drain_priority();
            self.handle_command(cmd);
        }
    }

    /// File a command into its lane.
    pub(super) fn enqueue(&mut self, cmd: PdfCommand) {
        if cmd.is_background() {
            self.background.push_back(cmd);
        } else {
            self.interactive.push_back(cmd);
        }
    }

    pub(super) fn handle_command(&mut self, cmd: PdfCommand) {
        match cmd {
            PdfCommand::Wake => {}
            PdfCommand::PageCount(path, resp) => {
                let _ = resp.send(self.page_count(&path));
            }
            PdfCommand::ExtractText(path, resp) => {
                let _ = resp.send(self.extract_text(&path));
            }
            PdfCommand::PageSizes(path, resp) => {
                let _ = resp.send(self.page_sizes(&path));
            }
            PdfCommand::RenderPage(path, index, scale, resp) => {
                let _ = resp.send(self.render_page(&path, index, scale));
            }
            PdfCommand::GetToc(path, resp) => {
                let _ = resp.send(self.get_toc(&path));
            }
            PdfCommand::GetReferences(path, resp) => {
                let _ = resp.send(self.get_references(&path));
            }
            PdfCommand::GetEmbeddedToc(path, resp) => {
                let _ = resp.send(self.get_embedded_toc(&path));
            }
            PdfCommand::GetLinks(path, index, resp) => {
                let _ = resp.send(self.get_links(&path, index));
            }
            PdfCommand::SearchText(path, query, regex, match_case, resp) => {
                let _ = resp.send(self.search_text(&path, &query, regex, match_case));
            }
            PdfCommand::RenderLinkPreview(path, index, dest_y, target_width_px, resp) => {
                let _ = resp.send(self.render_link_preview(&path, index, dest_y, target_width_px));
            }
            PdfCommand::RenderRegion(path, index, region, target_width_px, resp) => {
                let _ = resp.send(self.render_region(&path, index, region, target_width_px));
            }
            PdfCommand::GetPageText(path, index, resp)
            | PdfCommand::GetPageTextBackground(path, index, resp) => {
                let _ = resp.send(self.get_page_text(&path, index));
            }
            PdfCommand::Attachments(request) => self.serve_attachments(request),
            PdfCommand::RecoverToc(path, page_texts, resp) => {
                let _ = resp.send(
                    self.open_document(&path)
                        .map(|doc| recover_toc(doc, &page_texts)),
                );
            }
        }
    }

    pub(super) fn drain_priority(&mut self) {
        while let Ok(mut priority) = self.priority_receiver.try_recv() {
            while let Ok(newer) = self.priority_receiver.try_recv() {
                // Superseded, not failed: the caller must not report an error.
                let _ = priority.resp.send(Err(PdfError::Skipped));
                priority = newer;
            }
            let res = self.render_cached(&priority.path, priority.page_index, priority.scale);
            let _ = priority.resp.send(res);
        }
    }

    pub(super) fn open_document(&mut self, path: &str) -> PdfResult<&PdfDocument<'a>> {
        let stamp = file_stamp(path);
        if self.stamps.get(path) != stamp.as_ref() {
            self.documents.retain(|(cached, _)| cached != path);
        }
        let documents = &self.documents;
        self.stamps
            .retain(|cached, _| cached == path || documents.iter().any(|(p, _)| p == cached));
        match stamp {
            Some(stamp) => self.stamps.insert(path.to_string(), stamp),
            None => self.stamps.remove(path),
        };
        let pdfium = self.pdfium;
        lru_get_or_insert_with(&mut self.documents, DOC_CACHE_CAPACITY, path, || {
            pdfium
                .load_pdf_from_file(path, None)
                .map_err(pdf_load_error)
        })
    }

    fn is_outside_visible_window(&self, path: &str, index: u16) -> bool {
        let Ok(range_lock) = self.visible_range.lock() else {
            return false;
        };
        let Some((start, end, ref range_path)) = *range_lock else {
            return false;
        };
        if range_path != path {
            return false;
        }
        let buffered_start = start.saturating_sub(2);
        let buffered_end = end.saturating_add(2);
        index < buffered_start || index > buffered_end
    }

    pub(super) fn page_count(&mut self, path: &str) -> PdfResult<u16> {
        let doc = self.open_document(path)?;
        Ok(doc.pages().len() as u16)
    }

    pub(super) fn extract_text(&mut self, path: &str) -> PdfResult<String> {
        let doc = self.open_document(path)?;
        let mut text = String::new();
        for page in doc.pages().iter() {
            if let Ok(tp) = page.text() {
                text.push_str(&tp.all());
                text.push('\n');
            }
        }
        Ok(text)
    }

    pub(super) fn page_sizes(&mut self, path: &str) -> PdfResult<Vec<(f32, f32)>> {
        let doc = self.open_document(path)?;
        let pages = doc.pages();
        // Sizes straight from the page tree: loading every page object just
        // to read its box made opening a long PDF visibly slower.
        let mut sizes = Vec::with_capacity(pages.len() as usize);
        let mut fallback = (612.0, 792.0);
        for index in 0..pages.len() {
            if let Ok(rect) = pages.page_size(index) {
                fallback = (rect.width().value, rect.height().value);
            }
            sizes.push(fallback);
        }
        Ok(sizes)
    }

    pub(super) fn render_page(
        &mut self,
        path: &str,
        index: u16,
        scale: f32,
    ) -> PdfResult<DynamicImage> {
        if self.is_outside_visible_window(path, index) {
            return Err(PdfError::Skipped);
        }
        self.render_cached(path, index, scale)
    }

    pub(super) fn render_cached(
        &mut self,
        path: &str,
        index: u16,
        scale: f32,
    ) -> PdfResult<DynamicImage> {
        let doc = self.open_document(path)?;
        render_page_from_document(doc, index, scale)
    }

    pub(super) fn get_toc(&mut self, path: &str) -> PdfResult<(Vec<TocEntry>, bool)> {
        let doc = self.open_document(path)?;
        let embedded = embedded_toc(doc);
        if !embedded.is_empty() {
            return Ok((embedded, false));
        }
        let page_texts = build_all_page_texts(doc);
        Ok((recover_toc(doc, &page_texts), true))
    }

    pub(super) fn get_references(
        &mut self,
        path: &str,
    ) -> PdfResult<Vec<crate::references::ReferenceLink>> {
        let doc = self.open_document(path)?;
        let page_texts = build_all_page_texts(doc);
        let embedded = embedded_toc(doc);
        let toc = if embedded.is_empty() {
            recover_toc(doc, &page_texts)
        } else {
            embedded
        };
        Ok(crate::references::resolve_references(&page_texts, &toc))
    }

    pub(super) fn get_embedded_toc(&mut self, path: &str) -> PdfResult<Vec<TocEntry>> {
        let doc = self.open_document(path)?;
        Ok(embedded_toc(doc))
    }

    pub(super) fn get_links(&mut self, path: &str, index: u16) -> PdfResult<Vec<LinkInfo>> {
        let doc = self.open_document(path)?;
        let pages = doc.pages();
        if i32::from(index) >= pages.len() {
            return Err(PdfError::Other("Page out of bounds".to_string()));
        }
        let page = pages
            .get(index as i32)
            .map_err(|e| PdfError::Other(e.to_string()))?;
        let page_height = page.height().value;
        let to_view = super::geometry::PageToView::for_page(&page);
        let mut links = Vec::new();
        for link in page.links().iter() {
            let Ok(rect) = link.rect() else {
                continue;
            };
            // Displayed box, then to the top-left origin `LinkInfo` uses.
            let view = to_view.rect(
                rect.left().value,
                rect.bottom().value,
                rect.right().value,
                rect.top().value,
            );
            let bbox = PdfRect {
                y: page_height - (view.y + view.height),
                ..view
            };
            let (dest_page, dest_y, uri) = resolve_link_target(&link, page_height);
            links.push(LinkInfo {
                bbox,
                dest_page,
                dest_y,
                uri,
            });
        }
        Ok(links)
    }

    pub(super) fn search_text(
        &mut self,
        path: &str,
        query: &str,
        regex: bool,
        match_case: bool,
    ) -> PdfResult<Vec<PdfSearchMatch>> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let re = if regex {
            Some(
                regex::RegexBuilder::new(query)
                    .case_insensitive(!match_case)
                    .build()
                    .map_err(|err| PdfError::Other(format!("Invalid PDF search regex: {err}")))?,
            )
        } else {
            None
        };
        let _ = self.open_document(path)?;
        let Some(doc_idx) = self.documents.iter().rposition(|(p, _)| p == path) else {
            return Err(PdfError::Other("PDF document was not loaded".to_string()));
        };
        let page_count = self.documents[doc_idx].1.pages().len();
        let mut matches = Vec::new();

        for index in 0..page_count {
            if self.is_channel_disconnected() {
                return Err(PdfError::WorkerGone);
            }
            let page_hits = {
                let doc = &self.documents[doc_idx].1;
                search_single_page(doc, index, query, re.as_ref(), match_case)?
            };
            for hit in page_hits {
                matches.push(hit);
                if matches.len() >= PDF_SEARCH_MATCH_LIMIT {
                    break;
                }
            }
            if matches.len() >= PDF_SEARCH_MATCH_LIMIT {
                break;
            }
            // PDFium is single-threaded, so yield to the newest priority render
            // after each 8-page chunk without losing our open document cache.
            if index % 8 == 7 || index + 1 == page_count {
                drain_priority_from_cache(self.pdfium, &self.documents, &self.priority_receiver);
            }
        }
        Ok(matches)
    }

    fn is_channel_disconnected(&mut self) -> bool {
        loop {
            match self.receiver.try_recv() {
                Ok(cmd) => self.enqueue(cmd),
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => {
                    return self.interactive.is_empty() && self.background.is_empty();
                }
            }
        }
    }

    pub(super) fn render_link_preview(
        &mut self,
        path: &str,
        index: u32,
        dest_y: Option<f32>,
        target_width_px: u32,
    ) -> PdfResult<LinkPreviewResult> {
        let doc = self.open_document(path)?;
        let page = doc
            .pages()
            .get(index as i32)
            .map_err(|e| PdfError::Other(e.to_string()))?;
        let page_w = page.width().value.max(1.0);
        let scale = (target_width_px as f32 / page_w).clamp(1.5, 6.0);
        let full_width = (page.width().value * scale) as i32;
        let full_height = (page.height().value * scale) as i32;
        let render_config = PdfRenderConfig::new()
            .set_target_width(full_width)
            .set_target_height(full_height);
        let bitmap = page
            .render_with_config(&render_config)
            .map_err(|e| PdfError::Other(e.to_string()))?;
        let dynamic_image = bitmap
            .as_image()
            .map_err(|e| PdfError::Other(e.to_string()))?;

        let (result_image, center_ratio) = match dest_y {
            Some(y) => {
                let window_h_px = ((PREVIEW_WINDOW_PT * scale).round() as u32).max(1);
                let center_px = y * scale;
                let win_top = center_px - window_h_px as f32 / 2.0;
                let page_rgba = dynamic_image.to_rgba8();
                let mut canvas = image::RgbaImage::from_pixel(
                    full_width as u32,
                    window_h_px,
                    image::Rgba([255, 255, 255, 255]),
                );
                image::imageops::overlay(&mut canvas, &page_rgba, 0, (-win_top).round() as i64);
                (image::DynamicImage::ImageRgba8(canvas), 0.5)
            }
            None => (dynamic_image, 0.0),
        };

        let mut buf = std::io::Cursor::new(Vec::new());
        result_image
            .write_to(&mut buf, image::ImageFormat::Png)
            .map_err(|e| PdfError::Other(e.to_string()))?;
        Ok(LinkPreviewResult {
            image_data: buf.into_inner(),
            center_ratio,
        })
    }

    pub(super) fn render_region(
        &mut self,
        path: &str,
        index: u16,
        region: PageRegion,
        target_width_px: u32,
    ) -> PdfResult<RegionImage> {
        let doc = self.open_document(path)?;
        let pages = doc.pages();
        if i32::from(index) >= pages.len() {
            return Err(PdfError::Other(format!(
                "Page {} is past the end",
                index + 1
            )));
        }
        let page = pages
            .get(i32::from(index))
            .map_err(|e| PdfError::Other(e.to_string()))?;
        let (page_w, page_h) = (page.width().value.max(1.0), page.height().value.max(1.0));
        let Some((x, y, w, h)) = clamp_region(region, page_w, page_h) else {
            return Err(PdfError::Other(
                "The crop lies outside the page".to_string(),
            ));
        };
        // The page is rendered whole and the region cut out of it; the scale is
        // capped so a tiny crop cannot ask for a gigantic page bitmap.
        let scale = (target_width_px as f32 / w).clamp(1.0, REGION_MAX_SCALE);
        let full_w = (page_w * scale).round() as i32;
        let full_h = (page_h * scale).round() as i32;
        let bitmap = page
            .render_with_config(
                &PdfRenderConfig::new()
                    .set_target_width(full_w)
                    .set_target_height(full_h),
            )
            .map_err(|e| PdfError::Other(e.to_string()))?;
        let page_rgba = bitmap
            .as_image()
            .map_err(|e| PdfError::Other(e.to_string()))?
            .to_rgba8();
        let px = |v: f32, max: u32| ((v * scale).round().max(0.0) as u32).min(max);
        let (left, top) = (px(x, page_rgba.width()), px(y, page_rgba.height()));
        let right = px(x + w, page_rgba.width())
            .max(left + 1)
            .min(page_rgba.width());
        let bottom = px(y + h, page_rgba.height())
            .max(top + 1)
            .min(page_rgba.height());
        let crop =
            image::imageops::crop_imm(&page_rgba, left, top, right - left, bottom - top).to_image();
        Ok(RegionImage {
            width: crop.width(),
            height: crop.height(),
            rgba: crop.into_raw(),
        })
    }

    pub(super) fn get_page_text(&mut self, path: &str, index: u16) -> PdfResult<PdfPageText> {
        let doc = self.open_document(path)?;
        let pages = doc.pages();
        if i32::from(index) >= pages.len() {
            return Err(PdfError::Other("Page index out of bounds".to_string()));
        }
        let page = pages
            .get(index as i32)
            .map_err(|e| PdfError::Other(e.to_string()))?;
        build_page_text(&page, index)
    }
}

pub(super) fn render_page_from_document(
    doc: &PdfDocument<'_>,
    index: u16,
    scale: f32,
) -> PdfResult<DynamicImage> {
    let pages = doc.pages();
    if i32::from(index) >= pages.len() {
        return Err(PdfError::Other("Page index out of bounds".to_string()));
    }
    let page = pages
        .get(index as i32)
        .map_err(|e| PdfError::Other(format!("Failed to get page: {e:?}")))?;

    let render_config = PdfRenderConfig::new()
        .set_target_width((page.width().value * scale) as i32)
        .set_target_height((page.height().value * scale) as i32);

    let bitmap = page
        .render_with_config(&render_config)
        .map_err(|e| PdfError::Other(format!("Failed to render page: {e:?}")))?;

    bitmap
        .as_image()
        .map_err(|e| PdfError::Other(format!("Failed to convert to image: {e:?}")))
}

fn resolve_link_target(
    link: &PdfLink<'_>,
    page_h: f32,
) -> (Option<u32>, Option<f32>, Option<String>) {
    let mut dest_page = None;
    let mut dest_y = None;
    let mut uri = None;
    if let Some(action) = link.action() {
        match action {
            PdfAction::Uri(ref uri_action) => uri = uri_action.uri().ok(),
            PdfAction::LocalDestination(ref local) => {
                if let Ok(dest) = local.destination() {
                    let (p, y) = extract_destination_coords(&dest, page_h);
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
        let (p, y) = extract_destination_coords(&dest, page_h);
        dest_page = p;
        if dest_y.is_none() {
            dest_y = y;
        }
    }
    (dest_page, dest_y, uri)
}

fn extract_destination_coords(
    dest: &PdfDestination<'_>,
    page_h: f32,
) -> (Option<u32>, Option<f32>) {
    let page = dest.page_index().ok().map(|i| i as u32);
    let y = match dest.view_settings() {
        Ok(PdfDestinationViewSettings::SpecificCoordinatesAndZoom(_, Some(y_pts), _))
        | Ok(PdfDestinationViewSettings::FitPageHorizontallyToWindow(Some(y_pts)))
        | Ok(PdfDestinationViewSettings::FitBoundsHorizontallyToWindow(Some(y_pts))) => {
            Some(page_h - y_pts.value)
        }
        _ => None,
    };
    (page, y)
}

fn drain_priority_from_cache(
    pdfium: &Pdfium,
    documents: &VecDeque<(String, PdfDocument<'_>)>,
    priority_receiver: &Receiver<PriorityRender>,
) {
    while let Ok(mut priority) = priority_receiver.try_recv() {
        while let Ok(newer) = priority_receiver.try_recv() {
            let _ = priority.resp.send(Err(PdfError::Skipped));
            priority = newer;
        }
        let rendered =
            if let Some((_, cached)) = documents.iter().rev().find(|(p, _)| p == &priority.path) {
                render_page_from_document(cached, priority.page_index, priority.scale)
            } else {
                pdfium
                    .load_pdf_from_file(&priority.path, None)
                    .map_err(pdf_load_error)
                    .and_then(|other| {
                        render_page_from_document(&other, priority.page_index, priority.scale)
                    })
            };
        let _ = priority.resp.send(rendered);
    }
}

fn search_single_page(
    doc: &PdfDocument<'_>,
    index: i32,
    query: &str,
    re: Option<&regex::Regex>,
    match_case: bool,
) -> PdfResult<Vec<PdfSearchMatch>> {
    let page = doc
        .pages()
        .get(index)
        .map_err(|e| PdfError::Other(e.to_string()))?;
    let text_page = page.text().map_err(|e| PdfError::Other(e.to_string()))?;
    let to_view = super::geometry::PageToView::for_page(&page);
    let mut page_text = String::new();
    let mut char_indices = Vec::new();
    for c in text_page.chars().iter() {
        if let Some(ch) = c.unicode_char() {
            page_text.push(ch);
            char_indices.push(c.index());
        }
    }
    let raw_matches = match re {
        Some(re) => regex_page_matches(&text_page, &page_text, &char_indices, re, &to_view),
        None => pdfium_page_matches(&text_page, &char_indices, query, match_case, &to_view),
    };
    // Byte offset of every char, so each match's context is a slice rather
    // than a walk from the start of the page.
    let mut offsets: Vec<usize> = page_text.char_indices().map(|(i, _)| i).collect();
    offsets.push(page_text.len());
    let last = offsets.len() - 1;
    let mut out = Vec::new();
    for (pos, match_len, rects) in raw_matches {
        if rects.is_empty() {
            continue;
        }
        let start = pos.saturating_sub(48).min(last);
        let end = (start + match_len + 96).min(last);
        let context = page_text[offsets[start]..offsets[end]]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        out.push(PdfSearchMatch {
            page_index: index as u16,
            context,
            rects,
        });
    }
    Ok(out)
}

fn regex_page_matches(
    text_page: &pdfium_render::prelude::PdfPageText<'_>,
    page_text: &str,
    char_indices: &[PdfPageTextCharIndex],
    re: &regex::Regex,
    to_view: &super::geometry::PageToView,
) -> Vec<(usize, usize, Vec<PdfRect>)> {
    // Matches come in order: count chars from the previous match, not from
    // the start of the page each time.
    let mut counted = (0usize, 0usize); // (byte offset, chars before it)
    re.find_iter(page_text)
        .filter_map(|found| {
            counted.1 += page_text[counted.0..found.start()].chars().count();
            counted.0 = found.start();
            let match_char_idx = counted.1;
            let match_char_count = found.as_str().chars().count();
            if match_char_idx >= char_indices.len() || match_char_count == 0 {
                return None;
            }
            let char_start = char_indices[match_char_idx];
            let char_end_idx = (match_char_idx + match_char_count - 1).min(char_indices.len() - 1);
            let char_count = char_indices[char_end_idx] - char_start + 1;
            let rects = text_page
                .segments_subset(char_start, char_count)
                .iter()
                .map(|segment| {
                    let bounds = segment.bounds();
                    to_view.rect(
                        bounds.left().value,
                        bounds.bottom().value,
                        bounds.right().value,
                        bounds.top().value,
                    )
                })
                .collect::<Vec<_>>();
            Some((match_char_idx, match_char_count, rects))
        })
        .collect()
}

fn pdfium_page_matches(
    text_page: &pdfium_render::prelude::PdfPageText<'_>,
    char_indices: &[PdfPageTextCharIndex],
    query: &str,
    match_case: bool,
    to_view: &super::geometry::PageToView,
) -> Vec<(usize, usize, Vec<PdfRect>)> {
    let options = PdfSearchOptions::new().match_case(match_case);
    let Ok(search) = text_page.search(query, &options) else {
        return Vec::new();
    };
    search
        .iter(PdfSearchDirection::SearchForward)
        .map(|segments| {
            let rects = segments
                .iter()
                .map(|segment| {
                    let bounds = segment.bounds();
                    to_view.rect(
                        bounds.left().value,
                        bounds.bottom().value,
                        bounds.right().value,
                        bounds.top().value,
                    )
                })
                .collect::<Vec<_>>();
            let mut min_char_index = None;
            let mut max_char_index = None;
            for segment in segments.iter() {
                if let Ok(chars) = segment.chars() {
                    for ch in chars.iter() {
                        let idx = ch.index();
                        if min_char_index.is_none_or(|min| idx < min) {
                            min_char_index = Some(idx);
                        }
                        if max_char_index.is_none_or(|max| idx > max) {
                            max_char_index = Some(idx);
                        }
                    }
                }
            }
            let char_start = min_char_index.unwrap_or(0);
            let char_count = max_char_index.map_or(0, |max| max - char_start + 1);
            let page_text_idx = char_indices
                .binary_search(&char_start)
                .unwrap_or_else(|x| x);
            let match_char_count = if char_count > 0 {
                let end_idx = char_indices
                    .binary_search(&(char_start + char_count - 1))
                    .unwrap_or_else(|x| x);
                end_idx
                    .checked_sub(page_text_idx)
                    .map_or(0, |diff| diff + 1)
            } else {
                0
            };
            (page_text_idx, match_char_count, rects)
        })
        .collect()
}

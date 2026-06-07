use crate::application::pdf_service::{LinkPreviewResult, PdfSearchMatch, TocEntry};
use crate::domain::pdf::{LinkInfo, PdfPageText};
use image::DynamicImage;
use std::sync::mpsc;

pub enum RenderCommand {
    Wake,
    PageCount(String, mpsc::SyncSender<Result<u16, String>>),
    PageSizes(String, mpsc::SyncSender<Result<Vec<(f32, f32)>, String>>),
    RenderPage(
        String,
        u16,
        f32,
        mpsc::SyncSender<Result<DynamicImage, String>>,
    ),
    GetToc(String, mpsc::SyncSender<Result<Vec<TocEntry>, String>>),
    GetLinks(String, u16, mpsc::SyncSender<Result<Vec<LinkInfo>, String>>),
    RenderLinkPreview(
        String,
        u32,
        Option<f32>,
        mpsc::SyncSender<Result<LinkPreviewResult, String>>,
    ),
}

pub enum QueryCommand {
    SearchText {
        path: String,
        query: String,
        regex: bool,
        match_case: bool,
        result_sender: mpsc::Sender<PdfSearchMatch>,
        done_sender: mpsc::Sender<Result<(), String>>,
        search_id: u64,
    },
    CancelSearch {
        search_id: u64,
    },
    GetPageText(String, u16, mpsc::SyncSender<Result<PdfPageText, String>>),
}

pub struct PriorityRender {
    pub path: String,
    pub page_index: u16,
    pub scale: f32,
    pub resp: mpsc::SyncSender<Result<DynamicImage, String>>,
}

pub struct ActivePdfSearch {
    pub search_id: u64,
    pub path: String,
    pub query: String,
    pub regex: bool,
    pub match_case: bool,
    pub result_sender: mpsc::Sender<PdfSearchMatch>,
    pub done_sender: mpsc::Sender<Result<(), String>>,
    pub page_idx: u16,
    pub total_pages: u16,
}

use crate::infrastructure::pdfium::binding::{bind_pdfium, with_pdfium_access};
use crate::infrastructure::pdfium::document::ensure_document;
use crate::infrastructure::pdfium::render::{
    link_preview_content_crop, link_preview_crop, render_page_from_cache,
};
use crate::infrastructure::pdfium::text::{get_page_text_impl, scan_page_for_search};
use pdfium_render::prelude::*;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub fn spawn_render_worker(
    render_receiver: mpsc::Receiver<RenderCommand>,
    priority_receiver: mpsc::Receiver<PriorityRender>,
    visible_range: Arc<Mutex<Option<(u16, u16, String)>>>,
) {
    std::thread::Builder::new()
        .name("pdf_render_worker".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
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

                    let res = with_pdfium_access(|| {
                        render_page_from_cache(
                            &pdfium,
                            &mut current_document,
                            &priority.path,
                            priority.page_index,
                            priority.scale,
                        )
                    });
                    let _ = priority.resp.send(res);
                }

                match cmd {
                    RenderCommand::Wake => {}
                    RenderCommand::PageCount(path, resp) => {
                        let res = with_pdfium_access(|| {
                            ensure_document(&pdfium, &mut current_document, &path)?;
                            let Some((_, doc)) = current_document.as_ref() else {
                                return Err("PDF document was not loaded".to_string());
                            };
                            Ok(doc.pages().len() as u16)
                        });
                        let _ = resp.send(res);
                    }
                    RenderCommand::PageSizes(path, resp) => {
                        let res = with_pdfium_access(|| {
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
                        });
                        let _ = resp.send(res);
                    }
                    RenderCommand::RenderPage(path, index, scale, resp) => {
                        let skipped = {
                            if let Ok(range_lock) = visible_range.lock() {
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
                            let res = with_pdfium_access(|| render_page_from_cache(
                                &pdfium,
                                &mut current_document,
                                &path,
                                index,
                                scale,
                            ));
                            let _ = resp.send(res);
                        }
                    }
                    RenderCommand::GetToc(path, resp) => {
                        let res = with_pdfium_access(|| {
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
                            Ok(crate::application::pdf_service::parse_bookmarks(&bookmarks))
                        });
                        let _ = resp.send(res);
                    }
                    RenderCommand::GetLinks(path, index, resp) => {
                        let res = with_pdfium_access(|| {
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
                                let bbox = crate::domain::pdf::PdfRect {
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
                        });
                        let _ = resp.send(res);
                    }
                    RenderCommand::RenderLinkPreview(path, index, dest_y, resp) => {
                        let res = with_pdfium_access(|| {
                            ensure_document(&pdfium, &mut current_document, &path)?;
                            let page_text = get_page_text_impl(
                                &pdfium,
                                &mut current_document,
                                &path,
                                index as u16,
                            )
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
                            let (crop_x, crop_y, crop_w, crop_h, center_ratio) =
                                if let Some(page_text) = page_text {
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
                                    link_preview_crop(
                                        full_width as u32,
                                        full_height as u32,
                                        dest_y,
                                        scale,
                                    )
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
                        });
                        let _ = resp.send(res);
                    }
                }
            }
        })
        .expect("Failed to spawn PDF render thread");
}

pub fn spawn_query_worker(query_receiver: mpsc::Receiver<QueryCommand>) {
    std::thread::Builder::new()
        .name("pdf_query_worker".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let pdfium = match bind_pdfium() {
                Ok(pdfium) => pdfium,
                Err(err) => {
                    eprintln!("Failed to bind PDFium in query thread: {err}");
                    return;
                }
            };

            let mut current_document: Option<(String, PdfDocument)> = None;
            let mut active_search: Option<ActivePdfSearch> = None;

            loop {
                let cmd = if active_search.is_some() {
                    match query_receiver.try_recv() {
                        Ok(c) => Some(c),
                        Err(mpsc::TryRecvError::Empty) => None,
                        Err(mpsc::TryRecvError::Disconnected) => break,
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
                            if active_search
                                .as_ref()
                                .is_some_and(|active| active.search_id == search_id)
                            {
                                active_search = None;
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
                                let total_pages = with_pdfium_access(|| {
                                    ensure_document(&pdfium, &mut current_document, &path)?;
                                    let Some((_, doc)) = current_document.as_ref() else {
                                        return Err("PDF document was not loaded".to_string());
                                    };
                                    Ok::<u16, String>(doc.pages().len() as u16)
                                });
                                match total_pages {
                                    Ok(total) => {
                                        active_search = Some(ActivePdfSearch {
                                            search_id,
                                            path,
                                            query,
                                            regex,
                                            match_case,
                                            result_sender,
                                            done_sender,
                                            page_idx: 0,
                                            total_pages: total,
                                        });
                                    }
                                    Err(err) => {
                                        let _ = done_sender.send(Err(err));
                                        active_search = None;
                                    }
                                }
                            }
                        }
                        QueryCommand::GetPageText(path, index, resp) => {
                            let res = with_pdfium_access(|| {
                                get_page_text_impl(&pdfium, &mut current_document, &path, index)
                            });
                            let _ = resp.send(res);
                        }
                    }
                }

                if let Some(active) = active_search.as_ref() {
                    if active.page_idx >= active.total_pages {
                        let _ = active.done_sender.send(Ok(()));
                        active_search = None;
                    } else {
                        let search_id = active.search_id;
                        let path = active.path.clone();
                        let query = active.query.clone();
                        let regex = active.regex;
                        let match_case = active.match_case;
                        let page_idx = active.page_idx;
                        let result_sender = active.result_sender.clone();
                        let matches = with_pdfium_access(|| {
                            scan_page_for_search(
                                &pdfium,
                                &mut current_document,
                                &path,
                                page_idx,
                                &query,
                                regex,
                                match_case,
                            )
                        });
                        let mut send_err = false;
                        for m in matches {
                            if result_sender.send(m).is_err() {
                                send_err = true;
                                break;
                            }
                        }
                        if send_err {
                            active_search = None;
                        } else if let Some(active) = active_search.as_mut()
                            && active.search_id == search_id
                        {
                            active.page_idx += 1;
                        }
                    }
                }
            }
        })
        .expect("Failed to spawn PDF query thread");
}

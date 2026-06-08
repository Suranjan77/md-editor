use crate::domain::PageIndex;
use crate::domain::pdf::{LinkInfo, PdfPageText, PdfRect};
use crate::infrastructure::pdfium::binding::bind_pdfium;
use crate::infrastructure::pdfium::worker::{
    PriorityRender, QueryCommand, RenderCommand, spawn_query_worker, spawn_render_worker,
};
use image::DynamicImage;
use pdfium_render::prelude::PdfBookmark;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TocEntry {
    pub title: String,
    pub page_index: Option<u32>,
    pub children: Vec<TocEntry>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PdfSearchMatch {
    pub page_index: u16,
    pub context: String,
    pub rects: Vec<PdfRect>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LinkPreviewResult {
    pub image_data: Vec<u8>,
    pub center_ratio: f32,
}

pub type PdfSearchResultReceiver = std::sync::mpsc::Receiver<PdfSearchMatch>;
pub type PdfSearchDoneReceiver = std::sync::mpsc::Receiver<Result<(), String>>;

pub struct PdfRenderer {
    render_sender: std::sync::mpsc::Sender<RenderCommand>,
    query_sender: std::sync::mpsc::Sender<QueryCommand>,
    priority_sender: std::sync::mpsc::Sender<PriorityRender>,
    visible_range: Arc<Mutex<Option<(u16, u16, String)>>>,
}

impl PdfRenderer {
    fn worker_page_index(page_index: PageIndex) -> Result<u16, String> {
        u16::try_from(page_index.get())
            .map_err(|_| format!("PDF page index {} exceeds worker range", page_index.get()))
    }

    pub fn new() -> Result<Self, String> {
        bind_pdfium().map_err(|e| format!("Failed to initialize PDF engine: {e}"))?;

        let (render_sender, render_receiver) = std::sync::mpsc::channel();
        let (query_sender, query_receiver) = std::sync::mpsc::channel();
        let (priority_sender, priority_receiver) = std::sync::mpsc::channel::<PriorityRender>();
        let visible_range: Arc<Mutex<Option<(u16, u16, String)>>> = Arc::new(Mutex::new(None));

        spawn_render_worker(render_receiver, priority_receiver, visible_range.clone());
        spawn_query_worker(query_receiver);

        Ok(Self {
            render_sender,
            query_sender,
            priority_sender,
            visible_range,
        })
    }

    pub fn set_visible_range(&self, start: PageIndex, end: PageIndex, path: &str) {
        let (Ok(start), Ok(end)) = (Self::worker_page_index(start), Self::worker_page_index(end))
        else {
            return;
        };
        if let Ok(mut range_lock) = self.visible_range.lock() {
            *range_lock = Some((start, end, path.to_string()));
        }
    }

    pub fn render_page(
        &self,
        path: &str,
        page_index: PageIndex,
        scale: f32,
    ) -> Result<DynamicImage, String> {
        let page_index = Self::worker_page_index(page_index)?;
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
        page_index: PageIndex,
        scale: f32,
    ) -> Result<DynamicImage, String> {
        let page_index = Self::worker_page_index(page_index)?;
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

    pub fn get_page_links(
        &self,
        path: &str,
        page_index: PageIndex,
    ) -> Result<Vec<LinkInfo>, String> {
        let page_index = Self::worker_page_index(page_index)?;
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
    ) -> Result<(PdfSearchResultReceiver, PdfSearchDoneReceiver), String> {
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
        page_index: PageIndex,
        dest_y: Option<f32>,
    ) -> Result<LinkPreviewResult, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.render_sender
            .send(RenderCommand::RenderLinkPreview(
                path.to_string(),
                page_index.get(),
                dest_y,
                tx,
            ))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    pub fn get_page_text(&self, path: &str, page_index: PageIndex) -> Result<PdfPageText, String> {
        let page_index = Self::worker_page_index(page_index)?;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.query_sender
            .send(QueryCommand::GetPageText(path.to_string(), page_index, tx))
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }
}

pub fn parse_bookmarks(bookmarks: &[PdfBookmark]) -> Vec<TocEntry> {
    let mut entries = Vec::new();
    for bookmark in bookmarks.iter() {
        let title = bookmark.title().unwrap_or_default();
        let page_index = bookmark
            .destination()
            .and_then(|dest| dest.page_index().ok())
            .map(|idx| idx as u32);
        let child_bookmarks: Vec<PdfBookmark> = bookmark.iter_direct_children().collect();
        let children = parse_bookmarks(&child_bookmarks);
        entries.push(TocEntry {
            title,
            page_index,
            children,
        });
    }
    entries
}

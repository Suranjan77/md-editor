//! `PdfRenderer`: public handle and typed command channels to the PDFium worker thread.

use super::worker::Worker;
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
    pub(super) resp: std::sync::mpsc::SyncSender<PdfResult<DynamicImage>>,
}

pub(super) enum PdfCommand {
    Wake,
    PageCount(String, std::sync::mpsc::SyncSender<PdfResult<u16>>),
    ExtractText(String, std::sync::mpsc::SyncSender<PdfResult<String>>),
    PageSizes(
        String,
        std::sync::mpsc::SyncSender<PdfResult<Vec<(f32, f32)>>>,
    ),
    RenderPage(
        String,
        u16,
        f32,
        std::sync::mpsc::SyncSender<PdfResult<DynamicImage>>,
    ),
    GetToc(
        String,
        std::sync::mpsc::SyncSender<PdfResult<(Vec<TocEntry>, bool)>>,
    ),
    GetReferences(
        String,
        std::sync::mpsc::SyncSender<PdfResult<Vec<crate::references::ReferenceLink>>>,
    ),
    GetEmbeddedToc(
        String,
        std::sync::mpsc::SyncSender<PdfResult<Vec<TocEntry>>>,
    ),
    GetLinks(
        String,
        u16,
        std::sync::mpsc::SyncSender<PdfResult<Vec<LinkInfo>>>,
    ),
    SearchText(
        String,
        String,
        bool,
        bool,
        std::sync::mpsc::SyncSender<PdfResult<Vec<PdfSearchMatch>>>,
    ),
    RenderLinkPreview(
        String,
        u32,
        Option<f32>,
        u32,
        std::sync::mpsc::SyncSender<PdfResult<LinkPreviewResult>>,
    ),
    RenderRegion(
        String,
        u16,
        PageRegion,
        u32,
        std::sync::mpsc::SyncSender<PdfResult<RegionImage>>,
    ),
    GetPageText(
        String,
        u16,
        std::sync::mpsc::SyncSender<PdfResult<PdfPageText>>,
    ),
    /// `GetPageText` for a whole-document scan, in the background lane.
    GetPageTextBackground(
        String,
        u16,
        std::sync::mpsc::SyncSender<PdfResult<PdfPageText>>,
    ),
    Attachments(super::attachments::AttachmentRequest),
    /// Recover an outline from page text the caller already extracted.
    RecoverToc(
        String,
        std::sync::Arc<Vec<PdfPageText>>,
        std::sync::mpsc::SyncSender<PdfResult<Vec<TocEntry>>>,
    ),
}

impl PdfCommand {
    /// Whole-document work, served only once no interactive command waits.
    pub(super) fn is_background(&self) -> bool {
        matches!(
            self,
            PdfCommand::ExtractText(..)
                | PdfCommand::GetToc(..)
                | PdfCommand::GetReferences(..)
                | PdfCommand::SearchText(..)
                | PdfCommand::GetPageTextBackground(..)
                | PdfCommand::RecoverToc(..)
        )
    }
}

impl PdfRenderer {
    pub fn new() -> PdfResult<Self> {
        let (sender, receiver) = std::sync::mpsc::channel();
        let (priority_sender, priority_receiver) = std::sync::mpsc::channel::<PriorityRender>();
        let visible_range: std::sync::Arc<std::sync::Mutex<Option<(u16, u16, String)>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let worker_range = visible_range.clone();

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

            let mut worker = Worker::new(&pdfium, receiver, priority_receiver, worker_range);
            worker.run();
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

    pub fn render_page(&self, path: &str, page_index: u16, scale: f32) -> PdfResult<DynamicImage> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::RenderPage(
                path.to_string(),
                page_index,
                scale,
                tx,
            ))
            .map_err(|_| PdfError::WorkerGone)?;
        receive(rx)
    }

    pub fn render_page_priority(
        &self,
        path: &str,
        page_index: u16,
        scale: f32,
    ) -> PdfResult<DynamicImage> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.priority_sender
            .send(PriorityRender {
                path: path.to_string(),
                page_index,
                scale,
                resp: tx,
            })
            .map_err(|_| PdfError::WorkerGone)?;
        self.sender
            .send(PdfCommand::Wake)
            .map_err(|_| PdfError::WorkerGone)?;
        receive(rx)
    }

    pub fn page_count(&self, path: &str) -> PdfResult<u16> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::PageCount(path.to_string(), tx))
            .map_err(|_| PdfError::WorkerGone)?;
        receive(rx)
    }

    /// Extract the full plain text of a PDF (all pages, newline-separated).
    /// Used to feed PDF content into the vault's full-text search index.
    pub fn extract_document_text(&self, path: &str) -> PdfResult<String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::ExtractText(path.to_string(), tx))
            .map_err(|_| PdfError::WorkerGone)?;
        receive(rx)
    }

    pub fn page_sizes(&self, path: &str) -> PdfResult<Vec<(f32, f32)>> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::PageSizes(path.to_string(), tx))
            .map_err(|_| PdfError::WorkerGone)?;
        receive(rx)
    }

    /// Returns the document's table of contents along with a flag that is
    /// `true` when the outline was synthesized from page text because the PDF
    /// has no embedded bookmarks.
    pub fn get_toc(&self, path: &str) -> PdfResult<(Vec<TocEntry>, bool)> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::GetToc(path.to_string(), tx))
            .map_err(|_| PdfError::WorkerGone)?;
        receive(rx)
    }

    /// Resolve internal cross-references (numbered equations, figures, tables,
    /// sections) for the whole document. This performs a one-time full text
    /// scan; callers should cache the result by document id (the result is
    /// stable for a given file). See [`crate::references`].
    pub fn get_references(&self, path: &str) -> PdfResult<Vec<crate::references::ReferenceLink>> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::GetReferences(path.to_string(), tx))
            .map_err(|_| PdfError::WorkerGone)?;
        receive(rx)
    }

    /// The embedded outline (bookmarks) only — no text scan, so cheap on every
    /// document. The chunked reference resolver uses this plus per-page text
    /// extraction to avoid a monolithic full-document command.
    pub fn get_embedded_toc(&self, path: &str) -> PdfResult<Vec<TocEntry>> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::GetEmbeddedToc(path.to_string(), tx))
            .map_err(|_| PdfError::WorkerGone)?;
        receive(rx)
    }

    /// The files embedded in the PDF (cheap: no page is loaded).
    pub fn list_attachments(&self, path: &str) -> PdfResult<Vec<PdfAttachmentInfo>> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::Attachments(
                super::attachments::AttachmentRequest::List(path.to_string(), tx),
            ))
            .map_err(|_| PdfError::WorkerGone)?;
        receive(rx)
    }

    /// The bytes of the attachment listed as `index` and `name`. Fails rather
    /// than return another file when the PDF changed since it was listed.
    pub fn attachment_bytes(&self, path: &str, index: u16, name: &str) -> PdfResult<Vec<u8>> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::Attachments(
                super::attachments::AttachmentRequest::Bytes(
                    path.to_string(),
                    index,
                    name.to_string(),
                    tx,
                ),
            ))
            .map_err(|_| PdfError::WorkerGone)?;
        receive(rx)
    }

    pub fn get_page_links(&self, path: &str, page_index: u16) -> PdfResult<Vec<LinkInfo>> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::GetLinks(path.to_string(), page_index, tx))
            .map_err(|_| PdfError::WorkerGone)?;
        receive(rx)
    }

    pub fn search_text(
        &self,
        path: &str,
        query: &str,
        regex: bool,
        match_case: bool,
    ) -> PdfResult<Vec<PdfSearchMatch>> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::SearchText(
                path.to_string(),
                query.to_string(),
                regex,
                match_case,
                tx,
            ))
            .map_err(|_| PdfError::WorkerGone)?;
        receive(rx)
    }

    pub fn render_link_preview(
        &self,
        path: &str,
        page_index: u32,
        dest_y: Option<f32>,
        target_width_px: u32,
    ) -> PdfResult<LinkPreviewResult> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::RenderLinkPreview(
                path.to_string(),
                page_index,
                dest_y,
                target_width_px,
                tx,
            ))
            .map_err(|_| PdfError::WorkerGone)?;
        receive(rx)
    }

    /// Render `region` of a page about `target_width_px` wide (a crop a note
    /// shows inline). The region is clamped to the page.
    pub fn render_region(
        &self,
        path: &str,
        page_index: u16,
        region: PageRegion,
        target_width_px: u32,
    ) -> PdfResult<RegionImage> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::RenderRegion(
                path.to_string(),
                page_index,
                region,
                target_width_px,
                tx,
            ))
            .map_err(|_| PdfError::WorkerGone)?;
        receive(rx)
    }

    pub fn get_page_text(&self, path: &str, page_index: u16) -> PdfResult<PdfPageText> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::GetPageText(path.to_string(), page_index, tx))
            .map_err(|_| PdfError::WorkerGone)?;
        receive(rx)
    }

    /// `get_page_text` for a whole-document scan: queued behind every render,
    /// link and visible-page request, so the scan never delays the screen.
    pub fn get_page_text_background(&self, path: &str, page_index: u16) -> PdfResult<PdfPageText> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::GetPageTextBackground(
                path.to_string(),
                page_index,
                tx,
            ))
            .map_err(|_| PdfError::WorkerGone)?;
        receive(rx)
    }

    /// Recover a bookmark-less document's outline from page text already
    /// extracted (with [`Self::get_page_text_background`]): the printed
    /// contents page through its links, then its text, then typography.
    /// Equivalent to [`Self::get_toc`]'s fallback without a second text pass.
    pub fn recover_toc(
        &self,
        path: &str,
        page_texts: std::sync::Arc<Vec<PdfPageText>>,
    ) -> PdfResult<Vec<TocEntry>> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(PdfCommand::RecoverToc(path.to_string(), page_texts, tx))
            .map_err(|_| PdfError::WorkerGone)?;
        receive(rx)
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

/// Wait for the worker's typed answer.
fn receive<T>(rx: std::sync::mpsc::Receiver<PdfResult<T>>) -> PdfResult<T> {
    rx.recv().map_err(|_| PdfError::WorkerGone)?
}

//! Async PDF worker (impl plan P5.1): `render_tile` / `page_chars` /
//! `page_links` run on one dedicated thread instead of the update loop, so
//! scrolling a huge document never blocks input on pdfium FFI. One thread is
//! not just simplicity — pdfium is single-threaded and every renderer method
//! takes the process-wide mutex anyway (pitfall P4), so more workers would
//! only queue on the lock.
//!
//! Shape: jobs go in over a plain `std::sync::mpsc` channel (submitting
//! never blocks); results come back through a delivery closure — in
//! production that closure feeds the iced subscription stream
//! ([`subscribe`]), in tests it collects into a vec. The worker is generic
//! over the job executor so its threading/ordering semantics are testable
//! without pdfium or a window.
//!
//! The shell treats the worker as optional: every request site falls back to
//! the old synchronous path when no [`WorkerHandle`] is installed. That is
//! deliberate — windowless suites run no subscriptions, so they keep
//! deterministic synchronous semantics, and the app upgrade is pure
//! responsiveness, never correctness.

use std::path::PathBuf;
use std::sync::mpsc;

use md_pdf::TileKey;

/// A request the update loop hands to the worker thread.
#[derive(Debug, Clone)]
pub enum PdfJob {
    Tile {
        path: PathBuf,
        key: TileKey,
    },
    PageGlyphs {
        path: PathBuf,
        page: u32,
    },
    PageLinks {
        path: PathBuf,
        page: u32,
    },
    MarkdownImage {
        document: PathBuf,
        key: String,
        abs_path: PathBuf,
    },
    MarkdownMath {
        document: PathBuf,
        key: String,
        tex: String,
    },
}

/// What the worker sends back. Routed to a session by `path` (sessions know
/// their vault-relative path; document ids never cross the thread).
#[derive(Debug, Clone)]
pub enum PdfJobOutput {
    Tile {
        path: PathBuf,
        key: TileKey,
        handle: iced::widget::image::Handle,
        bytes: usize,
    },
    TileFailed {
        path: PathBuf,
        key: TileKey,
        error: String,
    },
    PageGlyphs {
        path: PathBuf,
        page: u32,
        chars: Vec<md_pdf::CharBox>,
    },
    PageLinks {
        path: PathBuf,
        page: u32,
        links: Vec<md_pdf::LinkBox>,
    },
    MarkdownAsset {
        document: PathBuf,
        key: String,
        handle: iced::widget::image::Handle,
        width: f32,
        height: f32,
    },
    MarkdownAssetFailed {
        document: PathBuf,
        key: String,
        error: String,
    },
}

/// Cheap-to-clone submitter for the worker thread. Dropping every clone ends
/// the thread.
#[derive(Debug, Clone)]
pub struct WorkerHandle {
    tx: mpsc::Sender<PdfJob>,
}

impl WorkerHandle {
    /// Queue a job. Never blocks; a dead worker swallows the job (the
    /// synchronous fallback paths remain correct without it).
    pub fn submit(&self, job: PdfJob) {
        let _ = self.tx.send(job);
    }
}

/// Spawn the worker thread: `execute` runs each job (FIFO), `deliver` ships
/// each produced output. Returns the submit handle.
pub fn spawn<E, D>(execute: E, deliver: D) -> WorkerHandle
where
    E: Fn(&PdfJob) -> Option<PdfJobOutput> + Send + 'static,
    D: Fn(PdfJobOutput) + Send + 'static,
{
    let (tx, rx) = mpsc::channel::<PdfJob>();
    std::thread::spawn(move || {
        while let Ok(job) = rx.recv() {
            if let Some(out) = execute(&job) {
                deliver(out);
            }
        }
    });
    WorkerHandle { tx }
}

/// The production executor: one pdfium call per job, errors degrade the way
/// the synchronous paths do (failed tile reports, failed glyphs/links become
/// empty sets so the page is not re-requested every frame).
pub fn execute_job(job: &PdfJob) -> Option<PdfJobOutput> {
    match job {
        PdfJob::MarkdownImage {
            document,
            key,
            abs_path,
        } => Some(match super::markdown_assets::load_image(abs_path) {
            Ok((handle, width, height)) => PdfJobOutput::MarkdownAsset {
                document: document.clone(),
                key: key.clone(),
                handle,
                width,
                height,
            },
            Err(error) => PdfJobOutput::MarkdownAssetFailed {
                document: document.clone(),
                key: key.clone(),
                error,
            },
        }),
        PdfJob::MarkdownMath { document, key, tex } => {
            Some(match super::markdown_assets::render_math(tex) {
                Ok((handle, width, height)) => PdfJobOutput::MarkdownAsset {
                    document: document.clone(),
                    key: key.clone(),
                    handle,
                    width,
                    height,
                },
                Err(error) => PdfJobOutput::MarkdownAssetFailed {
                    document: document.clone(),
                    key: key.clone(),
                    error,
                },
            })
        }
        #[cfg(feature = "pdfium")]
        PdfJob::Tile { path, key } => {
            let renderer = super::pdf_view::renderer()?;
            Some(match renderer.render_tile(path, *key) {
                Ok(tile) => {
                    let bytes = tile.byte_size();
                    let handle =
                        iced::widget::image::Handle::from_rgba(tile.width, tile.height, tile.rgba);
                    PdfJobOutput::Tile {
                        path: path.clone(),
                        key: *key,
                        handle,
                        bytes,
                    }
                }
                Err(e) => PdfJobOutput::TileFailed {
                    path: path.clone(),
                    key: *key,
                    error: e.to_string(),
                },
            })
        }
        #[cfg(feature = "pdfium")]
        PdfJob::PageGlyphs { path, page } => {
            let renderer = super::pdf_view::renderer()?;
            Some(PdfJobOutput::PageGlyphs {
                path: path.clone(),
                page: *page,
                chars: renderer.page_chars(path, *page).unwrap_or_default(),
            })
        }
        #[cfg(feature = "pdfium")]
        PdfJob::PageLinks { path, page } => {
            let renderer = super::pdf_view::renderer()?;
            Some(PdfJobOutput::PageLinks {
                path: path.clone(),
                page: *page,
                links: renderer.page_links(path, *page).unwrap_or_default(),
            })
        }
        #[cfg(not(feature = "pdfium"))]
        PdfJob::Tile { .. } | PdfJob::PageGlyphs { .. } | PdfJob::PageLinks { .. } => None,
    }
}

/// The subscription stream: spawns the worker, hands its [`WorkerHandle`] to
/// the app as the first message, then forwards every output. The handshake
/// is what lets windowless tests (which run no subscriptions) stay on the
/// synchronous fallback for free.
pub fn subscribe() -> impl iced::futures::Stream<Item = super::Message> {
    use iced::futures::{SinkExt, StreamExt};
    iced::stream::channel(64, async move |mut output| {
        let (result_tx, mut result_rx) = iced::futures::channel::mpsc::unbounded();
        let handle = spawn(execute_job, move |out| {
            let _ = result_tx.unbounded_send(out);
        });
        let _ = output.send(super::Message::PdfWorkerReady(handle)).await;
        while let Some(out) = result_rx.next().await {
            let _ = output.send(super::Message::PdfWorker(out)).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use super::*;

    fn glyph_job(page: u32) -> PdfJob {
        PdfJob::PageGlyphs {
            path: Path::new("paper.pdf").to_path_buf(),
            page,
        }
    }

    fn glyph_output(job: &PdfJob) -> Option<PdfJobOutput> {
        let PdfJob::PageGlyphs { path, page } = job else {
            return None;
        };
        Some(PdfJobOutput::PageGlyphs {
            path: path.clone(),
            page: *page,
            chars: Vec::new(),
        })
    }

    #[test]
    fn worker_delivers_results_in_request_order() {
        let (tx, rx) = mpsc::channel();
        let handle = spawn(glyph_output, move |output| {
            let _ = tx.send(output);
        });
        for page in 0..4 {
            handle.submit(glyph_job(page));
        }
        let pages: Vec<u32> = (0..4)
            .filter_map(|_| rx.recv_timeout(Duration::from_secs(1)).ok())
            .filter_map(|output| match output {
                PdfJobOutput::PageGlyphs { page, .. } => Some(page),
                _ => None,
            })
            .collect();
        assert_eq!(pages, vec![0, 1, 2, 3]);
    }

    #[test]
    fn markdown_image_job_loads_dimensions_off_thread_path() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let image_path = dir.path().join("plot.png");
        let image = image::RgbaImage::from_pixel(8, 6, image::Rgba([1, 2, 3, 255]));
        image
            .save(&image_path)
            .unwrap_or_else(|error| panic!("{error}"));
        let job = PdfJob::MarkdownImage {
            document: dir.path().join("note.md"),
            key: "image:plot.png".to_string(),
            abs_path: image_path,
        };
        let Some(PdfJobOutput::MarkdownAsset { width, height, .. }) = execute_job(&job) else {
            panic!("expected loaded Markdown asset");
        };
        assert_eq!((width, height), (8.0, 6.0));
    }

    #[test]
    fn submitting_large_document_work_does_not_wait_for_renderer() {
        let (tx, rx) = mpsc::channel();
        let handle = spawn(
            |job| {
                std::thread::sleep(Duration::from_millis(2));
                glyph_output(job)
            },
            move |output| {
                let _ = tx.send(output);
            },
        );

        let started = Instant::now();
        for page in 0..500 {
            handle.submit(glyph_job(page));
        }
        assert!(
            started.elapsed() < Duration::from_millis(16),
            "queueing 500 pages blocked the caller"
        );
        assert!(
            rx.recv_timeout(Duration::from_secs(1)).is_ok(),
            "worker never delivered queued work"
        );
    }
}

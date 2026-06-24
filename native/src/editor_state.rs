//! Editor pane sub-state.
//!
//! Owns the document buffer, the highlighted/rendered lines, the debounced
//! highlight pipeline bookkeeping, the buffer revision (used by SearchState to
//! invalidate its match cache), the table of contents, the editor
//! scroll/viewport geometry, and the image/math resource caches used when
//! rendering markdown content.
//!
//! The editing/highlighting methods still live on the shell and read through
//! `self.editor`; moving them here is a sensible follow-up.
//!
//! Final domain in the `MdEditor` decomposition; see
//! `docs/refactor-mdeditor-decomposition.md`.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use iced::widget::image::Handle;
use iced::Task;
use image::GenericImageView;

use crate::editor::buffer::DocBuffer;
use crate::editor::highlight::{self, StyledLine};
use crate::messages::Message;
use crate::views;

/// Debounce window before a queued highlight pass actually runs. Owned here
/// alongside the highlight pipeline; the keyboard subscription on the shell
/// references it to schedule the debounce tick.
pub const HIGHLIGHT_DEBOUNCE: Duration = Duration::from_millis(80);

/// Above this line count, an opened document is shown with plain placeholders
/// first and highlighted asynchronously.
pub const HUGE_DOC_LINE_THRESHOLD: usize = 5_000;
/// Above this line count, edits debounce re-highlighting onto a background task
/// instead of highlighting synchronously.
pub const LARGE_DOC_LINE_THRESHOLD: usize = 1_000;

pub struct EditorPane {
    pub buffer: DocBuffer,
    pub highlighted_lines: Vec<StyledLine>,

    pub highlight_generation: u64,
    pub pending_highlight_generation: Option<u64>,
    pub pending_highlight_requested_at: Option<Instant>,
    pub pending_highlight_text: Option<String>,

    /// Bumped on every text change; read by SearchState to invalidate its
    /// in-document match cache.
    pub buffer_revision: u64,

    pub toc_visible: bool,
    pub toc_entries: Vec<views::toc::TocEntry>,

    pub scroll_y: f32,
    pub viewport_width: f32,
    pub viewport_height: f32,

    pub image_cache: HashMap<String, (Handle, f32, f32)>,
    pub math_cache: HashMap<String, (Handle, f32, f32)>,
}

impl EditorPane {
    pub fn new() -> Self {
        Self {
            buffer: DocBuffer::new(),
            highlighted_lines: Vec::new(),
            highlight_generation: 0,
            pending_highlight_generation: None,
            pending_highlight_requested_at: None,
            pending_highlight_text: None,
            buffer_revision: 0,
            toc_visible: false,
            toc_entries: Vec::new(),
            scroll_y: 0.0,
            viewport_width: 900.0,
            viewport_height: 720.0,
            image_cache: HashMap::new(),
            math_cache: HashMap::new(),
        }
    }

    // ── Highlighting ─────────────────────────────────────────────────

    /// Re-run highlighting for the current buffer. Bumps the generation, then
    /// either highlights synchronously, defers a large edit to a debounced
    /// task, or shows placeholders + an async task for a freshly opened huge
    /// document.
    ///
    /// Returns the async highlight task (if any) and whether the caller should
    /// now load image/math resources for the freshly highlighted lines (true
    /// only on the synchronous path; the async paths load resources when their
    /// `HighlightReady` arrives).
    pub fn refresh_highlighting(&mut self, opened_file: bool) -> (Task<Message>, bool) {
        let text = self.buffer.text();
        let line_count = self.buffer.line_count();
        self.highlight_generation = self.highlight_generation.wrapping_add(1);
        let generation = self.highlight_generation;
        self.pending_highlight_generation = None;
        self.pending_highlight_requested_at = None;
        self.pending_highlight_text = None;

        if opened_file && line_count > HUGE_DOC_LINE_THRESHOLD {
            self.highlighted_lines = plain_highlight_placeholders(&text);
            return (Self::highlight_task(generation, text), false);
        }

        if !opened_file && line_count > LARGE_DOC_LINE_THRESHOLD {
            self.pending_highlight_generation = Some(generation);
            self.pending_highlight_requested_at = Some(Instant::now());
            self.pending_highlight_text = Some(text);
            return (Task::none(), false);
        }

        self.highlighted_lines = highlight::highlight_markdown(&text);
        (Task::none(), true)
    }

    pub fn highlight_task(generation: u64, text: String) -> Task<Message> {
        Task::perform(
            async move { highlight::highlight_markdown(&text) },
            move |lines| Message::HighlightReady(generation, lines),
        )
    }

    /// Handle messages that mutate only this pane's own state: caching a
    /// rendered LaTeX image and firing a debounced highlight pass. Arms that
    /// need vault paths to resolve resources (`HighlightReady`) stay on the
    /// shell.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::MathRendered(tex, res) => {
                if let Ok(tuple) = res {
                    self.math_cache.insert(tex, tuple);
                }
                Task::none()
            }
            Message::HighlightDebounceElapsed => {
                if self
                    .pending_highlight_requested_at
                    .is_some_and(|requested| requested.elapsed() < HIGHLIGHT_DEBOUNCE)
                {
                    return Task::none();
                }
                let Some(generation) = self.pending_highlight_generation else {
                    return Task::none();
                };
                let Some(text) = self.pending_highlight_text.take() else {
                    self.pending_highlight_generation = None;
                    self.pending_highlight_requested_at = None;
                    return Task::none();
                };
                self.pending_highlight_generation = None;
                self.pending_highlight_requested_at = None;
                Self::highlight_task(generation, text)
            }
            _ => Task::none(),
        }
    }

    // ── Resource loading for rendered content ────────────────────────

    /// Synchronously load any not-yet-cached images referenced by the
    /// highlighted lines, resolving paths relative to the active document.
    pub fn load_images(&mut self, vault_root: &str, active_path: &str) {
        let Some(base_path) = std::path::Path::new(vault_root)
            .join(active_path)
            .parent()
            .map(|path| path.to_path_buf())
        else {
            return;
        };

        for line in &self.highlighted_lines {
            for span in &line.spans {
                if span.is_image {
                    if let Some(path) = &span.image_path {
                        if !self.image_cache.contains_key(path) {
                            let img_path = base_path.join(path);
                            if let Ok(img) = image::open(&img_path) {
                                let (width, height) = img.dimensions();
                                let handle = Handle::from_rgba(
                                    width,
                                    height,
                                    img.into_rgba8().into_raw(),
                                );
                                self.image_cache.insert(
                                    path.clone(),
                                    (handle, width as f32, height as f32),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Spawn render tasks for any not-yet-cached math spans.
    pub fn load_math(&self) -> Task<Message> {
        let mut tasks = Vec::new();
        for line in &self.highlighted_lines {
            for span in &line.spans {
                if span.is_math {
                    let tex = span.visible_text(false).trim_matches('$').trim().to_string();
                    if !tex.is_empty() && !self.math_cache.contains_key(&tex) {
                        let tex_clone = tex.clone();
                        tasks.push(Task::perform(
                            async move { (tex_clone.clone(), render_latex_task(&tex_clone)) },
                            |(t, r)| Message::MathRendered(t, r),
                        ));
                    }
                }
            }
        }
        Task::batch(tasks)
    }
}

/// Render a single-line plain highlighting (no markdown parsing) for very large
/// documents, used as an instant placeholder before async highlighting lands.
pub(crate) fn plain_highlight_placeholders(text: &str) -> Vec<StyledLine> {
    text.split('\n')
        .enumerate()
        .map(|(idx, line)| {
            let mut styled = StyledLine::new();
            styled.block_id = idx;
            styled
                .spans
                .push(crate::editor::highlight::StyledSpan::plain(line));
            styled
        })
        .collect()
}

fn render_latex_task(tex: &str) -> Result<(Handle, f32, f32), String> {
    use ratex_layout::{LayoutOptions, layout, to_display_list};
    use ratex_parser::parser::parse;
    use ratex_render::{RenderOptions, render_to_png};
    use ratex_types::color::Color as RatexColor;
    use ratex_types::math_style::MathStyle;

    let options = RenderOptions {
        font_size: 24.0,
        padding: 4.0,
        background_color: RatexColor {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.0,
        },
        font_dir: String::new(),
        device_pixel_ratio: 2.0,
    };

    let layout_opts = LayoutOptions::default()
        .with_style(MathStyle::Display)
        .with_color(RatexColor {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        });

    let ast = parse(tex).map_err(|e| format!("Parse error: {}", e))?;
    let lbox = layout(&ast, &layout_opts);
    let display_list = to_display_list(&lbox);
    let bytes =
        render_to_png(&display_list, &options).map_err(|e| format!("Render error: {:?}", e))?;

    let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
    let (w, h) = img.dimensions();
    Ok((Handle::from_bytes(bytes), w as f32 / 2.0, h as f32 / 2.0))
}

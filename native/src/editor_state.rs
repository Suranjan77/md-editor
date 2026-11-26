//! Editor pane sub-state.
//!
//! Owns the document buffer, its highlighted lines (kept in step with the text
//! incrementally), caret-reveal requests, the buffer revision (used by SearchState to
//! invalidate its match cache), the table of contents, the editor
//! scroll/viewport geometry, and the image/math resource caches used when
//! rendering markdown content.
//!
//! The editing/highlighting methods still live on the shell and read through
//! `self.editor`; moving them here is a sensible follow-up.
//!
//! Final domain in the `MdEditor` decomposition.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use iced::Task;
use iced::widget::image::Handle;
use image::GenericImageView;

use crate::editor::buffer::DocBuffer;
use crate::editor::highlight::Highlighted;
use crate::messages::{EditorMessage, Message};
use crate::views;

/// Idle time after the last edit before the document is written to disk.
/// Short enough that the unsaved window is never meaningful, long enough that
/// continuous typing does not write on every keystroke.
pub const AUTOSAVE_DEBOUNCE: Duration = Duration::from_millis(400);

/// How often the autosave subscription checks whether the debounce has
/// elapsed, while there are unwritten edits. Finer than the debounce so the
/// actual write lands close to it rather than up to a full window late.
pub const AUTOSAVE_POLL: Duration = Duration::from_millis(100);

/// How many visited documents keep their buffer (and undo history) parked in
/// memory. Bounds session memory on a large vault.
const MAX_RETAINED_BUFFERS: usize = 32;

pub struct EditorPane {
    pub buffer: DocBuffer,
    /// The buffer's highlighted lines, always in step with its text.
    pub highlight: Highlighted,

    /// Bumped on every text change; read by SearchState to invalidate its
    /// in-document match cache.
    pub buffer_revision: u64,

    pub toc_visible: bool,
    pub toc_entries: Vec<views::toc::TocEntry>,
    /// True when `toc_entries` were synthesized from PDF page text (no embedded
    /// bookmarks), so the UI can flag the outline as generated/heuristic.
    pub toc_is_synthetic: bool,

    pub scroll_y: f32,
    pub viewport_width: f32,
    pub viewport_height: f32,
    /// Bumped to ask the editor widget where the caret is, so it can be
    /// scrolled into view; see [`crate::editor::renderer::CaretView`].
    pub reveal_request: u64,
    /// The request not yet answered, and how it wants the caret shown.
    reveal_pending: Option<(u64, crate::editor::renderer::Reveal)>,
    /// Where the caret was last reported, for anchoring the completion popup.
    pub caret_view: Option<crate::editor::renderer::CaretView>,
    /// The open `[[` / `/` completion, if any.
    pub completion: Option<crate::autocomplete::Completion>,

    pub image_cache: HashMap<String, (Handle, f32, f32)>,
    /// Bumped whenever `image_cache` changes, so a refreshed image of a new
    /// size re-lays out its line even when the cache's length is unchanged.
    images_revision: u64,
    /// Images being decoded off the UI thread, and those that failed (missing,
    /// unreadable) — neither is tried again on every keystroke. Absolute paths.
    pending_images: std::collections::HashSet<std::path::PathBuf>,
    failed_images: std::collections::HashSet<std::path::PathBuf>,
    /// The same for `pdf://…&crop=` images, by link text.
    pending_crops: std::collections::HashSet<String>,
    failed_crops: std::collections::HashSet<String>,
    /// Images (by link text) whose file changed: decoded again, shown as they
    /// were until the new picture arrives, so a note does not flicker while a
    /// sketch it shows autosaves.
    stale_images: std::collections::HashSet<String>,
    pub math_cache: HashMap<String, crate::editor::renderer::MathRender>,
    /// TeX being rendered, and TeX that failed to parse: rendered once, not
    /// once per keystroke or per repeat in the note.
    pending_math: std::collections::HashSet<String>,
    failed_math: std::collections::HashSet<String>,
    /// Display scale the current math_cache contents were rasterized for.
    math_scale_factor: f32,

    /// Buffers for documents visited earlier in this session, keyed by
    /// vault-relative path, each with the scroll offset it was left at.
    /// Switching files parks the outgoing buffer here rather than dropping it,
    /// so its undo history, cursor, and reading position all survive a round
    /// trip back to the file.
    retained: HashMap<String, (DocBuffer, f32)>,
    /// Recency order for `retained`, least recent first.
    retained_order: Vec<String>,

    /// When the buffer last changed with edits still unwritten. The autosave
    /// subscription is armed while this is set.
    pub autosave_pending_since: Option<Instant>,

    /// [`md_editor_core::vault::content_hash`] of the open note as last read
    /// from or written to disk. A save that finds different text on disk
    /// keeps that version as a conflict copy instead of overwriting it.
    pub disk_hash: Option<u64>,
}

impl EditorPane {
    pub fn new() -> Self {
        Self {
            buffer: DocBuffer::new(),
            highlight: Highlighted::default(),
            buffer_revision: 0,
            toc_visible: false,
            toc_entries: Vec::new(),
            toc_is_synthetic: false,
            scroll_y: 0.0,
            viewport_width: 900.0,
            viewport_height: 720.0,
            reveal_request: 0,
            reveal_pending: None,
            caret_view: None,
            completion: None,
            image_cache: HashMap::new(),
            images_revision: 0,
            pending_images: Default::default(),
            failed_images: Default::default(),
            pending_crops: Default::default(),
            failed_crops: Default::default(),
            stale_images: Default::default(),
            math_cache: HashMap::new(),
            pending_math: Default::default(),
            failed_math: Default::default(),
            math_scale_factor: 1.0,
            retained: HashMap::new(),
            retained_order: Vec::new(),
            autosave_pending_since: None,
            disk_hash: None,
        }
    }

    // ── Retained buffers ─────────────────────────────────────────────

    /// Names what the editor widget lays out — the highlighted lines and the
    /// images and equations they show; see
    /// [`crate::editor::renderer::Editor::layout_revision`]. Images and
    /// equations are only ever added, or all dropped when the display scale
    /// changes, so their counts and that scale identify them.
    pub fn layout_revision(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (
            self.highlight.revision(),
            self.image_cache.len(),
            self.images_revision,
            self.math_cache.len(),
            self.math_scale_factor.to_bits(),
            crate::design::theme_revision(),
        )
            .hash(&mut hasher);
        hasher.finish()
    }

    /// Ask for the caret to be brought into view. While a request is
    /// unanswered the stronger way of showing it wins, so a jump that centres
    /// isn't downgraded by the cursor move made on the way.
    pub fn request_reveal(&mut self, reveal: crate::editor::renderer::Reveal) {
        let reveal = self
            .reveal_pending
            .map_or(reveal, |(_, pending)| pending.max(reveal));
        self.reveal_request = self.reveal_request.wrapping_add(1);
        self.reveal_pending = Some((self.reveal_request, reveal));
    }

    /// Claim the answer to `request`, if it is the one outstanding.
    pub fn take_reveal(&mut self, request: u64) -> Option<crate::editor::renderer::Reveal> {
        match self.reveal_pending {
            Some((pending, reveal)) if pending == request => {
                self.reveal_pending = None;
                Some(reveal)
            }
            _ => None,
        }
    }

    /// Park `buffer` under `path` so a later visit can resume its undo history.
    ///
    /// Callers must flush unsaved edits before parking; the registry exists to
    /// preserve *history*, not to stand in for saving.
    pub fn retain_buffer(&mut self, path: String, buffer: DocBuffer, scroll_y: f32) {
        self.retained_order.retain(|p| p != &path);
        self.retained_order.push(path.clone());
        self.retained.insert(path, (buffer, scroll_y));

        while self.retained_order.len() > MAX_RETAINED_BUFFERS {
            let evicted = self.retained_order.remove(0);
            self.retained.remove(&evicted);
        }
    }

    /// Reclaim the parked buffer for `path`, but only when it still matches
    /// what is on disk. A file edited outside the app invalidates the parked
    /// history, so in that case the caller falls back to a fresh buffer.
    pub fn take_retained_matching(
        &mut self,
        path: &str,
        disk_text: &str,
    ) -> Option<(DocBuffer, f32)> {
        let matches = self
            .retained
            .get(path)
            .is_some_and(|(buffer, _)| buffer.text() == disk_text);

        if !matches {
            self.retained.remove(path);
            self.retained_order.retain(|p| p != path);
            return None;
        }

        self.retained_order.retain(|p| p != path);
        self.retained.remove(path)
    }

    /// Apply `edit` to the parked buffer for `path`, if there is one.
    pub fn update_retained(&mut self, path: &str, edit: impl FnOnce(&mut DocBuffer)) {
        if let Some((buffer, _)) = self.retained.get_mut(path) {
            edit(buffer);
        }
    }

    /// Drop the parked buffer for `path`, and for anything under it when
    /// `path` is a folder. Used when an entry is deleted or renamed so stale
    /// history cannot be resurrected under a new file.
    pub fn forget_retained(&mut self, path: &str) {
        let child_prefix = format!("{path}/");
        let gone = |p: &String| p == path || p.starts_with(&child_prefix);
        self.retained.retain(|p, _| !gone(p));
        self.retained_order.retain(|p| !gone(p));
    }

    /// Drop every parked buffer. Used when the vault changes: relative paths
    /// from the old vault must not resurrect history in the new one.
    pub fn forget_all_retained(&mut self) {
        self.retained.clear();
        self.retained_order.clear();
    }

    /// Arm the autosave debounce. Called whenever buffer text changes.
    pub fn mark_dirty_now(&mut self) {
        self.autosave_pending_since = Some(Instant::now());
    }

    /// True when the debounce window has elapsed and a write should happen.
    pub fn autosave_due(&self) -> bool {
        self.autosave_pending_since
            .is_some_and(|since| since.elapsed() >= AUTOSAVE_DEBOUNCE)
    }

    // ── Highlighting ─────────────────────────────────────────────────

    /// Bring highlighting in step with the buffer. Only the lines an edit can
    /// have affected are re-highlighted, and it happens synchronously, so the
    /// lines on screen always match the text.
    pub fn refresh_highlighting(&mut self) {
        let text = self.buffer.text();
        self.highlight.update(&text);
    }

    /// Handle messages that mutate only this pane's own state: caching a
    /// rendered LaTeX image. Arms that
    /// need vault paths to resolve resources (`HighlightReady`) stay on the
    /// shell.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Editor(EditorMessage::MathRendered(tex, scale, ink, res)) => {
                // Drop results rasterized for a display scale we've since
                // moved away from (the cache was flushed on rescale; a stale
                // insert would stay blurry until the next flush).
                // Also drop renders in a colour from before a theme switch.
                if (scale - self.math_scale_factor).abs() < 0.01
                    && ink == crate::design::tokens().doc_math
                {
                    match res {
                        Ok(render) => {
                            self.math_cache.insert(tex.clone(), render);
                        }
                        Err(_) => {
                            // Half-typed TeX fails a new way per keystroke;
                            // keep the memory of failures bounded.
                            if self.failed_math.len() > MAX_FAILED_MATH {
                                self.failed_math.clear();
                            }
                            self.failed_math.insert(tex.clone());
                        }
                    }
                }
                self.pending_math.remove(&tex);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    // ── Resource loading for rendered content ────────────────────────

    /// Decode, off the UI thread, every image the highlighted lines show that
    /// is neither cached, nor being decoded, nor known to fail. Paths resolve
    /// relative to the active document.
    pub fn load_images(&mut self, vault_root: &str, active_path: &str) -> Task<Message> {
        let Some(base_path) = image_base(vault_root, active_path) else {
            return Task::none();
        };
        let mut tasks = Vec::new();
        for line in &self.highlight.lines {
            for span in &line.spans {
                let Some(key) = span.image_path.as_ref().filter(|_| span.is_image) else {
                    continue;
                };
                let fresh = self.image_cache.contains_key(key) && !self.stale_images.contains(key);
                if fresh || is_pdf_crop(key) {
                    continue;
                }
                let abs = base_path.join(image_link_path(key));
                if self.failed_images.contains(&abs) || !self.pending_images.insert(abs.clone()) {
                    continue;
                }
                let key = key.clone();
                tasks.push(Task::perform(
                    crate::app::blocking({
                        let abs = abs.clone();
                        move || crate::raster::decode(&abs).map(crate::raster::Decoded::handle)
                    }),
                    move |result| {
                        Message::Editor(EditorMessage::ImageLoaded(
                            key.clone(),
                            abs.clone(),
                            result,
                        ))
                    },
                ));
            }
        }
        Task::batch(tasks)
    }

    /// A decode finished. It is kept only if `key` still resolves to `abs`
    /// from the document open now.
    pub fn image_loaded(
        &mut self,
        key: String,
        abs: std::path::PathBuf,
        result: Result<(Handle, f32, f32), String>,
        vault_root: Option<&str>,
        active_path: Option<&str>,
    ) {
        if !self.pending_images.remove(&abs) {
            return; // forgotten (document switched, file changed) meanwhile
        }
        let current = vault_root
            .zip(active_path)
            .and_then(|(root, active)| image_base(root, active))
            .map(|base| base.join(image_link_path(&key)));
        match result {
            Ok(image) if current.as_ref() == Some(&abs) => {
                self.stale_images.remove(&key);
                self.image_cache.insert(key, image);
                self.images_revision = self.images_revision.wrapping_add(1);
            }
            Ok(_) => {}
            Err(_) => {
                // Gone or unreadable now: stop showing the old picture.
                if self.stale_images.remove(&key) && self.image_cache.remove(&key).is_some() {
                    self.images_revision = self.images_revision.wrapping_add(1);
                }
                self.failed_images.insert(abs);
            }
        }
    }

    /// `pdf://…&crop=` images the lines show that are neither rendered, nor
    /// rendering, nor known to fail. They are marked as rendering: the caller
    /// starts each one and reports back with [`Self::pdf_crop_loaded`].
    pub fn take_wanted_pdf_crops(&mut self) -> Vec<String> {
        let mut wanted = Vec::new();
        for line in &self.highlight.lines {
            for span in line.spans.iter().filter(|s| s.is_image) {
                let Some(key) = span.image_path.as_ref().filter(|k| is_pdf_crop(k)) else {
                    continue;
                };
                let fresh = self.image_cache.contains_key(key) && !self.stale_images.contains(key);
                if fresh || self.failed_crops.contains(key) {
                    continue;
                }
                if self.pending_crops.insert(key.clone()) {
                    wanted.push(key.clone());
                }
            }
        }
        wanted
    }

    /// A crop finished rendering. Dropped if the images were forgotten since.
    pub fn pdf_crop_loaded(&mut self, key: String, result: Result<(Handle, f32, f32), String>) {
        if !self.pending_crops.remove(&key) {
            return;
        }
        match result {
            Ok(image) => {
                self.stale_images.remove(&key);
                self.image_cache.insert(key, image);
                self.images_revision = self.images_revision.wrapping_add(1);
            }
            Err(e) => {
                log::warn!("Could not show {key}: {e}");
                if self.stale_images.remove(&key) && self.image_cache.remove(&key).is_some() {
                    self.images_revision = self.images_revision.wrapping_add(1);
                }
                self.failed_crops.insert(key);
            }
        }
    }

    /// Files changed on disk (absolute paths): mark the images the note shows
    /// from them for decoding again, keeping the old pictures up meanwhile.
    /// Any changed PDF re-renders every crop. Returns whether anything shown
    /// is affected (the caller then loads resources).
    pub fn refresh_changed_files(
        &mut self,
        vault_root: &str,
        active_path: &str,
        changed: &[std::path::PathBuf],
        pdf_changed: bool,
    ) -> bool {
        let Some(base) = image_base(vault_root, active_path) else {
            return false;
        };
        let changed: Vec<std::path::PathBuf> = changed.iter().map(|p| lexical(p)).collect();
        let mut affected = false;
        let keys: Vec<String> = self
            .highlight
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .filter(|span| span.is_image)
            .filter_map(|span| span.image_path.clone())
            .collect();
        for key in keys {
            if is_pdf_crop(&key) {
                if pdf_changed {
                    self.failed_crops.remove(&key);
                    self.pending_crops.remove(&key);
                    if self.image_cache.contains_key(&key) {
                        self.stale_images.insert(key);
                    }
                    affected = true;
                }
                continue;
            }
            let abs = base.join(image_link_path(&key));
            if !changed.contains(&lexical(&abs)) {
                continue;
            }
            self.failed_images.remove(&abs);
            // A decode in flight may have read the old file: let a new one start.
            self.pending_images.remove(&abs);
            if self.image_cache.contains_key(&key) {
                self.stale_images.insert(key);
            }
            affected = true;
        }
        affected
    }

    /// Drop every decoded image and what is known about missing ones: on a
    /// document switch (links are relative to the note) or when image files
    /// change on disk.
    pub fn forget_images(&mut self) {
        self.image_cache.clear();
        self.pending_images.clear();
        self.failed_images.clear();
        self.pending_crops.clear();
        self.failed_crops.clear();
        self.stale_images.clear();
        self.images_revision = self.images_revision.wrapping_add(1);
    }

    /// Drop rendered math (a new display scale or ink colour) and the record
    /// of renders in flight or failed.
    pub fn forget_math(&mut self) {
        self.math_cache.clear();
        self.pending_math.clear();
        self.failed_math.clear();
    }

    /// Spawn render tasks for any not-yet-cached math spans. `scale_factor` is
    /// the window's device-pixel ratio; the cache must be flushed when it
    /// changes (see the WindowRescaled handler) since it is not part of the
    /// key. Results carry the scale they were rendered for, so renders still
    /// in flight across a rescale are dropped instead of re-populating the
    /// cache with stale-density bitmaps.
    pub fn load_math(&mut self, scale_factor: f32) -> Task<Message> {
        self.math_scale_factor = scale_factor;
        let mut tasks = Vec::new();
        for line in &self.highlight.lines {
            for span in &line.spans {
                if span.is_math {
                    let tex = span
                        .visible_text(false)
                        .trim_matches('$')
                        .trim()
                        .to_string();
                    if !tex.is_empty()
                        && !self.math_cache.contains_key(&tex)
                        && !self.failed_math.contains(&tex)
                        && self.pending_math.insert(tex.clone())
                    {
                        let tex_clone = tex.clone();
                        // Chosen here, on the UI thread, not when the task runs.
                        let ink = crate::design::tokens().doc_math;
                        tasks.push(Task::perform(
                            crate::app::blocking(move || {
                                let render = render_latex_task(&tex_clone, scale_factor, ink);
                                (tex_clone, render)
                            }),
                            move |(t, r)| {
                                Message::Editor(EditorMessage::MathRendered(
                                    t,
                                    scale_factor,
                                    ink,
                                    r,
                                ))
                            },
                        ));
                    }
                }
            }
        }
        Task::batch(tasks)
    }
}

pub(crate) fn render_latex_task(
    tex: &str,
    scale_factor: f32,
    ink: iced::Color,
) -> Result<crate::editor::renderer::MathRender, String> {
    use ratex_layout::{LayoutOptions, layout, to_display_list};
    use ratex_parser::parser::parse;
    use ratex_render::{RenderOptions, render_to_png};
    use ratex_types::color::Color as RatexColor;
    use ratex_types::math_style::MathStyle;

    // Rasterize twice: inline math is displayed at 1.0× logical size, block
    // math at MATH_BLOCK_SCALE×. Giving each context a bitmap whose pixel
    // density matches its display size exactly (logical × scale_factor device
    // pixels) means both draw 1:1 on the device grid — resampling at a
    // fractional ratio renders thin glyph strokes alternately crisp and
    // blurry.
    let render_at =
        |device_pixel_ratio: f32, style: MathStyle| -> Result<(Handle, f32, f32), String> {
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
                device_pixel_ratio,
            };

            let layout_opts = LayoutOptions::default()
                .with_style(style)
                .with_color(RatexColor {
                    r: ink.r,
                    g: ink.g,
                    b: ink.b,
                    a: 1.0,
                });

            let ast = parse(tex).map_err(|e| format!("Parse error: {}", e))?;
            let lbox = layout(&ast, &layout_opts);
            let display_list = to_display_list(&lbox);
            let bytes = render_to_png(&display_list, &options)
                .map_err(|e| format!("Render error: {:?}", e))?;

            let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
            let (w, h) = img.dimensions();
            // Logical size = bitmap pixels / raster DPR; layout works in logical
            // units, so this is DPR-invariant.
            Ok((
                Handle::from_bytes(bytes),
                w as f32 / device_pixel_ratio,
                h as f32 / device_pixel_ratio,
            ))
        };

    let scale = scale_factor.clamp(1.0, 6.0);
    let block_scale = crate::editor::renderer::MATH_BLOCK_SCALE;
    // `$…$` in a line takes text style (smaller limits and fractions), as in
    // LaTeX; `$$…$$` display style.
    let (inline_handle, width, height) = render_at(scale, MathStyle::Text)?;
    let (block_handle, block_w, block_h) =
        render_at((block_scale * scale).min(6.0), MathStyle::Display)?;
    Ok(crate::editor::renderer::MathRender {
        inline_handle,
        block_handle,
        width,
        height,
        block_width: block_w * block_scale,
        block_height: block_h * block_scale,
    })
}

/// The file an image link points at: `<…>` (CommonMark's form for paths with
/// spaces) unwrapped and `%XX` escapes decoded.
/// Most failed TeX strings remembered before the record is reset.
const MAX_FAILED_MATH: usize = 1000;

/// `path` with `.` and `..` resolved without touching the disk, so an image
/// link like `../img/a.png` compares equal to the file it names.
fn lexical(path: &std::path::Path) -> std::path::PathBuf {
    let mut out = std::path::PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// An image link that shows a region of a PDF page, rendered live.
pub(crate) fn is_pdf_crop(link: &str) -> bool {
    link.trim().starts_with("pdf://") && link.contains("crop=")
}

/// The folder images in `active_path` resolve against.
fn image_base(vault_root: &str, active_path: &str) -> Option<std::path::PathBuf> {
    std::path::Path::new(vault_root)
        .join(active_path)
        .parent()
        .map(std::path::Path::to_path_buf)
}

pub(crate) fn image_link_path(link: &str) -> String {
    let link = link.trim();
    let link = link
        .strip_prefix('<')
        .and_then(|l| l.strip_suffix('>'))
        .unwrap_or(link);
    crate::platform::percent_decode(link)
}

#[cfg(test)]
mod tests;

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
use crate::messages::Message;
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

    pub image_cache: HashMap<String, (Handle, f32, f32)>,
    pub math_cache: HashMap<String, crate::editor::renderer::MathRender>,
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
            image_cache: HashMap::new(),
            math_cache: HashMap::new(),
            math_scale_factor: 1.0,
            retained: HashMap::new(),
            retained_order: Vec::new(),
            autosave_pending_since: None,
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
            self.math_cache.len(),
            self.math_scale_factor.to_bits(),
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

    /// Drop the parked buffer for `path`, and for anything under it when
    /// `path` is a folder. Used when an entry is deleted or renamed so stale
    /// history cannot be resurrected under a new file.
    pub fn forget_retained(&mut self, path: &str) {
        let child_prefix = format!("{path}/");
        let gone = |p: &String| p == path || p.starts_with(&child_prefix);
        self.retained.retain(|p, _| !gone(p));
        self.retained_order.retain(|p| !gone(p));
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
            Message::MathRendered(tex, scale, res) => {
                // Drop results rasterized for a display scale we've since
                // moved away from (the cache was flushed on rescale; a stale
                // insert would stay blurry until the next flush).
                if (scale - self.math_scale_factor).abs() < 0.01
                    && let Ok(render) = res
                {
                    self.math_cache.insert(tex, render);
                }
                Task::none()
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

        for line in &self.highlight.lines {
            for span in &line.spans {
                if span.is_image
                    && let Some(path) = &span.image_path
                    && !self.image_cache.contains_key(path)
                {
                    let img_path = base_path.join(path);
                    if let Ok(img) = image::open(&img_path) {
                        let (width, height) = img.dimensions();
                        let handle = Handle::from_rgba(width, height, img.into_rgba8().into_raw());
                        self.image_cache
                            .insert(path.clone(), (handle, width as f32, height as f32));
                    }
                }
            }
        }
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
                    if !tex.is_empty() && !self.math_cache.contains_key(&tex) {
                        let tex_clone = tex.clone();
                        tasks.push(Task::perform(
                            async move {
                                (
                                    tex_clone.clone(),
                                    render_latex_task(&tex_clone, scale_factor),
                                )
                            },
                            move |(t, r)| Message::MathRendered(t, scale_factor, r),
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
    let render_at = |device_pixel_ratio: f32| -> Result<(Handle, f32, f32), String> {
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
        // Logical size = bitmap pixels / raster DPR; layout works in logical
        // units, so this is DPR-invariant.
        Ok((
            Handle::from_bytes(bytes),
            w as f32 / device_pixel_ratio,
            h as f32 / device_pixel_ratio,
        ))
    };

    let scale = scale_factor.clamp(1.0, 6.0);
    let (inline_handle, width, height) = render_at(scale)?;
    let (block_handle, _, _) =
        render_at((crate::editor::renderer::MATH_BLOCK_SCALE * scale).min(6.0))?;
    Ok(crate::editor::renderer::MathRender {
        inline_handle,
        block_handle,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::buffer::EditorCommand;

    /// The layout revision changes exactly when something the editor lays out
    /// does: the lines, an arriving image or equation, or the math scale.
    #[test]
    fn layout_revision_names_the_lines_and_resources_laid_out() {
        let mut pane = EditorPane::new();
        pane.buffer = crate::editor::buffer::DocBuffer::from_text("$$\nx\n$$\n![a](i.png)");
        pane.refresh_highlighting();
        let initial = pane.layout_revision();
        assert_eq!(
            pane.layout_revision(),
            initial,
            "stable while nothing changes"
        );

        let handle = iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4]);
        pane.math_cache.insert(
            "x".into(),
            crate::editor::renderer::MathRender {
                inline_handle: handle.clone(),
                block_handle: handle.clone(),
                width: 10.0,
                height: 10.0,
            },
        );
        let with_math = pane.layout_revision();
        assert_ne!(with_math, initial, "an equation arrived");

        pane.image_cache.insert("i.png".into(), (handle, 1.0, 1.0));
        let with_image = pane.layout_revision();
        assert_ne!(with_image, with_math, "an image arrived");

        pane.math_scale_factor = 2.0;
        let rescaled = pane.layout_revision();
        assert_ne!(rescaled, with_image, "the math scale changed");

        pane.buffer.insert_at_cursor("y");
        pane.refresh_highlighting();
        assert_ne!(pane.layout_revision(), rescaled, "the lines changed");
    }

    fn edited(text: &str) -> DocBuffer {
        let mut buffer = DocBuffer::from_text(text);
        buffer.execute(EditorCommand::InsertText("!".to_string()));
        buffer
    }

    /// Parking a buffer and picking it up again must return the same document
    /// *with its history* — that is the whole point of the registry.
    #[test]
    fn retained_buffer_round_trip_keeps_undo_history() {
        let mut pane = EditorPane::new();
        let buffer = edited("hello");
        let text = buffer.text();

        pane.retain_buffer("notes/a.md".to_string(), buffer, 128.0);

        let (mut resumed, scroll) = pane
            .take_retained_matching("notes/a.md", &text)
            .expect("buffer matching disk should be resumable");
        assert_eq!(scroll, 128.0, "scroll position comes back with the buffer");

        assert!(resumed.execute(EditorCommand::Undo).text_changed);
        assert_eq!(
            resumed.text(),
            "hello",
            "undo history survived the round trip"
        );
    }

    /// If the file changed on disk while parked, the history no longer
    /// describes that file and must be discarded rather than replayed onto it.
    #[test]
    fn retained_buffer_is_dropped_when_disk_content_diverges() {
        let mut pane = EditorPane::new();
        pane.retain_buffer("notes/a.md".to_string(), DocBuffer::from_text("old"), 0.0);

        assert!(
            pane.take_retained_matching("notes/a.md", "changed elsewhere")
                .is_none(),
            "a diverged file must not resume stale history"
        );
        // The stale entry is evicted, not left to match a later coincidence.
        assert!(pane.take_retained_matching("notes/a.md", "old").is_none());
    }

    /// A deleted file must not hand its history to a new file of the same name.
    #[test]
    fn forgetting_a_path_drops_its_buffer() {
        let mut pane = EditorPane::new();
        pane.retain_buffer("notes/a.md".to_string(), DocBuffer::from_text("x"), 0.0);
        pane.forget_retained("notes/a.md");
        assert!(pane.take_retained_matching("notes/a.md", "x").is_none());
    }

    /// Session memory stays bounded on a large vault.
    #[test]
    fn registry_evicts_least_recently_used() {
        let mut pane = EditorPane::new();
        for i in 0..(MAX_RETAINED_BUFFERS + 5) {
            pane.retain_buffer(format!("n{i}.md"), DocBuffer::from_text("x"), 0.0);
        }
        assert_eq!(pane.retained.len(), MAX_RETAINED_BUFFERS);
        assert!(
            pane.take_retained_matching("n0.md", "x").is_none(),
            "oldest entry should have been evicted"
        );
        let newest = format!("n{}.md", MAX_RETAINED_BUFFERS + 4);
        assert!(pane.take_retained_matching(&newest, "x").is_some());
    }
}

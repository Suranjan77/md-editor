//! Handwritten notes.
//!
//! Windows-only for now: pen pressure is captured straight from the pointer
//! input stack because iced discards it (see [`pen_win`]). Everything above
//! the capture layer is platform neutral, so the surface still works with a
//! mouse anywhere the app builds.
//!
//! The pieces:
//!
//! * [`stroke`] — pure geometry: smoothing raw samples, turning a pressure
//!   path into a fillable outline.
//! * [`document`] — the strokes on a page, undo/redo, and the file format.
//! * [`pen`] — sample types and the stream feeding captured input to the app.
//! * [`canvas`] — the iced drawing surface.

pub mod camera;
pub mod canvas;
pub mod document;
pub mod pen;
pub mod radial;
pub mod stroke;

#[cfg(windows)]
mod pen_win;

use std::sync::Mutex;

use iced::{Color, Rectangle};

use document::{InkDocument, InkStroke};
use pen::{PenPhase, PenSample};
use stroke::{InkPoint, StrokeOptions, Vec2};

/// File extension for a handwritten page stored in the vault.
pub const INK_EXTENSION: &str = "ink";

/// How close a stroke must pass to the eraser, in logical pixels, to be taken.
const ERASER_RADIUS: f32 = 9.0;

/// Writing closer than this to the bottom of the surface pulls the page up.
///
/// Handwriting runs out of room constantly; without this you stop mid-sentence
/// to pan. Keeping the nib pinned at this line is what "scroll while writing"
/// means in every note app that has it.
const ADVANCE_MARGIN: f32 = 120.0;

/// What the pen does on the surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Pen,
    /// Translucent, uniform-width, and drawn behind writing so it never
    /// obscures it — the convention every note app follows.
    Highlighter,
    Eraser,
    /// Circle strokes to select them, then drag to move or delete.
    Lasso,
}

/// How much of a stroke the eraser takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EraserMode {
    /// Rub out only the part under the tip, splitting what is left.
    #[default]
    Partial,
    /// Take the whole stroke at a touch. Faster for clearing a word.
    Stroke,
}

/// Opacity of highlighter ink. Low enough that writing stays readable through
/// it, high enough to register as a highlight.
pub const HIGHLIGHTER_ALPHA: f32 = 0.32;

/// Highlighters are much broader than pens — that is what makes them read as
/// highlighting rather than as a thick pen.
pub const HIGHLIGHTER_SIZE: f32 = 20.0;

/// Render options for a stroke of the given kind.
///
/// A highlighter ignores pressure and tapering: a real one lays down a flat
/// band of constant width, and mimicking that is what stops it looking like a
/// fat pen.
pub fn options_for(kind: document::StrokeKind, base: StrokeOptions) -> StrokeOptions {
    match kind {
        document::StrokeKind::Pen => base,
        // Smoothing is inherited; only the pressure response changes.
        document::StrokeKind::Highlighter => StrokeOptions {
            thinning: 0.0,
            taper_start: 0.0,
            taper_end: 0.0,
            ..base
        },
    }
}

/// Pen widths offered in the toolbar, in logical pixels at full pressure.
pub const SIZE_PRESETS: [(&str, f32); 4] =
    [("XS", 1.6), ("S", 3.2), ("M", 5.5), ("L", 9.0)];

/// Highlighter widths. Broader across the board — a highlighter narrower than
/// the text it covers reads as a mistake.
pub const HIGHLIGHTER_SIZE_PRESETS: [(&str, f32); 3] =
    [("S", 14.0), ("M", 20.0), ("L", 30.0)];

/// Whether two colours are the same swatch.
///
/// Compared with a tolerance rather than `==`: presets are authored as `f32`
/// literals while theme colours come from 8-bit values, so exact equality can
/// leave the active swatch silently unhighlighted. The tolerance is just under
/// one 8-bit step, so distinct presets never collide.
pub fn same_color(a: Color, b: Color) -> bool {
    const EPSILON: f32 = 1.0 / 255.0;
    (a.r - b.r).abs() < EPSILON && (a.g - b.g).abs() < EPSILON && (a.b - b.b).abs() < EPSILON
}

/// Ink colours offered in the toolbar. Tuned for the dark theme.
pub const COLOR_PRESETS: [(&str, Color); 6] = [
    ("Ink", Color::from_rgb(0.89, 0.90, 0.93)),
    ("Amber", Color::from_rgb(0.98, 0.75, 0.35)),
    ("Rose", Color::from_rgb(0.93, 0.49, 0.47)),
    ("Mint", Color::from_rgb(0.55, 0.85, 0.72)),
    ("Sky", Color::from_rgb(0.48, 0.72, 0.95)),
    ("Violet", Color::from_rgb(0.72, 0.62, 0.95)),
];

/// Where the ink surface sits inside the window, in logical pixels.
///
/// Pen samples bypass iced entirely — they are read in the window procedure
/// and arrive in window-client coordinates — so something has to translate
/// them into surface-local space. The surface itself is the only place that
/// reliably knows its own position, so it records it on each draw and the
/// mapping below reads it back.
static VIEWPORT: Mutex<Option<Rectangle>> = Mutex::new(None);

/// Window scale factor, mirrored here so the pen hook — which sees only
/// physical pixels and cannot reach app state — can convert into the logical
/// coordinates the viewport is recorded in. Stored as bits because `f32` has
/// no atomic type.
static SCALE_FACTOR: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(1.0f32.to_bits());

pub(crate) fn set_viewport(bounds: Rectangle) {
    if let Ok(mut viewport) = VIEWPORT.lock() {
        *viewport = Some(bounds);
    }
}

pub fn set_scale_factor(factor: f32) {
    SCALE_FACTOR.store(
        factor.to_bits(),
        std::sync::atomic::Ordering::Relaxed,
    );
}

fn scale_factor() -> f32 {
    f32::from_bits(SCALE_FACTOR.load(std::sync::atomic::Ordering::Relaxed)).max(0.001)
}

fn viewport() -> Option<Rectangle> {
    VIEWPORT.lock().ok().and_then(|viewport| *viewport)
}

/// Whether a window-client point, in **physical** pixels, lands on the ink
/// surface.
///
/// The pen hook claims input only inside this rect. Everywhere else — the
/// toolbar, the sidebar — pen messages fall through to the normal event path
/// so the pen can press buttons exactly like a mouse. Without this the pen can
/// draw but cannot change tools, which makes the toolbar useless to the very
/// device the feature exists for.
pub(crate) fn client_point_is_on_canvas(x: f32, y: f32) -> bool {
    viewport().is_some_and(|bounds| point_is_on_canvas(x, y, bounds, scale_factor()))
}

/// The hit test itself, taking its inputs explicitly so it can be exercised
/// without touching the process-wide viewport and scale factor.
fn point_is_on_canvas(x: f32, y: f32, bounds: Rectangle, scale: f32) -> bool {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let (x, y) = (x / scale, y / scale);

    x >= bounds.x
        && y >= bounds.y
        && x <= bounds.x + bounds.width
        && y <= bounds.y + bounds.height
}

/// Map a captured pen sample into surface-local logical coordinates.
///
/// `reject_outside` is set for the sample that starts a stroke, so a pen that
/// lands on the toolbar does not begin drawing. Later samples pass through
/// unchecked — a stroke that runs off the edge should keep its shape rather
/// than kinking at the boundary.
fn map_to_canvas(sample: &PenSample, scale_factor: f32, reject_outside: bool) -> Option<Vec2> {
    let bounds = viewport()?;
    let scale = if scale_factor > 0.0 { scale_factor } else { 1.0 };

    let x = sample.x / scale - bounds.x;
    let y = sample.y / scale - bounds.y;

    if reject_outside && (x < 0.0 || y < 0.0 || x > bounds.width || y > bounds.height) {
        return None;
    }

    Some(Vec2::new(x, y))
}

/// Everything the handwriting surface needs to live in the app.
pub struct InkState {
    /// Whether the surface is on screen. Drives pen capture: while false, pen
    /// input falls through to the rest of the app as ordinary mouse input.
    visible: bool,
    pub tool: Tool,
    pub eraser_mode: EraserMode,
    pub options: StrokeOptions,
    pub color: Color,
    /// Highlighter width, kept separate so switching tools does not clobber
    /// the pen's width and vice versa.
    pub highlighter_size: f32,
    pub highlighter_color: Color,
    pub document: InkDocument,
    /// Retained geometry for finished strokes; cleared whenever the document
    /// changes so the next draw rebuilds it.
    pub cache: iced::widget::canvas::Cache,
    /// Vault-relative path this page is stored at, once saved.
    pub path: Option<String>,
    /// Unsaved changes.
    pub dirty: bool,

    /// Pan and zoom onto the page. Strokes are stored in page coordinates, so
    /// this only affects how they are drawn and where input lands.
    pub camera: camera::Camera,

    /// The stroke currently being drawn, in **page** coordinates.
    live: Vec<InkPoint>,
    drawing: bool,
    erasing: bool,

    /// Where the pen is hovering, in surface-local screen pixels, when it is
    /// in range but not touching.
    hover: Option<Vec2>,
    /// The radial menu, while it is open.
    radial: Option<radial::RadialMenu>,

    /// Lasso being drawn, in page coordinates.
    lasso: Vec<Vec2>,
    /// Indices of the currently selected strokes, in z-order.
    selection: Vec<usize>,
    /// Page-space origin of an in-progress selection drag, and how far it has
    /// moved so far. The move is applied incrementally so the strokes track
    /// the pen, then recorded once on release as a single undoable step.
    drag_from: Option<Vec2>,
    drag_total: Vec2,
}

impl Default for InkState {
    fn default() -> Self {
        Self::new()
    }
}

impl InkState {
    pub fn new() -> Self {
        Self {
            visible: false,
            tool: Tool::Pen,
            eraser_mode: EraserMode::default(),
            options: StrokeOptions::default(),
            // Start on a real swatch so the palette shows a selection from the
            // first frame.
            color: COLOR_PRESETS[0].1,
            highlighter_size: HIGHLIGHTER_SIZE,
            highlighter_color: COLOR_PRESETS[1].1,
            document: InkDocument::new(),
            cache: iced::widget::canvas::Cache::new(),
            path: None,
            dirty: false,
            camera: camera::Camera::new(),
            live: Vec::new(),
            drawing: false,
            erasing: false,
            hover: None,
            radial: None,
            lasso: Vec::new(),
            selection: Vec::new(),
            drag_from: None,
            drag_total: Vec2::new(0.0, 0.0),
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Show or hide the surface, starting or stopping pen capture with it.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        pen::set_capturing(visible);
        if !visible {
            // Don't leave a half-drawn stroke behind when the surface is
            // dismissed mid-stroke.
            self.cancel_stroke();
        }
    }

    /// Eraser radius in **page** units.
    ///
    /// The tip is a fixed size on screen — like a physical eraser held against
    /// the display — so its reach across the page shrinks as you zoom in. This
    /// is what keeps the ring drawn at [`ERASER_RADIUS`] screen pixels honest
    /// about what it will take.
    pub fn eraser_radius(&self) -> f32 {
        ERASER_RADIUS / self.camera.zoom.max(0.001)
    }

    /// Eraser radius in screen pixels, for drawing the cursor ring.
    pub fn eraser_screen_radius(&self) -> f32 {
        ERASER_RADIUS
    }

    pub fn live_points(&self) -> &[InkPoint] {
        &self.live
    }

    /// Whether a stroke is being laid down right now.
    pub fn is_drawing(&self) -> bool {
        self.drawing
    }

    /// The lasso currently being drawn, in page coordinates.
    pub fn lasso(&self) -> &[Vec2] {
        &self.lasso
    }

    /// Indices of the selected strokes, in z-order.
    pub fn selection(&self) -> &[usize] {
        &self.selection
    }

    pub fn has_selection(&self) -> bool {
        !self.selection.is_empty()
    }

    /// Drop the selection and any half-drawn lasso.
    pub fn clear_selection(&mut self) -> bool {
        let had = !self.selection.is_empty() || !self.lasso.is_empty();
        self.selection.clear();
        self.lasso.clear();
        self.drag_from = None;
        had
    }

    /// Delete the selected strokes.
    pub fn delete_selection(&mut self) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        let removed = self.document.remove_strokes(&self.selection);
        self.selection.clear();
        self.lasso.clear();
        if removed {
            self.invalidate();
        }
        removed
    }

    /// Page-space bounding box enclosing the selection, as `(min, max)`.
    pub fn selection_bounds(&self) -> Option<(Vec2, Vec2)> {
        let mut bounds: Option<(Vec2, Vec2)> = None;
        for &index in &self.selection {
            let Some(stroke) = self.document.strokes().get(index) else {
                continue;
            };
            let Some(stroke_box) = stroke.path_bounds() else {
                continue;
            };
            bounds = Some(match bounds {
                None => stroke_box,
                Some((min, max)) => (
                    Vec2::new(min.x.min(stroke_box.0.x), min.y.min(stroke_box.0.y)),
                    Vec2::new(max.x.max(stroke_box.1.x), max.y.max(stroke_box.1.y)),
                ),
            });
        }
        bounds
    }

    /// Whether a page point falls inside the selection's bounding box.
    /// Used to decide between dragging the selection and starting a new lasso.
    fn selection_contains(&self, point: Vec2) -> bool {
        // A little slack so grabbing near the edge of thin strokes still works.
        let pad = self.eraser_radius();
        self.selection_bounds().is_some_and(|(min, max)| {
            point.x >= min.x - pad
                && point.x <= max.x + pad
                && point.y >= min.y - pad
                && point.y <= max.y + pad
        })
    }

    /// Shift the view. Returns whether anything moved.
    pub fn pan_by(&mut self, delta: Vec2) -> bool {
        if delta.x == 0.0 && delta.y == 0.0 {
            return false;
        }
        self.camera.pan_by(delta);
        // Geometry is cached with the transform baked in, so a moved view has
        // to be re-tessellated.
        self.cache.clear();
        true
    }

    /// Zoom in or out about the middle of the surface.
    ///
    /// The toolbar's zoom buttons go through here rather than the wheel path:
    /// a pen has no scroll wheel, so buttons are the only way to zoom without
    /// putting it down.
    pub fn zoom_step(&mut self, notches: f32) -> bool {
        let centre = viewport()
            .map(|bounds| Vec2::new(bounds.width * 0.5, bounds.height * 0.5))
            .unwrap_or(Vec2::new(0.0, 0.0));
        self.zoom_by_wheel(centre, notches)
    }

    /// Zoom about a screen-space anchor. Returns whether the zoom changed.
    pub fn zoom_by_wheel(&mut self, anchor: Vec2, notches: f32) -> bool {
        if self.camera.zoom_by_wheel(anchor, notches) {
            self.cache.clear();
            true
        } else {
            false
        }
    }

    pub fn reset_view(&mut self) -> bool {
        if self.camera.is_identity() {
            return false;
        }
        self.camera.reset();
        self.cache.clear();
        true
    }

    /// Feed a batch of captured pen samples. Returns whether anything changed
    /// and the view needs redrawing.
    pub fn apply_samples(&mut self, samples: &[PenSample], scale_factor: f32) -> bool {
        let mut changed = false;

        for sample in samples {
            if sample.phase == PenPhase::Leave {
                changed |= self.hover.take().is_some();
                continue;
            }

            let starting = sample.phase == PenPhase::Down;
            let Some(position) = map_to_canvas(sample, scale_factor, starting) else {
                continue;
            };

            match sample.phase {
                PenPhase::Hover => {
                    // Cheap and constant, so it is worth a redraw every time:
                    // this indicator is how the surface is aimed.
                    self.hover = Some(position);
                    changed = true;
                }
                PenPhase::Down => {
                    if sample.barrel {
                        // The side button summons the menu instead of drawing.
                        self.radial = Some(radial::RadialMenu::new(position));
                    } else {
                        self.begin(position, sample.pressure, sample.eraser);
                    }
                    changed = true;
                }
                PenPhase::Move => {
                    self.hover = Some(position);
                    if let Some(menu) = &mut self.radial {
                        menu.cursor = position;
                        changed = true;
                    } else {
                        changed |= self.extend(position, sample.pressure);
                    }
                }
                PenPhase::Up => {
                    if self.radial.is_some() {
                        changed |= self.commit_radial();
                    } else {
                        changed |= self.extend(position, sample.pressure);
                        changed |= self.finish();
                    }
                }
                PenPhase::Leave => unreachable!("handled above"),
            }
        }

        changed
    }

    /// Where the pointing device is, in surface-local screen pixels.
    ///
    /// Fed by both the pen (hover samples) and the mouse, so the aiming
    /// indicators track whichever moved last.
    pub fn hover(&self) -> Option<Vec2> {
        self.hover
    }

    pub fn set_hover(&mut self, position: Vec2) {
        self.hover = Some(position);
    }

    pub fn radial_menu(&self) -> Option<radial::RadialMenu> {
        self.radial
    }

    /// Apply whatever the open radial menu is pointing at and close it.
    fn commit_radial(&mut self) -> bool {
        let Some(menu) = self.radial.take() else {
            return false;
        };

        match menu.highlighted() {
            Some(radial::RadialAction::Tool(tool)) => {
                if tool != Tool::Lasso {
                    self.clear_selection();
                }
                self.tool = tool;
            }
            Some(radial::RadialAction::Color(color)) => {
                if self.tool == Tool::Highlighter {
                    self.highlighter_color = color;
                } else {
                    self.color = color;
                }
            }
            // Lifted in the dead zone or flicked away: cancelled.
            None => {}
        }
        true
    }

    /// Pull the page up when the pen writes into the bottom margin, so a line
    /// can be continued without stopping to pan.
    ///
    /// Takes a **screen** position because the margin is a property of the
    /// window, not of the page.
    fn advance_if_near_bottom(&mut self, screen_y: f32) -> bool {
        let Some(bounds) = viewport() else {
            return false;
        };
        let threshold = bounds.height - ADVANCE_MARGIN;
        if threshold <= 0.0 || screen_y <= threshold {
            return false;
        }

        // Pin the nib at the threshold rather than jumping: a discrete scroll
        // mid-letter is disorienting.
        self.camera.pan_by(Vec2::new(0.0, threshold - screen_y));
        self.cache.clear();
        true
    }

    /// Begin an interaction at a **screen**-space position.
    ///
    /// What that means depends on the tool: drawing a stroke, erasing, drawing
    /// a lasso, or — when the press lands on an existing selection — starting
    /// to drag it.
    pub fn begin(&mut self, screen: Vec2, pressure: f32, inverted: bool) {
        let position = self.camera.to_page(screen);

        // Keep the aiming indicators pinned to the nib from the first sample,
        // not from wherever the last hover left them.
        self.hover = Some(screen);
        self.live.clear();
        self.drag_from = None;
        self.drag_total = Vec2::new(0.0, 0.0);

        // An inverted pen erases regardless of the selected tool.
        if inverted || self.tool == Tool::Eraser {
            self.erasing = true;
            self.drawing = false;
            // The whole drag is one undoable gesture; without this, undo would
            // rewind an eraser sweep one input sample at a time.
            self.document.begin_batch();
            self.erase_at(position);
            return;
        }

        self.erasing = false;

        if self.tool == Tool::Lasso {
            self.drawing = false;
            if self.selection_contains(position) {
                // Grabbing the existing selection moves it instead of starting
                // a new lasso.
                self.drag_from = Some(position);
            } else {
                self.selection.clear();
                self.lasso.clear();
                self.lasso.push(position);
            }
            return;
        }

        self.drawing = true;
        self.live.push(InkPoint {
            pos: position,
            pressure,
        });
    }

    /// Continue the current interaction. Returns whether anything changed.
    pub fn extend(&mut self, screen: Vec2, pressure: f32) -> bool {
        let position = self.camera.to_page(screen);
        // Covers a mouse drag too, which never produces hover samples.
        self.hover = Some(screen);

        if self.erasing {
            return self.erase_at(position);
        }

        if self.tool == Tool::Lasso {
            if let Some(from) = self.drag_from {
                // Apply the move incrementally so the strokes follow the pen;
                // the whole gesture is recorded once on release.
                let delta = position.sub(from);
                if delta.x == 0.0 && delta.y == 0.0 {
                    return false;
                }
                let selection = self.selection.clone();
                self.document.translate_without_history(&selection, delta);
                self.drag_total = self.drag_total.add(delta);
                self.drag_from = Some(position);
                self.cache.clear();
                return true;
            }
            if !self.lasso.is_empty() {
                self.lasso.push(position);
                return true;
            }
            return false;
        }

        if !self.drawing {
            return false;
        }
        self.live.push(InkPoint {
            pos: position,
            pressure,
        });
        // Advancing moves the camera, so it must happen after the sample is
        // recorded in page space — the stroke itself does not move.
        self.advance_if_near_bottom(screen.y);
        true
    }

    /// End the current interaction, committing whatever it produced. Returns
    /// whether anything changed.
    pub fn finish(&mut self) -> bool {
        if self.erasing {
            self.erasing = false;
            self.drawing = false;
            self.document.end_batch();
            return false;
        }

        if self.tool == Tool::Lasso {
            if self.drag_from.take().is_some() {
                let moved = self.drag_total;
                self.drag_total = Vec2::new(0.0, 0.0);
                if moved.x == 0.0 && moved.y == 0.0 {
                    return false;
                }
                // The strokes already sit where the drag left them, so the
                // history only needs the total to undo by.
                let selection = self.selection.clone();
                self.document.record_move(&selection, moved);
                self.dirty = true;
                return true;
            }

            // Only a gesture that actually drew a lasso resolves to a
            // selection. Without this, a stray pen-up — one whose press landed
            // outside the surface and was rejected — would silently clear a
            // selection the user is still working with.
            if self.lasso.is_empty() {
                return false;
            }

            self.selection = if self.lasso.len() >= 3 {
                self.document.select_in_polygon(&self.lasso)
            } else {
                // A tap rather than a loop: treat it as "deselect".
                Vec::new()
            };
            self.lasso.clear();
            return true;
        }

        if !self.drawing {
            return false;
        }

        self.drawing = false;

        // A single sample is a legitimate dot — the tittle on an `i`.
        if self.live.is_empty() {
            return false;
        }

        let (size, color, kind) = self.active_stroke_style();
        let stroke = InkStroke::from_points(&self.live, size, color, kind);
        self.document.push(stroke);
        self.live.clear();
        self.invalidate();
        true
    }

    /// Size, colour and kind the next committed stroke will carry.
    ///
    /// The highlighter's alpha is baked into the stored colour rather than
    /// applied at render time, so a page keeps its appearance even if the
    /// default opacity is retuned later.
    pub fn active_stroke_style(&self) -> (f32, [f32; 4], document::StrokeKind) {
        if self.tool == Tool::Highlighter {
            let mut color = rgba(self.highlighter_color);
            color[3] = HIGHLIGHTER_ALPHA;
            (self.highlighter_size, color, document::StrokeKind::Highlighter)
        } else {
            (self.options.size, rgba(self.color), document::StrokeKind::Pen)
        }
    }

    /// Render options and colour for the stroke currently under the pen.
    pub fn live_style(&self) -> (StrokeOptions, Color) {
        let (size, color, kind) = self.active_stroke_style();
        let options = options_for(kind, StrokeOptions {
            size,
            ..self.options
        });
        (options, Color::from_rgba(color[0], color[1], color[2], color[3]))
    }

    /// Set the page's background ruling.
    pub fn set_paper(&mut self, paper: document::PaperStyle) {
        if self.document.paper != paper {
            self.document.paper = paper;
            self.dirty = true;
            self.cache.clear();
        }
    }

    /// Drop an in-progress stroke or lasso without committing it.
    pub fn cancel_stroke(&mut self) {
        self.live.clear();
        self.lasso.clear();
        self.radial = None;
        self.hover = None;
        if self.erasing {
            // Close the gesture rather than leaving the document collecting
            // edits into a batch nothing will ever commit.
            self.document.end_batch();
        }
        self.drawing = false;
        self.erasing = false;
        self.drag_from = None;
        self.drag_total = Vec2::new(0.0, 0.0);
    }

    fn erase_at(&mut self, position: Vec2) -> bool {
        let radius = self.eraser_radius();
        let erased = match self.eraser_mode {
            EraserMode::Partial => self.document.erase_partial_at(position, radius),
            EraserMode::Stroke => self.document.erase_at(position, radius),
        };
        if erased {
            // Removing strokes shifts every later index, so any selection
            // pointing at them is now meaningless.
            self.selection.clear();
            self.invalidate();
            true
        } else {
            false
        }
    }

    pub fn undo(&mut self) -> bool {
        let changed = self.document.undo();
        if changed {
            self.selection.clear();
            self.invalidate();
        }
        changed
    }

    pub fn redo(&mut self) -> bool {
        let changed = self.document.redo();
        if changed {
            self.selection.clear();
            self.invalidate();
        }
        changed
    }

    pub fn clear(&mut self) -> bool {
        let changed = self.document.clear();
        if changed {
            self.selection.clear();
            self.invalidate();
        }
        changed
    }

    /// Replace the page with a loaded document.
    pub fn load(&mut self, document: InkDocument, path: Option<String>) {
        self.document = document;
        self.path = path;
        self.dirty = false;
        self.cancel_stroke();
        self.selection.clear();
        self.camera.reset();
        self.cache.clear();
    }

    /// Mark the rendered page stale and the document unsaved.
    fn invalidate(&mut self) {
        self.cache.clear();
        self.dirty = true;
    }
}

fn rgba(color: Color) -> [f32; 4] {
    [color.r, color.g, color.b, color.a]
}

/// Absolute path of a vault-relative ink page, with the usual traversal check.
fn resolve(vault_root: &str, rel_path: &str) -> Result<std::path::PathBuf, String> {
    md_editor_core::vault::resolve_vault_path_checked(std::path::Path::new(vault_root), rel_path)
}

/// Write a page to the vault.
///
/// Deliberately bypasses `vault::save_file`: that mirrors content into the
/// full-text search index, and a page of stroke coordinates would only
/// pollute search results.
pub fn save_to_vault(
    vault_root: &str,
    rel_path: &str,
    document: &InkDocument,
) -> Result<(), String> {
    let path = resolve(vault_root, rel_path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
    }
    std::fs::write(&path, document.to_json()?)
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))
}

/// Read a page back from the vault.
pub fn load_from_vault(vault_root: &str, rel_path: &str) -> Result<InkDocument, String> {
    let path = resolve(vault_root, rel_path)?;
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    InkDocument::from_json(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface() -> InkState {
        // Pretend the surface occupies the window below a toolbar.
        set_viewport(Rectangle {
            x: 200.0,
            y: 40.0,
            width: 800.0,
            height: 600.0,
        });
        InkState::new()
    }

    fn sample(phase: PenPhase, x: f32, y: f32, pressure: f32) -> PenSample {
        PenSample {
            phase,
            x,
            y,
            pressure,
            barrel: false,
            eraser: false,
        }
    }

    #[test]
    fn samples_map_into_surface_local_coordinates() {
        // At 2x scale, a client point of (600, 240) is logical (300, 120),
        // which is (100, 80) inside a surface starting at (200, 40).
        set_viewport(Rectangle {
            x: 200.0,
            y: 40.0,
            width: 800.0,
            height: 600.0,
        });
        let mapped = map_to_canvas(&sample(PenPhase::Down, 600.0, 240.0, 0.5), 2.0, true).unwrap();
        assert!((mapped.x - 100.0).abs() < 0.01);
        assert!((mapped.y - 80.0).abs() < 0.01);
    }

    /// A surface sitting below a toolbar and right of a sidebar.
    fn surface_bounds() -> Rectangle {
        Rectangle {
            x: 200.0,
            y: 90.0,
            width: 800.0,
            height: 600.0,
        }
    }

    #[test]
    fn pen_presses_off_the_canvas_are_left_for_the_ui() {
        // The pen hook only claims input that lands on the surface; anywhere
        // else it falls through so the pen can press toolbar buttons.
        let bounds = surface_bounds();
        assert!(point_is_on_canvas(400.0, 300.0, bounds, 1.0), "middle of the canvas");
        assert!(!point_is_on_canvas(400.0, 40.0, bounds, 1.0), "up on the toolbar");
        assert!(!point_is_on_canvas(80.0, 300.0, bounds, 1.0), "over the sidebar");
        assert!(!point_is_on_canvas(400.0, 800.0, bounds, 1.0), "below the canvas");
    }

    #[test]
    fn the_canvas_hit_test_accounts_for_display_scaling() {
        let bounds = surface_bounds();
        // Physical (800, 400) is logical (400, 200) — inside.
        assert!(point_is_on_canvas(800.0, 400.0, bounds, 2.0));
        // Physical (400, 100) is logical (200, 50) — above the surface.
        assert!(!point_is_on_canvas(400.0, 100.0, bounds, 2.0));
    }

    #[test]
    fn a_nonsense_scale_factor_does_not_break_the_hit_test() {
        let bounds = surface_bounds();
        assert!(point_is_on_canvas(400.0, 300.0, bounds, 0.0));
    }

    #[test]
    fn a_stroke_starting_outside_the_surface_is_ignored() {
        set_viewport(Rectangle {
            x: 200.0,
            y: 40.0,
            width: 800.0,
            height: 600.0,
        });
        // Over the sidebar, left of the surface.
        assert!(map_to_canvas(&sample(PenPhase::Down, 50.0, 100.0, 0.5), 1.0, true).is_none());
        // The same point mid-stroke is allowed through.
        assert!(map_to_canvas(&sample(PenPhase::Move, 50.0, 100.0, 0.5), 1.0, false).is_some());
    }

    #[test]
    fn a_full_pen_gesture_commits_one_stroke() {
        let mut ink = surface();
        let samples = vec![
            sample(PenPhase::Down, 300.0, 140.0, 0.4),
            sample(PenPhase::Move, 320.0, 150.0, 0.6),
            sample(PenPhase::Move, 340.0, 160.0, 0.8),
            sample(PenPhase::Up, 360.0, 170.0, 0.2),
        ];

        assert!(ink.apply_samples(&samples, 1.0));
        assert_eq!(ink.document.strokes().len(), 1);
        assert!(ink.live_points().is_empty(), "live stroke should be committed");
        assert!(ink.dirty);
    }

    #[test]
    fn an_inverted_pen_erases_without_switching_tools() {
        let mut ink = surface();
        ink.apply_samples(
            &[
                sample(PenPhase::Down, 300.0, 140.0, 0.5),
                sample(PenPhase::Move, 400.0, 140.0, 0.5),
                sample(PenPhase::Up, 500.0, 140.0, 0.5),
            ],
            1.0,
        );
        assert_eq!(ink.document.strokes().len(), 1);

        let mut erase = sample(PenPhase::Down, 400.0, 140.0, 0.5);
        erase.eraser = true;
        ink.apply_samples(&[erase], 1.0);

        assert_eq!(ink.document.strokes().len(), 0, "the inverted end should erase");
        assert_eq!(ink.tool, Tool::Pen, "erasing by inversion must not change the tool");
    }

    #[test]
    fn hiding_the_surface_stops_capture_and_drops_a_partial_stroke() {
        let mut ink = surface();
        ink.set_visible(true);
        assert!(pen::is_capturing());

        ink.apply_samples(&[sample(PenPhase::Down, 300.0, 140.0, 0.5)], 1.0);
        assert!(!ink.live_points().is_empty());

        ink.set_visible(false);
        assert!(!pen::is_capturing());
        assert!(ink.live_points().is_empty());
        assert_eq!(ink.document.strokes().len(), 0, "an abandoned stroke should not commit");
    }

    #[test]
    fn undo_after_a_stroke_restores_the_blank_page() {
        let mut ink = surface();
        ink.begin(Vec2::new(10.0, 10.0), 0.5, false);
        ink.extend(Vec2::new(40.0, 10.0), 0.5);
        ink.finish();
        assert_eq!(ink.document.strokes().len(), 1);

        assert!(ink.undo());
        assert!(ink.document.is_empty());
        assert!(ink.redo());
        assert_eq!(ink.document.strokes().len(), 1);
    }

    #[test]
    fn a_tap_leaves_a_dot() {
        let mut ink = surface();
        ink.begin(Vec2::new(10.0, 10.0), 0.7, false);
        assert!(ink.finish(), "a single sample should still commit");
        assert_eq!(ink.document.strokes().len(), 1);
    }

    /// Draw a horizontal stroke in screen space from `(x0, y)` to `(x1, y)`.
    fn draw_line(ink: &mut InkState, x0: f32, x1: f32, y: f32) {
        ink.begin(Vec2::new(x0, y), 0.8, false);
        let steps = 12;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            ink.extend(Vec2::new(x0 + (x1 - x0) * t, y), 0.8);
        }
        ink.finish();
    }

    #[test]
    fn strokes_are_stored_in_page_space_so_zoom_does_not_move_them() {
        let mut ink = InkState::new();
        draw_line(&mut ink, 100.0, 200.0, 50.0);
        let before = ink.document.strokes()[0].clone();

        ink.zoom_by_wheel(Vec2::new(0.0, 0.0), 4.0);

        assert_eq!(
            ink.document.strokes()[0],
            before,
            "zooming must not rewrite the document"
        );
    }

    #[test]
    fn drawing_while_zoomed_lands_where_the_pen_is() {
        let mut ink = InkState::new();
        // Zoom 2x about the origin: screen (200, 100) is page (100, 50).
        ink.camera.zoom_about(Vec2::new(0.0, 0.0), 2.0);

        ink.begin(Vec2::new(200.0, 100.0), 0.8, false);
        ink.finish();

        let point = ink.document.strokes()[0].points[0];
        assert!((point[0] - 100.0).abs() < 0.01, "got x = {}", point[0]);
        assert!((point[1] - 50.0).abs() < 0.01, "got y = {}", point[1]);
    }

    #[test]
    fn the_eraser_covers_a_constant_area_on_screen() {
        let mut ink = InkState::new();
        let at_1x = ink.eraser_radius();

        ink.camera.zoom_about(Vec2::new(0.0, 0.0), 2.0);
        let at_2x = ink.eraser_radius();

        assert!(
            (at_2x - at_1x / 2.0).abs() < 0.001,
            "the eraser tip should stay the same size on screen: {at_1x} then {at_2x}"
        );
    }

    #[test]
    fn a_lasso_selects_the_strokes_it_encloses() {
        let mut ink = InkState::new();
        draw_line(&mut ink, 10.0, 60.0, 20.0); // inside
        draw_line(&mut ink, 10.0, 60.0, 400.0); // far below

        ink.tool = Tool::Lasso;
        ink.begin(Vec2::new(0.0, 0.0), 0.0, false);
        ink.extend(Vec2::new(80.0, 0.0), 0.0);
        ink.extend(Vec2::new(80.0, 60.0), 0.0);
        ink.extend(Vec2::new(0.0, 60.0), 0.0);
        ink.finish();

        assert_eq!(ink.selection(), &[0], "only the enclosed stroke");
        assert!(ink.has_selection());
    }

    #[test]
    fn dragging_a_selection_moves_it_as_one_undo_step() {
        let mut ink = InkState::new();
        draw_line(&mut ink, 10.0, 60.0, 20.0);
        let original = ink.document.strokes()[0].clone();

        ink.tool = Tool::Lasso;
        ink.begin(Vec2::new(0.0, 0.0), 0.0, false);
        ink.extend(Vec2::new(80.0, 0.0), 0.0);
        ink.extend(Vec2::new(80.0, 60.0), 0.0);
        ink.extend(Vec2::new(0.0, 60.0), 0.0);
        ink.finish();
        assert_eq!(ink.selection(), &[0]);

        // Grab inside the selection and drag in two steps.
        ink.begin(Vec2::new(30.0, 20.0), 0.0, false);
        ink.extend(Vec2::new(50.0, 20.0), 0.0);
        ink.extend(Vec2::new(70.0, 25.0), 0.0);
        ink.finish();

        let moved = ink.document.strokes()[0].points[0];
        assert!((moved[0] - (original.points[0][0] + 40.0)).abs() < 0.01);
        assert!((moved[1] - (original.points[0][1] + 5.0)).abs() < 0.01);

        // One undo should return the whole drag, not just its last step.
        // Compared approximately: translating in place is float arithmetic, so
        // a round trip lands within rounding error rather than bit-identical.
        assert!(ink.undo());
        let restored = &ink.document.strokes()[0];
        assert_eq!(restored.points.len(), original.points.len());
        for (after, before) in restored.points.iter().zip(&original.points) {
            assert!(
                (after[0] - before[0]).abs() < 0.001 && (after[1] - before[1]).abs() < 0.001,
                "undo should put the stroke back: {after:?} vs {before:?}"
            );
        }
    }

    #[test]
    fn deleting_a_selection_removes_it_and_clears_the_highlight() {
        let mut ink = InkState::new();
        draw_line(&mut ink, 10.0, 60.0, 20.0);

        ink.tool = Tool::Lasso;
        ink.begin(Vec2::new(0.0, 0.0), 0.0, false);
        ink.extend(Vec2::new(80.0, 0.0), 0.0);
        ink.extend(Vec2::new(80.0, 60.0), 0.0);
        ink.extend(Vec2::new(0.0, 60.0), 0.0);
        ink.finish();

        assert!(ink.delete_selection());
        assert!(ink.document.is_empty());
        assert!(!ink.has_selection());
        assert!(!ink.delete_selection(), "nothing left to delete");
    }

    #[test]
    fn undo_drops_a_selection_whose_indices_would_be_stale() {
        let mut ink = InkState::new();
        draw_line(&mut ink, 10.0, 60.0, 20.0);

        ink.tool = Tool::Lasso;
        ink.begin(Vec2::new(0.0, 0.0), 0.0, false);
        ink.extend(Vec2::new(80.0, 0.0), 0.0);
        ink.extend(Vec2::new(80.0, 60.0), 0.0);
        ink.extend(Vec2::new(0.0, 60.0), 0.0);
        ink.finish();
        assert!(ink.has_selection());

        ink.undo();
        assert!(
            !ink.has_selection(),
            "indices into a changed document must not survive"
        );
    }

    #[test]
    fn a_lasso_with_too_few_points_selects_nothing() {
        let mut ink = InkState::new();
        draw_line(&mut ink, 10.0, 60.0, 20.0);

        ink.tool = Tool::Lasso;
        ink.begin(Vec2::new(0.0, 0.0), 0.0, false);
        ink.extend(Vec2::new(1.0, 1.0), 0.0);
        ink.finish();

        assert!(!ink.has_selection());
    }

    #[test]
    fn a_highlighter_stroke_is_translucent_flat_and_wide() {
        let mut ink = InkState::new();
        ink.tool = Tool::Highlighter;
        draw_line(&mut ink, 10.0, 60.0, 20.0);

        let stroke = &ink.document.strokes()[0];
        assert_eq!(stroke.kind, document::StrokeKind::Highlighter);
        assert!(
            (stroke.color[3] - HIGHLIGHTER_ALPHA).abs() < 0.001,
            "highlighter ink must be translucent"
        );
        assert!(
            stroke.size > StrokeOptions::default().size * 2.0,
            "a highlighter should be much broader than the pen"
        );

        // Pressure must not modulate the width, or it reads as a fat pen.
        let options = options_for(stroke.kind, ink.options);
        assert_eq!(options.thinning, 0.0);
        assert_eq!(options.taper_start, 0.0);
        assert_eq!(options.taper_end, 0.0);
    }

    #[test]
    fn switching_tools_keeps_each_tools_colour_and_width() {
        let mut ink = InkState::new();
        let pen_color = ink.color;
        let pen_size = ink.options.size;

        ink.tool = Tool::Highlighter;
        ink.highlighter_color = COLOR_PRESETS[3].1;
        ink.highlighter_size = 30.0;

        ink.tool = Tool::Pen;
        assert_eq!(ink.color, pen_color, "the pen keeps its own colour");
        assert_eq!(ink.options.size, pen_size, "and its own width");
    }

    #[test]
    fn an_eraser_sweep_is_one_undo_step_through_the_state_layer() {
        let mut ink = InkState::new();
        draw_line(&mut ink, 0.0, 50.0, 10.0);
        draw_line(&mut ink, 0.0, 50.0, 40.0);
        assert_eq!(ink.document.strokes().len(), 2);

        ink.tool = Tool::Eraser;
        ink.eraser_mode = EraserMode::Stroke;
        // A single drag across both lines.
        ink.begin(Vec2::new(25.0, 10.0), 0.0, false);
        ink.extend(Vec2::new(25.0, 25.0), 0.0);
        ink.extend(Vec2::new(25.0, 40.0), 0.0);
        ink.finish();
        assert!(ink.document.is_empty());

        assert!(ink.undo());
        assert_eq!(
            ink.document.strokes().len(),
            2,
            "one undo should restore the whole sweep, not one stroke"
        );
    }

    #[test]
    fn abandoning_an_eraser_sweep_does_not_strand_the_batch() {
        let mut ink = InkState::new();
        draw_line(&mut ink, 0.0, 50.0, 10.0);

        ink.tool = Tool::Eraser;
        ink.eraser_mode = EraserMode::Stroke;
        ink.begin(Vec2::new(25.0, 10.0), 0.0, false);
        // Surface dismissed mid-sweep.
        ink.set_visible(false);

        assert!(ink.document.is_empty());
        assert!(
            ink.undo(),
            "the interrupted sweep should still be on the history"
        );
        assert_eq!(ink.document.strokes().len(), 1);
    }

    #[test]
    fn the_partial_eraser_leaves_the_rest_of_the_stroke() {
        let mut ink = InkState::new();
        draw_line(&mut ink, 0.0, 100.0, 20.0);

        ink.tool = Tool::Eraser;
        ink.eraser_mode = EraserMode::Partial;
        ink.begin(Vec2::new(50.0, 20.0), 0.0, false);
        ink.finish();

        assert_eq!(
            ink.document.strokes().len(),
            2,
            "a partial erase should cut the line, not remove it"
        );
    }

    #[test]
    fn a_stray_pen_up_does_not_clear_the_selection() {
        let mut ink = InkState::new();
        draw_line(&mut ink, 10.0, 60.0, 20.0);

        ink.tool = Tool::Lasso;
        ink.begin(Vec2::new(0.0, 0.0), 0.0, false);
        ink.extend(Vec2::new(80.0, 0.0), 0.0);
        ink.extend(Vec2::new(80.0, 60.0), 0.0);
        ink.extend(Vec2::new(0.0, 60.0), 0.0);
        ink.finish();
        assert!(ink.has_selection());

        // A lift with no matching press — the press landed off-surface and was
        // rejected, so no lasso was ever started.
        assert!(!ink.finish(), "nothing happened, so nothing changed");
        assert!(
            ink.has_selection(),
            "the selection must survive an unmatched pen-up"
        );
    }

    #[test]
    fn a_lasso_tap_deselects() {
        let mut ink = InkState::new();
        draw_line(&mut ink, 10.0, 60.0, 20.0);

        ink.tool = Tool::Lasso;
        ink.begin(Vec2::new(0.0, 0.0), 0.0, false);
        ink.extend(Vec2::new(80.0, 0.0), 0.0);
        ink.extend(Vec2::new(80.0, 60.0), 0.0);
        ink.extend(Vec2::new(0.0, 60.0), 0.0);
        ink.finish();
        assert!(ink.has_selection());

        // Tap on empty space: a lasso is started but never becomes a loop.
        ink.begin(Vec2::new(300.0, 300.0), 0.0, false);
        assert!(ink.finish());
        assert!(!ink.has_selection(), "a tap should drop the selection");
    }

    #[test]
    fn the_starting_colour_is_one_of_the_swatches() {
        // Regression: the default was the theme's 8-bit text colour while the
        // first preset was an f32 literal, so no swatch ever read as selected.
        let ink = InkState::new();
        assert!(
            COLOR_PRESETS
                .iter()
                .any(|(_, preset)| same_color(ink.color, *preset)),
            "the palette must show a selection from the first frame"
        );
    }

    #[test]
    fn every_preset_matches_only_itself() {
        for (name, a) in COLOR_PRESETS {
            let matches = COLOR_PRESETS
                .iter()
                .filter(|(_, b)| same_color(a, *b))
                .count();
            assert_eq!(matches, 1, "{name} should be distinguishable from every other swatch");
        }
    }

    #[test]
    fn colour_matching_tolerates_eight_bit_rounding() {
        // The same colour authored two ways must still compare equal.
        let from_bytes = Color::from_rgb8(227, 229, 237);
        let from_floats = Color::from_rgb(0.890, 0.898, 0.929);
        assert!(same_color(from_bytes, from_floats));
        assert!(!same_color(from_bytes, COLOR_PRESETS[1].1));
    }

    fn hover_sample(x: f32, y: f32) -> PenSample {
        PenSample {
            phase: PenPhase::Hover,
            x,
            y,
            pressure: 0.0,
            eraser: false,
            barrel: false,
        }
    }

    #[test]
    fn hovering_reports_where_the_nib_is_aimed() {
        let mut ink = surface();
        assert!(ink.hover().is_none());

        assert!(ink.apply_samples(&[hover_sample(300.0, 140.0)], 1.0));
        let aim = ink.hover().expect("hover should be tracked");
        // Surface starts at (200, 40), so client (300, 140) is local (100, 100).
        assert!((aim.x - 100.0).abs() < 0.01);
        assert!((aim.y - 100.0).abs() < 0.01);

        // Hovering must not draw anything.
        assert!(ink.document.is_empty());
    }

    #[test]
    fn the_aim_indicator_follows_an_eraser_drag() {
        // Regression: the eraser ring was positioned from iced's mouse cursor,
        // which pen input never updates, so it sat frozen wherever the mouse
        // happened to be while the pen erased elsewhere.
        let mut ink = surface();
        ink.tool = Tool::Eraser;

        ink.begin(Vec2::new(50.0, 60.0), 0.0, false);
        assert_eq!(ink.hover(), Some(Vec2::new(50.0, 60.0)));

        ink.extend(Vec2::new(180.0, 210.0), 0.0);
        assert_eq!(
            ink.hover(),
            Some(Vec2::new(180.0, 210.0)),
            "the ring must track the tip through the drag"
        );
    }

    #[test]
    fn a_mouse_move_updates_the_same_aim_state_as_the_pen() {
        let mut ink = surface();
        ink.apply_samples(&[hover_sample(300.0, 140.0)], 1.0);
        assert_eq!(ink.hover(), Some(Vec2::new(100.0, 100.0)));

        // Mouse positions arrive already surface-local.
        ink.set_hover(Vec2::new(400.0, 320.0));
        assert_eq!(ink.hover(), Some(Vec2::new(400.0, 320.0)));
    }

    #[test]
    fn leaving_range_clears_the_aim_indicator() {
        let mut ink = surface();
        ink.apply_samples(&[hover_sample(300.0, 140.0)], 1.0);
        assert!(ink.hover().is_some());

        let leave = PenSample {
            phase: PenPhase::Leave,
            ..hover_sample(0.0, 0.0)
        };
        assert!(ink.apply_samples(&[leave], 1.0));
        assert!(ink.hover().is_none());
    }

    #[test]
    fn writing_into_the_bottom_margin_pulls_the_page_up() {
        // Surface is 600 tall, so the advance threshold is 600 - 120 = 480.
        let mut ink = surface();
        let before = ink.camera.pan.y;

        ink.begin(Vec2::new(100.0, 400.0), 0.8, false);
        assert_eq!(ink.camera.pan.y, before, "above the margin, nothing moves");

        ink.extend(Vec2::new(100.0, 540.0), 0.8);
        assert!(
            ink.camera.pan.y < before,
            "writing into the margin should advance the page"
        );
    }

    #[test]
    fn advancing_keeps_the_stroke_where_it_was_written() {
        let mut ink = surface();
        ink.begin(Vec2::new(100.0, 540.0), 0.8, false);
        let first = ink.live_points()[0].pos;

        ink.extend(Vec2::new(110.0, 545.0), 0.8);
        assert_eq!(
            ink.live_points()[0].pos,
            first,
            "the page moves under the ink, not the other way round"
        );
    }

    #[test]
    fn the_barrel_button_opens_the_menu_instead_of_drawing() {
        let mut ink = surface();
        let mut press = hover_sample(300.0, 140.0);
        press.phase = PenPhase::Down;
        press.barrel = true;
        press.pressure = 0.7;

        ink.apply_samples(&[press], 1.0);
        assert!(ink.radial_menu().is_some(), "the side button summons the ring");
        assert!(ink.live_points().is_empty(), "and lays down no ink");
    }

    #[test]
    fn the_menu_applies_the_slot_the_pen_lifts_on() {
        let mut ink = surface();
        assert_eq!(ink.tool, Tool::Pen);

        let mut press = hover_sample(300.0, 240.0);
        press.phase = PenPhase::Down;
        press.barrel = true;
        ink.apply_samples(&[press], 1.0);

        // Flick right, into the second tool slot, then lift.
        let mut moved = hover_sample(300.0 + radial::TOOL_RADIUS, 240.0);
        moved.phase = PenPhase::Move;
        let mut lifted = moved;
        lifted.phase = PenPhase::Up;
        ink.apply_samples(&[moved, lifted], 1.0);

        assert!(ink.radial_menu().is_none(), "lifting closes the menu");
        assert_eq!(ink.tool, Tool::Highlighter);
        assert!(ink.document.is_empty(), "the gesture must not leave a stroke");
    }

    #[test]
    fn lifting_in_the_dead_zone_cancels_the_menu() {
        let mut ink = surface();
        ink.tool = Tool::Eraser;

        let mut press = hover_sample(300.0, 240.0);
        press.phase = PenPhase::Down;
        press.barrel = true;
        ink.apply_samples(&[press], 1.0);

        let mut lifted = press;
        lifted.phase = PenPhase::Up;
        ink.apply_samples(&[lifted], 1.0);

        assert!(ink.radial_menu().is_none());
        assert_eq!(ink.tool, Tool::Eraser, "no travel means no change");
    }

    #[test]
    fn the_menu_can_set_a_colour_without_touching_the_tool() {
        let mut ink = surface();
        let target = COLOR_PRESETS[3].1;

        let mut press = hover_sample(400.0, 340.0);
        press.phase = PenPhase::Down;
        press.barrel = true;
        ink.apply_samples(&[press], 1.0);

        // Straight up into the outer ring is the first colour slot; walk round
        // to the fourth by angle instead.
        let menu = ink.radial_menu().unwrap();
        let slot = radial::slot_position(menu.center, 3, COLOR_PRESETS.len(), radial::COLOR_RADIUS);
        let mut moved = hover_sample(slot.x + 200.0, slot.y + 40.0);
        moved.phase = PenPhase::Move;
        let mut lifted = moved;
        lifted.phase = PenPhase::Up;
        ink.apply_samples(&[moved, lifted], 1.0);

        assert_eq!(ink.tool, Tool::Pen, "picking a colour must not change tools");
        assert!(same_color(ink.color, target));
    }

    #[test]
    fn changing_paper_marks_the_page_unsaved() {
        let mut ink = InkState::new();
        assert!(!ink.dirty);
        ink.set_paper(document::PaperStyle::Grid);
        assert_eq!(ink.document.paper, document::PaperStyle::Grid);
        assert!(ink.dirty);
    }

    #[test]
    fn panning_does_not_alter_the_document() {
        let mut ink = InkState::new();
        draw_line(&mut ink, 10.0, 60.0, 20.0);
        let before = ink.document.strokes()[0].clone();

        assert!(ink.pan_by(Vec2::new(120.0, -40.0)));
        assert!(!ink.pan_by(Vec2::new(0.0, 0.0)), "a zero pan is not a change");
        assert_eq!(ink.document.strokes()[0], before);

        assert!(ink.reset_view());
        assert!(!ink.reset_view(), "already at the identity view");
    }

    #[test]
    fn vault_round_trip_preserves_the_page() {
        let dir = std::env::temp_dir().join(format!("md_ink_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.to_str().unwrap();

        let mut ink = InkState::new();
        ink.begin(Vec2::new(1.0, 2.0), 0.5, false);
        ink.extend(Vec2::new(9.0, 12.0), 0.9);
        ink.finish();

        save_to_vault(root, "notes/page.ink", &ink.document).unwrap();
        let loaded = load_from_vault(root, "notes/page.ink").unwrap();
        assert_eq!(loaded.strokes(), ink.document.strokes());

        let _ = std::fs::remove_dir_all(&dir);
    }
}

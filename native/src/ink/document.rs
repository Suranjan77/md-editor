//! The ink document: the strokes on a page, their edit history, and the
//! on-disk format.
//!
//! Strokes are stored as the *sampled pen path* rather than the rendered
//! outline. The outline is a function of the path plus the current
//! [`StrokeOptions`], so keeping the path means a document can be re-rendered
//! at any zoom or with retuned smoothing without losing fidelity — and the
//! file stays small.

use serde::{Deserialize, Serialize};

use super::stroke::{self, InkPoint, Vec2};

/// Share of a stroke's samples that must fall inside a lasso for it to be
/// selected. Requiring most — rather than any — of the stroke keeps a lasso
/// from grabbing long neighbours it merely clips.
const SELECTION_THRESHOLD: f32 = 0.6;

/// Current on-disk format version. Bumped when the layout changes in a way
/// older readers cannot handle.
pub const FORMAT_VERSION: u32 = 1;

/// What kind of mark a stroke is.
///
/// Every note app separates these two, and they differ in more than colour:
/// a highlighter is translucent, ignores pressure, and sits *behind* writing
/// so it never obscures it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StrokeKind {
    #[default]
    Pen,
    Highlighter,
}

/// Background ruling drawn under the ink.
///
/// Writing on an empty void is materially harder than writing on a ruled page,
/// which is why every established note app ships these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PaperStyle {
    Plain,
    Ruled,
    Grid,
    /// The default: dots guide handwriting without ruling it, and stay out of
    /// the way of diagrams in a way solid lines do not.
    #[default]
    Dotted,
}

impl PaperStyle {
    pub const ALL: [(&'static str, PaperStyle); 4] = [
        ("Plain", PaperStyle::Plain),
        ("Ruled", PaperStyle::Ruled),
        ("Grid", PaperStyle::Grid),
        ("Dots", PaperStyle::Dotted),
    ];
}

/// A single stroke: the pen path, plus how it should be drawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InkStroke {
    /// Nominal stroke diameter in logical pixels at full pressure.
    pub size: f32,
    /// Stroke colour as linear RGBA.
    pub color: [f32; 4],
    /// Pen or highlighter. Defaulted so pages written before highlighters
    /// existed still load.
    #[serde(default)]
    pub kind: StrokeKind,
    /// `[x, y, pressure]` per sample. A flat triple rather than a struct keeps
    /// the JSON compact — a page of handwriting is tens of thousands of
    /// samples.
    pub points: Vec<[f32; 3]>,
}

impl InkStroke {
    pub fn from_points(
        points: &[InkPoint],
        size: f32,
        color: [f32; 4],
        kind: StrokeKind,
    ) -> Self {
        Self {
            size,
            color,
            kind,
            points: points
                .iter()
                .map(|p| [p.pos.x, p.pos.y, p.pressure])
                .collect(),
        }
    }

    pub fn ink_points(&self) -> Vec<InkPoint> {
        self.points
            .iter()
            .map(|[x, y, pressure]| InkPoint::new(*x, *y, *pressure))
            .collect()
    }

    /// Whether any part of the pen path passes within `radius` of `probe`.
    ///
    /// Tests the recorded path rather than the rendered outline: it is much
    /// cheaper, and an eraser that follows the visible line is what a user
    /// expects anyway.
    pub fn hit_by(&self, probe: Vec2, radius: f32) -> bool {
        if self.points.is_empty() {
            return false;
        }

        let point = |i: usize| Vec2::new(self.points[i][0], self.points[i][1]);

        if self.points.len() == 1 {
            return distance_to_segment(probe, point(0), point(0)) <= radius;
        }

        (0..self.points.len() - 1)
            .any(|i| distance_to_segment(probe, point(i), point(i + 1)) <= radius)
    }

    /// Axis-aligned bounds of the pen path, ignoring stroke width. Used to
    /// cull strokes outside the viewport before tessellating them.
    pub fn path_bounds(&self) -> Option<(Vec2, Vec2)> {
        let points: Vec<Vec2> = self
            .points
            .iter()
            .map(|[x, y, _]| Vec2::new(*x, *y))
            .collect();
        stroke::bounds(&points)
    }

    /// Shift every sample by `delta`.
    fn translate(&mut self, delta: Vec2) {
        for point in &mut self.points {
            point[0] += delta.x;
            point[1] += delta.y;
        }
    }

    /// Fraction of this stroke's samples that fall inside `polygon`.
    fn fraction_inside(&self, polygon: &[Vec2]) -> f32 {
        if self.points.is_empty() {
            return 0.0;
        }
        let inside = self
            .points
            .iter()
            .filter(|[x, y, _]| point_in_polygon(Vec2::new(*x, *y), polygon))
            .count();
        inside as f32 / self.points.len() as f32
    }
}

/// Whether `probe` lies inside `polygon`, by ray casting: count how many edges
/// a ray to the right crosses; odd means inside.
pub fn point_in_polygon(probe: Vec2, polygon: &[Vec2]) -> bool {
    if polygon.len() < 3 {
        return false;
    }

    let mut inside = false;
    let mut j = polygon.len() - 1;

    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[j];

        // Does the edge straddle the probe's row?
        if (a.y > probe.y) != (b.y > probe.y) {
            // x of the edge at the probe's y.
            let crossing = a.x + (probe.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if probe.x < crossing {
                inside = !inside;
            }
        }
        j = i;
    }

    inside
}

/// Cut the part of `stroke` lying within `radius` of `probe`, returning the
/// fragments that survive.
///
/// Returns `None` when the eraser did not touch the stroke, so the caller can
/// leave it — and its history — alone.
///
/// Single-sample fragments are dropped: they would render as a trail of dots
/// scattered along the erased path, which reads as debris rather than as ink.
fn split_around(stroke: &InkStroke, probe: Vec2, radius: f32) -> Option<Vec<InkStroke>> {
    let keep: Vec<bool> = stroke
        .points
        .iter()
        .map(|[x, y, _]| Vec2::new(*x, *y).dist(probe) > radius)
        .collect();

    if keep.iter().all(|kept| *kept) {
        return None;
    }

    let mut fragments = Vec::new();
    let mut current: Vec<[f32; 3]> = Vec::new();

    for (index, kept) in keep.iter().enumerate() {
        if *kept {
            current.push(stroke.points[index]);
        } else if !current.is_empty() {
            fragments.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        fragments.push(current);
    }

    Some(
        fragments
            .into_iter()
            .filter(|points| points.len() >= 2)
            .map(|points| InkStroke {
                points,
                ..stroke.clone()
            })
            .collect(),
    )
}

/// Shortest distance from `p` to the segment `a`–`b`.
fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let abx = b.x - a.x;
    let aby = b.y - a.y;
    let len_sq = abx * abx + aby * aby;

    // Degenerate segment: fall back to point distance.
    if len_sq <= f32::EPSILON {
        return ((p.x - a.x).powi(2) + (p.y - a.y).powi(2)).sqrt();
    }

    let t = (((p.x - a.x) * abx + (p.y - a.y) * aby) / len_sq).clamp(0.0, 1.0);
    let cx = a.x + abx * t;
    let cy = a.y + aby * t;
    ((p.x - cx).powi(2) + (p.y - cy).powi(2)).sqrt()
}

/// One reversible edit.
#[derive(Debug, Clone)]
enum InkOp {
    /// A stroke was appended.
    Added(InkStroke),
    /// Strokes were erased, recorded with the indices they occupied so undo
    /// puts them back in their original z-order.
    Erased(Vec<(usize, InkStroke)>),
    /// A selection was dragged. Reversible by translating back.
    Moved { indices: Vec<usize>, delta: Vec2 },
    /// One stroke was replaced by the fragments left after a partial erase.
    Replaced {
        index: usize,
        original: InkStroke,
        replacements: Vec<InkStroke>,
    },
    /// Several edits that happened as one gesture and undo as one.
    Batch(Vec<InkOp>),
}

/// The strokes on a page, with undo/redo history.
#[derive(Debug, Default)]
pub struct InkDocument {
    strokes: Vec<InkStroke>,
    /// Background ruling. A property of the page, so it travels with the file.
    pub paper: PaperStyle,
    undo_stack: Vec<InkOp>,
    redo_stack: Vec<InkOp>,
    /// Edits collected since [`InkDocument::begin_batch`], if a gesture is in
    /// progress.
    pending: Option<Vec<InkOp>>,
}

impl InkDocument {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn strokes(&self) -> &[InkStroke] {
        &self.strokes
    }

    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Start collecting edits into a single undoable gesture.
    ///
    /// An eraser drag fires an edit per input sample; without batching, undo
    /// would rewind it one sample at a time, which is not how any note app
    /// behaves. Nested calls are ignored.
    pub fn begin_batch(&mut self) {
        if self.pending.is_none() {
            self.pending = Some(Vec::new());
        }
    }

    /// Close the current gesture, pushing it as one history entry. A gesture
    /// that changed nothing leaves no entry at all.
    pub fn end_batch(&mut self) {
        let Some(mut ops) = self.pending.take() else {
            return;
        };
        match ops.len() {
            0 => {}
            // A single edit needs no wrapper.
            1 => self.undo_stack.push(ops.remove(0)),
            _ => self.undo_stack.push(InkOp::Batch(ops)),
        }
    }

    /// Record an edit, into the open gesture if there is one.
    fn record(&mut self, op: InkOp) {
        // Any new edit abandons the redo branch, batched or not.
        self.redo_stack.clear();
        match &mut self.pending {
            Some(batch) => batch.push(op),
            None => self.undo_stack.push(op),
        }
    }

    /// Append a finished stroke.
    pub fn push(&mut self, stroke: InkStroke) {
        self.strokes.push(stroke.clone());
        self.record(InkOp::Added(stroke));
    }

    /// Erase every stroke passing within `radius` of `probe`, whole.
    ///
    /// Returns whether anything was removed, so the caller knows if the
    /// rendered cache needs rebuilding.
    pub fn erase_at(&mut self, probe: Vec2, radius: f32) -> bool {
        let mut removed = Vec::new();

        // Walk backwards so the recorded indices stay valid as we remove.
        for i in (0..self.strokes.len()).rev() {
            if self.strokes[i].hit_by(probe, radius) {
                removed.push((i, self.strokes.remove(i)));
            }
        }

        if removed.is_empty() {
            return false;
        }

        removed.reverse(); // back to ascending index order
        self.record(InkOp::Erased(removed));
        true
    }

    /// Rub out only the part of each stroke under the eraser, splitting the
    /// remainder into fragments.
    ///
    /// This is the "standard" eraser of GoodNotes and OneNote, as distinct
    /// from the stroke eraser above which takes a whole line at a touch.
    pub fn erase_partial_at(&mut self, probe: Vec2, radius: f32) -> bool {
        let mut changed = false;

        // Backwards so each splice leaves the lower indices untouched.
        for index in (0..self.strokes.len()).rev() {
            let Some(replacements) = split_around(&self.strokes[index], probe, radius) else {
                continue;
            };

            let original = self.strokes[index].clone();
            self.strokes
                .splice(index..index + 1, replacements.iter().cloned());
            self.record(InkOp::Replaced {
                index,
                original,
                replacements,
            });
            changed = true;
        }

        changed
    }

    /// Indices of the strokes mostly enclosed by `polygon`, in z-order.
    pub fn select_in_polygon(&self, polygon: &[Vec2]) -> Vec<usize> {
        self.strokes
            .iter()
            .enumerate()
            .filter(|(_, stroke)| stroke.fraction_inside(polygon) >= SELECTION_THRESHOLD)
            .map(|(index, _)| index)
            .collect()
    }

    /// Remove the strokes at `indices`, as one undoable step.
    pub fn remove_strokes(&mut self, indices: &[usize]) -> bool {
        if indices.is_empty() {
            return false;
        }

        // Descending so each removal leaves the lower indices valid.
        let mut ordered: Vec<usize> = indices.to_vec();
        ordered.sort_unstable();
        ordered.dedup();

        let mut removed = Vec::with_capacity(ordered.len());
        for &index in ordered.iter().rev() {
            if index < self.strokes.len() {
                removed.push((index, self.strokes.remove(index)));
            }
        }

        if removed.is_empty() {
            return false;
        }

        removed.reverse();
        self.record(InkOp::Erased(removed));
        true
    }

    /// Move strokes without recording an undo step.
    ///
    /// Used to track a drag in progress: the strokes follow the pen frame by
    /// frame, and only the completed gesture is pushed onto the history with
    /// [`InkDocument::record_move`].
    pub fn translate_without_history(&mut self, indices: &[usize], delta: Vec2) {
        self.apply_translation(indices, delta);
    }

    /// Record an already-applied move so it can be undone as one step.
    ///
    /// The strokes are left alone: a drag has already walked them into place
    /// sample by sample. Rewinding and re-applying to record it would be two
    /// extra passes over every point, and each pass compounds rounding error.
    pub fn record_move(&mut self, indices: &[usize], delta: Vec2) -> bool {
        if indices.is_empty() || (delta.x == 0.0 && delta.y == 0.0) {
            return false;
        }
        self.record(InkOp::Moved {
            indices: indices.to_vec(),
            delta,
        });
        true
    }

    fn apply_translation(&mut self, indices: &[usize], delta: Vec2) {
        for &index in indices {
            if let Some(stroke) = self.strokes.get_mut(index) {
                stroke.translate(delta);
            }
        }
    }

    /// Remove every stroke in one undoable step.
    pub fn clear(&mut self) -> bool {
        if self.strokes.is_empty() {
            return false;
        }
        let removed: Vec<(usize, InkStroke)> = self.strokes.drain(..).enumerate().collect();
        self.record(InkOp::Erased(removed));
        true
    }

    /// Undo the most recent edit. Returns whether anything changed.
    pub fn undo(&mut self) -> bool {
        let Some(op) = self.undo_stack.pop() else {
            return false;
        };
        self.undo_op(&op);
        self.redo_stack.push(op);
        true
    }

    /// Redo the most recently undone edit. Returns whether anything changed.
    pub fn redo(&mut self) -> bool {
        let Some(op) = self.redo_stack.pop() else {
            return false;
        };
        self.redo_op(&op);
        self.undo_stack.push(op);
        true
    }

    fn undo_op(&mut self, op: &InkOp) {
        match op {
            InkOp::Added(_) => {
                self.strokes.pop();
            }
            InkOp::Erased(removed) => {
                // Ascending order means each insert lands at the index it
                // originally held.
                for (index, stroke) in removed {
                    let at = (*index).min(self.strokes.len());
                    self.strokes.insert(at, stroke.clone());
                }
            }
            InkOp::Moved { indices, delta } => {
                let (indices, back) = (indices.clone(), delta.scale(-1.0));
                self.apply_translation(&indices, back);
            }
            InkOp::Replaced {
                index,
                original,
                replacements,
            } => {
                let end = (index + replacements.len()).min(self.strokes.len());
                if *index <= end {
                    self.strokes.splice(*index..end, [original.clone()]);
                }
            }
            // Reverse order: the ops were applied front to back, so unwinding
            // back to front keeps every recorded index valid.
            InkOp::Batch(ops) => {
                for inner in ops.iter().rev() {
                    self.undo_op(inner);
                }
            }
        }
    }

    fn redo_op(&mut self, op: &InkOp) {
        match op {
            InkOp::Added(stroke) => self.strokes.push(stroke.clone()),
            InkOp::Erased(removed) => {
                for (index, _) in removed.iter().rev() {
                    if *index < self.strokes.len() {
                        self.strokes.remove(*index);
                    }
                }
            }
            InkOp::Moved { indices, delta } => {
                let (indices, delta) = (indices.clone(), *delta);
                self.apply_translation(&indices, delta);
            }
            InkOp::Replaced {
                index,
                replacements,
                ..
            } => {
                if *index < self.strokes.len() {
                    self.strokes
                        .splice(*index..*index + 1, replacements.iter().cloned());
                }
            }
            InkOp::Batch(ops) => {
                for inner in ops {
                    self.redo_op(inner);
                }
            }
        }
    }

    /// Serialize to the on-disk JSON representation.
    pub fn to_json(&self) -> Result<String, String> {
        let file = InkFile {
            version: FORMAT_VERSION,
            paper: self.paper,
            strokes: self.strokes.clone(),
        };
        serde_json::to_string(&file).map_err(|e| format!("Failed to encode ink document: {e}"))
    }

    /// Parse a document previously written by [`InkDocument::to_json`].
    ///
    /// History is not persisted, so a freshly loaded document has nothing to
    /// undo.
    pub fn from_json(raw: &str) -> Result<Self, String> {
        let file: InkFile =
            serde_json::from_str(raw).map_err(|e| format!("Failed to parse ink document: {e}"))?;

        if file.version > FORMAT_VERSION {
            return Err(format!(
                "Ink document version {} is newer than this build understands ({FORMAT_VERSION})",
                file.version
            ));
        }

        Ok(Self {
            strokes: file.strokes,
            paper: file.paper,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            pending: None,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct InkFile {
    version: u32,
    /// Defaulted so pages written before paper styles existed still load.
    #[serde(default)]
    paper: PaperStyle,
    strokes: Vec<InkStroke>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

    fn line(y: f32) -> InkStroke {
        let points: Vec<InkPoint> = (0..10)
            .map(|i| InkPoint::new(i as f32 * 5.0, y, 0.8))
            .collect();
        InkStroke::from_points(&points, 3.0, BLACK, StrokeKind::Pen)
    }

    #[test]
    fn push_then_undo_removes_the_stroke() {
        let mut doc = InkDocument::new();
        doc.push(line(10.0));
        assert_eq!(doc.strokes().len(), 1);

        assert!(doc.undo());
        assert!(doc.is_empty());
        assert!(!doc.can_undo());
        assert!(doc.can_redo());
    }

    #[test]
    fn redo_restores_an_undone_stroke() {
        let mut doc = InkDocument::new();
        doc.push(line(10.0));
        doc.undo();

        assert!(doc.redo());
        assert_eq!(doc.strokes().len(), 1);
        assert!(!doc.can_redo());
    }

    #[test]
    fn new_edit_clears_the_redo_branch() {
        let mut doc = InkDocument::new();
        doc.push(line(10.0));
        doc.undo();
        assert!(doc.can_redo());

        doc.push(line(20.0));
        assert!(!doc.can_redo(), "a new stroke should discard the redo branch");
    }

    #[test]
    fn erase_removes_only_strokes_under_the_probe() {
        let mut doc = InkDocument::new();
        doc.push(line(10.0));
        doc.push(line(80.0));

        assert!(doc.erase_at(Vec2::new(20.0, 10.5), 4.0));
        assert_eq!(doc.strokes().len(), 1, "only the stroke under the eraser should go");
        assert!(!doc.erase_at(Vec2::new(500.0, 500.0), 4.0), "empty space erases nothing");
    }

    #[test]
    fn undoing_an_erase_restores_z_order() {
        let mut doc = InkDocument::new();
        let bottom = line(10.0);
        let middle = line(10.2);
        let top = line(80.0);
        doc.push(bottom.clone());
        doc.push(middle.clone());
        doc.push(top.clone());

        // Erases the two overlapping strokes, leaving the far one.
        assert!(doc.erase_at(Vec2::new(20.0, 10.1), 4.0));
        assert_eq!(doc.strokes().len(), 1);

        assert!(doc.undo());
        assert_eq!(doc.strokes(), &[bottom, middle, top][..], "z-order must survive undo");
    }

    #[test]
    fn clear_is_a_single_undoable_step() {
        let mut doc = InkDocument::new();
        doc.push(line(10.0));
        doc.push(line(20.0));
        doc.push(line(30.0));

        assert!(doc.clear());
        assert!(doc.is_empty());

        assert!(doc.undo());
        assert_eq!(doc.strokes().len(), 3, "one undo should bring back the whole page");
    }

    #[test]
    fn clearing_an_empty_document_is_not_an_edit() {
        let mut doc = InkDocument::new();
        assert!(!doc.clear());
        assert!(!doc.can_undo());
    }

    #[test]
    fn json_round_trip_preserves_strokes() {
        let mut doc = InkDocument::new();
        doc.push(line(10.0));
        doc.push(line(20.0));

        let json = doc.to_json().unwrap();
        let restored = InkDocument::from_json(&json).unwrap();

        assert_eq!(restored.strokes(), doc.strokes());
        assert!(!restored.can_undo(), "history is not persisted");
    }

    #[test]
    fn rejects_a_document_from_a_newer_build() {
        let raw = format!(r#"{{"version":{},"strokes":[]}}"#, FORMAT_VERSION + 1);
        assert!(InkDocument::from_json(&raw).is_err());
    }

    #[test]
    fn hit_test_measures_distance_to_the_path_not_the_endpoints() {
        let stroke = line(0.0); // spans x 0..45 at y = 0
        // Directly above the middle of the stroke, just outside the radius.
        assert!(stroke.hit_by(Vec2::new(22.0, 2.0), 3.0));
        assert!(!stroke.hit_by(Vec2::new(22.0, 9.0), 3.0));
    }

    /// Square lasso covering x,y in 0..50.
    fn square() -> Vec<Vec2> {
        vec![
            Vec2::new(-5.0, -5.0),
            Vec2::new(50.0, -5.0),
            Vec2::new(50.0, 50.0),
            Vec2::new(-5.0, 50.0),
        ]
    }

    #[test]
    fn point_in_polygon_distinguishes_inside_from_outside() {
        let poly = square();
        assert!(point_in_polygon(Vec2::new(20.0, 20.0), &poly));
        assert!(!point_in_polygon(Vec2::new(200.0, 20.0), &poly));
        assert!(!point_in_polygon(Vec2::new(20.0, -80.0), &poly));
    }

    #[test]
    fn a_degenerate_polygon_selects_nothing() {
        assert!(!point_in_polygon(
            Vec2::new(0.0, 0.0),
            &[Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)]
        ));
    }

    #[test]
    fn lasso_selects_enclosed_strokes_only() {
        let mut doc = InkDocument::new();
        doc.push(line(10.0)); // inside: spans x 0..45 at y = 10
        doc.push(line(300.0)); // far outside

        let selected = doc.select_in_polygon(&square());
        assert_eq!(selected, vec![0]);
    }

    #[test]
    fn lasso_ignores_a_stroke_it_merely_clips() {
        let mut doc = InkDocument::new();
        // Long stroke mostly outside the lasso, with only its tail inside.
        let points: Vec<InkPoint> = (0..40)
            .map(|i| InkPoint::new(i as f32 * 20.0, 10.0, 0.8))
            .collect();
        doc.push(InkStroke::from_points(&points, 3.0, BLACK, StrokeKind::Pen));

        assert!(
            doc.select_in_polygon(&square()).is_empty(),
            "a stroke only clipped by the lasso should not be selected"
        );
    }

    #[test]
    fn a_tracked_drag_is_undone_as_one_step() {
        // Mirrors how a drag actually runs: the strokes are walked into place
        // sample by sample, then the total is recorded once.
        let mut doc = InkDocument::new();
        doc.push(line(10.0));
        let before = doc.strokes()[0].clone();

        doc.translate_without_history(&[0], Vec2::new(10.0, -2.0));
        doc.translate_without_history(&[0], Vec2::new(5.0, -2.0));
        assert!(doc.record_move(&[0], Vec2::new(15.0, -4.0)));

        assert!((doc.strokes()[0].points[0][0] - (before.points[0][0] + 15.0)).abs() < 1e-3);
        assert!((doc.strokes()[0].points[0][1] - (before.points[0][1] - 4.0)).abs() < 1e-3);

        assert!(doc.undo());
        assert!(
            (doc.strokes()[0].points[0][0] - before.points[0][0]).abs() < 1e-3,
            "one undo should reverse the whole drag"
        );

        assert!(doc.redo());
        assert!((doc.strokes()[0].points[0][0] - (before.points[0][0] + 15.0)).abs() < 1e-3);
    }

    #[test]
    fn a_zero_distance_move_is_not_recorded() {
        let mut doc = InkDocument::new();
        doc.push(line(10.0));
        assert!(!doc.record_move(&[0], Vec2::new(0.0, 0.0)));
        assert!(!doc.record_move(&[], Vec2::new(5.0, 5.0)));
        assert!(doc.can_undo(), "only the push should be on the stack");
        doc.undo();
        assert!(!doc.can_undo());
    }

    #[test]
    fn removing_a_selection_restores_z_order_on_undo() {
        let mut doc = InkDocument::new();
        let a = line(10.0);
        let b = line(20.0);
        let c = line(30.0);
        doc.push(a.clone());
        doc.push(b.clone());
        doc.push(c.clone());

        assert!(doc.remove_strokes(&[0, 2]));
        assert_eq!(doc.strokes(), &[b.clone()][..]);

        assert!(doc.undo());
        assert_eq!(doc.strokes(), &[a, b, c][..]);
    }

    #[test]
    fn removing_nothing_is_not_an_edit() {
        let mut doc = InkDocument::new();
        doc.push(line(10.0));
        assert!(!doc.remove_strokes(&[]));
        assert!(!doc.remove_strokes(&[99]), "out-of-range indices change nothing");
    }

    #[test]
    fn a_partial_erase_splits_a_stroke_in_two() {
        let mut doc = InkDocument::new();
        doc.push(line(0.0)); // spans x 0..45 at y = 0

        // Rub out the middle.
        assert!(doc.erase_partial_at(Vec2::new(22.5, 0.0), 6.0));
        assert_eq!(doc.strokes().len(), 2, "the stroke should be cut in two");

        let left = doc.strokes()[0].path_bounds().unwrap();
        let right = doc.strokes()[1].path_bounds().unwrap();
        assert!(left.1.x < right.0.x, "the fragments must not overlap");
    }

    #[test]
    fn a_partial_erase_at_the_end_leaves_one_fragment() {
        let mut doc = InkDocument::new();
        doc.push(line(0.0));

        assert!(doc.erase_partial_at(Vec2::new(45.0, 0.0), 8.0));
        assert_eq!(doc.strokes().len(), 1, "clipping the tail leaves one piece");
    }

    #[test]
    fn a_partial_erase_that_misses_changes_nothing() {
        let mut doc = InkDocument::new();
        doc.push(line(0.0));
        assert!(!doc.erase_partial_at(Vec2::new(500.0, 500.0), 6.0));
        assert_eq!(doc.strokes().len(), 1);
    }

    #[test]
    fn a_partial_erase_is_undoable_back_to_the_whole_stroke() {
        let mut doc = InkDocument::new();
        doc.push(line(0.0));
        let original = doc.strokes()[0].clone();

        doc.erase_partial_at(Vec2::new(22.5, 0.0), 6.0);
        assert_eq!(doc.strokes().len(), 2);

        assert!(doc.undo());
        assert_eq!(doc.strokes(), &[original.clone()][..], "undo restores the whole line");

        assert!(doc.redo());
        assert_eq!(doc.strokes().len(), 2, "redo re-cuts it");
    }

    #[test]
    fn an_eraser_drag_undoes_as_a_single_step() {
        let mut doc = InkDocument::new();
        doc.push(line(0.0));
        doc.push(line(30.0));
        doc.push(line(60.0));

        // One sweep touching all three, as a drag would.
        doc.begin_batch();
        doc.erase_at(Vec2::new(20.0, 0.0), 5.0);
        doc.erase_at(Vec2::new(20.0, 30.0), 5.0);
        doc.erase_at(Vec2::new(20.0, 60.0), 5.0);
        doc.end_batch();

        assert!(doc.is_empty());
        assert!(doc.undo(), "one undo should bring the sweep back");
        assert_eq!(doc.strokes().len(), 3, "the whole drag reverses at once");
    }

    #[test]
    fn a_batch_that_erased_nothing_leaves_no_history() {
        let mut doc = InkDocument::new();
        doc.push(line(0.0));
        doc.undo();
        assert!(!doc.can_undo());

        doc.begin_batch();
        doc.erase_at(Vec2::new(900.0, 900.0), 5.0);
        doc.end_batch();

        assert!(!doc.can_undo(), "a sweep over empty space is not an edit");
    }

    #[test]
    fn a_batched_sweep_redoes_in_order() {
        let mut doc = InkDocument::new();
        doc.push(line(0.0));
        doc.push(line(30.0));

        doc.begin_batch();
        doc.erase_at(Vec2::new(20.0, 0.0), 5.0);
        doc.erase_at(Vec2::new(20.0, 30.0), 5.0);
        doc.end_batch();

        doc.undo();
        assert_eq!(doc.strokes().len(), 2);
        assert!(doc.redo());
        assert!(doc.is_empty(), "redo should reapply the whole sweep");
    }

    #[test]
    fn highlighter_strokes_survive_a_round_trip() {
        let points: Vec<InkPoint> = (0..6).map(|i| InkPoint::new(i as f32, 0.0, 0.5)).collect();
        let mut doc = InkDocument::new();
        doc.paper = PaperStyle::Grid;
        doc.push(InkStroke::from_points(
            &points,
            20.0,
            [1.0, 0.8, 0.2, 0.32],
            StrokeKind::Highlighter,
        ));

        let restored = InkDocument::from_json(&doc.to_json().unwrap()).unwrap();
        assert_eq!(restored.strokes()[0].kind, StrokeKind::Highlighter);
        assert_eq!(restored.paper, PaperStyle::Grid, "paper travels with the page");
    }

    #[test]
    fn pages_written_before_kinds_and_paper_existed_still_load() {
        // No `kind` on the stroke and no `paper` on the file.
        let legacy = r#"{"version":1,"strokes":[{"size":3.0,"color":[1,1,1,1],"points":[[0,0,0.5],[1,1,0.5]]}]}"#;
        let doc = InkDocument::from_json(legacy).expect("legacy pages must still open");
        assert_eq!(doc.strokes()[0].kind, StrokeKind::Pen);
        assert_eq!(doc.paper, PaperStyle::default());
    }

    #[test]
    fn distance_to_degenerate_segment_is_point_distance() {
        let a = Vec2::new(4.0, 4.0);
        assert!((distance_to_segment(Vec2::new(4.0, 7.0), a, a) - 3.0).abs() < 1e-5);
    }
}

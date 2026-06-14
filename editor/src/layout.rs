//! The 3-phase layout protocol (plan §3.2): style → measure → paint, with an
//! explicit invalidation contract between the phases.
//!
//! v2's BUG-B existed because conceal/reveal changed line geometry *as a side
//! effect of styling*, and the per-line layout cache had no document reflow
//! pass — subsequent line offsets went stale and lines painted over each
//! other. Here:
//!
//! - Offsets are never stored per line; they are computed on demand from the
//!   [`HeightTree`], so a height change is *total* invalidation by
//!   construction (O(log n), not O(n)).
//! - Every mutation returns a [`Damage`] report; the paint phase repaints
//!   exactly `repaint` plus everything below `shifted_from` (a scroll-blit).
//! - Conceal is a style-and-measure input. Entering or leaving markup
//!   remeasures affected lines before paint; height changes flow through the
//!   same [`Damage`] and [`HeightTree`] path as edits, so offsets never stale.

use std::ops::Range;

use crate::height_tree::{HeightTree, OutOfBounds};
use crate::parse::{BlockState, LineKind};
use crate::style::Span;

/// Conceal state of a line.
///
/// - `Concealed` = caret elsewhere, every syntax marker hidden;
/// - `Revealed` = the whole line shows its source (caret on a block construct,
///   or whole-line reveal);
/// - `Partial(range)` = **element-level reveal** (Typora behavior): only the
///   inline element under the caret shows its markers/source; the rest of the
///   line stays concealed. The range is in **display char offsets** of the
///   styled line (the markers it keeps tile a contiguous block), so paint,
///   measure and hit-test all read it in the same coordinate space they
///   already use.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConcealMode {
    Concealed,
    Revealed,
    Partial(Range<usize>),
}

impl ConcealMode {
    /// Does the element covering display char `col` render as source
    /// (markers visible, inline assets suppressed) rather than concealed?
    /// `Concealed` reveals nothing, `Revealed` reveals everything, `Partial`
    /// reveals only its range.
    pub fn reveals_at(&self, col: usize) -> bool {
        match self {
            ConcealMode::Concealed => false,
            ConcealMode::Revealed => true,
            ConcealMode::Partial(range) => range.contains(&col),
        }
    }
}

/// Phase-1 output: what a line will look like. `display` is exactly what gets
/// measured and painted; concealed markers are absent. `spans` carry paint
/// semantics ([`crate::style::SpanKind`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledLine {
    pub display: String,
    pub conceal: ConcealMode,
    pub kind: LineKind,
    pub spans: Vec<Span>,
}

impl StyledLine {
    /// A spanless line — handy for test stylers.
    pub fn plain(display: impl Into<String>, conceal: ConcealMode) -> StyledLine {
        StyledLine {
            display: display.into(),
            conceal,
            kind: LineKind::Paragraph,
            spans: Vec::new(),
        }
    }
}

/// Phase 1: pure per-line styling. Key: (line text, block entry state,
/// conceal mode) — exactly the plan's cache key.
pub trait Styler {
    fn style(&self, text: &str, block: &BlockState, conceal: ConcealMode) -> StyledLine;
}

/// Phase-2 output for one line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineMeasure {
    pub height: f64,
    pub rows: u32,
}

/// Phase 2: turn a styled line into geometry at a wrap width.
pub trait Measurer {
    fn measure(&self, line: &StyledLine, wrap_width: f64) -> LineMeasure;

    /// Return the character offset for a click at (x, y) relative to the top-left of the layout.
    fn hit_test(&self, line: &StyledLine, wrap_width: f64, x: f64, y: f64) -> usize;
}

/// What the paint phase must do after a mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Damage {
    /// Lines whose pixels changed and need repainting.
    pub repaint: Range<usize>,
    /// First line whose vertical offset moved (everything from it downward
    /// re-positions; cheap as a blit). `None` = no geometry shift — the
    /// no geometry shift.
    pub shifted_from: Option<usize>,
}

impl Damage {
    pub fn none() -> Damage {
        Damage {
            repaint: 0..0,
            shifted_from: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.repaint.is_empty() && self.shifted_from.is_none()
    }

    pub fn merge(self, other: Damage) -> Damage {
        let repaint = if self.repaint.is_empty() {
            other.repaint
        } else if other.repaint.is_empty() {
            self.repaint
        } else {
            self.repaint.start.min(other.repaint.start)..self.repaint.end.max(other.repaint.end)
        };
        let shifted_from = match (self.shifted_from, other.shifted_from) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        Damage {
            repaint,
            shifted_from,
        }
    }
}

#[derive(Debug)]
struct LineRecord {
    text: String,
    block: BlockState,
    conceal: ConcealMode,
    measure: LineMeasure,
}

/// Owns the style/measure caches and the height tree; the single authority on
/// document geometry.
#[derive(Debug)]
pub struct LayoutEngine<S, M> {
    styler: S,
    measurer: M,
    wrap_width: f64,
    lines: Vec<LineRecord>,
    heights: HeightTree,
}

impl<S: Styler, M: Measurer> LayoutEngine<S, M> {
    pub fn new(styler: S, measurer: M, wrap_width: f64) -> LayoutEngine<S, M> {
        LayoutEngine {
            styler,
            measurer,
            wrap_width,
            lines: Vec::new(),
            heights: HeightTree::new(),
        }
    }

    fn style_measure(&self, text: &str, block: &BlockState, conceal: ConcealMode) -> LineMeasure {
        let styled = self.styler.style(text, block, conceal);
        self.measurer.measure(&styled, self.wrap_width)
    }

    pub fn set_wrap_width(&mut self, wrap_width: f64) {
        if (self.wrap_width - wrap_width).abs() < 1.0 {
            return;
        }
        self.wrap_width = wrap_width;
        self.remeasure();
    }

    pub fn remeasure(&mut self) {
        for i in 0..self.lines.len() {
            let measure = {
                let record = &self.lines[i];
                let styled = self
                    .styler
                    .style(&record.text, &record.block, record.conceal.clone());
                self.measurer.measure(&styled, self.wrap_width)
            };
            if measure != self.lines[i].measure {
                let _ = self.heights.set(i, measure.height);
                self.lines[i].measure = measure;
            }
        }
    }

    /// Replace the whole document (initial load). Everything is damage.
    pub fn set_text<I, T>(&mut self, lines: I)
    where
        I: IntoIterator<Item = (T, BlockState)>,
        T: AsRef<str>,
    {
        self.lines.clear();
        self.heights = HeightTree::new();
        for (line, block) in lines {
            let text = line.as_ref().to_string();
            let measure = self.style_measure(&text, &block, ConcealMode::Concealed);
            self.heights.push(measure.height);
            self.lines.push(LineRecord {
                text,
                block,
                conceal: ConcealMode::Concealed,
                measure,
            });
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn total_height(&self) -> f64 {
        self.heights.total_height()
    }

    pub fn height_of(&self, index: usize) -> Option<f64> {
        self.heights.get(index)
    }

    /// Top offset of a line — always answered from the height tree, never a
    /// stored value, so it can never be stale.
    pub fn offset_of(&self, index: usize) -> Result<f64, OutOfBounds> {
        self.heights.offset_of(index)
    }

    pub fn line_at(&self, y: f64) -> Option<usize> {
        self.heights.line_at_offset(y)
    }

    /// Phase 3 helper: the line range intersecting the viewport — paint is
    /// viewport-bounded by construction.
    pub fn visible_lines(&self, scroll_y: f64, viewport_height: f64) -> Range<usize> {
        if self.lines.is_empty() {
            return 0..0;
        }
        let first = self.line_at(scroll_y).unwrap_or(0);
        let last = self
            .line_at(scroll_y + viewport_height.max(0.0))
            .unwrap_or(first);
        first..(last + 1).min(self.lines.len())
    }

    /// Edit one line's text (and/or its block entry state). Restyles +
    /// remeasures it; if the height changed, every subsequent offset is
    /// already correct (sum tree) and the damage says so.
    pub fn replace_line(
        &mut self,
        index: usize,
        text: &str,
        block: BlockState,
    ) -> Result<Damage, OutOfBounds> {
        let conceal = self
            .lines
            .get(index)
            .map(|l| l.conceal.clone())
            .ok_or(OutOfBounds {
                index,
                len: self.lines.len(),
            })?;
        let measure = self.style_measure(text, &block, conceal.clone());
        self.apply_measure(index, text.to_string(), block, conceal, measure)
    }

    pub fn insert_line(
        &mut self,
        index: usize,
        text: &str,
        block: BlockState,
    ) -> Result<Damage, OutOfBounds> {
        if index > self.lines.len() {
            return Err(OutOfBounds {
                index,
                len: self.lines.len(),
            });
        }
        let measure = self.style_measure(text, &block, ConcealMode::Concealed);
        self.heights.insert(index, measure.height)?;
        self.lines.insert(
            index,
            LineRecord {
                text: text.to_string(),
                block,
                conceal: ConcealMode::Concealed,
                measure,
            },
        );
        Ok(Damage {
            repaint: index..index + 1,
            shifted_from: Some(index + 1),
        })
    }

    pub fn remove_line(&mut self, index: usize) -> Result<Damage, OutOfBounds> {
        if index >= self.lines.len() {
            return Err(OutOfBounds {
                index,
                len: self.lines.len(),
            });
        }
        self.heights.remove(index)?;
        self.lines.remove(index);
        Ok(Damage {
            repaint: index..index,
            shifted_from: Some(index),
        })
    }

    /// Apply a buffer [`crate::ChangedSpan`]: lines `first..first +
    /// old_lines` are replaced by `new_texts` (fetched from the buffer's
    /// final state). This is the whole buffer→layout bridge.
    pub fn splice<I, T>(
        &mut self,
        first: usize,
        old_lines: usize,
        new_texts: I,
    ) -> Result<Damage, OutOfBounds>
    where
        I: IntoIterator<Item = (T, BlockState)>,
        T: AsRef<str>,
    {
        let mut damage = Damage::none();
        let mut index = first;
        let end = first + old_lines;
        for (text, block) in new_texts {
            damage = damage.merge(if index < end {
                self.replace_line(index, text.as_ref(), block)?
            } else {
                self.insert_line(index, text.as_ref(), block)?
            });
            index += 1;
        }
        for _ in index..end {
            damage = damage.merge(self.remove_line(index)?);
        }
        Ok(damage)
    }

    /// Flip a line's conceal mode (caret entered/left it).
    pub fn set_conceal(&mut self, index: usize, mode: ConcealMode) -> Result<Damage, OutOfBounds> {
        let (text, block, old_conceal) = match self.lines.get(index) {
            Some(l) => (l.text.clone(), l.block.clone(), l.conceal.clone()),
            None => {
                return Err(OutOfBounds {
                    index,
                    len: self.lines.len(),
                });
            }
        };
        if old_conceal == mode {
            return Ok(Damage::none());
        }
        let measure = self.style_measure(&text, &block, mode.clone());
        self.apply_measure(index, text, block, mode, measure)
    }

    /// Caret moved between lines: conceal the old line, reveal the new one.
    pub fn caret_moved(
        &mut self,
        from: Option<usize>,
        to: Option<usize>,
    ) -> Result<Damage, OutOfBounds> {
        let mut damage = Damage::none();
        if let Some(from) = from
            && Some(from) != to
        {
            damage = damage.merge(self.set_conceal(from, ConcealMode::Concealed)?);
        }
        if let Some(to) = to {
            damage = damage.merge(self.set_conceal(to, ConcealMode::Revealed)?);
        }
        Ok(damage)
    }

    fn apply_measure(
        &mut self,
        index: usize,
        text: String,
        block: BlockState,
        conceal: ConcealMode,
        measure: LineMeasure,
    ) -> Result<Damage, OutOfBounds> {
        let old_height = self.heights.set(index, measure.height)?;
        let height_changed = (old_height - measure.height).abs() > f64::EPSILON;
        if let Some(rec) = self.lines.get_mut(index) {
            rec.text = text;
            rec.block = block;
            rec.conceal = conceal;
            rec.measure = measure;
        }
        Ok(Damage {
            repaint: index..index + 1,
            shifted_from: height_changed.then_some(index + 1),
        })
    }
}

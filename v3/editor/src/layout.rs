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
//! - Conceal is **layout-stable by design**: a [`Styler`] that reports
//!   `layout_stable() == true` guarantees concealed and revealed forms
//!   measure identically (reserved-width strategy), so caret motion produces
//!   damage confined to the two affected lines — the M1 golden gate.

use std::ops::Range;

use crate::height_tree::{HeightTree, OutOfBounds};

/// Conceal state of a line. `Concealed` = cursor elsewhere, syntax markers
/// hidden; `Revealed` = cursor on the line, markers visible (muted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConcealMode {
    Concealed,
    Revealed,
}

/// Phase-1 output: what a line will look like. Spans/attributes will grow
/// here with the real styler (ADR-0101); `display` is what gets measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledLine {
    pub display: String,
    pub conceal: ConcealMode,
}

/// Phase 1: pure per-line styling. Key: (line text, conceal mode); block
/// state joins the key when the incremental parser lands.
pub trait Styler {
    fn style(&self, text: &str, conceal: ConcealMode) -> StyledLine;

    /// True if this styler guarantees the reserved-width contract:
    /// `measure(style(t, Concealed)) == measure(style(t, Revealed))` for all
    /// `t`. Production stylers must return true; the contract is asserted by
    /// [`LayoutEngine::set_conceal`] in debug builds.
    fn layout_stable(&self) -> bool;
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
}

/// What the paint phase must do after a mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Damage {
    /// Lines whose pixels changed and need repainting.
    pub repaint: Range<usize>,
    /// First line whose vertical offset moved (everything from it downward
    /// re-positions; cheap as a blit). `None` = no geometry shift — the
    /// layout-stable case.
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

    fn merge(self, other: Damage) -> Damage {
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

    fn style_measure(&self, text: &str, conceal: ConcealMode) -> LineMeasure {
        let styled = self.styler.style(text, conceal);
        self.measurer.measure(&styled, self.wrap_width)
    }

    /// Replace the whole document (initial load). Everything is damage.
    pub fn set_text<I, T>(&mut self, lines: I)
    where
        I: IntoIterator<Item = T>,
        T: AsRef<str>,
    {
        self.lines.clear();
        self.heights = HeightTree::new();
        for line in lines {
            let text = line.as_ref().to_string();
            let measure = self.style_measure(&text, ConcealMode::Concealed);
            self.heights.push(measure.height);
            self.lines.push(LineRecord {
                text,
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

    /// Edit one line's text. Restyles + remeasures it; if the height changed,
    /// every subsequent offset is already correct (sum tree) and the damage
    /// says so.
    pub fn replace_line(&mut self, index: usize, text: &str) -> Result<Damage, OutOfBounds> {
        let conceal = self
            .lines
            .get(index)
            .map(|l| l.conceal)
            .ok_or(OutOfBounds {
                index,
                len: self.lines.len(),
            })?;
        let measure = self.style_measure(text, conceal);
        self.apply_measure(index, text.to_string(), conceal, measure)
    }

    pub fn insert_line(&mut self, index: usize, text: &str) -> Result<Damage, OutOfBounds> {
        if index > self.lines.len() {
            return Err(OutOfBounds {
                index,
                len: self.lines.len(),
            });
        }
        let measure = self.style_measure(text, ConcealMode::Concealed);
        self.heights.insert(index, measure.height)?;
        self.lines.insert(
            index,
            LineRecord {
                text: text.to_string(),
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

    /// Flip a line's conceal mode (caret entered/left it). With a
    /// layout-stable styler this never shifts geometry; the debug assertion
    /// enforces the contract on every styler that claims it.
    pub fn set_conceal(&mut self, index: usize, mode: ConcealMode) -> Result<Damage, OutOfBounds> {
        let (text, old_conceal) = match self.lines.get(index) {
            Some(l) => (l.text.clone(), l.conceal),
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
        let measure = self.style_measure(&text, mode);
        if self.styler.layout_stable() {
            debug_assert_eq!(
                self.lines.get(index).map(|l| l.measure.height),
                Some(measure.height),
                "layout-stable styler changed height on conceal flip at line {index}"
            );
        }
        self.apply_measure(index, text, mode, measure)
    }

    /// Caret moved between lines: conceal the old line, reveal the new one.
    /// With a layout-stable styler the returned damage is confined to those
    /// two lines and `shifted_from` is `None` — the M1 golden gate.
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
        conceal: ConcealMode,
        measure: LineMeasure,
    ) -> Result<Damage, OutOfBounds> {
        let old_height = self.heights.set(index, measure.height)?;
        let height_changed = (old_height - measure.height).abs() > f64::EPSILON;
        if let Some(rec) = self.lines.get_mut(index) {
            rec.text = text;
            rec.conceal = conceal;
            rec.measure = measure;
        }
        Ok(Damage {
            repaint: index..index + 1,
            shifted_from: height_changed.then_some(index + 1),
        })
    }
}

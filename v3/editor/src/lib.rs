//! Editor engine for md-editor v3 (plan §3.2) — toolkit-agnostic by
//! construction (ADR-0100): nothing in this crate knows about iced.
//!
//! Text model: [`buffer::Buffer`] — ropey rope, `Vec<Selection>` multi-cursor
//! model from day one, transactional [`buffer::EditorCommand`]s, and a
//! branching [`undo::UndoTree`] (editing after undo never destroys the redo
//! future).
//!
//! The rendering centerpiece is the 3-phase layout protocol with an explicit
//! invalidation contract, the structural fix for BUG-B:
//! 1. **Style** — pure, per line, keyed by (text, conceal mode).
//! 2. **Measure** — any height change updates the [`height_tree::HeightTree`],
//!    so *all* subsequent line offsets are correct in O(log n); stale offsets
//!    cannot exist because offsets are never cached per line.
//! 3. **Paint** — viewport-bounded, damage-tracked.

pub mod buffer;
pub mod document;
pub mod height_tree;
pub mod layout;
pub mod parse;
pub mod style;
pub mod undo;

pub use buffer::{ApplyResult, Buffer, ChangedSpan, Command, Movement};
pub use document::EditorDocument;
pub use height_tree::HeightTree;
pub use layout::{ConcealMode, Damage, LayoutEngine, LineMeasure, Measurer, StyledLine, Styler};
pub use parse::{BlockState, IncrementalParser, LineKind, LineParse};
pub use style::{MarkdownStyler, Span, SpanKind};
pub use undo::{EditOp, Selection, Transaction, UndoTree, UndoTreeSnapshot};

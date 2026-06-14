//! Rope-backed text buffer (plan §3.2): ropey + transactional commands —
//! v2's proven discipline (`native/src/editor/buffer/` is the quarry) —
//! rebuilt on the v3 invariants:
//!
//! - **Multi-cursor from day one:** the selection model is `Vec<Selection>`,
//!   kept sorted, merged, and non-empty. A single caret is the trivial case.
//! - **Undo is a tree** ([`crate::undo::UndoTree`]): nothing is ever lost,
//!   and the tree is snapshot-able for sidecar persistence.
//! - **Grapheme safety:** caret motion and backspace/delete operate on
//!   extended grapheme clusters (emoji ZWJ, CJK, CRLF), never inside them.
//! - **Layout bridge:** every text mutation reports a [`ChangedSpan`] that
//!   maps onto one [`crate::LayoutEngine::splice`] call, so buffer and
//!   layout can never disagree about line count.

use ropey::Rope;
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

mod edit_ops;
mod formatting;
mod typing;

pub use crate::undo::{EditOp, Selection, Transaction, UndoTree, UndoTreeSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Alignment {
    Left,
    Center,
    Right,
}

/// Caret movement unit. Word motion lands with the M3 ergonomics bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Movement {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    DocStart,
    DocEnd,
}

/// The transactional command set. Everything that mutates the buffer goes
/// through [`Buffer::apply`]; there is no other mutation path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Insert at every caret, replacing any selected ranges.
    Insert(String),
    DeleteBackward,
    DeleteForward,
    Move {
        movement: Movement,
        extend: bool,
    },
    SetCursor {
        line: usize,
        col: usize,
    },
    /// Replace the selection set (normalized; ignored if empty).
    SetSelections(Vec<Selection>),
    /// Add a caret (multi-cursor), keeping existing selections.
    AddCaret {
        line: usize,
        col: usize,
    },
    SelectAll,
    Undo,
    Redo,
    ToggleBold,
    ToggleItalic,
    ToggleCode,
    HeadingCycle,
    ToggleBullet,
    ToggleCheckbox,
    ToggleWikilink,
    SetHeading(usize),
    TableTab {
        backward: bool,
    },
}

/// The line-level consequence of one command, as a single splice: lines
/// `first..first + old_lines` were replaced by `first..first + new_lines`
/// (whose content must be re-fetched from the buffer). Lines outside the
/// span are untouched. This is the whole buffer→layout bridge: the layout
/// engine applies it as `new_lines` replace/insert calls plus
/// `old_lines - new_lines` removals (or vice versa).
///
/// Why a span and not per-edit deltas: undo replays ops in descending
/// offset order, so per-edit line indices are valid only against
/// *intermediate* rope states; a consumer patching from the final state
/// would mis-index. The span's `first` (min line touched) and the count of
/// untouched tail lines are final-state-valid in both replay orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangedSpan {
    pub first: usize,
    pub old_lines: usize,
    pub new_lines: usize,
}

/// What a command did, for the caller's redraw decision.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplyResult {
    pub text_changed: bool,
    pub selection_changed: bool,
    /// `Some` iff text changed; the line splice to forward to the layout.
    pub changed: Option<ChangedSpan>,
}

/// State of an insert-run being coalesced into one undo node ("hello" is one
/// undo step, "hello world" is two — whitespace breaks the run).
#[derive(Debug, Clone, Copy)]
struct Coalesce {
    node: usize,
    end: usize,
    whitespace: bool,
}

#[derive(Debug)]
pub struct Buffer {
    rope: Rope,
    selections: Vec<Selection>,
    /// Sticky column for Up/Down, parallel to `selections`.
    desired_cols: Vec<Option<usize>>,
    undo: UndoTree,
    coalesce: Option<Coalesce>,
    /// Undo-tree node the buffer was last saved at; dirtiness is positional,
    /// so undoing back to the save point makes the buffer clean again.
    saved_at: usize,
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Buffer {
    pub fn new() -> Buffer {
        Buffer::from_text("")
    }

    pub fn from_text(text: &str) -> Buffer {
        let undo = UndoTree::new();
        let saved_at = undo.current();
        Buffer {
            rope: Rope::from_str(text),
            selections: vec![Selection::caret(0)],
            desired_cols: vec![None],
            undo,
            coalesce: None,
            saved_at,
        }
    }

    // --- read access ---------------------------------------------------

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    /// Line content without its terminator (handles CRLF).
    pub fn line_text(&self, line: usize) -> String {
        if line >= self.line_count() {
            return String::new();
        }
        let text = self.rope.line(line).to_string();
        text.trim_end_matches(['\n', '\r']).to_string()
    }

    /// Sorted, non-overlapping, never empty.
    pub fn selections(&self) -> &[Selection] {
        &self.selections
    }

    /// The last selection — the one whose line the conceal logic reveals.
    pub fn primary(&self) -> Selection {
        match self.selections.last() {
            Some(s) => *s,
            None => Selection::caret(0), // unreachable: invariant keeps >= 1
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.primary().range();
        if start == end {
            return None;
        }
        Some(self.rope.slice(start..end).to_string())
    }

    pub fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.rope.len_chars());
        let line = self.rope.char_to_line(offset);
        let col = offset - self.rope.line_to_char(line);
        (line, col.min(self.line_text(line).chars().count()))
    }

    /// Char col, clamped to the line and snapped to a grapheme boundary
    /// (a raw char col can point inside an emoji cluster).
    pub fn line_col_to_offset(&self, line: usize, col: usize) -> usize {
        let line = line.min(self.line_count().saturating_sub(1));
        let offset = self.rope.line_to_char(line) + col.min(self.line_text(line).chars().count());
        snap_to_boundary(&self.rope, offset)
    }

    pub fn is_dirty(&self) -> bool {
        self.undo.current() != self.saved_at
    }

    pub fn mark_saved(&mut self) {
        self.saved_at = self.undo.current();
    }

    pub fn can_undo(&self) -> bool {
        self.undo.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.undo.can_redo()
    }

    /// Persistence seam: the undo tree as plain data (plan M3 "persistent
    /// undo"); restore with [`Buffer::restore_undo`].
    pub fn undo_snapshot(&self) -> UndoTreeSnapshot {
        self.undo.snapshot()
    }

    pub fn restore_undo(
        &mut self,
        snapshot: UndoTreeSnapshot,
    ) -> Result<(), crate::undo::UndoTreeError> {
        self.undo = UndoTree::from_snapshot(snapshot)?;
        self.saved_at = self.undo.current();
        self.coalesce = None;
        Ok(())
    }

    // --- the single mutation path ---------------------------------------

    pub fn apply(&mut self, command: Command) -> ApplyResult {
        // Any command other than plain insertion ends the coalesce run.
        if !matches!(command, Command::Insert(_)) {
            self.coalesce = None;
        }
        match command {
            Command::Insert(text) => self.insert(&text),
            Command::DeleteBackward => self.delete(Direction::Backward),
            Command::DeleteForward => self.delete(Direction::Forward),
            Command::Move { movement, extend } => self.move_carets(movement, extend),
            Command::SetCursor { line, col } => {
                let offset = self.line_col_to_offset(line, col);
                self.set_selections(vec![Selection::caret(offset)])
            }
            Command::SetSelections(sels) => self.set_selections(sels),
            Command::AddCaret { line, col } => {
                let offset = self.line_col_to_offset(line, col);
                let mut sels = self.selections.clone();
                sels.push(Selection::caret(offset));
                self.set_selections(sels)
            }
            Command::SelectAll => {
                self.set_selections(vec![Selection::new(0, self.rope.len_chars())])
            }
            Command::Undo => self.undo_step(),
            Command::Redo => self.redo_step(),
            Command::ToggleBold => self.toggle_wrap("**"),
            Command::ToggleItalic => self.toggle_wrap("*"),
            Command::ToggleCode => self.toggle_wrap("`"),
            Command::HeadingCycle => self.heading_cycle(),
            Command::ToggleBullet => self.toggle_bullet(),
            Command::ToggleCheckbox => self.toggle_checkbox(),
            Command::ToggleWikilink => self.toggle_wrap_double("[[", "]]"),
            Command::SetHeading(level) => self.set_heading(level),
            Command::TableTab { backward } => {
                if let Some(res) = self.table_cell_nav(backward) {
                    res
                } else {
                    self.normal_tab(backward)
                }
            }
        }
    }

    fn undo_step(&mut self) -> ApplyResult {
        let old_total = self.line_count();
        let Some(txn) = self.undo.undo() else {
            return ApplyResult::default();
        };
        let (ops, sels) = (txn.ops.clone(), txn.before.clone());
        let mut acc = SpanAcc::default();
        for op in ops.iter().rev() {
            self.apply_op(&op.inverse(), &mut acc);
        }
        self.replace_selections(sels);
        ApplyResult {
            text_changed: true,
            selection_changed: true,
            changed: acc.span(old_total, self.line_count()),
        }
    }

    fn redo_step(&mut self) -> ApplyResult {
        let old_total = self.line_count();
        let Some(txn) = self.undo.redo() else {
            return ApplyResult::default();
        };
        let (ops, sels) = (txn.ops.clone(), txn.after.clone());
        let mut acc = SpanAcc::default();
        for op in &ops {
            self.apply_op(op, &mut acc);
        }
        self.replace_selections(sels);
        ApplyResult {
            text_changed: true,
            selection_changed: true,
            changed: acc.span(old_total, self.line_count()),
        }
    }

    fn move_carets(&mut self, movement: Movement, extend: bool) -> ApplyResult {
        let vertical = matches!(movement, Movement::Up | Movement::Down);
        for i in 0..self.selections.len() {
            let sel = self.selections[i];
            // Collapsing a selection with plain Left/Right jumps to its edge.
            if !extend && !sel.is_caret() && matches!(movement, Movement::Left | Movement::Right) {
                let (start, end) = sel.range();
                let to = match movement {
                    Movement::Left => start,
                    _ => end,
                };
                self.selections[i] = Selection::caret(to);
                self.desired_cols[i] = None;
                continue;
            }
            let head = sel.head;
            let new_head = match movement {
                Movement::Left => prev_grapheme(&self.rope, head),
                Movement::Right => next_grapheme(&self.rope, head),
                Movement::Home => {
                    let (line, _) = self.offset_to_line_col(head);
                    self.rope.line_to_char(line)
                }
                Movement::End => {
                    let (line, _) = self.offset_to_line_col(head);
                    self.line_col_to_offset(line, usize::MAX)
                }
                Movement::DocStart => 0,
                Movement::DocEnd => self.rope.len_chars(),
                Movement::Up | Movement::Down => {
                    let (line, col) = self.offset_to_line_col(head);
                    let desired = *self.desired_cols[i].get_or_insert(col);
                    let target = match movement {
                        Movement::Up => line.saturating_sub(1),
                        _ => (line + 1).min(self.line_count().saturating_sub(1)),
                    };
                    self.line_col_to_offset(target, desired)
                }
            };
            self.selections[i] = if extend {
                Selection::new(sel.anchor, new_head)
            } else {
                Selection::caret(new_head)
            };
            if !vertical {
                self.desired_cols[i] = None;
            }
        }
        self.normalize();
        ApplyResult {
            selection_changed: true,
            ..ApplyResult::default()
        }
    }

    fn set_selections(&mut self, sels: Vec<Selection>) -> ApplyResult {
        if sels.is_empty() {
            return ApplyResult::default();
        }
        self.replace_selections(sels);
        ApplyResult {
            selection_changed: true,
            ..ApplyResult::default()
        }
    }

    // --- internals -------------------------------------------------------

    /// Commit (or coalesce) an insert transaction. A run of caret-typed
    /// inserts with uniform whitespace-ness amends one undo node.
    fn record(
        &mut self,
        ops: Vec<EditOp>,
        before: Vec<Selection>,
        replaced_range: bool,
        text: &str,
    ) {
        let after = self.selections.clone();
        let whitespace = text.chars().all(char::is_whitespace);
        // Only uniform text — all whitespace or none — may join a run; mixed
        // inserts (a paste like " world") stay their own undo step.
        let uniform = whitespace || !text.chars().any(char::is_whitespace);
        let coalescable = !replaced_range
            && ops.len() == 1
            && uniform
            && !text.contains('\n')
            && before.len() == 1;
        let continues = match (coalescable, self.coalesce, ops.first()) {
            (true, Some(c), Some(EditOp::Insert { at, .. })) => {
                c.end == *at && c.whitespace == whitespace
            }
            _ => false,
        };
        if continues
            && let (Some(c), Some(op)) = (self.coalesce, ops.first())
            && self.undo.amend(c.node, op.clone(), after.clone())
        {
            self.coalesce = Some(Coalesce {
                node: c.node,
                end: self.primary().head,
                whitespace,
            });
            return;
        }
        let node = self.undo.commit(Transaction { ops, before, after });
        self.coalesce = coalescable.then_some(Coalesce {
            node,
            end: self.primary().head,
            whitespace,
        });
    }

    fn apply_op(&mut self, op: &EditOp, acc: &mut SpanAcc) {
        match op {
            EditOp::Insert { at, text } => {
                self.rope_insert((*at).min(self.rope.len_chars()), text, acc);
            }
            EditOp::Delete { at, text } => {
                let start = (*at).min(self.rope.len_chars());
                let end = (start + text.chars().count()).min(self.rope.len_chars());
                self.rope_delete(start, end, acc);
            }
        }
    }

    // Both mutators measure `first`/`last` against the rope state *after*
    // the mutation rather than predicting from the inserted/removed text:
    // ropey treats a lone `\r` as a line break, so an edit adjacent to one
    // can merge or split a CRLF pair and shift line counts in ways textual
    // prediction gets wrong (found by the property suite).

    fn rope_insert(&mut self, at: usize, text: &str, acc: &mut SpanAcc) {
        let at = at.min(self.rope.len_chars());
        self.rope.insert(at, text);
        let first = self.rope.char_to_line(at);
        let last = self.rope.char_to_line(at + text.chars().count());
        acc.touch(first, self.rope.len_lines() - last - 1);
    }

    fn rope_delete(&mut self, start: usize, end: usize, acc: &mut SpanAcc) -> String {
        let removed = self.rope.slice(start..end).to_string();
        self.rope.remove(start..end);
        let first = self.rope.char_to_line(start.min(self.rope.len_chars()));
        acc.touch(first, self.rope.len_lines() - first - 1);
        removed
    }

    fn replace_selections(&mut self, sels: Vec<Selection>) {
        let len = self.rope.len_chars();
        // Clamp into the document *and* snap onto grapheme boundaries, so
        // the alignment invariant holds even for raw offsets from hit
        // testing or line/col conversion (a char col can split a cluster).
        self.selections = sels
            .into_iter()
            .map(|s| {
                Selection::new(
                    snap_to_boundary(&self.rope, s.anchor.min(len)),
                    snap_to_boundary(&self.rope, s.head.min(len)),
                )
            })
            .collect();
        if self.selections.is_empty() {
            self.selections.push(Selection::caret(len));
        }
        self.desired_cols = vec![None; self.selections.len()];
        self.normalize();
    }

    /// Restore the selection invariant: sorted by range, overlaps merged,
    /// duplicate carets deduplicated. `desired_cols` follows the survivors.
    fn normalize(&mut self) {
        let mut items: Vec<(Selection, Option<usize>)> = self
            .selections
            .iter()
            .copied()
            .zip(self.desired_cols.iter().copied())
            .collect();
        items.sort_by_key(|(s, _)| s.range());
        let mut out: Vec<(Selection, Option<usize>)> = Vec::with_capacity(items.len());
        for (sel, col) in items {
            let Some((last, _)) = out.last_mut() else {
                out.push((sel, col));
                continue;
            };
            let (ls, le) = last.range();
            let (s, e) = sel.range();
            // True overlap merges; so does a caret touching a range boundary
            // (it would otherwise double-type at that offset). Two ranges
            // that merely share a boundary stay separate.
            let overlaps = s < le || (s == le && (s == e || ls == le));
            if overlaps {
                *last = Selection::new(ls.min(s), le.max(e));
            } else {
                out.push((sel, col));
            }
        }
        self.selections = out.iter().map(|(s, _)| *s).collect();
        self.desired_cols = out.iter().map(|(_, c)| *c).collect();
    }
}

#[derive(Debug, Clone, Copy)]
enum Direction {
    Backward,
    Forward,
}

/// Accumulates a command's [`ChangedSpan`] across its primitive ops.
///
/// Per-op it records the touched line index (`first`) and the count of lines
/// *below* the touched region (`tail`), each measured against the rope state
/// at that moment. The minimum of each across ops is valid against the final
/// state regardless of replay direction: everything above `min(first)` and
/// everything in the smallest tail is, by construction, never touched.
#[derive(Debug, Default)]
struct SpanAcc {
    span: Option<(usize, usize)>,
}

impl SpanAcc {
    fn touch(&mut self, first: usize, tail: usize) {
        self.span = Some(match self.span {
            Some((f, t)) => (f.min(first), t.min(tail)),
            None => (first, tail),
        });
    }

    fn span(&self, old_total: usize, new_total: usize) -> Option<ChangedSpan> {
        let (first, tail) = self.span?;
        Some(ChangedSpan {
            first,
            old_lines: old_total.saturating_sub(first + tail),
            new_lines: new_total.saturating_sub(first + tail),
        })
    }
}

fn shift(offset: usize, delta: isize) -> usize {
    offset.saturating_add_signed(delta)
}

// --- grapheme boundaries over rope chunks (no full-text copies) -----------

/// True if `char_idx` sits on an extended-grapheme-cluster boundary.
pub fn is_grapheme_boundary(rope: &Rope, char_idx: usize) -> bool {
    let char_idx = char_idx.min(rope.len_chars());
    let byte_idx = rope.char_to_byte(char_idx);
    let (chunk, chunk_start, _, _) = rope.chunk_at_byte(byte_idx);
    let mut cursor = GraphemeCursor::new(byte_idx, rope.len_bytes(), true);
    loop {
        match cursor.is_boundary(chunk, chunk_start) {
            Ok(b) => return b,
            Err(GraphemeIncomplete::PreContext(n)) => {
                let (c, s, _, _) = rope.chunk_at_byte(n.saturating_sub(1));
                cursor.provide_context(c, s);
            }
            Err(_) => return true,
        }
    }
}

/// `char_idx` if it is a boundary, otherwise the boundary before it.
pub fn snap_to_boundary(rope: &Rope, char_idx: usize) -> usize {
    if is_grapheme_boundary(rope, char_idx) {
        char_idx
    } else {
        prev_grapheme(rope, char_idx)
    }
}

/// Previous extended-grapheme-cluster boundary before `char_idx`.
pub fn prev_grapheme(rope: &Rope, char_idx: usize) -> usize {
    let char_idx = char_idx.min(rope.len_chars());
    if char_idx == 0 {
        return 0;
    }
    let byte_idx = rope.char_to_byte(char_idx);
    let (mut chunk, mut chunk_start, _, _) = rope.chunk_at_byte(byte_idx);
    let mut cursor = GraphemeCursor::new(byte_idx, rope.len_bytes(), true);
    loop {
        match cursor.prev_boundary(chunk, chunk_start) {
            Ok(Some(b)) => return rope.byte_to_char(b),
            Ok(None) => return 0,
            Err(GraphemeIncomplete::PrevChunk) => {
                let (c, s, _, _) = rope.chunk_at_byte(chunk_start.saturating_sub(1));
                chunk = c;
                chunk_start = s;
            }
            Err(GraphemeIncomplete::PreContext(n)) => {
                let (c, s, _, _) = rope.chunk_at_byte(n.saturating_sub(1));
                cursor.provide_context(c, s);
            }
            // InvalidOffset/NextChunk cannot occur for prev_boundary from a
            // char boundary; degrade to a char step rather than panic.
            Err(_) => return char_idx - 1,
        }
    }
}

/// Next extended-grapheme-cluster boundary after `char_idx`.
pub fn next_grapheme(rope: &Rope, char_idx: usize) -> usize {
    let char_idx = char_idx.min(rope.len_chars());
    if char_idx == rope.len_chars() {
        return char_idx;
    }
    let byte_idx = rope.char_to_byte(char_idx);
    let (mut chunk, mut chunk_start, _, _) = rope.chunk_at_byte(byte_idx);
    let mut cursor = GraphemeCursor::new(byte_idx, rope.len_bytes(), true);
    loop {
        match cursor.next_boundary(chunk, chunk_start) {
            Ok(Some(b)) => return rope.byte_to_char(b),
            Ok(None) => return rope.len_chars(),
            Err(GraphemeIncomplete::NextChunk) => {
                let next_start = chunk_start + chunk.len();
                let (c, s, _, _) =
                    rope.chunk_at_byte(next_start.min(rope.len_bytes().saturating_sub(1)));
                chunk = c;
                chunk_start = s;
            }
            Err(GraphemeIncomplete::PreContext(n)) => {
                let (c, s, _, _) = rope.chunk_at_byte(n.saturating_sub(1));
                cursor.provide_context(c, s);
            }
            Err(_) => return (char_idx + 1).min(rope.len_chars()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caret(buffer: &Buffer) -> usize {
        buffer.primary().head
    }

    #[test]
    fn insert_and_linewise_report() {
        let mut buffer = Buffer::from_text("ab\ncd");
        buffer.apply(Command::SetCursor { line: 0, col: 2 });
        let result = buffer.apply(Command::Insert("X\nY".into()));
        assert_eq!(buffer.text(), "abX\nY\ncd");
        assert_eq!(
            result.changed,
            Some(ChangedSpan {
                first: 0,
                old_lines: 1,
                new_lines: 2
            })
        );
        assert_eq!(buffer.line_count(), 3);
    }

    #[test]
    fn delete_selection_spanning_lines() {
        let mut buffer = Buffer::from_text("one\ntwo\nthree");
        buffer.apply(Command::SetSelections(vec![Selection::new(2, 9)]));
        let result = buffer.apply(Command::DeleteBackward);
        assert_eq!(buffer.text(), "onhree");
        assert_eq!(
            result.changed,
            Some(ChangedSpan {
                first: 0,
                old_lines: 3,
                new_lines: 1
            })
        );
        assert_eq!(caret(&buffer), 2);
    }

    #[test]
    fn typing_coalesces_words_not_across_whitespace() {
        let mut buffer = Buffer::new();
        for ch in ["h", "e", "y", " ", "y", "o"] {
            buffer.apply(Command::Insert(ch.into()));
        }
        assert_eq!(buffer.text(), "hey yo");
        buffer.apply(Command::Undo);
        assert_eq!(buffer.text(), "hey ");
        buffer.apply(Command::Undo);
        assert_eq!(buffer.text(), "hey");
        buffer.apply(Command::Undo);
        assert_eq!(buffer.text(), "");
        buffer.apply(Command::Redo);
        buffer.apply(Command::Redo);
        buffer.apply(Command::Redo);
        assert_eq!(buffer.text(), "hey yo");
    }

    #[test]
    fn caret_movement_breaks_coalescing() {
        let mut buffer = Buffer::new();
        buffer.apply(Command::Insert("ab".into()));
        buffer.apply(Command::Move {
            movement: Movement::Left,
            extend: false,
        });
        buffer.apply(Command::Move {
            movement: Movement::Right,
            extend: false,
        });
        buffer.apply(Command::Insert("c".into()));
        buffer.apply(Command::Undo);
        assert_eq!(buffer.text(), "ab");
    }

    #[test]
    fn undo_tree_keeps_both_branches() {
        let mut buffer = Buffer::from_text("base ");
        buffer.apply(Command::Move {
            movement: Movement::DocEnd,
            extend: false,
        });
        buffer.apply(Command::Insert("one".into()));
        buffer.apply(Command::Undo);
        buffer.apply(Command::Insert("two".into()));
        assert_eq!(buffer.text(), "base two");
        // Undo "two", redo follows the newest branch back to "two".
        buffer.apply(Command::Undo);
        assert_eq!(buffer.text(), "base ");
        buffer.apply(Command::Redo);
        assert_eq!(buffer.text(), "base two");
        // The "one" branch still exists in the snapshot (3 non-root nodes
        // would be 2 if linear undo had discarded it).
        assert_eq!(buffer.undo_snapshot().nodes.len(), 3);
    }

    #[test]
    fn multi_cursor_insert_hits_every_caret() {
        let mut buffer = Buffer::from_text("a\nb\nc");
        buffer.apply(Command::SetCursor { line: 0, col: 1 });
        buffer.apply(Command::AddCaret { line: 1, col: 1 });
        buffer.apply(Command::AddCaret { line: 2, col: 1 });
        buffer.apply(Command::Insert("!".into()));
        assert_eq!(buffer.text(), "a!\nb!\nc!");
        // One undo reverts the whole multi-cursor edit.
        buffer.apply(Command::Undo);
        assert_eq!(buffer.text(), "a\nb\nc");
        assert_eq!(buffer.selections().len(), 3);
    }

    #[test]
    fn overlapping_selections_merge() {
        let mut buffer = Buffer::from_text("abcdef");
        buffer.apply(Command::SetSelections(vec![
            Selection::new(0, 3),
            Selection::new(2, 5),
            Selection::caret(5),
        ]));
        assert_eq!(buffer.selections(), &[Selection::new(0, 5)]);
    }

    #[test]
    fn backspace_removes_a_whole_emoji_cluster() {
        let mut buffer = Buffer::from_text("a👨‍👩‍👧‍👦b");
        buffer.apply(Command::Move {
            movement: Movement::DocEnd,
            extend: false,
        });
        buffer.apply(Command::Move {
            movement: Movement::Left,
            extend: false,
        });
        buffer.apply(Command::DeleteBackward);
        assert_eq!(buffer.text(), "ab");
    }

    #[test]
    fn crlf_is_one_step() {
        let buffer = Buffer::from_text("a\r\nb");
        assert_eq!(next_grapheme(&buffer.rope, 1), 3, "skips over \\r\\n");
        assert_eq!(prev_grapheme(&buffer.rope, 3), 1);
    }

    #[test]
    fn vertical_motion_keeps_desired_column() {
        let mut buffer = Buffer::from_text("longline\nab\nlongline");
        buffer.apply(Command::SetCursor { line: 0, col: 6 });
        buffer.apply(Command::Move {
            movement: Movement::Down,
            extend: false,
        });
        assert_eq!(buffer.offset_to_line_col(caret(&buffer)), (1, 2));
        buffer.apply(Command::Move {
            movement: Movement::Down,
            extend: false,
        });
        assert_eq!(buffer.offset_to_line_col(caret(&buffer)), (2, 6));
    }

    #[test]
    fn dirty_tracks_the_save_point_through_undo() {
        let mut buffer = Buffer::new();
        assert!(!buffer.is_dirty());
        buffer.apply(Command::Insert("x".into()));
        assert!(buffer.is_dirty());
        buffer.mark_saved();
        assert!(!buffer.is_dirty());
        buffer.apply(Command::Undo);
        assert!(buffer.is_dirty());
        buffer.apply(Command::Redo);
        assert!(!buffer.is_dirty(), "redo back to the save point is clean");
    }
}

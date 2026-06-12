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

pub use crate::undo::{EditOp, Selection, Transaction, UndoTree, UndoTreeSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Alignment {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListKind {
    Unordered,
    Checkbox,
    Ordered(u64, char),
}

struct ListPrefix {
    kind: ListKind,
    raw: String,
    indent: String,
}

fn matching_pair(c: char) -> Option<char> {
    match c {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '*' => Some('*'),
        '_' => Some('_'),
        '`' => Some('`'),
        _ => None,
    }
}

fn is_closing_char(c: char) -> bool {
    matches!(c, ')' | ']' | '}' | '"' | '\'' | '*' | '_' | '`')
}

fn parse_list_prefix(line: &str) -> Option<ListPrefix> {
    let indent_len = line
        .chars()
        .take_while(|c| c.is_whitespace() && *c != '\n' && *c != '\r')
        .count();
    let indent: String = line.chars().take(indent_len).collect();
    let rest = &line[indent.len()..];

    if rest.starts_with("- [ ] ") || rest.starts_with("- [x] ") || rest.starts_with("- [X] ") {
        return Some(ListPrefix {
            kind: ListKind::Checkbox,
            raw: rest[..6].to_string(),
            indent,
        });
    }
    if rest.starts_with("* [ ] ") || rest.starts_with("* [x] ") || rest.starts_with("* [X] ") {
        return Some(ListPrefix {
            kind: ListKind::Checkbox,
            raw: rest[..6].to_string(),
            indent,
        });
    }

    if rest.starts_with("- ") || rest.starts_with("* ") || rest.starts_with("+ ") {
        return Some(ListPrefix {
            kind: ListKind::Unordered,
            raw: rest[..2].to_string(),
            indent,
        });
    }

    let digit_count = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digit_count > 0 {
        let num_str = &rest[..digit_count];
        let post_digits = &rest[digit_count..];
        if (post_digits.starts_with(". ") || post_digits.starts_with(") "))
            && let Ok(num) = num_str.parse::<u64>()
            && let Some(delimiter) = post_digits.chars().next()
        {
            return Some(ListPrefix {
                kind: ListKind::Ordered(num, delimiter),
                raw: rest[..digit_count + 2].to_string(),
                indent,
            });
        }
    }

    None
}

fn is_separator_row(row_cells: &[String]) -> bool {
    row_cells.iter().all(|cell| {
        let trimmed = cell.trim();
        !trimmed.is_empty() && trimmed.chars().all(|c| c == '-' || c == ':' || c == ' ')
    })
}

fn is_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() > 1
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

    fn insert(&mut self, text: &str) -> ApplyResult {
        if text.is_empty() {
            return ApplyResult::default();
        }
        let old_total = self.line_count();
        let before = self.selections.clone();
        let mut ops: Vec<EditOp> = Vec::new();
        let mut acc = SpanAcc::default();
        let mut new_sels = Vec::with_capacity(before.len());
        let mut delta: isize = 0;
        let mut replaced_range = false;

        let is_newline = text == "\n" || text == "\r\n";

        if is_newline {
            for sel in &before {
                let (start, end) = sel.range();
                let start = shift(start, delta);
                let end = shift(end, delta);
                let new_caret = self.insert_newline(start, end, &mut acc, &mut ops, &mut delta);
                new_sels.push(Selection::caret(new_caret));
            }
            self.replace_selections(new_sels);
            let after = self.selections.clone();
            let _node = self.undo.commit(Transaction { ops, before, after });
            self.coalesce = None;
            return ApplyResult {
                text_changed: true,
                selection_changed: true,
                changed: acc.span(old_total, self.line_count()),
            };
        }

        let is_single_char = text.chars().count() == 1;
        let typed_char = is_single_char.then(|| text.chars().next()).flatten();
        let is_url = text.starts_with("http://") || text.starts_with("https://");

        for sel in &before {
            let (start, end) = sel.range();
            let start = shift(start, delta);
            let end = shift(end, delta);

            if end > start {
                replaced_range = true;
                if is_url {
                    let removed = self.rope_delete(start, end, &mut acc);
                    ops.push(EditOp::Delete {
                        at: start,
                        text: removed.clone(),
                    });
                    let replacement = format!("[{}]({})", removed, text);
                    self.rope_insert(start, &replacement, &mut acc);
                    ops.push(EditOp::Insert {
                        at: start,
                        text: replacement.clone(),
                    });

                    let new_caret = start + replacement.chars().count();
                    new_sels.push(Selection::caret(new_caret));
                    delta += replacement.chars().count() as isize - (end - start) as isize;
                } else if let Some(c) = typed_char
                    && let Some(close_c) = matching_pair(c)
                {
                    let removed = self.rope_delete(start, end, &mut acc);
                    ops.push(EditOp::Delete {
                        at: start,
                        text: removed.clone(),
                    });
                    let replacement = format!("{}{}{}", c, removed, close_c);
                    self.rope_insert(start, &replacement, &mut acc);
                    ops.push(EditOp::Insert {
                        at: start,
                        text: replacement.clone(),
                    });

                    new_sels.push(Selection::new(
                        start + 1,
                        start + 1 + removed.chars().count(),
                    ));
                    delta += 2;
                } else {
                    let removed = self.rope_delete(start, end, &mut acc);
                    ops.push(EditOp::Delete {
                        at: start,
                        text: removed,
                    });
                    self.rope_insert(start, text, &mut acc);
                    ops.push(EditOp::Insert {
                        at: start,
                        text: text.to_string(),
                    });
                    let text_chars = text.chars().count();
                    new_sels.push(Selection::caret(start + text_chars));
                    delta += text_chars as isize - (end - start) as isize;
                }
            } else {
                if let Some(c) = typed_char {
                    let next_char = if start < self.rope.len_chars() {
                        Some(self.rope.char(start))
                    } else {
                        None
                    };

                    if is_closing_char(c) && next_char == Some(c) {
                        new_sels.push(Selection::caret(start + 1));
                    } else if let Some(close_c) = matching_pair(c) {
                        let should_pair = next_char.is_none_or(|nc| !nc.is_alphanumeric());
                        if should_pair {
                            let pair_str = format!("{}{}", c, close_c);
                            self.rope_insert(start, &pair_str, &mut acc);
                            ops.push(EditOp::Insert {
                                at: start,
                                text: pair_str,
                            });
                            new_sels.push(Selection::caret(start + 1));
                            delta += 2;
                        } else {
                            self.rope_insert(start, text, &mut acc);
                            ops.push(EditOp::Insert {
                                at: start,
                                text: text.to_string(),
                            });
                            new_sels.push(Selection::caret(start + 1));
                            delta += 1;
                        }
                    } else {
                        self.rope_insert(start, text, &mut acc);
                        ops.push(EditOp::Insert {
                            at: start,
                            text: text.to_string(),
                        });
                        new_sels.push(Selection::caret(start + 1));
                        delta += 1;
                    }
                } else {
                    self.rope_insert(start, text, &mut acc);
                    ops.push(EditOp::Insert {
                        at: start,
                        text: text.to_string(),
                    });
                    let text_chars = text.chars().count();
                    new_sels.push(Selection::caret(start + text_chars));
                    delta += text_chars as isize;
                }
            }
        }
        self.replace_selections(new_sels);
        self.record(ops, before, replaced_range, text);
        ApplyResult {
            text_changed: true,
            selection_changed: true,
            changed: acc.span(old_total, self.line_count()),
        }
    }

    fn delete(&mut self, direction: Direction) -> ApplyResult {
        let old_total = self.line_count();
        let before = self.selections.clone();
        let mut ops: Vec<EditOp> = Vec::new();
        let mut acc = SpanAcc::default();
        let mut new_sels = Vec::with_capacity(before.len());
        let mut delta: isize = 0;
        for sel in &before {
            let (start, end) = sel.range();
            let (start, end) = (shift(start, delta), shift(end, delta));
            let (start, end) = if end > start {
                (start, end)
            } else {
                match direction {
                    Direction::Backward => {
                        if start > 0 && start < self.rope.len_chars() {
                            let prev_c = self.rope.char(start - 1);
                            let next_c = self.rope.char(start);
                            if let Some(expected_next) = matching_pair(prev_c) {
                                if next_c == expected_next {
                                    (start - 1, start + 1)
                                } else {
                                    (prev_grapheme(&self.rope, start), start)
                                }
                            } else {
                                (prev_grapheme(&self.rope, start), start)
                            }
                        } else {
                            (prev_grapheme(&self.rope, start), start)
                        }
                    }
                    Direction::Forward => (start, next_grapheme(&self.rope, start)),
                }
            };
            if end > start {
                let removed = self.rope_delete(start, end, &mut acc);
                ops.push(EditOp::Delete {
                    at: start,
                    text: removed,
                });
                delta -= (end - start) as isize;
            }
            new_sels.push(Selection::caret(start));
        }
        let text_changed = !ops.is_empty();
        self.replace_selections(new_sels);
        if text_changed {
            let after = self.selections.clone();
            self.undo.commit(Transaction { ops, before, after });
        }
        ApplyResult {
            text_changed,
            selection_changed: true,
            changed: acc.span(old_total, self.line_count()),
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

    fn toggle_wrap(&mut self, marker: &str) -> ApplyResult {
        let old_total = self.line_count();
        let before = self.selections.clone();
        let mut ops: Vec<EditOp> = Vec::new();
        let mut acc = SpanAcc::default();
        let mut new_sels = Vec::with_capacity(before.len());
        let mut delta: isize = 0;
        let m_len = marker.chars().count();

        for sel in &before {
            let (start, end) = sel.range();
            let start = shift(start, delta);
            let end = shift(end, delta);

            let is_empty = start == end;
            let mut unwrapped = false;

            if is_empty {
                if start >= m_len && start + m_len <= self.rope.len_chars() {
                    let prev = self.rope.slice(start - m_len..start).to_string();
                    let next = self.rope.slice(start..start + m_len).to_string();
                    if prev == marker && next == marker {
                        let r2 = self.rope_delete(start, start + m_len, &mut acc);
                        ops.push(EditOp::Delete {
                            at: start,
                            text: r2,
                        });
                        let r1 = self.rope_delete(start - m_len, start, &mut acc);
                        ops.push(EditOp::Delete {
                            at: start - m_len,
                            text: r1,
                        });

                        new_sels.push(Selection::caret(start - m_len));
                        delta -= (m_len * 2) as isize;
                        unwrapped = true;
                    }
                }
            } else {
                if end - start >= m_len * 2 {
                    let first = self.rope.slice(start..start + m_len).to_string();
                    let last = self.rope.slice(end - m_len..end).to_string();
                    if first == marker && last == marker {
                        let r2 = self.rope_delete(end - m_len, end, &mut acc);
                        ops.push(EditOp::Delete {
                            at: end - m_len,
                            text: r2,
                        });
                        let r1 = self.rope_delete(start, start + m_len, &mut acc);
                        ops.push(EditOp::Delete {
                            at: start,
                            text: r1,
                        });

                        new_sels.push(Selection::new(start, end - m_len * 2));
                        delta -= (m_len * 2) as isize;
                        unwrapped = true;
                    }
                }

                if !unwrapped && start >= m_len && end + m_len <= self.rope.len_chars() {
                    let prev = self.rope.slice(start - m_len..start).to_string();
                    let next = self.rope.slice(end..end + m_len).to_string();
                    if prev == marker && next == marker {
                        let r2 = self.rope_delete(end, end + m_len, &mut acc);
                        ops.push(EditOp::Delete { at: end, text: r2 });
                        let r1 = self.rope_delete(start - m_len, start, &mut acc);
                        ops.push(EditOp::Delete {
                            at: start - m_len,
                            text: r1,
                        });

                        new_sels.push(Selection::new(start - m_len, end - m_len));
                        delta -= (m_len * 2) as isize;
                        unwrapped = true;
                    }
                }
            }

            if !unwrapped {
                self.rope_insert(end, marker, &mut acc);
                ops.push(EditOp::Insert {
                    at: end,
                    text: marker.to_string(),
                });
                self.rope_insert(start, marker, &mut acc);
                ops.push(EditOp::Insert {
                    at: start,
                    text: marker.to_string(),
                });

                if is_empty {
                    new_sels.push(Selection::caret(start + m_len));
                } else {
                    new_sels.push(Selection::new(start + m_len, end + m_len));
                }
                delta += (m_len * 2) as isize;
            }
        }

        let text_changed = !ops.is_empty();
        self.replace_selections(new_sels);
        if text_changed {
            let after = self.selections.clone();
            self.undo.commit(Transaction { ops, before, after });
        }
        ApplyResult {
            text_changed,
            selection_changed: true,
            changed: acc.span(old_total, self.line_count()),
        }
    }

    fn toggle_wrap_double(&mut self, start_marker: &str, end_marker: &str) -> ApplyResult {
        let old_total = self.line_count();
        let before = self.selections.clone();
        let mut ops: Vec<EditOp> = Vec::new();
        let mut acc = SpanAcc::default();
        let mut new_sels = Vec::with_capacity(before.len());
        let mut delta: isize = 0;
        let s_len = start_marker.chars().count();
        let e_len = end_marker.chars().count();

        for sel in &before {
            let (start, end) = sel.range();
            let start = shift(start, delta);
            let end = shift(end, delta);

            let is_empty = start == end;
            let mut unwrapped = false;

            if is_empty {
                if start >= s_len && start + e_len <= self.rope.len_chars() {
                    let prev = self.rope.slice(start - s_len..start).to_string();
                    let next = self.rope.slice(start..start + e_len).to_string();
                    if prev == start_marker && next == end_marker {
                        let r2 = self.rope_delete(start, start + e_len, &mut acc);
                        ops.push(EditOp::Delete {
                            at: start,
                            text: r2,
                        });
                        let r1 = self.rope_delete(start - s_len, start, &mut acc);
                        ops.push(EditOp::Delete {
                            at: start - s_len,
                            text: r1,
                        });

                        new_sels.push(Selection::caret(start - s_len));
                        delta -= (s_len + e_len) as isize;
                        unwrapped = true;
                    }
                }
            } else {
                if end - start >= s_len + e_len {
                    let first = self.rope.slice(start..start + s_len).to_string();
                    let last = self.rope.slice(end - e_len..end).to_string();
                    if first == start_marker && last == end_marker {
                        let r2 = self.rope_delete(end - e_len, end, &mut acc);
                        ops.push(EditOp::Delete {
                            at: end - e_len,
                            text: r2,
                        });
                        let r1 = self.rope_delete(start, start + s_len, &mut acc);
                        ops.push(EditOp::Delete {
                            at: start,
                            text: r1,
                        });

                        new_sels.push(Selection::new(start, end - s_len - e_len));
                        delta -= (s_len + e_len) as isize;
                        unwrapped = true;
                    }
                }

                if !unwrapped && start >= s_len && end + e_len <= self.rope.len_chars() {
                    let prev = self.rope.slice(start - s_len..start).to_string();
                    let next = self.rope.slice(end..end + e_len).to_string();
                    if prev == start_marker && next == end_marker {
                        let r2 = self.rope_delete(end, end + e_len, &mut acc);
                        ops.push(EditOp::Delete { at: end, text: r2 });
                        let r1 = self.rope_delete(start - s_len, start, &mut acc);
                        ops.push(EditOp::Delete {
                            at: start - s_len,
                            text: r1,
                        });

                        new_sels.push(Selection::new(start - s_len, end - s_len));
                        delta -= (s_len + e_len) as isize;
                        unwrapped = true;
                    }
                }
            }

            if !unwrapped {
                self.rope_insert(end, end_marker, &mut acc);
                ops.push(EditOp::Insert {
                    at: end,
                    text: end_marker.to_string(),
                });
                self.rope_insert(start, start_marker, &mut acc);
                ops.push(EditOp::Insert {
                    at: start,
                    text: start_marker.to_string(),
                });

                if is_empty {
                    new_sels.push(Selection::caret(start + s_len));
                } else {
                    new_sels.push(Selection::new(start + s_len, end + s_len));
                }
                delta += (s_len + e_len) as isize;
            }
        }

        let text_changed = !ops.is_empty();
        self.replace_selections(new_sels);
        if text_changed {
            let after = self.selections.clone();
            self.undo.commit(Transaction { ops, before, after });
        }
        ApplyResult {
            text_changed,
            selection_changed: true,
            changed: acc.span(old_total, self.line_count()),
        }
    }

    fn mutate_lines<F>(&mut self, mut mutate_fn: F) -> ApplyResult
    where
        F: FnMut(&str) -> (usize, usize, String),
    {
        let old_total = self.line_count();
        let before = self.selections.clone();
        let mut ops: Vec<EditOp> = Vec::new();
        let mut acc = SpanAcc::default();

        let mut lines = std::collections::BTreeSet::new();
        for sel in &before {
            let (start, end) = sel.range();
            let start_line = self.rope.char_to_line(start);
            let end_line = self.rope.char_to_line(end);
            for l in start_line..=end_line {
                lines.insert(l);
            }
        }

        let mut new_sels = before.clone();

        for l in lines.into_iter().rev() {
            if l >= self.rope.len_lines() {
                continue;
            }
            let line_start = self.rope.line_to_char(l);
            let line_end = self.rope.line_to_char(l + 1).min(self.rope.len_chars());
            let line_str = self.rope.slice(line_start..line_end).to_string();

            let (local_offset, delete_len, insert_text) = mutate_fn(&line_str);
            let edit_at = line_start + local_offset;

            let delta = insert_text.chars().count() as isize - delete_len as isize;

            if delete_len > 0 {
                let removed = self.rope_delete(edit_at, edit_at + delete_len, &mut acc);
                ops.push(EditOp::Delete {
                    at: edit_at,
                    text: removed,
                });
            }
            if !insert_text.is_empty() {
                self.rope_insert(edit_at, &insert_text, &mut acc);
                ops.push(EditOp::Insert {
                    at: edit_at,
                    text: insert_text,
                });
            }

            for sel in &mut new_sels {
                if sel.anchor >= edit_at {
                    sel.anchor = shift(sel.anchor, delta);
                }
                if sel.head >= edit_at {
                    sel.head = shift(sel.head, delta);
                }
            }
        }

        let text_changed = !ops.is_empty();
        self.replace_selections(new_sels);
        if text_changed {
            let after = self.selections.clone();
            self.undo.commit(Transaction { ops, before, after });
        }
        ApplyResult {
            text_changed,
            selection_changed: true,
            changed: acc.span(old_total, self.line_count()),
        }
    }

    fn heading_cycle(&mut self) -> ApplyResult {
        self.mutate_lines(|line| {
            if line.starts_with("###### ") {
                (0, 7, String::new())
            } else if line.starts_with("##### ") {
                (0, 6, "###### ".to_string())
            } else if line.starts_with("#### ") {
                (0, 5, "##### ".to_string())
            } else if line.starts_with("### ") {
                (0, 4, "#### ".to_string())
            } else if line.starts_with("## ") {
                (0, 3, "### ".to_string())
            } else if line.starts_with("# ") {
                (0, 2, "## ".to_string())
            } else {
                (0, 0, "# ".to_string())
            }
        })
    }

    fn toggle_bullet(&mut self) -> ApplyResult {
        self.mutate_lines(|line| {
            if line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ") {
                (0, 2, String::new())
            } else {
                (0, 0, "- ".to_string())
            }
        })
    }

    fn toggle_checkbox(&mut self) -> ApplyResult {
        self.mutate_lines(|line| {
            let list_marker = if line.starts_with("- ") {
                Some("- ")
            } else if line.starts_with("* ") {
                Some("* ")
            } else if line.starts_with("+ ") {
                Some("+ ")
            } else {
                None
            };

            if let Some(marker) = list_marker {
                let after_marker = &line[marker.len()..];
                if after_marker.starts_with("[ ] ") {
                    (marker.len(), 4, "[x] ".to_string())
                } else if after_marker.starts_with("[x] ") {
                    (marker.len(), 4, "[ ] ".to_string())
                } else {
                    (marker.len(), 0, "[ ] ".to_string())
                }
            } else {
                (0, 0, "- [ ] ".to_string())
            }
        })
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

    fn insert_newline(
        &mut self,
        start: usize,
        end: usize,
        acc: &mut SpanAcc,
        ops: &mut Vec<EditOp>,
        delta: &mut isize,
    ) -> usize {
        let start = if end > start {
            let removed = self.rope_delete(start, end, acc);
            ops.push(EditOp::Delete {
                at: start,
                text: removed,
            });
            *delta -= (end - start) as isize;
            start
        } else {
            start
        };

        let line_idx = self.rope.char_to_line(start);
        let line_start = self.rope.line_to_char(line_idx);
        let line_text: String = self.rope.slice(line_start..start).to_string();

        if let Some(prefix) = parse_list_prefix(&line_text) {
            let rest_of_line = &line_text[prefix.indent.len() + prefix.raw.len()..];
            if rest_of_line.trim().is_empty() {
                let delete_start = line_start;
                let delete_end = line_start + prefix.indent.len() + prefix.raw.len();
                let removed = self.rope_delete(delete_start, delete_end, acc);
                ops.push(EditOp::Delete {
                    at: delete_start,
                    text: removed,
                });
                *delta -= (delete_end - delete_start) as isize;
                return delete_start;
            } else {
                let continued_raw = match &prefix.kind {
                    ListKind::Unordered => prefix.raw.clone(),
                    ListKind::Checkbox => {
                        if prefix.raw.starts_with("- [") {
                            "- [ ] ".to_string()
                        } else {
                            "* [ ] ".to_string()
                        }
                    }
                    ListKind::Ordered(num, delimiter) => {
                        format!("{}{}{} ", "", num + 1, delimiter)
                    }
                };
                let continued = format!("{}{}", prefix.indent, continued_raw);
                let text_to_insert = format!("\n{}", continued);
                self.rope_insert(start, &text_to_insert, acc);
                ops.push(EditOp::Insert {
                    at: start,
                    text: text_to_insert.clone(),
                });
                *delta += text_to_insert.chars().count() as isize;

                if let ListKind::Ordered(_, delimiter) = prefix.kind {
                    let mut next_line_idx = line_idx + 2;
                    let mut current_delta = *delta;
                    while next_line_idx < self.rope.len_lines() {
                        let next_line_start = self.rope.line_to_char(next_line_idx);
                        let next_line_end = self
                            .rope
                            .line_to_char(next_line_idx + 1)
                            .min(self.rope.len_chars());
                        let next_line_text: String =
                            self.rope.slice(next_line_start..next_line_end).to_string();

                        if let Some(next_prefix) = parse_list_prefix(&next_line_text) {
                            if let ListKind::Ordered(next_num, next_delim) = next_prefix.kind {
                                if next_delim == delimiter && next_prefix.indent == prefix.indent {
                                    let new_num = next_num + 1;
                                    let old_prefix_len =
                                        next_prefix.indent.len() + next_prefix.raw.len();

                                    let del_start = next_line_start;
                                    let del_end = next_line_start + old_prefix_len;
                                    let removed = self.rope_delete(del_start, del_end, acc);
                                    ops.push(EditOp::Delete {
                                        at: del_start,
                                        text: removed,
                                    });
                                    current_delta -= old_prefix_len as isize;

                                    let new_prefix_raw =
                                        format!("{}{}{} ", next_prefix.indent, new_num, next_delim);
                                    self.rope_insert(del_start, &new_prefix_raw, acc);
                                    ops.push(EditOp::Insert {
                                        at: del_start,
                                        text: new_prefix_raw.clone(),
                                    });
                                    current_delta += new_prefix_raw.chars().count() as isize;
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                        next_line_idx += 1;
                    }
                    *delta = current_delta;
                }

                return start + text_to_insert.chars().count();
            }
        }

        self.rope_insert(start, "\n", acc);
        ops.push(EditOp::Insert {
            at: start,
            text: "\n".to_string(),
        });
        *delta += 1;
        start + 1
    }

    fn table_cell_nav(&mut self, backward: bool) -> Option<ApplyResult> {
        let before = self.selections.clone();
        if before.len() != 1 {
            return None;
        }
        let sel = before[0];
        let (start, end) = sel.range();
        if start != end {
            return None;
        }

        let line_idx = self.rope.char_to_line(start);
        let line_start = self.rope.line_to_char(line_idx);
        let line_text = self.rope.line(line_idx).to_string();

        if !is_table_row(&line_text) {
            return None;
        }

        let mut start_line = line_idx;
        while start_line > 0 {
            let prev = self.rope.line(start_line - 1).to_string();
            if is_table_row(&prev) {
                start_line -= 1;
            } else {
                break;
            }
        }
        let mut end_line = line_idx;
        while end_line + 1 < self.rope.len_lines() {
            let next = self.rope.line(end_line + 1).to_string();
            if is_table_row(&next) {
                end_line += 1;
            } else {
                break;
            }
        }

        let mut rows: Vec<Vec<String>> = Vec::new();
        for l in start_line..=end_line {
            let row_text = self.rope.line(l).to_string();
            let trimmed_row = row_text.trim();
            let parts: Vec<&str> = trimmed_row.split('|').collect();
            let mut row_cells = Vec::new();
            let start_p = if parts.first() == Some(&"") { 1 } else { 0 };
            let end_p = if parts.last() == Some(&"") {
                parts.len().saturating_sub(1)
            } else {
                parts.len()
            };
            for part in parts.iter().take(end_p).skip(start_p) {
                row_cells.push(part.trim().to_string());
            }
            rows.push(row_cells);
        }

        let mut col_count = 0;
        for r in &rows {
            col_count = col_count.max(r.len());
        }
        if col_count == 0 {
            return None;
        }

        for r in &mut rows {
            while r.len() < col_count {
                r.push(String::new());
            }
        }

        let mut alignments = vec![Alignment::Left; col_count];
        let mut sep_row_idx = None;
        for (i, r) in rows.iter().enumerate() {
            if is_separator_row(r) {
                sep_row_idx = Some(i);
                for (c, cell) in r.iter().enumerate() {
                    let trimmed = cell.trim();
                    let left = trimmed.starts_with(':');
                    let right = trimmed.ends_with(':');
                    alignments[c] = if left && right {
                        Alignment::Center
                    } else if right {
                        Alignment::Right
                    } else {
                        Alignment::Left
                    };
                }
                break;
            }
        }

        let mut col_widths = vec![3; col_count];
        for (i, r) in rows.iter().enumerate() {
            if Some(i) == sep_row_idx {
                continue;
            }
            for (c, cell) in r.iter().enumerate() {
                col_widths[c] = col_widths[c].max(cell.chars().count());
            }
        }

        let line_text_before_caret = self.rope.slice(line_start..start).to_string();
        let pipe_count = line_text_before_caret.chars().filter(|c| *c == '|').count();
        let current_cell_col = pipe_count.saturating_sub(1).min(col_count - 1);
        let current_cell_row = line_idx - start_line;

        let mut target_row = current_cell_row;
        let mut target_col = current_cell_col;
        let mut append_row = false;

        if backward {
            if target_col > 0 {
                target_col -= 1;
            } else if target_row > 0 {
                target_row -= 1;
                if Some(target_row) == sep_row_idx {
                    if target_row > 0 {
                        target_row -= 1;
                    } else {
                        target_row = current_cell_row;
                    }
                }
                target_col = col_count - 1;
            }
        } else {
            if target_col + 1 < col_count {
                target_col += 1;
            } else {
                target_row += 1;
                if Some(target_row) == sep_row_idx {
                    target_row += 1;
                }
                target_col = 0;
                if target_row >= rows.len() {
                    append_row = true;
                }
            }
        }

        if append_row {
            let new_row = vec![String::new(); col_count];
            rows.push(new_row);
            target_row = rows.len() - 1;
            target_col = 0;
        }

        let mut formatted_lines = Vec::new();
        let mut target_caret_offset = None;
        let mut current_char_offset = self.rope.line_to_char(start_line);

        for (r_idx, r) in rows.iter().enumerate() {
            let mut line_str = "|".to_string();

            for (c_idx, cell) in r.iter().enumerate() {
                let width = col_widths[c_idx];
                if Some(r_idx) == sep_row_idx {
                    let indicator = match alignments[c_idx] {
                        Alignment::Left => {
                            let mut s = ":".to_string();
                            s.push_str(&"-".repeat(width - 1));
                            s
                        }
                        Alignment::Center => {
                            let mut s = ":".to_string();
                            s.push_str(&"-".repeat(width - 2));
                            s.push(':');
                            s
                        }
                        Alignment::Right => {
                            let mut s = "-".to_string();
                            s.push_str(&"-".repeat(width - 2));
                            s.push(':');
                            s
                        }
                    };
                    line_str.push_str(&format!(" {} |", indicator));
                } else {
                    let cell_len = cell.chars().count();
                    let pad_needed = width.saturating_sub(cell_len);
                    let formatted_cell = match alignments[c_idx] {
                        Alignment::Left => {
                            format!("{}{}", cell, " ".repeat(pad_needed))
                        }
                        Alignment::Right => {
                            format!("{}{}", " ".repeat(pad_needed), cell)
                        }
                        Alignment::Center => {
                            let left_pad = pad_needed / 2;
                            let right_pad = pad_needed - left_pad;
                            format!("{}{}{}", " ".repeat(left_pad), cell, " ".repeat(right_pad))
                        }
                    };

                    if r_idx == target_row && c_idx == target_col {
                        let caret_offset_in_cell = match alignments[c_idx] {
                            Alignment::Left => 1,
                            Alignment::Right => 1 + pad_needed,
                            Alignment::Center => 1 + pad_needed / 2,
                        };
                        target_caret_offset = Some(
                            current_char_offset + line_str.chars().count() + caret_offset_in_cell,
                        );
                    }

                    line_str.push_str(&format!(" {} |", formatted_cell));
                }
            }

            line_str.push('\n');
            current_char_offset += line_str.chars().count();
            formatted_lines.push(line_str);
        }

        let table_start_char = self.rope.line_to_char(start_line);
        let table_end_char = self
            .rope
            .line_to_char(end_line + 1)
            .min(self.rope.len_chars());
        let old_total = self.line_count();

        let mut ops = Vec::new();
        let mut acc = SpanAcc::default();

        let removed = self.rope_delete(table_start_char, table_end_char, &mut acc);
        ops.push(EditOp::Delete {
            at: table_start_char,
            text: removed,
        });

        let replacement = formatted_lines.concat();
        self.rope_insert(table_start_char, &replacement, &mut acc);
        ops.push(EditOp::Insert {
            at: table_start_char,
            text: replacement,
        });

        let final_caret = target_caret_offset.unwrap_or(table_start_char);
        self.replace_selections(vec![Selection::caret(final_caret)]);

        let after = self.selections.clone();
        self.undo.commit(Transaction { ops, before, after });

        Some(ApplyResult {
            text_changed: true,
            selection_changed: true,
            changed: acc.span(old_total, self.line_count()),
        })
    }

    fn normal_tab(&mut self, backward: bool) -> ApplyResult {
        if backward {
            self.mutate_lines(|line| {
                if line.starts_with("  ") {
                    (0, 2, String::new())
                } else if line.starts_with(" ") {
                    (0, 1, String::new())
                } else {
                    (0, 0, String::new())
                }
            })
        } else {
            self.insert("  ")
        }
    }

    fn set_heading(&mut self, level: usize) -> ApplyResult {
        let level = level.clamp(1, 6);
        self.mutate_lines(|line| {
            let current_level = if line.starts_with("###### ") {
                6
            } else if line.starts_with("##### ") {
                5
            } else if line.starts_with("#### ") {
                4
            } else if line.starts_with("### ") {
                3
            } else if line.starts_with("## ") {
                2
            } else if line.starts_with("# ") {
                1
            } else {
                0
            };

            if current_level == level {
                let prefix_len = level + 1;
                (0, prefix_len, String::new())
            } else {
                let prefix_len = if current_level > 0 {
                    current_level + 1
                } else {
                    0
                };
                let new_prefix = format!("{} ", "#".repeat(level));
                (0, prefix_len, new_prefix)
            }
        })
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

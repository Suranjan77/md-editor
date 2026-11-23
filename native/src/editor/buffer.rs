use std::time::{Duration, Instant};

use ropey::Rope;
use serde::{Deserialize, Serialize};

/// Consecutive single-character edits committed within this window merge into
/// one undo step. Long enough to swallow a burst of typing, short enough that a
/// deliberate pause reliably starts a new step.
const UNDO_COALESCE_WINDOW: Duration = Duration::from_millis(300);

/// Whether `next` continues the run of typing (or deleting) recorded in `prev`,
/// and so belongs in the same undo step.
///
/// Only single-character, same-direction, adjacent edits merge. A newline ends
/// a run, so undo always stops at a line boundary at the latest.
fn can_coalesce(prev: &[EditOp], next: &[EditOp]) -> bool {
    let (Some(last), 1) = (prev.last(), next.len()) else {
        return false;
    };

    match (last, &next[0]) {
        (
            EditOp::Insert {
                char_offset: prev_at,
                text: prev_text,
            },
            EditOp::Insert {
                char_offset: next_at,
                text: next_text,
            },
        ) => {
            next_text.chars().count() == 1
                && !next_text.contains('\n')
                && !prev_text.contains('\n')
                // The new character lands exactly where the previous one ended.
                && *next_at == prev_at + prev_text.chars().count()
        }
        (
            EditOp::Delete {
                char_offset: prev_at,
                text: prev_text,
            },
            EditOp::Delete {
                char_offset: next_at,
                text: next_text,
            },
        ) => {
            next_text.chars().count() == 1
                && !next_text.contains('\n')
                && !prev_text.contains('\n')
                // Backspace: each delete removes the character just before the
                // previous one. Forward-delete repeats at the same offset.
                && (*next_at + next_text.chars().count() == *prev_at || next_at == prev_at)
        }
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub anchor: usize,
    pub focus: usize,
}

impl Selection {
    pub fn new(anchor: usize, focus: usize) -> Option<Self> {
        if anchor == focus {
            None
        } else {
            Some(Self { anchor, focus })
        }
    }

    pub fn range(self) -> (usize, usize) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EditOp {
    Insert { char_offset: usize, text: String },
    Delete { char_offset: usize, text: String },
}

impl EditOp {
    fn inverse(&self) -> Self {
        match self {
            EditOp::Insert { char_offset, text } => EditOp::Delete {
                char_offset: *char_offset,
                text: text.clone(),
            },
            EditOp::Delete { char_offset, text } => EditOp::Insert {
                char_offset: *char_offset,
                text: text.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditTransaction {
    ops: Vec<EditOp>,
    before_cursor: usize,
    after_cursor: usize,
    before_selection: Option<Selection>,
    after_selection: Option<Selection>,
    before_revision: u64,
    after_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Movement {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

/// Which side of a row break the caret is on. Where a wrapped line breaks,
/// one column is both the end of a row and the start of the next.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Affinity {
    /// At the end of the earlier row, as after clicking past a row's end.
    Upstream,
    /// At the start of the later row.
    #[default]
    Downstream,
}

#[derive(Debug, Clone)]
pub enum EditorCommand {
    InsertText(String),
    DeleteSelection,
    DeleteBackward,
    DeleteForward,
    MoveCursor {
        movement: Movement,
        extend: bool,
    },
    SetCursor {
        line: usize,
        col: usize,
        affinity: Affinity,
    },
    SetSelection {
        anchor_line: usize,
        anchor_col: usize,
        focus_line: usize,
        focus_col: usize,
        /// Of the focus, where the caret is.
        affinity: Affinity,
    },
    SelectAll,
    ToggleCheckbox {
        line: usize,
    },
    FormatBold,
    FormatItalic,
    FormatInlineCode,
    InsertLink,
    /// A single typed character that may participate in auto-pairing
    /// (brackets/quotes). Falls back to plain insertion when not paired.
    TypePaired(char),
    Undo,
    Redo,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommandResult {
    pub text_changed: bool,
    pub projection_changed: bool,
    pub media_changed: bool,
}

impl CommandResult {
    fn changed() -> Self {
        Self {
            text_changed: true,
            projection_changed: true,
            media_changed: true,
        }
    }
}

pub struct DocBuffer {
    rope: Rope,
    cursor_offset: usize,
    selection_offsets: Option<Selection>,
    desired_col: Option<usize>,

    pub cursor_line: usize,
    pub cursor_col: usize,
    /// Which row the caret is drawn on when its column is a row break. Every
    /// change resets it; only placing the caret by position sets it, so it
    /// can never outlive the placement it describes.
    pub cursor_affinity: Affinity,
    pub selection: Option<(usize, usize, usize, usize)>,
    pub dirty: bool,

    undo_stack: Vec<EditTransaction>,
    redo_stack: Vec<EditTransaction>,
    /// When the newest undo transaction was recorded, for coalescing runs of
    /// typing into a single step.
    last_commit_at: Option<Instant>,
    revision: u64,
    saved_revision: u64,
    next_revision: u64,
    eol: &'static str,
}

impl DocBuffer {
    pub fn new() -> Self {
        Self::from_text("")
    }

    pub fn from_text(text: &str) -> Self {
        let mut buffer = Self {
            rope: Rope::from_str(text),
            cursor_offset: 0,
            selection_offsets: None,
            desired_col: None,
            cursor_line: 0,
            cursor_col: 0,
            cursor_affinity: Affinity::Downstream,
            selection: None,
            dirty: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_commit_at: None,
            revision: 0,
            saved_revision: 0,
            next_revision: 1,
            eol: dominant_eol(text),
        };
        buffer.sync_public_state();
        buffer
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Replace the whole document, discarding undo history.
    ///
    /// Only for loading a different document into an existing buffer. To
    /// rewrite the text of the *current* document — replace-all, for instance —
    /// use [`DocBuffer::replace_all_text`], which stays undoable.
    pub fn set_text(&mut self, text: &str) {
        self.rope = Rope::from_str(text);
        self.cursor_offset = self.cursor_offset.min(self.rope.len_chars());
        self.selection_offsets = None;
        self.desired_col = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.last_commit_at = None;
        self.eol = dominant_eol(text);
        self.revision = self.next_revision;
        self.next_revision = self.next_revision.wrapping_add(1);
        self.update_dirty();
        self.sync_public_state();
    }

    /// Record the current revision as the one on disk. Undoing or redoing
    /// back to it clears `dirty` again.
    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
        self.update_dirty();
    }

    /// Rewrite the entire document as one undoable edit.
    ///
    /// A bulk rewrite is exactly the edit a user is most likely to want back,
    /// so it is recorded as a single transaction rather than clearing the undo
    /// stack the way [`DocBuffer::set_text`] does.
    pub fn replace_all_text(&mut self, text: &str) -> CommandResult {
        let old_text = self.rope.to_string();
        if old_text == text {
            return CommandResult::default();
        }

        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;

        let ops = vec![
            EditOp::Delete {
                char_offset: 0,
                text: old_text,
            },
            EditOp::Insert {
                char_offset: 0,
                text: text.to_string(),
            },
        ];
        for op in &ops {
            self.apply_op(op);
        }

        self.cursor_offset = before_cursor.min(self.rope.len_chars());
        self.selection_offsets = None;
        // A bulk rewrite is never part of a typing run.
        self.last_commit_at = None;
        self.commit_transaction(ops, before_cursor, before_selection);
        self.last_commit_at = None;
        CommandResult::changed()
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn line_text(&self, line: usize) -> String {
        if line < self.line_count() {
            self.rope
                .line(line)
                .to_string()
                .trim_end_matches(['\n', '\r'])
                .to_string()
        } else {
            String::new()
        }
    }

    pub fn cursor_offset(&self) -> usize {
        self.cursor_offset
    }

    pub fn selection_offsets(&self) -> Option<Selection> {
        self.selection_offsets
    }

    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection_offsets?.range();
        Some(self.rope.slice(start..end).to_string())
    }

    pub fn execute(&mut self, command: EditorCommand) -> CommandResult {
        match command {
            EditorCommand::InsertText(text) => self.insert_text(&text),
            EditorCommand::DeleteSelection => self.delete_selection(),
            EditorCommand::DeleteBackward => self.delete_backward(),
            EditorCommand::DeleteForward => self.delete_forward(),
            EditorCommand::MoveCursor { movement, extend } => {
                self.move_cursor(movement, extend);
                CommandResult::default()
            }
            EditorCommand::SetCursor {
                line,
                col,
                affinity,
            } => {
                self.set_cursor(line, col);
                self.cursor_affinity = affinity;
                CommandResult::default()
            }
            EditorCommand::SetSelection {
                anchor_line,
                anchor_col,
                focus_line,
                focus_col,
                affinity,
            } => {
                self.set_selection(anchor_line, anchor_col, focus_line, focus_col);
                self.cursor_affinity = affinity;
                CommandResult::default()
            }
            EditorCommand::SelectAll => {
                self.select_all();
                CommandResult::default()
            }
            EditorCommand::ToggleCheckbox { line } => self.toggle_checkbox(line),
            EditorCommand::FormatBold => self.wrap_selection_or_insert("**", "**", "bold"),
            EditorCommand::FormatItalic => self.wrap_selection_or_insert("*", "*", "italic"),
            EditorCommand::FormatInlineCode => self.wrap_selection_or_insert("`", "`", "code"),
            EditorCommand::InsertLink => self.wrap_selection_or_insert("[", "](url)", "link"),
            EditorCommand::TypePaired(c) => self.type_paired(c),
            EditorCommand::Undo => {
                if self.undo() {
                    CommandResult::changed()
                } else {
                    CommandResult::default()
                }
            }
            EditorCommand::Redo => {
                if self.redo() {
                    CommandResult::changed()
                } else {
                    CommandResult::default()
                }
            }
        }
    }

    pub fn insert_at_cursor(&mut self, text: &str) {
        self.insert_text(text);
    }

    pub fn backspace(&mut self) {
        self.delete_backward();
    }

    pub fn delete(&mut self) {
        self.delete_forward();
    }

    pub fn move_cursor_left(&mut self) {
        self.move_cursor(Movement::Left, false);
    }

    pub fn move_cursor_right(&mut self) {
        self.move_cursor(Movement::Right, false);
    }

    pub fn move_cursor_up(&mut self) {
        self.move_cursor(Movement::Up, false);
    }

    pub fn move_cursor_down(&mut self) {
        self.move_cursor(Movement::Down, false);
    }

    pub fn move_cursor_home(&mut self) {
        self.move_cursor(Movement::Home, false);
    }

    pub fn move_cursor_end(&mut self) {
        self.move_cursor(Movement::End, false);
    }

    pub fn undo(&mut self) -> bool {
        let Some(transaction) = self.undo_stack.pop() else {
            return false;
        };
        for op in transaction.ops.iter().rev().map(EditOp::inverse) {
            self.apply_op(&op);
        }
        self.cursor_offset = transaction.before_cursor.min(self.rope.len_chars());
        self.selection_offsets = transaction.before_selection;
        self.redo_stack.push(transaction);
        self.revision = self
            .redo_stack
            .last()
            .map(|transaction| transaction.before_revision)
            .unwrap_or(0);
        self.update_dirty();
        self.desired_col = None;
        // An edit made right after undoing starts its own step rather than
        // merging into the transaction we just moved off the stack.
        self.last_commit_at = None;
        self.sync_public_state();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(transaction) = self.redo_stack.pop() else {
            return false;
        };
        for op in &transaction.ops {
            self.apply_op(op);
        }
        self.cursor_offset = transaction.after_cursor.min(self.rope.len_chars());
        self.selection_offsets = transaction.after_selection;
        self.revision = transaction.after_revision;
        self.undo_stack.push(transaction);
        self.update_dirty();
        self.desired_col = None;
        // Likewise after redo: the restored transaction is closed to further
        // coalescing.
        self.last_commit_at = None;
        self.sync_public_state();
        true
    }

    pub fn set_cursor(&mut self, line: usize, col: usize) {
        self.cursor_offset = self.caret_stop(line, col);
        self.selection_offsets = None;
        self.desired_col = None;
        self.sync_public_state();
    }

    /// Place the cursor at a raw character offset, clamped to the document.
    ///
    /// Used when restoring a position recorded in a previous session, where the
    /// file may since have been edited elsewhere and grown shorter.
    pub fn set_cursor_offset(&mut self, offset: usize) {
        self.cursor_offset = offset.min(self.rope.len_chars());
        self.selection_offsets = None;
        self.desired_col = None;
        self.sync_public_state();
    }

    pub fn set_selection(
        &mut self,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) {
        let anchor = self.caret_stop(start_line, start_col);
        let focus = self.caret_stop(end_line, end_col);
        self.cursor_offset = focus;
        self.selection_offsets = Selection::new(anchor, focus);
        self.desired_col = None;
        self.sync_public_state();
    }

    pub fn select_all(&mut self) {
        self.cursor_offset = self.rope.len_chars();
        self.selection_offsets = Selection::new(0, self.cursor_offset);
        self.desired_col = None;
        self.sync_public_state();
    }

    fn insert_text(&mut self, text: &str) -> CommandResult {
        if text.is_empty() {
            return CommandResult::default();
        }

        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        let mut ops = Vec::new();
        let insert_at = if let Some(selection) = self.selection_offsets {
            let (start, end) = selection.range();
            let removed = self.rope.slice(start..end).to_string();
            self.rope.remove(start..end);
            ops.push(EditOp::Delete {
                char_offset: start,
                text: removed,
            });
            start
        } else {
            self.cursor_offset
        };

        let mut text_to_insert = text.to_string();

        if text == "\n" {
            text_to_insert = self.eol.to_string();
            let line_idx = self.rope.char_to_line(insert_at);
            let line_text = self.line_text(line_idx);
            if let Some(list_item) = parse_list_item(&line_text) {
                let line_start = self.rope.line_to_char(line_idx);
                let marker_end = line_start
                    + list_item.indent.chars().count()
                    + list_item.marker.chars().count();
                if insert_at >= marker_end {
                    if list_item.is_empty {
                        // Empty list item: clear the prefix on the current line and do NOT insert newline!
                        let marker_start = line_start + list_item.indent.chars().count();
                        let marker_len = list_item.marker.chars().count();
                        let marker_text = self
                            .rope
                            .slice(marker_start..marker_start + marker_len)
                            .to_string();
                        self.rope.remove(marker_start..marker_start + marker_len);
                        ops.push(EditOp::Delete {
                            char_offset: marker_start,
                            text: marker_text,
                        });

                        self.cursor_offset = marker_start;
                        self.selection_offsets = None;
                        self.commit_transaction(ops, before_cursor, before_selection);
                        return CommandResult::changed();
                    } else {
                        // Non-empty list item: auto-continue on next line!
                        let next_prefix = format!("{}{}", list_item.indent, list_item.next_marker);
                        text_to_insert = format!("{}{}", self.eol, next_prefix);
                    }
                }
            }
        }

        self.rope.insert(insert_at, &text_to_insert);
        ops.push(EditOp::Insert {
            char_offset: insert_at,
            text: text_to_insert.clone(),
        });

        self.cursor_offset = insert_at + text_to_insert.chars().count();
        self.selection_offsets = None;
        self.commit_transaction(ops, before_cursor, before_selection);
        CommandResult::changed()
    }

    fn delete_selection(&mut self) -> CommandResult {
        let Some(selection) = self.selection_offsets else {
            return CommandResult::default();
        };
        let (start, end) = selection.range();
        if start == end {
            self.selection_offsets = None;
            self.sync_public_state();
            return CommandResult::default();
        }

        let removed = self.rope.slice(start..end).to_string();
        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        self.rope.remove(start..end);
        self.cursor_offset = start;
        self.selection_offsets = None;
        self.commit_transaction(
            vec![EditOp::Delete {
                char_offset: start,
                text: removed,
            }],
            before_cursor,
            before_selection,
        );
        CommandResult::changed()
    }

    fn delete_backward(&mut self) -> CommandResult {
        if self.selection_offsets.is_some() {
            return self.delete_selection();
        }
        let start = self.grapheme_before(self.cursor_offset);
        if start == self.cursor_offset {
            return CommandResult::default();
        }
        // Backspace inside an empty auto-inserted pair removes both halves.
        let previous = self.char_at(self.cursor_offset - 1);
        let next = self.char_at(self.cursor_offset);
        if matches!(
            (previous, next),
            (Some('('), Some(')'))
                | (Some('['), Some(']'))
                | (Some('{'), Some('}'))
                | (Some('"'), Some('"'))
                | (Some('\''), Some('\''))
                | (Some('`'), Some('`'))
        ) {
            return self.delete_range(self.cursor_offset - 1, self.cursor_offset + 1);
        }
        self.delete_range(start, self.cursor_offset)
    }

    fn delete_forward(&mut self) -> CommandResult {
        if self.selection_offsets.is_some() {
            return self.delete_selection();
        }
        let end = self.grapheme_after(self.cursor_offset);
        if end == self.cursor_offset {
            return CommandResult::default();
        }
        self.delete_range(self.cursor_offset, end)
    }

    /// The caret stop before `offset`: the start of the grapheme cluster
    /// before it, or the end of the previous line's text — a line break,
    /// `\r\n` included, is one stop.
    fn grapheme_before(&self, offset: usize) -> usize {
        let (line, col) = self.offset_to_line_col(offset);
        if col == 0 {
            return match line.checked_sub(1) {
                Some(prev) => self.line_col_to_offset(prev, usize::MAX),
                None => 0,
            };
        }
        self.rope.line_to_char(line) + grapheme_boundary_before(&self.line_text(line), col)
    }

    /// The caret stop after `offset`: the end of the grapheme cluster after
    /// it, or the start of the next line.
    fn grapheme_after(&self, offset: usize) -> usize {
        let (line, col) = self.offset_to_line_col(offset);
        let text = self.line_text(line);
        if col >= text.chars().count() {
            return if line + 1 < self.line_count() {
                self.rope.line_to_char(line + 1)
            } else {
                self.rope.len_chars()
            };
        }
        self.rope.line_to_char(line) + grapheme_boundary_after(&text, col)
    }

    /// `(line, col)` as an offset, moved back to the start of the grapheme
    /// cluster it falls inside.
    fn caret_stop(&self, line: usize, col: usize) -> usize {
        let offset = self.line_col_to_offset(line, col);
        let (line, col) = self.offset_to_line_col(offset);
        self.rope.line_to_char(line) + grapheme_boundary_at_or_before(&self.line_text(line), col)
    }

    fn delete_range(&mut self, start: usize, end: usize) -> CommandResult {
        let removed = self.rope.slice(start..end).to_string();
        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        self.rope.remove(start..end);
        self.cursor_offset = start;
        self.selection_offsets = None;
        self.commit_transaction(
            vec![EditOp::Delete {
                char_offset: start,
                text: removed,
            }],
            before_cursor,
            before_selection,
        );
        CommandResult::changed()
    }

    fn move_cursor(&mut self, movement: Movement, extend: bool) {
        let anchor = if extend {
            self.selection_offsets
                .map(|selection| selection.anchor)
                .unwrap_or(self.cursor_offset)
        } else {
            self.selection_offsets = None;
            self.cursor_offset
        };

        self.cursor_offset = match movement {
            Movement::Left => self.grapheme_before(self.cursor_offset),
            Movement::Right => self.grapheme_after(self.cursor_offset),
            Movement::Home => {
                self.desired_col = None;
                let line = self
                    .rope
                    .char_to_line(self.cursor_offset.min(self.rope.len_chars()));
                self.rope.line_to_char(line)
            }
            Movement::End => {
                self.desired_col = None;
                let line = self
                    .rope
                    .char_to_line(self.cursor_offset.min(self.rope.len_chars()));
                self.line_col_to_offset(line, self.line_text(line).chars().count())
            }
            Movement::Up => self.vertical_offset(-1),
            Movement::Down => self.vertical_offset(1),
        };

        if extend {
            self.selection_offsets = Selection::new(anchor, self.cursor_offset);
        } else {
            self.selection_offsets = None;
        }

        if !matches!(movement, Movement::Up | Movement::Down) {
            self.desired_col = None;
        }
        self.sync_public_state();
    }

    fn vertical_offset(&mut self, delta: isize) -> usize {
        let (line, col) = self.offset_to_line_col(self.cursor_offset);
        let desired_col = *self.desired_col.get_or_insert(col);
        let max_line = self.line_count().saturating_sub(1);
        let target_line = if delta < 0 {
            line.saturating_sub(delta.unsigned_abs())
        } else {
            (line + delta as usize).min(max_line)
        };
        self.caret_stop(target_line, desired_col)
    }

    fn toggle_checkbox(&mut self, line: usize) -> CommandResult {
        let line_text = self.line_text(line);
        let indent_chars = line_text
            .chars()
            .take_while(|ch| ch.is_whitespace())
            .count();
        let marker_start = self.line_col_to_offset(line, indent_chars);
        let marker = line_text
            .chars()
            .skip(indent_chars)
            .take(5)
            .collect::<String>();
        let replacement = match marker.as_str() {
            "- [ ]" => "- [x]",
            "- [x]" | "- [X]" => "- [ ]",
            _ => return CommandResult::default(),
        };

        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        self.rope.remove(marker_start..marker_start + 5);
        self.rope.insert(marker_start, replacement);
        self.commit_transaction(
            vec![
                EditOp::Delete {
                    char_offset: marker_start,
                    text: marker,
                },
                EditOp::Insert {
                    char_offset: marker_start,
                    text: replacement.to_string(),
                },
            ],
            before_cursor,
            before_selection,
        );
        CommandResult::changed()
    }

    fn wrap_selection_or_insert(
        &mut self,
        prefix: &str,
        suffix: &str,
        placeholder: &str,
    ) -> CommandResult {
        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        let mut ops = Vec::new();

        if let Some(selection) = self.selection_offsets {
            let (start, end) = selection.range();

            let has_inner_formatting =
                (end - start >= prefix.chars().count() + suffix.chars().count()) && {
                    let text = self.rope.slice(start..end).to_string();
                    text.starts_with(prefix) && text.ends_with(suffix)
                };

            let has_outer_formatting = start >= prefix.chars().count()
                && end + suffix.chars().count() <= self.rope.len_chars()
                && {
                    let before = self
                        .rope
                        .slice(start - prefix.chars().count()..start)
                        .to_string();
                    let after = self
                        .rope
                        .slice(end..end + suffix.chars().count())
                        .to_string();
                    before == prefix && after == suffix
                };

            if has_inner_formatting {
                // Delete suffix first (at end - suffix.chars().count()..end)
                let suffix_start = end - suffix.chars().count();
                let suffix_text = self.rope.slice(suffix_start..end).to_string();
                self.rope.remove(suffix_start..end);
                ops.push(EditOp::Delete {
                    char_offset: suffix_start,
                    text: suffix_text,
                });

                // Delete prefix (at start..start + prefix.chars().count())
                let prefix_end = start + prefix.chars().count();
                let prefix_text = self.rope.slice(start..prefix_end).to_string();
                self.rope.remove(start..prefix_end);
                ops.push(EditOp::Delete {
                    char_offset: start,
                    text: prefix_text,
                });

                self.cursor_offset = suffix_start - prefix.chars().count();
                self.selection_offsets =
                    Selection::new(start, suffix_start - prefix.chars().count());
            } else if has_outer_formatting {
                // Delete suffix (at end..end + suffix.chars().count())
                let suffix_end = end + suffix.chars().count();
                let suffix_text = self.rope.slice(end..suffix_end).to_string();
                self.rope.remove(end..suffix_end);
                ops.push(EditOp::Delete {
                    char_offset: end,
                    text: suffix_text,
                });

                // Delete prefix (at start - prefix.chars().count()..start)
                let prefix_start = start - prefix.chars().count();
                let prefix_text = self.rope.slice(prefix_start..start).to_string();
                self.rope.remove(prefix_start..start);
                ops.push(EditOp::Delete {
                    char_offset: prefix_start,
                    text: prefix_text,
                });

                self.cursor_offset = end - prefix.chars().count();
                self.selection_offsets =
                    Selection::new(start - prefix.chars().count(), end - prefix.chars().count());
            } else {
                self.rope.insert(end, suffix);
                ops.push(EditOp::Insert {
                    char_offset: end,
                    text: suffix.to_string(),
                });
                self.rope.insert(start, prefix);
                ops.push(EditOp::Insert {
                    char_offset: start,
                    text: prefix.to_string(),
                });
                self.cursor_offset = end + prefix.chars().count() + suffix.chars().count();
                self.selection_offsets =
                    Selection::new(start + prefix.chars().count(), end + prefix.chars().count());
            }
        } else {
            let text = format!("{prefix}{placeholder}{suffix}");
            self.rope.insert(self.cursor_offset, &text);
            ops.push(EditOp::Insert {
                char_offset: self.cursor_offset,
                text,
            });
            let start = self.cursor_offset + prefix.chars().count();
            let end = start + placeholder.chars().count();
            self.cursor_offset = end;
            self.selection_offsets = Selection::new(start, end);
        }

        self.commit_transaction(ops, before_cursor, before_selection);
        CommandResult::changed()
    }

    fn char_at(&self, offset: usize) -> Option<char> {
        if offset < self.rope.len_chars() {
            Some(self.rope.char(offset))
        } else {
            None
        }
    }

    /// Handle a typed character with auto-pairing for brackets and quotes:
    /// insert a matching closer (cursor between), wrap a selection, or skip over
    /// an existing closer. Non-paired characters insert normally.
    fn type_paired(&mut self, c: char) -> CommandResult {
        const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];

        // Opening bracket: wrap a selection, else insert an empty pair.
        if let Some(&(open, close)) = PAIRS.iter().find(|&&(o, _)| o == c) {
            if self.selection_offsets.is_some() {
                return self.wrap_selection_or_insert(&open.to_string(), &close.to_string(), "");
            }
            return self.insert_pair_empty(open, close);
        }

        // Closing bracket: skip over a closer already at the cursor.
        if let Some(&(_, close)) = PAIRS.iter().find(|&&(_, cl)| cl == c) {
            if self.selection_offsets.is_none() && self.char_at(self.cursor_offset) == Some(close) {
                self.move_cursor(Movement::Right, false);
                return CommandResult::default();
            }
            return self.insert_text(&c.to_string());
        }

        // Quote characters use the same glyph for open and close.
        if matches!(c, '"' | '\'' | '`') {
            if self.selection_offsets.is_some() {
                return self.wrap_selection_or_insert(&c.to_string(), &c.to_string(), "");
            }
            // Skip over an identical quote immediately ahead.
            if self.char_at(self.cursor_offset) == Some(c) {
                self.move_cursor(Movement::Right, false);
                return CommandResult::default();
            }
            // Don't auto-pair a quote that follows a word character, so
            // apostrophes in contractions ("don't") stay single.
            let prev = self
                .cursor_offset
                .checked_sub(1)
                .and_then(|i| self.char_at(i));
            if prev.is_some_and(|p| p.is_alphanumeric()) {
                return self.insert_text(&c.to_string());
            }
            return self.insert_pair_empty(c, c);
        }

        self.insert_text(&c.to_string())
    }

    /// Insert `open`+`close` at the cursor and leave the cursor between them.
    fn insert_pair_empty(&mut self, open: char, close: char) -> CommandResult {
        let before_cursor = self.cursor_offset;
        let before_selection = self.selection_offsets;
        let insert_at = self.cursor_offset;
        let text = format!("{open}{close}");
        self.rope.insert(insert_at, &text);
        let ops = vec![EditOp::Insert {
            char_offset: insert_at,
            text,
        }];
        self.cursor_offset = insert_at + 1;
        self.selection_offsets = None;
        self.commit_transaction(ops, before_cursor, before_selection);
        CommandResult::changed()
    }

    fn commit_transaction(
        &mut self,
        ops: Vec<EditOp>,
        before_cursor: usize,
        before_selection: Option<Selection>,
    ) {
        let before_revision = self.revision;
        let after_revision = self.next_revision;
        self.next_revision = self.next_revision.wrapping_add(1);
        self.revision = after_revision;
        self.update_dirty();
        self.desired_col = None;

        let now = Instant::now();
        let recent = self
            .last_commit_at
            .is_some_and(|at| now.duration_since(at) <= UNDO_COALESCE_WINDOW);

        // Fold a continuing run of typing (or of backspaces) into the previous
        // transaction, so one Ctrl+Z takes back a word-sized burst instead of a
        // single character. Anything that is not a direct continuation — a
        // pause, a caret jump, a newline, a selection edit — starts a new step.
        if recent
            && before_selection.is_none()
            && let Some(previous) = self.undo_stack.last_mut()
            && previous.after_selection.is_none()
            && previous.after_cursor == before_cursor
            && can_coalesce(&previous.ops, &ops)
        {
            previous.ops.extend(ops);
            previous.after_cursor = self.cursor_offset;
            previous.after_selection = self.selection_offsets;
            self.last_commit_at = Some(now);
            self.redo_stack.clear();
            self.sync_public_state();
            return;
        }

        self.undo_stack.push(EditTransaction {
            ops,
            before_cursor,
            after_cursor: self.cursor_offset,
            before_selection,
            after_selection: self.selection_offsets,
            before_revision,
            after_revision,
        });
        self.last_commit_at = Some(now);
        self.redo_stack.clear();
        self.sync_public_state();
    }

    fn update_dirty(&mut self) {
        self.dirty = self.revision != self.saved_revision;
    }

    fn apply_op(&mut self, op: &EditOp) {
        match op {
            EditOp::Insert { char_offset, text } => {
                let offset = (*char_offset).min(self.rope.len_chars());
                self.rope.insert(offset, text);
            }
            EditOp::Delete { char_offset, text } => {
                let start = (*char_offset).min(self.rope.len_chars());
                let end = (start + text.chars().count()).min(self.rope.len_chars());
                self.rope.remove(start..end);
            }
        }
    }

    fn line_col_to_offset(&self, line: usize, col: usize) -> usize {
        let max_line = self.line_count().saturating_sub(1);
        let line = line.min(max_line);
        let line_start = self.rope.line_to_char(line);
        let line_len = self.line_text(line).chars().count();
        line_start + col.min(line_len)
    }

    fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        let clamped = offset.min(self.rope.len_chars());
        let line = self.rope.char_to_line(clamped);
        let line_start = self.rope.line_to_char(line);
        let col = clamped.saturating_sub(line_start);
        (line, col.min(self.line_text(line).chars().count()))
    }

    fn sync_public_state(&mut self) {
        self.cursor_affinity = Affinity::Downstream;
        self.cursor_offset = self.cursor_offset.min(self.rope.len_chars());
        let (line, col) = self.offset_to_line_col(self.cursor_offset);
        self.cursor_line = line;
        self.cursor_col = col;
        self.selection = self.selection_offsets.map(|selection| {
            let (start, end) = selection.range();
            let (start_line, start_col) = self.offset_to_line_col(start);
            let (end_line, end_col) = self.offset_to_line_col(end);
            (start_line, start_col, end_line, end_col)
        });
    }
}

/// Char offsets of the grapheme cluster boundaries in `text`, from 0 to its
/// length inclusive.
fn grapheme_boundaries(text: &str) -> impl Iterator<Item = usize> + '_ {
    use unicode_segmentation::UnicodeSegmentation;
    std::iter::once(0).chain(text.graphemes(true).scan(0, |chars, cluster| {
        *chars += cluster.chars().count();
        Some(*chars)
    }))
}

/// The last cluster boundary strictly before `col` (0 when there is none).
fn grapheme_boundary_before(text: &str, col: usize) -> usize {
    grapheme_boundaries(text)
        .take_while(|&boundary| boundary < col)
        .last()
        .unwrap_or(0)
}

/// The first cluster boundary strictly after `col`, or the text's length.
fn grapheme_boundary_after(text: &str, col: usize) -> usize {
    grapheme_boundaries(text)
        .find(|&boundary| boundary > col)
        .unwrap_or_else(|| text.chars().count())
}

/// `col` itself when it is a cluster boundary, else the boundary before it.
fn grapheme_boundary_at_or_before(text: &str, col: usize) -> usize {
    grapheme_boundaries(text)
        .take_while(|&boundary| boundary <= col)
        .last()
        .unwrap_or(0)
}

struct ListItem {
    indent: String,
    marker: String,
    next_marker: String,
    is_empty: bool,
}

fn dominant_eol(text: &str) -> &'static str {
    let crlf = text.match_indices("\r\n").count();
    let lf = text.bytes().filter(|byte| *byte == b'\n').count();
    if crlf > lf.saturating_sub(crlf) {
        "\r\n"
    } else {
        "\n"
    }
}

fn parse_list_item(line_text: &str) -> Option<ListItem> {
    // Find leading whitespace (indentation)
    let mut indent_len = 0;
    for c in line_text.chars() {
        if c.is_whitespace() && c != '\n' && c != '\r' {
            indent_len += c.len_utf8();
        } else {
            break;
        }
    }
    let indent = line_text[..indent_len].to_string();
    let rest = line_text[indent_len..].trim_end_matches(&['\r', '\n'][..]);

    // Check Checklist: e.g. "- [ ] ", "* [ ] ", "- [x] ", etc.
    // Index by char, not byte: the char after the marker may be multibyte.
    if rest.starts_with("- [") || rest.starts_with("* [") || rest.starts_with("+ [") {
        let head: Vec<char> = rest.chars().take(6).collect();
        if head.len() >= 5 && head[4] == ']' && matches!(head[3], ' ' | 'x' | 'X') {
            let has_space_after = head.len() >= 6 && head[5] == ' ';
            // The marker "- [x] " is all ASCII, so char count == byte count here.
            let marker_len = if has_space_after { 6 } else { 5 };
            let marker = rest[..marker_len].to_string();
            let content = &rest[marker_len..];
            let is_empty = content.trim().is_empty();
            let bullet = &rest[..1];
            let next_marker = format!("{} [ ] ", bullet);
            return Some(ListItem {
                indent,
                marker,
                next_marker,
                is_empty,
            });
        }
    }

    // Check Unordered List: e.g. "- ", "* ", "+ " (or just "-", "*", "+" at the end of line)
    if rest == "-" || rest == "*" || rest == "+" {
        return Some(ListItem {
            indent,
            marker: rest.to_string(),
            next_marker: format!("{} ", rest),
            is_empty: true,
        });
    }
    if rest.starts_with("- ") || rest.starts_with("* ") || rest.starts_with("+ ") {
        let marker = rest[..2].to_string();
        let content = &rest[2..];
        let is_empty = content.trim().is_empty();
        return Some(ListItem {
            indent,
            marker: marker.clone(),
            next_marker: marker,
            is_empty,
        });
    }

    // Check ordered list: e.g. "1. ", "123. " or just "1.", "123." at the end of line
    let mut dot_idx = None;
    for (idx, c) in rest.char_indices() {
        if c.is_ascii_digit() {
            continue;
        } else if c == '.' {
            dot_idx = Some(idx);
            break;
        } else {
            break;
        }
    }
    if let Some(dot) = dot_idx
        && dot > 0
    {
        // `dot` only follows ASCII digits, so dot + 1 is a char boundary; the
        // char after it may be multibyte, so inspect it via chars() not slicing.
        let after_dot = rest[dot + 1..].chars().next();
        let is_at_end = after_dot.is_none();
        let has_space_after = after_dot == Some(' ');
        if is_at_end || has_space_after {
            let marker_len = if has_space_after { dot + 2 } else { dot + 1 };
            let marker = rest[..marker_len].to_string();
            let content = &rest[marker_len..];
            let is_empty = content.trim().is_empty();
            if let Ok(num) = rest[..dot].parse::<usize>() {
                let next_marker = format!("{}. ", num + 1);
                return Some(ListItem {
                    indent,
                    marker,
                    next_marker,
                    is_empty,
                });
            }
        }
    }

    None
}

#[cfg(test)]
mod tests;

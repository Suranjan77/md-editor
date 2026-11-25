//! `DocBuffer`: the document rope, caret and selection, command router, and dirty tracking.

use std::time::Instant;

use ropey::Rope;
use serde::{Deserialize, Serialize};

mod edit;
mod format;
mod lists;
mod movement;
mod undo;
mod words;

#[cfg(test)]
mod tests;

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
    pub(super) fn inverse(&self) -> Self {
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
    pub(super) ops: Vec<EditOp>,
    pub(super) before_cursor: usize,
    pub(super) after_cursor: usize,
    pub(super) before_selection: Option<Selection>,
    pub(super) after_selection: Option<Selection>,
    pub(super) before_revision: u64,
    pub(super) after_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Movement {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    /// Ctrl+Left / Ctrl+Right.
    WordLeft,
    WordRight,
    /// Ctrl+Home / Ctrl+End.
    DocStart,
    DocEnd,
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
    /// Insert text as a block on its own line (a pasted image): line breaks
    /// are added before and after as needed. One undo step.
    InsertBlock(String),
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
    InsertTable,
    InsertMath,
    InsertCheckbox,
    /// A single typed character that may participate in auto-pairing
    /// (brackets/quotes). Falls back to plain insertion when not paired.
    TypePaired(char),
    Undo,
    Redo,
    /// Ctrl+Backspace / Ctrl+Delete: delete to the word boundary (or the selection).
    DeleteWordBackward,
    DeleteWordForward,
    /// Tab over a multi-line selection / Shift+Tab: one indent level per line.
    IndentLines,
    OutdentLines,
    /// Double-click: select the word at this position.
    SelectWord {
        line: usize,
        col: usize,
    },
    /// Triple-click: select this whole line.
    SelectLine {
        line: usize,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommandResult {
    pub text_changed: bool,
    pub projection_changed: bool,
    pub media_changed: bool,
}

impl CommandResult {
    pub(super) fn changed() -> Self {
        Self {
            text_changed: true,
            projection_changed: true,
            media_changed: true,
        }
    }
}

pub struct DocBuffer {
    pub(super) rope: Rope,
    pub(super) cursor_offset: usize,
    pub(super) selection_offsets: Option<Selection>,
    pub(super) desired_col: Option<usize>,

    pub cursor_line: usize,
    pub cursor_col: usize,
    /// Which row the caret is drawn on when its column is a row break. Every
    /// change resets it; only placing the caret by position sets it, so it
    /// can never outlive the placement it describes.
    pub cursor_affinity: Affinity,
    pub selection: Option<(usize, usize, usize, usize)>,
    pub dirty: bool,

    pub(super) undo_stack: Vec<EditTransaction>,
    pub(super) redo_stack: Vec<EditTransaction>,
    /// When the newest undo transaction was recorded, for coalescing runs of
    /// typing into a single step.
    pub(super) last_commit_at: Option<Instant>,
    pub(super) revision: u64,
    pub(super) saved_revision: u64,
    pub(super) next_revision: u64,
    pub(super) eol: &'static str,
    /// `(revision, words)` from the last [`Self::word_count`]: the status bar
    /// asks every frame, and a revision names one text.
    word_count_cache: std::cell::Cell<Option<(u64, usize)>>,
}

impl Default for DocBuffer {
    fn default() -> Self {
        Self::new()
    }
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
            word_count_cache: std::cell::Cell::new(None),
        };
        buffer.sync_public_state();
        buffer
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Whitespace-separated words in the buffer, for the status bar. Counted
    /// across the rope's chunks (a word split between two chunks is one
    /// word), and cached per revision.
    pub fn word_count(&self) -> usize {
        if let Some((revision, words)) = self.word_count_cache.get()
            && revision == self.revision
        {
            return words;
        }
        let mut words = 0;
        let mut in_word = false;
        for c in self.rope.chunks().flat_map(str::chars) {
            let space = c.is_whitespace();
            if !space && !in_word {
                words += 1;
            }
            in_word = !space;
        }
        self.word_count_cache.set(Some((self.revision, words)));
        words
    }

    /// Replace the whole document, discarding undo history.
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
            EditorCommand::InsertBlock(text) => self.insert_block(&text),
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
            EditorCommand::InsertTable => {
                self.insert_block("| Column 1 | Column 2 |\n| :--- | :--- |\n| Cell | Cell |")
            }
            EditorCommand::InsertMath => self.insert_block("$$\nE = mc^2\n$$"),
            EditorCommand::InsertCheckbox => self.insert_checkbox_at_cursor(),
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
            EditorCommand::DeleteWordBackward => self.delete_word(false),
            EditorCommand::DeleteWordForward => self.delete_word(true),
            EditorCommand::IndentLines => self.indent_lines(),
            EditorCommand::OutdentLines => self.outdent_lines(),
            EditorCommand::SelectWord { line, col } => {
                self.select_word_at(line, col);
                CommandResult::default()
            }
            EditorCommand::SelectLine { line } => {
                self.select_line(line);
                CommandResult::default()
            }
        }
    }

    pub(super) fn char_at(&self, offset: usize) -> Option<char> {
        if offset < self.rope.len_chars() {
            Some(self.rope.char(offset))
        } else {
            None
        }
    }

    pub(super) fn update_dirty(&mut self) {
        self.dirty = self.revision != self.saved_revision;
    }

    pub(super) fn line_col_to_offset(&self, line: usize, col: usize) -> usize {
        let max_line = self.line_count().saturating_sub(1);
        let line = line.min(max_line);
        let line_start = self.rope.line_to_char(line);
        let line_len = self.line_text(line).chars().count();
        line_start + col.min(line_len)
    }

    pub(super) fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        let clamped = offset.min(self.rope.len_chars());
        let line = self.rope.char_to_line(clamped);
        let line_start = self.rope.line_to_char(line);
        let col = clamped.saturating_sub(line_start);
        (line, col.min(self.line_text(line).chars().count()))
    }

    pub(super) fn sync_public_state(&mut self) {
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

pub(super) fn dominant_eol(text: &str) -> &'static str {
    let crlf = text.match_indices("\r\n").count();
    let lf = text.bytes().filter(|byte| *byte == b'\n').count();
    if crlf > lf.saturating_sub(crlf) {
        "\r\n"
    } else {
        "\n"
    }
}

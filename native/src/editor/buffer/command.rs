#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Movement {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
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
    },
    SetSelection {
        anchor_line: usize,
        anchor_col: usize,
        focus_line: usize,
        focus_col: usize,
    },
    SelectAll,
    ToggleCheckbox {
        line: usize,
    },
    FormatBold,
    FormatItalic,
    FormatInlineCode,
    InsertLink,
    ToggleHeading,
    ToggleBlockquote,
    ToggleUnorderedList,
    ToggleOrderedList,
    InsertCodeBlock,
    InsertMathBlock,
    InsertTable,
    InsertPdfQuoteLink {
        selected_text: String,
        page_number: u16,
        link: String,
    },
    InsertPdfAnnotationLink {
        selected_text: String,
        page_number: u16,
        link: String,
    },
    DuplicateLine,
    MoveLineUp,
    MoveLineDown,
    ReplaceAll {
        query: String,
        replacement: String,
        regex: bool,
        match_case: bool,
    },
    ReplaceTextRange {
        line: usize,
        start_col: usize,
        end_col: usize,
        replacement: String,
    },
    Undo,
    Redo,
    ConvertToH1 {
        line: usize,
    },
    ConvertToH2 {
        line: usize,
    },
    ConvertToH3 {
        line: usize,
    },
    ConvertToParagraph {
        line: usize,
    },
    RemoveCheckbox {
        line: usize,
    },
    InsertRowAbove {
        line: usize,
    },
    InsertRowBelow {
        line: usize,
    },
    DeleteRow {
        line: usize,
    },
    InsertColumnLeft {
        line: usize,
    },
    InsertColumnRight {
        line: usize,
    },
    DeleteColumn {
        line: usize,
    },
    SetCodeLanguage {
        line: usize,
        language: String,
    },
    ConvertQuoteToParagraph {
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
    pub(crate) fn changed() -> Self {
        Self {
            text_changed: true,
            projection_changed: true,
            media_changed: true,
        }
    }
}

impl EditorCommand {
    pub fn changes_text(&self) -> bool {
        !matches!(
            self,
            EditorCommand::MoveCursor { .. }
                | EditorCommand::SetCursor { .. }
                | EditorCommand::SetSelection { .. }
                | EditorCommand::SelectAll
        )
    }

    pub fn changes_projection(&self) -> bool {
        self.changes_text()
    }

    pub fn may_change_media(&self) -> bool {
        self.changes_text()
    }

    pub fn should_keep_cursor_visible(&self) -> bool {
        matches!(
            self,
            EditorCommand::InsertText(_)
                | EditorCommand::DeleteSelection
                | EditorCommand::DeleteBackward
                | EditorCommand::DeleteForward
                | EditorCommand::MoveCursor { .. }
                | EditorCommand::SetCursor { .. }
                | EditorCommand::SetSelection { .. }
                | EditorCommand::SelectAll
                | EditorCommand::ToggleCheckbox { .. }
                | EditorCommand::FormatBold
                | EditorCommand::FormatItalic
                | EditorCommand::FormatInlineCode
                | EditorCommand::InsertLink
                | EditorCommand::ToggleHeading
                | EditorCommand::ToggleBlockquote
                | EditorCommand::ToggleUnorderedList
                | EditorCommand::ToggleOrderedList
                | EditorCommand::InsertCodeBlock
                | EditorCommand::InsertMathBlock
                | EditorCommand::InsertTable
                | EditorCommand::InsertPdfQuoteLink { .. }
                | EditorCommand::InsertPdfAnnotationLink { .. }
                | EditorCommand::DuplicateLine
                | EditorCommand::MoveLineUp
                | EditorCommand::MoveLineDown
                | EditorCommand::ReplaceAll { .. }
                | EditorCommand::ReplaceTextRange { .. }
                | EditorCommand::Undo
                | EditorCommand::Redo
                | EditorCommand::ConvertToH1 { .. }
                | EditorCommand::ConvertToH2 { .. }
                | EditorCommand::ConvertToH3 { .. }
                | EditorCommand::ConvertToParagraph { .. }
                | EditorCommand::RemoveCheckbox { .. }
                | EditorCommand::InsertRowAbove { .. }
                | EditorCommand::InsertRowBelow { .. }
                | EditorCommand::DeleteRow { .. }
                | EditorCommand::InsertColumnLeft { .. }
                | EditorCommand::InsertColumnRight { .. }
                | EditorCommand::DeleteColumn { .. }
                | EditorCommand::SetCodeLanguage { .. }
                | EditorCommand::ConvertQuoteToParagraph { .. }
        )
    }
}

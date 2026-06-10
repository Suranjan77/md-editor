#![allow(dead_code)]

pub(crate) use crate::features::citations::CitationMessage;
pub(crate) use crate::features::editor::EditorMessage;
pub(crate) use crate::features::overlays::OverlayMessage;
pub(crate) use crate::features::pdf::message::PdfMessage;
pub(crate) use crate::features::search::SearchMessage;
pub(crate) use crate::features::shell::ShellMessage;
pub(crate) use crate::features::system::SystemMessage;
pub(crate) use crate::features::tracker::{TrackerMessage, TrackerTab};
pub(crate) use crate::features::workspace::WorkspaceMessage;

#[derive(Debug, Clone)]
pub(crate) enum Message {
    Shell(ShellMessage),
    Workspace(WorkspaceMessage),
    Editor(EditorMessage),
    Pdf(PdfMessage),
    Search(SearchMessage),
    Citation(CitationMessage),
    Tracker(TrackerMessage),
    Overlay(OverlayMessage),
    System(SystemMessage),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shortcut {
    Save,
    OpenVault,
    NewFile,
    Search,
    CommandPalette,
    ToggleSidebar,
    NavBack,
    NavForward,
    ToggleBacklinks,
    FocusMode,
    TableOfContents,
    StudyTracker,
    SplitView,
    Escape,
    ZoomIn,
    ZoomOut,
    ZoomFit,
    GoToPage,
    PdfSearch,
    PdfHighlight,
    PdfUnderline,
    PdfStrike,
    PdfOpenCompanionNote,
    InsertPdfQuote,
    InsertPdfHighlight,
    PdfFirstPage,
    PdfLastPage,
    PdfZoomInput,
    FollowCitation,
    ShowUsages,
    CitationPalette,
    ExcerptModeToggle,
    ExcerptInsertBatch,
    Submit,
    ThemeDark,
    ThemeLight,
    ThemeHighContrast,
    ToggleReducedMotion,
    HelpAndShortcuts,
    SwitchPane,
    ToggleDiagnostics,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CitationItem {
    Selection {
        text: String,
        page_index: u16,
    },
    Annotation {
        id: String,
        text: String,
        page_index: u16,
    },
    SearchHit {
        path: String,
        page_index: u16,
        snippet: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EditorBlockActionKind {
    ConvertToH1,
    ConvertToH2,
    ConvertToH3,
    ConvertToParagraph,
    ToggleCheckbox,
    RemoveCheckbox,
    InsertRowAbove,
    InsertRowBelow,
    DeleteRow,
    InsertColumnLeft,
    InsertColumnRight,
    DeleteColumn,
    CopyCode,
    SetCodeLanguage(String),
    ConvertQuoteToParagraph,
    OpenPdfCitation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchWrapStatus {
    WrappedForward,
    WrappedBackward,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EditorLinkActionKind {
    OpenLink,
    CopyLinkTarget,
    CreateNote,
    RepairLink(String),
}

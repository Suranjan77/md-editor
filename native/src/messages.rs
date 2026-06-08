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

#[allow(non_snake_case, non_upper_case_globals)]
impl Message {
    pub(crate) const PdfFitToWidth: Self = Self::Pdf(PdfMessage::FitToWidth);
    pub(crate) const PdfFitToPage: Self = Self::Pdf(PdfMessage::FitToPage);
    pub(crate) const PdfRotateClockwise: Self = Self::Pdf(PdfMessage::RotateClockwise);
    pub(crate) const PdfFirstPage: Self = Self::Pdf(PdfMessage::FirstPage);
    pub(crate) const PdfLastPage: Self = Self::Pdf(PdfMessage::LastPage);
    pub(crate) const PdfSearchToggle: Self = Self::Pdf(PdfMessage::SearchToggle);
    pub(crate) const PdfGoToPage: Self = Self::Pdf(PdfMessage::GoToPage);
    pub(crate) const PdfNavBack: Self = Self::Pdf(PdfMessage::NavBack);
    pub(crate) const PdfNavForward: Self = Self::Pdf(PdfMessage::NavForward);
    pub(crate) const ClosePdfLinkPreview: Self = Self::Pdf(PdfMessage::CloseLinkPreview);
    pub(crate) const PdfSelectionCleared: Self = Self::Pdf(PdfMessage::SelectionCleared);
    pub(crate) const PdfCopySelection: Self = Self::Pdf(PdfMessage::CopySelection);
    pub(crate) const PdfInsertQuoteLink: Self = Self::Pdf(PdfMessage::InsertQuoteLink);
    pub(crate) const PdfToggleAnnotationsSidebar: Self =
        Self::Pdf(PdfMessage::ToggleAnnotationsSidebar);
    pub(crate) const PdfExportAnnotations: Self = Self::Pdf(PdfMessage::ExportAnnotations);

    pub(crate) fn PdfZoomChanged(value: f32) -> Self {
        Self::Pdf(PdfMessage::ZoomChanged(value))
    }

    pub(crate) fn PdfWheelScrolledForZoom(value: f32) -> Self {
        Self::Pdf(PdfMessage::WheelScrolledForZoom(value))
    }

    pub(crate) fn PdfLoaded(generation: u64, pages: u16) -> Self {
        Self::Pdf(PdfMessage::Loaded(generation, pages))
    }

    pub(crate) fn PdfPageSizesLoaded(
        generation: u64,
        path: String,
        sizes: Vec<(f32, f32)>,
    ) -> Self {
        Self::Pdf(PdfMessage::PageSizesLoaded(generation, path, sizes))
    }

    pub(crate) fn PdfRendered(generation: u64, page: u16, image: image::DynamicImage) -> Self {
        Self::Pdf(PdfMessage::Rendered(generation, page, image))
    }

    pub(crate) fn PdfRenderFailed(generation: u64, page: u16) -> Self {
        Self::Pdf(PdfMessage::RenderFailed(generation, page))
    }

    pub(crate) fn PdfRenderSkipped(generation: u64, page: u16) -> Self {
        Self::Pdf(PdfMessage::RenderSkipped(generation, page))
    }

    pub(crate) fn PdfLeftClicked(
        page: u16,
        x: f32,
        y: f32,
        modifiers: iced::keyboard::Modifiers,
    ) -> Self {
        Self::Pdf(PdfMessage::LeftClicked(page, x, y, modifiers))
    }

    pub(crate) fn PdfTocLoaded(
        generation: u64,
        entries: Vec<md_editor_core::application::pdf_service::TocEntry>,
    ) -> Self {
        Self::Pdf(PdfMessage::TocLoaded(generation, entries))
    }

    pub(crate) fn PdfPageLinksLoaded(
        generation: u64,
        page: u16,
        links: Vec<md_editor_core::domain::pdf::LinkInfo>,
    ) -> Self {
        Self::Pdf(PdfMessage::PageLinksLoaded(generation, page, links))
    }

    pub(crate) fn PdfSearchMatchesFound(
        generation: u64,
        matches: Vec<md_editor_core::application::pdf_service::PdfSearchMatch>,
    ) -> Self {
        Self::Pdf(PdfMessage::SearchMatchesFound(generation, matches))
    }

    pub(crate) fn PdfSearchFinished(generation: u64, result: Result<(), String>) -> Self {
        Self::Pdf(PdfMessage::SearchFinished(generation, result))
    }

    pub(crate) fn PdfSearchResultClicked(page: u16) -> Self {
        Self::Pdf(PdfMessage::SearchResultClicked(page))
    }

    pub(crate) fn PdfScrollBy(delta: f32) -> Self {
        Self::Pdf(PdfMessage::ScrollBy(delta))
    }

    pub(crate) fn PdfLinkPreviewResult(
        result: Result<md_editor_core::application::pdf_service::LinkPreviewResult, String>,
    ) -> Self {
        Self::Pdf(PdfMessage::LinkPreviewResult(result))
    }

    pub(crate) fn PdfDocumentIdComputed(
        document: Option<(String, String, u64, Option<i64>)>,
    ) -> Self {
        Self::Pdf(PdfMessage::DocumentIdComputed(document))
    }

    pub(crate) fn PdfPageTextLoaded(
        generation: u64,
        page: u16,
        result: Result<md_editor_core::domain::pdf::PdfPageText, String>,
    ) -> Self {
        Self::Pdf(PdfMessage::PageTextLoaded(generation, page, result))
    }

    pub(crate) fn PdfSelectionChanged(page: u16, anchor: usize, focus: usize) -> Self {
        Self::Pdf(PdfMessage::SelectionChanged(page, anchor, focus))
    }

    pub(crate) fn PdfSelectionFinished(page: u16, anchor: usize, focus: usize) -> Self {
        Self::Pdf(PdfMessage::SelectionFinished(page, anchor, focus))
    }

    pub(crate) fn PdfInsertAnnotationLink(id: String) -> Self {
        Self::Pdf(PdfMessage::InsertAnnotationLink(id))
    }

    pub(crate) fn PdfCreateHighlight(
        color: md_editor_core::domain::pdf::PdfAnnotationColor,
    ) -> Self {
        Self::Pdf(PdfMessage::CreateHighlight(color))
    }

    pub(crate) fn PdfCreateAnnotation(
        kind: md_editor_core::domain::pdf::PdfAnnotationKind,
        color: md_editor_core::domain::pdf::PdfAnnotationColor,
    ) -> Self {
        Self::Pdf(PdfMessage::CreateAnnotation(kind, color))
    }

    pub(crate) fn PdfDeleteHighlight(id: String) -> Self {
        Self::Pdf(PdfMessage::DeleteHighlight(id))
    }

    pub(crate) fn PdfAddQuickNote(id: String, note: String) -> Self {
        Self::Pdf(PdfMessage::AddQuickNote(id, note))
    }

    pub(crate) fn PdfLinkNote(id: String, path: String) -> Self {
        Self::Pdf(PdfMessage::LinkNote(id, path))
    }

    pub(crate) fn PdfOpenLinkedNote(path: String) -> Self {
        Self::Pdf(PdfMessage::OpenLinkedNote(path))
    }

    pub(crate) fn PdfOpenCompanionNote(path: String) -> Self {
        Self::Pdf(PdfMessage::OpenCompanionNote(path))
    }

    pub(crate) fn PdfFilterAnnotationsByColor(
        color: Option<md_editor_core::domain::pdf::PdfAnnotationColor>,
    ) -> Self {
        Self::Pdf(PdfMessage::FilterAnnotationsByColor(color))
    }

    pub(crate) fn PdfFilterAnnotationsByPage(page: Option<u16>) -> Self {
        Self::Pdf(PdfMessage::FilterAnnotationsByPage(page))
    }

    pub(crate) fn PdfFilterAnnotationsByTag(tag: Option<String>) -> Self {
        Self::Pdf(PdfMessage::FilterAnnotationsByTag(tag))
    }

    pub(crate) fn PdfFilterAnnotationsByLinked(value: Option<bool>) -> Self {
        Self::Pdf(PdfMessage::FilterAnnotationsByLinked(value))
    }

    pub(crate) fn PdfFilterAnnotationsByUnresolved(value: Option<bool>) -> Self {
        Self::Pdf(PdfMessage::FilterAnnotationsByUnresolved(value))
    }

    pub(crate) fn PdfEditAnnotationNote(id: String, page: u16) -> Self {
        Self::Pdf(PdfMessage::EditAnnotationNote(id, page))
    }

    pub(crate) fn PdfToggleAnnotationStatus(id: String) -> Self {
        Self::Pdf(PdfMessage::ToggleAnnotationStatus(id))
    }

    pub(crate) fn PdfEditAnnotationTags(id: String) -> Self {
        Self::Pdf(PdfMessage::EditAnnotationTags(id))
    }

    pub(crate) fn PdfUpdateAnnotationTags(id: String, tags: String) -> Self {
        Self::Pdf(PdfMessage::UpdateAnnotationTags(id, tags))
    }

    pub(crate) fn PdfAnnotationsExported(result: Result<String, String>) -> Self {
        Self::Pdf(PdfMessage::AnnotationsExported(result))
    }

    pub(crate) fn PdfContextMenuAction(action: crate::views::modals::PdfContextMenuItem) -> Self {
        Self::Pdf(PdfMessage::ContextMenuAction(action))
    }

    pub(crate) fn PdfTocClicked(index: usize) -> Self {
        Self::Pdf(PdfMessage::TocClicked(index))
    }

    pub(crate) fn PdfLinkNoteFolderSelected(folder: String) -> Self {
        Self::Pdf(PdfMessage::LinkNoteFolderSelected(folder))
    }

    pub(crate) fn PdfLinkNoteFileSelected(path: String) -> Self {
        Self::Pdf(PdfMessage::LinkNoteFileSelected(path))
    }

    pub(crate) fn PdfLinkNotePickerSearchChanged(query: String) -> Self {
        Self::Pdf(PdfMessage::LinkNotePickerSearchChanged(query))
    }
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

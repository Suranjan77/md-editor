#[derive(Debug, Clone)]
pub enum Message {
    // ── Vault ────────────────────────────────────────────────────
    OpenVaultDialog,
    VaultOpened(Option<String>),
    VaultIndexed(Vec<md_editor_core::types::FileEntry>),
    CreateFileDialog,
    CreateFolderDialog,

    // ── Sidebar ──────────────────────────────────────────────────
    SidebarToggle,
    SidebarFileClicked(String),
    SidebarFolderToggled(String),

    // ── Navigation ───────────────────────────────────────────────
    GlobalSearchOpen,
    SearchClose,
    SearchQueryChanged(String),
    SearchReplaceChanged(String),
    SearchRegexToggled(bool),
    SearchMatchCaseToggled(bool),
    SearchPrevious,
    SearchNext,
    SearchReplaceAll,
    SearchResultClicked(String),
    CommandPaletteOpen,
    CommandPaletteQueryChanged(String),
    CommandPaletteCommandClicked(Shortcut),
    NameModalInputChanged(String),
    NameModalSubmit(String),
    NameModalSubmitCurrent,
    NameModalCancel,
    PdfLinkNoteFolderSelected(String),
    PdfLinkNoteFileSelected(String),
    PdfLinkNotePickerSearchChanged(String),
    DeleteFile(String),
    DeleteFileDialog(String),

    // ── Editor ───────────────────────────────────────────────────
    EditorCommand(crate::editor::buffer::EditorCommand),
    EditorCommandNoScroll(crate::editor::buffer::EditorCommand),
    EditorSave,
    EditorCheckboxToggle(usize),
    EditorCursorMove(usize, usize),
    EditorScrolled {
        y: f32,
        viewport_width: f32,
        viewport_height: f32,
    },
    ScrollEditorToTarget(f32),
    HighlightReady(u64, Vec<crate::editor::highlight::StyledLine>),
    HighlightDebounceElapsed,

    // ── PDF ──────────────────────────────────────────────────────
    PdfZoomChanged(f32),
    PdfFitToWidth,
    PdfLoaded(u64, u16), // render generation, total pages
    PdfPageSizesLoaded(u64, String, Vec<(f32, f32)>),
    PdfRendered(u64, u16, image::DynamicImage),
    PdfRenderFailed(u64, u16),
    PdfRenderSkipped(u64, u16),
    PdfScrolled {
        y: f32,
        viewport_height: f32,
    },
    PdfLeftClicked(u16, f32, f32, iced::keyboard::Modifiers),
    PdfRightClicked(u16, f32, f32),
    PdfTocLoaded(u64, Vec<md_editor_core::pdf::TocEntry>),
    PdfPageLinksLoaded(u64, u16, Vec<md_editor_core::pdf::LinkInfo>),
    PdfSearchResult(Result<Vec<md_editor_core::pdf::PdfSearchMatch>, String>),
    PdfSearchResultClicked(u16),
    PdfScrollBy(f32),
    PdfLinkPreviewResult(Result<md_editor_core::pdf::LinkPreviewResult, String>),
    ClosePdfLinkPreview,
    // ── PDF Study Updates ──────────────────────────────────────────
    PdfDocumentIdComputed(Option<(String, String, u64, Option<i64>)>),
    PdfPageTextLoaded(u64, u16, Result<md_editor_core::pdf::PdfPageText, String>),
    PdfSelectionChanged(u16, usize, usize),
    PdfSelectionCleared,
    PdfSelectionFinished(u16, usize, usize),
    PdfCopySelection,
    PdfCreateHighlight(md_editor_core::pdf::PdfAnnotationColor),
    PdfDeleteHighlight(String),
    PdfAddQuickNote(String, String),
    PdfLinkNote(String, String),
    PdfOpenLinkedNote(String),
    PdfAnnotationFocused {
        document_path: String,
        annotation_id: String,
        page: u16,
    },

    // ── Tracker ──────────────────────────────────────────────────
    TrackerToggle,
    TrackerStart,
    TrackerStop,
    TrackerTabSelected(TrackerTab),
    TrackerProjectStatusChanged(String, String),
    TrackerGateToggled(String, usize),
    TrackerReadingToggled(String, usize),
    TrackerConfigEdited(iced::widget::text_editor::Action),
    TrackerConfigSave,
    TrackerManualDateChanged(String),
    TrackerManualHoursChanged(String),
    TrackerManualNotesChanged(String),
    TrackerManualAdd,
    TrackerSessionDelete(i64),

    // ── Toast ───────────────────────────────────────────────────
    ShowToast(String),
    ToastHide,
    MathRendered(
        String,
        Result<(iced::widget::image::Handle, f32, f32), String>,
    ),

    // ── System ───────────────────────────────────────────────────
    Tick,
    KeyboardShortcut(Shortcut),
    ToggleTOC,
    TocClicked(usize),
    SplitViewToggle,
    SplitViewDragStart,
    SplitViewDragging(f32),
    SplitViewDragEnd,
    WindowResized(f32, f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackerTab {
    Dashboard,
    Log,
    Projects,
    Gates,
    Reading,
    Config,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    Save,
    OpenVault,
    NewFile,
    Search,
    CommandPalette,
    ToggleSidebar,
    ToggleBacklinks,
    FocusMode,
    TableOfContents,
    StudyTracker,
    SplitView,
    Escape,
}

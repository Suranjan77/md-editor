#[derive(Debug, Clone)]
pub enum Message {
    // ── Vault ────────────────────────────────────────────────────
    OpenVaultDialog,
    VaultOpened(Option<String>),
    VaultIndexed(
        u64,
        String,
        Result<Option<Vec<md_editor_core::types::FileEntry>>, String>,
    ),
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
    GlobalSearchSubmit,
    CommandPaletteOpen,
    CommandPaletteQueryChanged(String),
    CommandPaletteCommandClicked(Shortcut),
    CommandPaletteMove(i32),
    OverlaySubmitCurrent,

    // ── Research graph ───────────────────────────────────────────
    GraphToggle,
    GraphRefresh,
    GraphSnapshotLoaded(u64, Result<md_editor_core::types::GraphSnapshot, String>),
    GraphScopeGlobal,
    GraphScopeLocal,
    GraphLocalDepthChanged(u8),
    GraphQueryChanged(String),
    GraphShowPdfsToggled(bool),
    GraphShowMissingToggled(bool),
    GraphShowOrphansToggled(bool),
    GraphNodeSelected(Option<String>),
    GraphNodeMoved {
        path: String,
        x: f32,
        y: f32,
    },
    GraphNodeOpen(String),
    /// Select a node *and* recenter the viewport on it ("reveal in graph").
    GraphNodeFocused(String),
    /// Pin/unpin a node so the force simulation stops moving it.
    GraphPinToggled(String),
    /// Release every pinned node back to the simulation.
    GraphUnpinAll,
    GraphFitView,
    GraphResetLayout,
    /// Multiply the zoom about the canvas center (toolbar zoom buttons).
    GraphZoomBy(f32),
    /// Pause or resume the force simulation.
    GraphPhysicsToggled,
    /// Turn every node/link filter back on.
    GraphFiltersReset,
    /// Show or hide the right-hand inspector panel.
    GraphInspectorToggled,
    GraphPhysicsTick,
    /// Pan/zoom committed by the graph canvas (mirrored into `GraphState` so the
    /// GPU render layer and the label overlay share one transform).
    GraphSetView {
        pan_x: f32,
        pan_y: f32,
        zoom: f32,
    },
    /// Node currently under the cursor, or `None` when the cursor leaves all nodes.
    GraphHovered(Option<String>),

    NameModalInputChanged(String),
    NameModalSubmit(String),
    NameModalSubmitCurrent,
    NameModalCancel,
    PdfLinkNoteFolderSelected(String),
    PdfLinkNoteFileSelected(String),
    PdfLinkNotePickerSearchChanged(String),
    DeleteFile(String),
    DeleteFileDialog(String),
    RenameEntryDialog(String),
    UnsavedChangesSave,
    UnsavedChangesDiscard,
    UnsavedChangesCancel,

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
    PdfLoaded(u64, Result<u16, String>), // render generation, total pages or load error
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
    PdfTocLoaded(u64, Vec<md_editor_core::pdf::TocEntry>, bool),
    PdfPageLinksLoaded(u64, u16, Vec<md_editor_core::pdf::LinkInfo>),
    PdfReferencesLoaded(String, Vec<md_editor_core::references::ReferenceLink>),
    PdfSearchResult(u64, Result<Vec<md_editor_core::pdf::PdfSearchMatch>, String>),
    PdfSearchResultClicked(u16),
    PdfScrollBy(f32),
    PdfLinkPreviewResult(Result<md_editor_core::pdf::LinkPreviewResult, String>),
    ClosePdfLinkPreview,
    // ── PDF Study Updates ──────────────────────────────────────────
    PdfDocumentIdComputed(u64, Option<(String, String, u64, Option<i64>)>),
    PdfPageTextLoaded(u64, u16, Result<md_editor_core::pdf::PdfPageText, String>),
    PdfSelectionChanged(u16, usize, usize),
    PdfSelectionCleared,
    PdfSelectionFinished(u16, usize, usize),
    PdfCopySelection,
    PdfCreateHighlight(md_editor_core::pdf::PdfAnnotationColor),
    PdfQuickHighlight,
    PdfDeleteHighlight(String),
    PdfCopyAnnotationText(String),
    PdfSearchLooseToggled(bool),
    PdfOrphanReport,
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
    TrackerGateToggled(String, String, usize),
    TrackerReadingToggled(String, String, usize),
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
    KeyboardShortcut(Shortcut),
    ToggleTOC,
    TocClicked(usize),
    SplitViewToggle,
    SplitViewDragStart,
    SplitViewDragging(f32),
    SplitViewDragEnd,
    WindowResized(f32, f32),
    WindowOpened(iced::window::Id),
    WindowRescaled(f32),
    WindowCloseRequested(iced::window::Id),
    WindowCloseNow(iced::window::Id),
    /// Vault-relative paths that changed on disk (filesystem watcher), debounced.
    VaultFilesChanged(Vec<String>),
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
    KnowledgeGraph,
    FocusMode,
    TableOfContents,
    StudyTracker,
    SplitView,
    Escape,
}

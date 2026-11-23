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
    /// Empty space in the file tree was clicked: drop the selection so new
    /// entries go to the vault root.
    SidebarSelectionCleared,

    // ── Navigation ───────────────────────────────────────────────
    GlobalSearchOpen,
    SearchClose,
    SearchQueryChanged(String),
    SearchDebounceElapsed,
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
    /// A vault file picked from the palette's search results.
    CommandPaletteFileClicked(String),
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

    // ── Editor ───────────────────────────────────────────────────
    EditorCommand(crate::editor::buffer::EditorCommand),
    EditorCommandNoScroll(crate::editor::buffer::EditorCommand),
    EditorSave,
    EditorCheckboxToggle(usize),
    EditorScrolled {
        y: f32,
        viewport_width: f32,
        viewport_height: f32,
    },
    /// The editor widget's answer to a request to bring the caret into view.
    EditorCaretView(crate::editor::renderer::CaretView),
    /// A wheel turn or click took over from an animated editor scroll.
    EditorScrollInterrupted,
    /// The autosave debounce window elapsed; write the buffer if still dirty.
    AutosaveElapsed,
    /// A frame is being prepared; advance in-flight UI transitions.
    AnimationTick(std::time::Instant),

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
    // vault-relative path the TOC belongs to, outline entries, synthetic flag.
    // Gated on the path (not the render generation): the TOC is a
    // document-level artifact and must survive the zoom/fit generation bumps
    // that happen while it is still being extracted.
    PdfTocLoaded(String, Vec<md_editor_core::pdf::TocEntry>, bool),
    PdfPageLinksLoaded(u64, u16, Vec<md_editor_core::pdf::LinkInfo>),
    PdfReferencesLoaded(String, Vec<md_editor_core::references::ReferenceLink>),
    PdfSearchResult(
        u64,
        Result<Vec<md_editor_core::pdf::PdfSearchMatch>, String>,
    ),
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

    // ── Presentations ────────────────────────────────────────────
    /// Load generation, then the prepared deck or why it could not be read.
    PptxLoaded(
        u64,
        Result<std::sync::Arc<crate::slides::LoadedDeck>, String>,
    ),
    PptxScrolled {
        x: f32,
        y: f32,
        viewport_height: f32,
    },
    PptxZoomIn,
    PptxZoomOut,
    PptxZoomFit,
    /// Move this many slides; negative moves back.
    PptxStep(i32),
    // ── Ink (handwritten notes) ──────────────────────────────────
    InkToggle,
    /// Raw window handle, once resolved, so pen capture can be installed.
    InkPenHookReady(u64),
    /// A batch of captured pen samples, in window-client physical pixels.
    InkPenSamples(Vec<crate::ink::pen::PenSample>),
    /// Mouse fallback, in surface-local logical pixels.
    InkPointerDown(f32, f32),
    InkPointerMove(f32, f32),
    InkPointerUp,
    /// Mouse moved over the canvas without drawing, in surface-local logical
    /// pixels. Keeps the aiming indicators live for mouse users.
    InkHoverMoved(f32, f32),
    InkToolSelected(crate::ink::Tool),
    InkEraserModeSelected(crate::ink::EraserMode),
    InkPaperSelected(crate::ink::document::PaperStyle),
    InkColorSelected(iced::Color),
    InkSizeSelected(f32),
    /// Shift the view by a screen-space delta.
    InkPan {
        dx: f32,
        dy: f32,
    },
    /// Zoom about a surface-local anchor, in scroll-wheel notches.
    InkZoom {
        x: f32,
        y: f32,
        notches: f32,
    },
    /// Zoom about the middle of the surface, in wheel-equivalent notches.
    InkZoomStep(f32),
    InkResetView,
    InkDeleteSelection,
    InkUndo,
    InkRedo,
    InkClear,
    InkSave,
    InkSaved(Result<String, String>),

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
    // tex, scale factor the bitmaps were rasterized for, render result
    MathRendered(
        String,
        f32,
        Result<crate::editor::renderer::MathRender, String>,
    ),

    // ── System ───────────────────────────────────────────────────
    KeyboardShortcut(Shortcut),
    ToggleTOC,
    TocClicked(usize),
    SplitViewToggle,
    SplitViewDragStart,
    SplitViewDragging(f32),
    SplitViewDragEnd,
    WindowResized(iced::window::Id, f32, f32),
    WindowOpened(iced::window::Id),
    WindowClosed(iced::window::Id),
    WindowRescaled(iced::window::Id, f32),
    /// The window manager asked to close; flush unsaved work, then exit.
    WindowCloseRequested,
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
    Handwriting,
    Escape,
}

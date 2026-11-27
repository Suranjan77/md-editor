//! `Message`: one nested enum per feature family (routed by `app/mod.rs::update_inner`), plus `Shortcut`, `SettingsSection` and `TrackerTab`.

#[derive(Debug, Clone)]
pub enum Message {
    Vault(VaultMessage),
    Tabs(TabsMessage),
    Desks(DesksMessage),
    Settings(SettingsMessage),
    Graph(GraphMessage),
    Shell(ShellMessage),
    Editor(EditorMessage),
    Pdf(PdfMessage),
    Search(SearchMessage),
    Ink(InkMessage),
    KeyboardShortcut(Shortcut),
    /// From a pane's view: an interaction focuses that pane first; scrolls
    /// and measurements run in it without moving focus.
    FromPane(crate::app::workspace::PaneId, Box<Message>),
    /// A result for a pane (a task it started): runs in that pane, focus stays.
    ToPane(crate::app::workspace::PaneId, Box<Message>),
}

/// Saved workspace layouts ("desks"). Handled by `app/desks.rs`.
#[derive(Debug, Clone)]
pub enum DesksMessage {
    OpenDialog,
    OpenSaveDialog,
    Save(String),
    Switch(String),
    Delete(String),
    OpenFromPalette(String),
}

/// Document tab strip and tab lifecycle. Handled by `app/tabs_handler.rs`.
#[derive(Debug, Clone)]
pub enum TabsMessage {
    Activate(crate::tabs::TabId),
    OpenBackground(String),
    Close(crate::tabs::TabId),
    CloseActive,
    CloseOthers(crate::tabs::TabId),
    Reopen,
    Next,
    Previous,
    MruNext,
    MruPrevious,
    SelectOrdinal(usize),
    MoveBy(isize),
    DragStart(crate::tabs::TabId),
    DragOver(usize),
    DragEnd,
    ToggleOverflowMenu,
    /// Move a tab to the other pane (splitting if needed).
    MoveToOtherPane(crate::tabs::TabId),
}

/// Settings view sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Appearance,
    Editor,
    Files,
    Keyboard,
    About,
}

/// Attachments destination mode in Settings -> Files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentsMode {
    VaultRoot,
    BesideNote,
    Custom,
}

/// Application preferences. Handled by `app/settings_handler.rs`.
#[derive(Debug, Clone)]
pub enum SettingsMessage {
    Toggle,
    Close,
    SelectSection(SettingsSection),
    ThemeModeSelected(crate::design::ThemeMode),
    SystemThemeChanged(iced::theme::Mode),
    InterfaceScaleSelected(u32),
    ReducedMotionSelected(crate::design::motion::ReducedMotionPref),
    ReadingFontSelected(crate::design::ReadingFont),
    EditorFontSizeChanged(f32),
    ContentMeasureSelected(crate::design::ContentMeasure),
    LineNumbersToggled(bool),
    AttachmentsModeSelected(AttachmentsMode),
    AttachmentsCustomChanged(String),
    PickAttachmentsFolder,
    AttachmentsFolderPicked(Option<String>),
    ExcludedFoldersChanged(String),
    /// Apply the edited excluded-folders list and re-index the vault.
    ExcludedFoldersApply,
}

/// Keyboard navigation inside the file-tree sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarNavKey {
    Up,
    Down,
    Left,
    Right,
    Enter,
}

/// Vault lifecycle, the file tree, and the name/delete/link-note modals. Handled by `app/vault*.rs`.
#[derive(Debug, Clone)]
pub enum VaultMessage {
    OpenVaultDialog,
    CreateVaultDialog,
    VaultOpened(Option<String>),
    VaultCreated(Option<String>),
    OpenRecentVault(String),
    VaultIndexed(
        u64,
        String,
        Result<Option<Vec<md_editor_core::types::FileEntry>>, String>,
    ),
    CreateFileDialog,
    CreateFolderDialog,
    NewNoteInFolder(String),
    NewFolderInFolder(String),
    CreateNoteFromPalette(String),
    SidebarToggle,
    SidebarFilterChanged(String),
    SidebarKeyNav(SidebarNavKey),
    SidebarFileClicked(String),
    SidebarFolderToggled(String),
    /// Empty space in the file tree was clicked: drop the selection so new
    /// entries go to the vault root.
    SidebarSelectionCleared,
    RevealInFileManager(String),
    /// Open a file in the pane beside the focused one (splitting if needed).
    OpenToSide(String),
    CopyRelativePath(String),
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
    /// Vault-relative paths that changed on disk (filesystem watcher), debounced.
    VaultFilesChanged(Vec<String>),
}

/// The knowledge graph. Handled by `app/graph.rs`.
#[derive(Debug, Clone)]
pub enum GraphMessage {
    Toggle,
    Refresh,
    SnapshotLoaded(u64, Result<md_editor_core::types::GraphSnapshot, String>),
    ScopeGlobal,
    ScopeLocal,
    LocalDepthChanged(u8),
    QueryChanged(String),
    ShowPdfsToggled(bool),
    ShowSketchesToggled(bool),
    ShowMissingToggled(bool),
    ShowOrphansToggled(bool),
    NodeSelected(Option<String>),
    /// Step selection forward or backward across currently visible nodes (`Tab` / `Shift+Tab`).
    CycleNode(isize),
    /// Pin or unpin the currently selected node (`P` key).
    ToggleSelectedPin,
    NodeMoved {
        path: String,
        x: f32,
        y: f32,
    },
    NodeOpen(String),
    /// Select a node *and* recenter the viewport on it ("reveal in graph").
    NodeFocused(String),
    /// Pin/unpin a node so the force simulation stops moving it.
    PinToggled(String),
    /// Release every pinned node back to the simulation.
    UnpinAll,
    OpenPinboardsModal,
    OpenSavePinboardModal,
    SavePinboard(String),
    LoadPinboard(String),
    DeletePinboard(String),
    FitView,
    ResetLayout,
    /// Multiply the zoom about the canvas center (toolbar zoom buttons).
    ZoomBy(f32),
    /// Pause or resume the force simulation.
    PhysicsToggled,
    /// Turn every node/link filter back on.
    FiltersReset,
    /// Show or hide the right-hand inspector panel.
    InspectorToggled,
    PhysicsTick,
    /// Pan/zoom committed by the graph canvas (mirrored into `GraphState` so the
    /// GPU render layer and the label overlay share one transform).
    SetView {
        pan_x: f32,
        pan_y: f32,
        zoom: f32,
        /// When this is a fit: the fit revision and canvas size it frames.
        fitted: Option<(u64, f32, f32)>,
    },
    /// Node currently under the cursor, or `None` when the cursor leaves all nodes.
    Hovered(Option<String>),
}

/// Target surface of an open right-click context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMenuTarget {
    SidebarEntry { path: String, is_dir: bool },
    SidebarEmpty,
    Tab(crate::tabs::TabId),
    Editor,
}

/// Shell chrome: command palette, toasts, split view, resize handles, context menus, drag-and-drop, windows, study tracker and image zoom.
#[derive(Debug, Clone)]
pub enum ShellMessage {
    CommandPaletteOpen,
    CommandPaletteQueryChanged(String),
    CommandPaletteCommandClicked(Shortcut),
    /// A vault file picked from the palette's search results.
    CommandPaletteFileClicked(String),
    /// `Ctrl+Enter` in the command palette: open the selected file to the side.
    CommandPaletteOpenToSide,
    OverlaySubmitCurrent,
    ImageZoomIn,
    ImageZoomOut,
    ImageZoomFit,
    ImageZoomActual,
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
    ShowToast(String),
    ToastHide,
    DismissToast(u64),
    RetrySave,
    SplitViewToggle,
    /// Close a pane of the split; its tabs move to the other pane.
    ClosePane(crate::app::workspace::PaneId),
    /// The panes area's size, for turning divider drags into a ratio.
    WorkspaceMeasured(iced::Size),
    SplitViewDragStart,
    /// The pointer while the divider is dragged.
    SplitViewDragging(iced::Point),
    SplitViewDragEnd,
    SplitViewReset,
    SidebarResizeStart,
    SidePanelResizeStart,
    PanelResizeDrag(f32),
    PanelResizeEnd,
    ResetSidebarWidth,
    ResetSidePanelWidth,
    OpenContextMenu(ContextMenuTarget, f32, f32),
    CloseContextMenu,
    CursorMoved(f32, f32),
    /// A file dragged over, and dropped on, the given window.
    FileHovered(iced::window::Id, std::path::PathBuf),
    FilesHoveredLeft,
    FileDropped(iced::window::Id, std::path::PathBuf),
    FocusRegionNext,
    FocusRegionPrev,
    WindowResized(iced::window::Id, f32, f32),
    WindowOpened(iced::window::Id),
    WindowClosed(iced::window::Id),
    WindowRescaled(iced::window::Id, f32),
    /// The window manager asked to close this window: for the main window,
    /// flush unsaved work before it exits; for the ink window, its page.
    WindowCloseRequested(iced::window::Id),
}

/// The Markdown editor, autosave, math rendering, image insertion and the outline. Handled by `app/editor.rs`.
#[derive(Debug, Clone)]
pub enum EditorMessage {
    /// Ctrl+V in the editor: read the clipboard (off the UI thread) and paste
    /// an image, image files, or text.
    Paste,
    /// What the clipboard held.
    Pasted(crate::platform::ClipboardPayload),
    /// Open file dialog to pick images to insert at the caret.
    InsertImageDialog,
    /// Images picked from the file dialog.
    ImagesPicked(Vec<std::path::PathBuf>),
    /// Right-click inside the editor at window coordinates `(x, y)`.
    OpenContextMenu(f32, f32),
    Command(crate::editor::buffer::EditorCommand),
    CommandNoScroll(crate::editor::buffer::EditorCommand),
    Save,
    CheckboxToggle(usize),
    Scrolled {
        y: f32,
        viewport_width: f32,
        viewport_height: f32,
    },
    /// The editor widget's answer to a request to bring the caret into view.
    CaretView(crate::editor::renderer::CaretView),
    /// A wheel turn or click took over from an animated editor scroll.
    ScrollInterrupted,
    /// The autosave debounce window elapsed; write the buffer if still dirty.
    AutosaveElapsed,
    /// A frame is being prepared; advance in-flight UI transitions.
    AnimationTick(std::time::Instant),
    /// TeX, the display scale and the ink colour it was rasterized for, and the result.
    MathRendered(
        String,
        f32,
        iced::Color,
        Result<crate::editor::renderer::MathRender, String>,
    ),
    ToggleTOC,
    TocClicked(usize),
    /// An image in the note finished decoding: its link text, the file, and
    /// the image (handle, width, height) or why it could not be read.
    ImageLoaded(
        String,
        std::path::PathBuf,
        Result<(iced::widget::image::Handle, f32, f32), String>,
    ),
    /// A `pdf://…&crop=` image in the note finished rendering: its link text
    /// and the image (handle, width, height) or why it could not be drawn.
    PdfCropLoaded(
        String,
        Result<(iced::widget::image::Handle, f32, f32), String>,
    ),
    /// Move the completion popup's selection by this many rows.
    CompletionMove(isize),
    /// Insert the selected completion (`Enter` / `Tab`).
    CompletionAccept,
    /// Insert this completion (clicked).
    CompletionPick(usize),
    /// Close the completion popup (`Escape`).
    CompletionClose,
    /// Opt-in fenced code block execution completed.
    CodeBlockFinished {
        expected_path: String,
        expected_close_line: usize,
        expected_body: String,
        result: Result<String, String>,
    },
}

/// The PDF viewer and its study tools. Handled by `app/pdf.rs`.
#[derive(Debug, Clone)]
pub enum PdfMessage {
    ZoomChanged(f32),
    // The editable page/zoom fields and Fit-page button are handled but the PDF
    // toolbar (`views/pdf_viewer.rs`) does not show them yet (UI overhaul §3).
    #[allow(dead_code)]
    ZoomInputChanged(String),
    #[allow(dead_code)]
    ZoomInputSubmit,
    #[allow(dead_code)]
    PageInputChanged(String),
    #[allow(dead_code)]
    PageInputSubmit,
    /// Refit to the pane width (open, resize, split): stops at 200% unless the
    /// user asked for a fit with `FitToWidthClicked`.
    FitToWidth,
    /// The toolbar's Fit width: fill the pane, past 200% if that is what fits.
    FitToWidthClicked,
    #[allow(dead_code)]
    FitToPage,
    Loaded(u64, Result<u16, String>),
    PageSizesLoaded(u64, String, Vec<(f32, f32)>),
    Rendered(u64, u16, PageImage),
    RenderFailed(u64, u16),
    RenderSkipped(u64, u16),
    Scrolled {
        y: f32,
        viewport_height: f32,
    },
    /// The page list's viewport was laid out at this size (first show or resize).
    ViewportMeasured(iced::Size),
    LeftClicked(u16, f32, f32, iced::keyboard::Modifiers),
    RightClicked(u16, f32, f32),
    TocLoaded(String, Vec<md_editor_core::pdf::TocEntry>, bool),
    PageLinksLoaded(u64, u16, Vec<md_editor_core::pdf::LinkInfo>),
    ReferencesLoaded(String, Vec<md_editor_core::references::ReferenceLink>),
    /// A PDF search finished; the `u64` is its `SearchState::pdf_search_seq`.
    SearchResult(
        u64,
        Result<Vec<md_editor_core::pdf::PdfSearchMatch>, String>,
    ),
    SearchResultClicked(u16),
    ScrollBy(f32),
    LinkPreviewResult(Result<md_editor_core::pdf::LinkPreviewResult, String>),
    ClosePdfLinkPreview,
    DocumentIdComputed(u64, Option<(String, String, u64, Option<i64>)>),
    PageTextLoaded(u64, u16, Result<md_editor_core::pdf::PdfPageText, String>),
    SelectionChanged(u16, usize, usize),
    SelectionCleared,
    /// Alt+drag picked a rectangle of a page, as fractions of it: show that
    /// crop in the open note (or copy its link).
    RegionSelected(u16, iced::Rectangle),
    SelectionFinished(u16, usize, usize),
    CopySelection,
    CreateHighlight(md_editor_core::pdf::PdfAnnotationColor),
    QuickHighlight,
    DeleteHighlight(String),
    CopyAnnotationText(String),
    SearchLooseToggled(bool),
    OrphanReport,
    AddQuickNote(String, String),
    LinkNote(String, String),
    OpenLinkedNote(String),
    AnnotationFocused {
        document_path: String,
        annotation_id: String,
        page: u16,
    },
    /// Toggle or switch non-destructive PDF margin ink tool (`Pen` / `Eraser` / `None`).
    InkToolToggled(Option<crate::pdf_pane::PdfInkTool>),
    /// Pick color for future PDF margin ink strokes (activates `Pen` if needed).
    InkColorSelected(md_editor_core::pdf::PdfAnnotationColor),
    /// Live preview update during PDF margin ink pointer/stylus drag.
    InkLiveUpdated(Option<(u16, Vec<[f32; 3]>)>),
    /// Commit a finished margin ink stroke in normalized page coordinates `[x_frac, y_frac, pressure]`.
    InkStrokeCommitted {
        page: u16,
        points: Vec<[f32; 3]>,
    },
    /// Erase any PDF margin ink stroke on `page` intersecting `(x_frac, y_frac)`.
    InkEraseAt {
        page: u16,
        x_frac: f32,
        y_frac: f32,
    },
    /// Remove the most recently created margin ink stroke on the active PDF.
    UndoInkStroke,
    /// The embedded files of the PDF at this vault path were listed.
    AttachmentsLoaded(String, Vec<md_editor_core::pdf::PdfAttachmentInfo>),
    /// Open an embedded file of the PDF at this vault path in the system's app.
    OpenAttachment(String, md_editor_core::pdf::PdfAttachmentInfo),
    /// Copy an embedded file of the PDF at this vault path into the vault.
    SaveAttachment(String, md_editor_core::pdf::PdfAttachmentInfo),
    /// `SaveAttachment` stored the file.
    AttachmentSaved(md_editor_core::attachments::Stored),
    /// Reading, opening or saving an attachment failed.
    AttachmentFailed(String),
}

/// Global search and in-document find/replace. Handled by `app/search.rs`.
#[derive(Debug, Clone)]
pub enum SearchMessage {
    GlobalSearchOpen,
    Close,
    // Handled; the find bar (`views/search.rs`) has no replace toggle or
    // replace-one button yet (UI overhaul §3).
    #[allow(dead_code)]
    ToggleReplaceRow,
    QueryChanged(String),
    DebounceElapsed,
    ReplaceChanged(String),
    RegexToggled(bool),
    MatchCaseToggled(bool),
    Previous,
    Next,
    #[allow(dead_code)]
    ReplaceOne,
    ReplaceAll,
    ResultClicked(String),
    GlobalSearchMoveSelection(isize),
    GlobalSearchSubmit,
}

/// Handwritten notes. Handled by `app/ink.rs`.
#[derive(Debug, Clone)]
pub enum InkMessage {
    Toggle,
    PenHookReady(u64),
    PenSamples(Vec<crate::ink::pen::PenSample>),
    PointerDown(f32, f32),
    PointerMove(f32, f32),
    PointerUp,
    HoverMoved(f32, f32),
    ToolSelected(crate::ink::Tool),
    EraserModeSelected(crate::ink::EraserMode),
    PaperSelected(crate::ink::document::PaperStyle),
    ColorSelected(iced::Color),
    SizeSelected(f32),
    Pan {
        dx: f32,
        dy: f32,
    },
    Zoom {
        x: f32,
        y: f32,
        notches: f32,
    },
    ZoomStep(f32),
    ResetView,
    DeleteSelection,
    Undo,
    Redo,
    Clear,
    Save,
    Saved(Result<String, String>),
    /// The ink autosave delay passed with unsaved strokes on the page.
    AutosaveElapsed,
}

impl From<VaultMessage> for Message {
    fn from(message: VaultMessage) -> Self {
        Message::Vault(message)
    }
}

impl From<TabsMessage> for Message {
    fn from(message: TabsMessage) -> Self {
        Message::Tabs(message)
    }
}

impl From<DesksMessage> for Message {
    fn from(message: DesksMessage) -> Self {
        Message::Desks(message)
    }
}

impl From<SettingsMessage> for Message {
    fn from(message: SettingsMessage) -> Self {
        Message::Settings(message)
    }
}

impl From<GraphMessage> for Message {
    fn from(message: GraphMessage) -> Self {
        Message::Graph(message)
    }
}

impl From<ShellMessage> for Message {
    fn from(message: ShellMessage) -> Self {
        Message::Shell(message)
    }
}

impl From<EditorMessage> for Message {
    fn from(message: EditorMessage) -> Self {
        Message::Editor(message)
    }
}

impl From<PdfMessage> for Message {
    fn from(message: PdfMessage) -> Self {
        Message::Pdf(message)
    }
}

impl From<SearchMessage> for Message {
    fn from(message: SearchMessage) -> Self {
        Message::Search(message)
    }
}

impl From<InkMessage> for Message {
    fn from(message: InkMessage) -> Self {
        Message::Ink(message)
    }
}

/// A rendered PDF page. Wrapped so `Debug` prints its size, not its pixels.
#[derive(Clone)]
pub struct PageImage(pub image::DynamicImage);

impl std::fmt::Debug for PageImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PageImage({}x{})", self.0.width(), self.0.height())
    }
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
    DailyNote,
    InsertImage,
    Search,
    GlobalSearch,
    CommandPalette,
    CloseTab,
    ReopenTab,
    NextTab,
    PreviousTab,
    ToggleSidebar,
    ToggleBacklinks,
    KnowledgeGraph,
    FocusMode,
    TableOfContents,
    StudyTracker,
    SplitView,
    /// Split side by side (Vertical axis).
    SplitRight,
    /// Split stacked (Horizontal axis).
    SplitDown,
    /// Close the second pane, keeping its tabs in the first.
    Unsplit,
    /// Focus the other pane of the split (splitting first if there is none).
    FocusOtherPane,
    /// Move the focused pane's tab to the other pane.
    MoveTabToOtherPane,
    /// Side by side ⇄ stacked.
    ToggleSplitAxis,
    /// Open the Save Desk dialog.
    SaveDesk,
    /// Open the Saved Desks dialog.
    SwitchDesk,
    Handwriting,
    /// A new handwritten sketch, shown in the open note at the caret.
    InsertSketch,
    /// Evaluate selected or enclosing inline/display math.
    EvaluateMath,
    /// Run the enclosing fenced code block and insert/update `> [!output]`.
    RunCodeBlock,
    Settings,
    RenameEntry,
    CycleFocusNext,
    CycleFocusPrev,
    Escape,
}

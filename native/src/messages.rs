#[derive(Debug, Clone)]
pub enum Message {
    // ── Vault ────────────────────────────────────────────────────
    OpenVaultDialog,
    VaultOpened(Option<String>),
    CreateFileDialog,
    CreateFolderDialog,
    FileLoaded(String, String),

    // ── Sidebar ──────────────────────────────────────────────────
    SidebarToggle,
    SidebarFileClicked(String),
    SidebarFolderToggled(String),

    // ── Navigation ───────────────────────────────────────────────
    BacklinksToggle,
    SearchOpen,
    SearchClose,
    SearchQueryChanged(String),
    SearchResultClicked(String),
    CommandPaletteOpen,
    CommandPaletteClose,
    CommandPaletteQueryChanged(String),
    CommandPaletteCommandClicked(Shortcut),
    NameModalInputChanged(String),
    NameModalSubmit(String),
    NameModalCancel,
    DeleteFile(String),
    DeleteFileDialog(String),
    RenameFile(String),

    // ── Editor ───────────────────────────────────────────────────
    EditorAction(EditorAction),
    EditorContentChanged(String),
    EditorSave,
    EditorCheckboxToggle(usize),
    EditorCursorMove(usize, usize),

    // ── PDF ──────────────────────────────────────────────────────
    PdfPageChanged(u16),
    PdfZoomChanged(f32),
    PdfLoaded(u16), // Total pages
    PdfRendered(u64, u16, image::DynamicImage),
    PdfRenderFailed(u64, u16),
    PdfScrolled {
        y: f32,
        viewport_height: f32,
    },
    PdfLeftClicked(u16, f32, f32),
    PdfRightClicked(u16, f32, f32),
    PdfTocLoaded(Vec<md_editor_core::pdf::TocEntry>),
    PdfPageLinksLoaded(u16, Vec<md_editor_core::pdf::LinkInfo>),
    PdfLinkPreviewResult(Result<md_editor_core::pdf::LinkPreviewResult, String>),
    ClosePdfLinkPreview,

    // ── Tracker ──────────────────────────────────────────────────
    TrackerToggle,
    TrackerStart,
    TrackerStop,
    TrackerSave(f32, String), // hours, notes
    TrackerLoaded(Vec<md_editor_core::tracker::StudySession>),
    TrackerTabSelected(TrackerTab),
    TrackerProjectStatusChanged(String, String),
    TrackerGateToggled(String, usize),
    TrackerReadingToggled(String, usize),
    TrackerConfigChanged(String),
    TrackerConfigSave,

    // ── Toast ───────────────────────────────────────────────────
    ToastShow(String),
    ToastHide,
    MathRendered(
        String,
        Result<(iced::widget::image::Handle, f32, f32), String>,
    ),

    // ── System ───────────────────────────────────────────────────
    Tick,
    KeyboardShortcut(Shortcut),
    FocusModeToggle,
    ToggleTOC,
    TocClicked(usize),
    SplitViewToggle,
    SplitViewDragStart,
    SplitViewDragging(f32),
    SplitViewDragEnd,
    WindowResized(f32),
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
    Escape,
}

#[derive(Debug, Clone)]
pub enum EditorAction {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    MoveHome,
    MoveEnd,
    Backspace,
    Delete,
    Undo,
    Redo,
}

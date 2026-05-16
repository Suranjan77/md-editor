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
    PdfRendered(u16, image::DynamicImage),
    PdfRightClicked(f32, f32),

    // ── Tracker ──────────────────────────────────────────────────
    TrackerToggle,
    TrackerStart,
    TrackerStop,
    TrackerSave(f32, String), // hours, notes
    TrackerLoaded(Vec<md_editor_core::tracker::StudySession>),

    // ── Toast ───────────────────────────────────────────────────
    ToastShow(String),
    ToastHide,

    // ── System ───────────────────────────────────────────────────
    Tick,
    KeyboardShortcut(Shortcut),
    FocusModeToggle,
    ToggleTOC,
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

//! Single-source-of-truth keyboard shortcut registry for subscriptions, menus, tooltips, palette and settings.

use crate::messages::Shortcut;
use crate::ui::Icon;

/// Metadata for an application shortcut.
#[derive(Debug, Clone, Copy)]
pub struct ShortcutSpec {
    pub shortcut: Shortcut,
    pub label: &'static str,
    pub chord: &'static str,
    pub category: &'static str,
    pub icon: Icon,
    pub in_palette: bool,
}

/// Complete registry of application-level commands and shortcuts.
///
/// Order matters for the Command Palette when the query is empty (`New File` is
/// first). Every view (tooltips, menus, Command Palette, Settings -> Keyboard)
/// reads from this slice so labels and chords never drift.
pub const REGISTRY: &[ShortcutSpec] = &[
    ShortcutSpec {
        shortcut: Shortcut::NewFile,
        label: "New File",
        chord: "Ctrl N",
        category: "Vault & Files",
        icon: Icon::FilePlus,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::OpenVault,
        label: "Open Vault",
        chord: "Ctrl O",
        category: "Vault & Files",
        icon: Icon::FolderOpen,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::Save,
        label: "Save File",
        chord: "Ctrl S",
        category: "Vault & Files",
        icon: Icon::Check,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::InsertImage,
        label: "Insert Image…",
        chord: "",
        category: "Editor",
        icon: Icon::Image,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::Search,
        label: "Find in Document",
        chord: "Ctrl F",
        category: "Search",
        icon: Icon::Search,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::GlobalSearch,
        label: "Search Vault",
        chord: "Ctrl Shift F",
        category: "Search",
        icon: Icon::Search,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::CommandPalette,
        label: "Command Palette",
        chord: "Ctrl P",
        category: "Navigation",
        icon: Icon::Command,
        in_palette: false,
    },
    ShortcutSpec {
        shortcut: Shortcut::CloseTab,
        label: "Close Tab",
        chord: "Ctrl W",
        category: "Tabs",
        icon: Icon::X,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::ReopenTab,
        label: "Reopen Closed Tab",
        chord: "Ctrl Shift T",
        category: "Tabs",
        icon: Icon::Undo,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::NextTab,
        label: "Next Tab",
        chord: "Ctrl Tab",
        category: "Tabs",
        icon: Icon::ChevronRight,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::PreviousTab,
        label: "Previous Tab",
        chord: "Ctrl Shift Tab",
        category: "Tabs",
        icon: Icon::ChevronLeft,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::ToggleSidebar,
        label: "Toggle Sidebar",
        chord: "Ctrl B",
        category: "Panels & Views",
        icon: Icon::LayoutPanelLeft,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::ToggleBacklinks,
        label: "Toggle Backlinks",
        chord: "Ctrl Shift B",
        category: "Panels & Views",
        icon: Icon::Link,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::TableOfContents,
        label: "Table of Contents",
        chord: "Ctrl Shift O",
        category: "Panels & Views",
        icon: Icon::ListTree,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::KnowledgeGraph,
        label: "Open Knowledge Graph",
        chord: "Ctrl G",
        category: "Panels & Views",
        icon: Icon::Network,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::Handwriting,
        label: "Handwriting Canvas",
        chord: "Ctrl I",
        category: "Panels & Views",
        icon: Icon::Pen,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::InsertSketch,
        label: "Insert Sketch",
        chord: "Ctrl Shift I",
        category: "Panels & Views",
        icon: Icon::Pen,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::StudyTracker,
        label: "Study Tracker",
        chord: "Ctrl T",
        category: "Panels & Views",
        icon: Icon::Clock,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::SplitView,
        label: "Split View",
        chord: "Ctrl \\",
        category: "Panels & Views",
        icon: Icon::Split,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::FocusMode,
        label: "Focus Mode",
        chord: "F11",
        category: "Panels & Views",
        icon: Icon::Maximize,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::Settings,
        label: "Settings",
        chord: "Ctrl ,",
        category: "Application",
        icon: Icon::Gear,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::RenameEntry,
        label: "Rename Selected File",
        chord: "F2",
        category: "Vault & Files",
        icon: Icon::Pencil,
        in_palette: true,
    },
    ShortcutSpec {
        shortcut: Shortcut::CycleFocusNext,
        label: "Focus Next Region",
        chord: "F6",
        category: "Accessibility",
        icon: Icon::FitPage,
        in_palette: false,
    },
    ShortcutSpec {
        shortcut: Shortcut::CycleFocusPrev,
        label: "Focus Previous Region",
        chord: "Shift F6",
        category: "Accessibility",
        icon: Icon::FitPage,
        in_palette: false,
    },
    ShortcutSpec {
        shortcut: Shortcut::Escape,
        label: "Close Overlay / Dismiss",
        chord: "Esc",
        category: "Application",
        icon: Icon::X,
        in_palette: false,
    },
];

/// Look up metadata for a `Shortcut`.
pub fn spec(shortcut: Shortcut) -> &'static ShortcutSpec {
    REGISTRY
        .iter()
        .find(|item| item.shortcut == shortcut)
        .unwrap_or(&REGISTRY[0])
}

/// Human-readable chord string for `shortcut` (empty if unbound).
pub fn chord(shortcut: Shortcut) -> &'static str {
    spec(shortcut).chord
}

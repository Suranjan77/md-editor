#![allow(dead_code)]

use crate::app_shell::{AppShellPane, CommandGroup};
use crate::messages::Shortcut;

#[derive(Debug, Clone, Copy)]
pub struct CommandContext {
    pub markdown_open: bool,
    pub pdf_open: bool,
    pub image_open: bool,
    pub active_pane: AppShellPane,
    pub has_vault: bool,
    pub pdf_has_selection: bool,
    pub has_focused_annotation: bool,
}

#[derive(Debug, Clone)]
pub struct CommandMetadata {
    pub id: Shortcut,
    pub name: &'static str,
    pub icon: &'static str,
    pub group: CommandGroup,
    pub default_shortcut: Option<&'static str>,
}

impl CommandMetadata {
    pub fn is_enabled(&self, ctx: CommandContext) -> Result<(), &'static str> {
        let id = self.id;
        if id == Shortcut::Save && !ctx.markdown_open {
            return Err("No active markdown file to save");
        }
        if id == Shortcut::NewFile && !ctx.has_vault {
            return Err("Open a vault first");
        }
        if id == Shortcut::Search && !ctx.has_vault {
            return Err("Open a vault first");
        }
        if id == Shortcut::ToggleSidebar && !ctx.has_vault {
            return Err("Open a vault first");
        }
        if id == Shortcut::ToggleBacklinks && !ctx.markdown_open {
            return Err("Open a markdown file first");
        }
        if id == Shortcut::TableOfContents && !ctx.markdown_open && !ctx.pdf_open {
            return Err("No document open to show outline");
        }
        if id == Shortcut::SplitView && (!ctx.markdown_open || !ctx.pdf_open) {
            return Err("Requires both markdown and PDF open");
        }
        if matches!(
            id,
            Shortcut::ZoomIn
                | Shortcut::ZoomOut
                | Shortcut::ZoomFit
                | Shortcut::GoToPage
                | Shortcut::PdfSearch
                | Shortcut::PdfHighlight
                | Shortcut::PdfZoomInput
                | Shortcut::PdfFirstPage
                | Shortcut::PdfLastPage
        ) && !ctx.pdf_open
        {
            return Err("Requires an open PDF document");
        }
        if id == Shortcut::InsertPdfQuote {
            if !ctx.markdown_open || !ctx.pdf_open {
                return Err("Requires both markdown and PDF open");
            }
            if !ctx.pdf_has_selection {
                return Err("Requires active PDF text selection");
            }
        }
        if id == Shortcut::InsertPdfHighlight {
            if !ctx.markdown_open || !ctx.pdf_open {
                return Err("Requires both markdown and PDF open");
            }
            if !ctx.has_focused_annotation {
                return Err("Requires a focused PDF annotation");
            }
        }
        if id == Shortcut::FollowCitation && !ctx.markdown_open {
            return Err("Open a markdown file first");
        }
        if id == Shortcut::ShowUsages && !ctx.markdown_open && !ctx.pdf_open {
            return Err("Requires an open file");
        }
        if matches!(id, Shortcut::CitationPalette | Shortcut::ExcerptModeToggle)
            && (!ctx.markdown_open || !ctx.pdf_open)
        {
            return Err("Requires both markdown and PDF open");
        }
        if id == Shortcut::ExcerptInsertBatch && !ctx.markdown_open {
            return Err("Open a markdown file first");
        }
        if id == Shortcut::SwitchPane && (!ctx.markdown_open || !ctx.pdf_open) {
            return Err("Requires both markdown and PDF open");
        }
        Ok(())
    }
}

pub fn get_command_registry() -> Vec<CommandMetadata> {
    vec![
        CommandMetadata {
            id: Shortcut::NewFile,
            name: "New File",
            icon: "+",
            group: CommandGroup::File,
            default_shortcut: Some("Ctrl+N"),
        },
        CommandMetadata {
            id: Shortcut::OpenVault,
            name: "Open Vault",
            icon: "O",
            group: CommandGroup::File,
            default_shortcut: Some("Ctrl+O"),
        },
        CommandMetadata {
            id: Shortcut::Save,
            name: "Save",
            icon: "S",
            group: CommandGroup::File,
            default_shortcut: Some("Ctrl+S"),
        },
        CommandMetadata {
            id: Shortcut::Search,
            name: "Search Vault",
            icon: "/",
            group: CommandGroup::Search,
            default_shortcut: Some("Ctrl+F"),
        },
        CommandMetadata {
            id: Shortcut::ToggleSidebar,
            name: "Toggle Sidebar",
            icon: "S",
            group: CommandGroup::View,
            default_shortcut: Some("Ctrl+B"),
        },
        CommandMetadata {
            id: Shortcut::NavBack,
            name: "Navigate Back",
            icon: "<",
            group: CommandGroup::Navigation,
            default_shortcut: Some("Alt+Left"),
        },
        CommandMetadata {
            id: Shortcut::NavForward,
            name: "Navigate Forward",
            icon: ">",
            group: CommandGroup::Navigation,
            default_shortcut: Some("Alt+Right"),
        },
        CommandMetadata {
            id: Shortcut::ToggleBacklinks,
            name: "Toggle Backlinks",
            icon: "B",
            group: CommandGroup::Research,
            default_shortcut: Some("Ctrl+Alt+B"),
        },
        CommandMetadata {
            id: Shortcut::TableOfContents,
            name: "Toggle Table of Contents",
            icon: "T",
            group: CommandGroup::View,
            default_shortcut: Some("Ctrl+T"),
        },
        CommandMetadata {
            id: Shortcut::StudyTracker,
            name: "Study Tracker",
            icon: "R",
            group: CommandGroup::Research,
            default_shortcut: Some("Ctrl+Alt+S"),
        },
        CommandMetadata {
            id: Shortcut::SplitView,
            name: "Split View",
            icon: "|",
            group: CommandGroup::View,
            default_shortcut: Some("Split"),
        },
        CommandMetadata {
            id: Shortcut::FocusMode,
            name: "Focus Mode",
            icon: "F",
            group: CommandGroup::View,
            default_shortcut: Some("Focus"),
        },
        CommandMetadata {
            id: Shortcut::FollowCitation,
            name: "Follow Citation",
            icon: "G",
            group: CommandGroup::Research,
            default_shortcut: Some("Alt+G"),
        },
        CommandMetadata {
            id: Shortcut::ShowUsages,
            name: "Show Usages",
            icon: "U",
            group: CommandGroup::Research,
            default_shortcut: Some("Alt+U"),
        },
        CommandMetadata {
            id: Shortcut::CitationPalette,
            name: "Citation Palette",
            icon: "C",
            group: CommandGroup::Research,
            default_shortcut: Some("Alt+C"),
        },
        CommandMetadata {
            id: Shortcut::ExcerptModeToggle,
            name: "Toggle Excerpt Mode",
            icon: "E",
            group: CommandGroup::Research,
            default_shortcut: Some("Alt+E"),
        },
        CommandMetadata {
            id: Shortcut::ExcerptInsertBatch,
            name: "Insert Excerpts Batch",
            icon: "I",
            group: CommandGroup::Research,
            default_shortcut: Some("Alt+I"),
        },
        CommandMetadata {
            id: Shortcut::ZoomIn,
            name: "Zoom In",
            icon: "+",
            group: CommandGroup::View,
            default_shortcut: Some("Ctrl++"),
        },
        CommandMetadata {
            id: Shortcut::ZoomOut,
            name: "Zoom Out",
            icon: "-",
            group: CommandGroup::View,
            default_shortcut: Some("Ctrl+-"),
        },
        CommandMetadata {
            id: Shortcut::ZoomFit,
            name: "Zoom Fit",
            icon: "0",
            group: CommandGroup::View,
            default_shortcut: Some("Ctrl+0"),
        },
        CommandMetadata {
            id: Shortcut::GoToPage,
            name: "Go to Page",
            icon: "P",
            group: CommandGroup::Navigation,
            default_shortcut: Some("Ctrl+G"),
        },
        CommandMetadata {
            id: Shortcut::PdfSearch,
            name: "PDF Search",
            icon: "S",
            group: CommandGroup::Search,
            default_shortcut: Some("Ctrl+R"),
        },
        CommandMetadata {
            id: Shortcut::PdfHighlight,
            name: "PDF Highlight",
            icon: "H",
            group: CommandGroup::Annotation,
            default_shortcut: Some("Ctrl+H"),
        },
        CommandMetadata {
            id: Shortcut::InsertPdfQuote,
            name: "Insert PDF Quote",
            icon: "Q",
            group: CommandGroup::Research,
            default_shortcut: Some("Quote"),
        },
        CommandMetadata {
            id: Shortcut::InsertPdfHighlight,
            name: "Insert PDF Highlight",
            icon: "H",
            group: CommandGroup::Research,
            default_shortcut: Some("Cite"),
        },
        CommandMetadata {
            id: Shortcut::PdfFirstPage,
            name: "PDF First Page",
            icon: "^",
            group: CommandGroup::Navigation,
            default_shortcut: Some("Home"),
        },
        CommandMetadata {
            id: Shortcut::PdfLastPage,
            name: "PDF Last Page",
            icon: "$",
            group: CommandGroup::Navigation,
            default_shortcut: Some("End"),
        },
        CommandMetadata {
            id: Shortcut::ThemeDark,
            name: "Switch to Dark Theme",
            icon: "D",
            group: CommandGroup::View,
            default_shortcut: None,
        },
        CommandMetadata {
            id: Shortcut::ThemeLight,
            name: "Switch to Light Theme",
            icon: "L",
            group: CommandGroup::View,
            default_shortcut: None,
        },
        CommandMetadata {
            id: Shortcut::ThemeHighContrast,
            name: "Switch to High Contrast Theme",
            icon: "H",
            group: CommandGroup::View,
            default_shortcut: None,
        },
        CommandMetadata {
            id: Shortcut::SwitchPane,
            name: "Switch Active Pane Focus",
            icon: "P",
            group: CommandGroup::View,
            default_shortcut: Some("Alt+P"),
        },
    ]
}

pub fn detect_shortcut_conflicts() -> Vec<(&'static str, Vec<Shortcut>)> {
    use std::collections::HashMap;
    let registry = get_command_registry();
    let mut shortcut_map: HashMap<&'static str, Vec<Shortcut>> = HashMap::new();
    for cmd in registry {
        if let Some(shortcut) = cmd.default_shortcut {
            shortcut_map.entry(shortcut).or_default().push(cmd.id);
        }
    }
    shortcut_map
        .into_iter()
        .filter(|(_, ids)| ids.len() > 1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_shortcut_conflicts() {
        let conflicts = detect_shortcut_conflicts();
        assert!(
            conflicts.is_empty(),
            "Shortcut conflicts detected: {:?}",
            conflicts
        );
    }
}

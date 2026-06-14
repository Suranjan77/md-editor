//! Default command set + bindings. This is data, not behavior: handlers
//! attach in the shell. Deliberately includes the exact chord pair that was
//! v2's BUG-A — `ctrl+z` means undo in a markdown editor and zoom-input in a
//! PDF editor — as two *scoped* bindings that the conflict checker proves
//! disjoint and the resolver disambiguates by focus.

use crate::command::{CommandId, CommandRegistry, CommandSpec, RegistryError};
use crate::input::{Binding, Chord, EditorKind, Key, Mods, Scope};

fn bind(scope: Scope, chord: Chord, id: CommandId) -> Binding {
    Binding::new(scope, chord, id)
}

/// Build the default registry. Infallible in practice — a CI test asserts
/// both registration and keymap construction succeed, which is exactly the
/// "conflicts are statically detected in CI" gate from plan §3.1.
pub fn default_registry() -> Result<CommandRegistry, RegistryError> {
    let mut reg = CommandRegistry::new();
    let md = Scope::Editor(EditorKind::Markdown);
    let pdf = Scope::Editor(EditorKind::Pdf);

    let table: Vec<CommandSpec> = vec![
        // -- global (reachable even under a modal overlay) -------------------
        spec(
            "app.quit",
            "Quit",
            "Application",
            vec![bind(Scope::Global, Chord::ctrl('q'), CommandId("app.quit"))],
        ),
        spec(
            "app.settings",
            "Settings",
            "Application",
            vec![bind(
                Scope::Workspace,
                Chord::new(Mods::CTRL, Key::Char(',')),
                CommandId("app.settings"),
            )],
        ),
        spec("app.force-quit", "Force Quit", "Application", vec![]),
        // -- workspace --------------------------------------------------------
        spec(
            "palette.open",
            "Command Palette",
            "Workspace",
            vec![bind(
                Scope::Workspace,
                Chord::new(Mods::CTRL_SHIFT, Key::Char('p')),
                CommandId("palette.open"),
            )],
        ),
        spec(
            "file.quick-open",
            "Quick Open File",
            "Workspace",
            vec![bind(
                Scope::Workspace,
                Chord::ctrl('p'),
                CommandId("file.quick-open"),
            )],
        ),
        spec("vault.open", "Open Vault Folder", "Workspace", vec![]),
        spec("file.new-note", "New Note", "Workspace", vec![]),
        spec("file.new-folder", "New Folder", "Workspace", vec![]),
        spec("file.rename", "Rename File", "Workspace", vec![]),
        spec("file.delete", "Delete File", "Workspace", vec![]),
        spec(
            "workspace.refresh-files",
            "Refresh File Panel",
            "Workspace",
            vec![],
        ),
        spec(
            "workspace.collapse-files",
            "Collapse File Folders",
            "Workspace",
            vec![],
        ),
        spec(
            "workspace.split-right",
            "Split Right",
            "Workspace",
            vec![bind(
                Scope::Workspace,
                Chord::ctrl('\\'),
                CommandId("workspace.split-right"),
            )],
        ),
        spec("workspace.split-down", "Split Down", "Workspace", vec![]),
        spec("workspace.close-pane", "Close Pane", "Workspace", vec![]),
        spec(
            "workspace.toggle-files",
            "Toggle File Panel",
            "Workspace",
            vec![bind(
                Scope::Workspace,
                Chord::ctrl('b'),
                CommandId("workspace.toggle-files"),
            )],
        ),
        spec(
            "workspace.toggle-tracker",
            "Toggle Study Tracker",
            "Workspace",
            vec![bind(
                Scope::Workspace,
                Chord::new(Mods::CTRL_SHIFT, Key::Char('t')),
                CommandId("workspace.toggle-tracker"),
            )],
        ),
        spec(
            "workspace.close-tab",
            "Close Tab",
            "Workspace",
            vec![bind(
                Scope::Workspace,
                Chord::ctrl('w'),
                CommandId("workspace.close-tab"),
            )],
        ),
        spec(
            "workspace.force-close-tab",
            "Force Close Tab",
            "Workspace",
            vec![],
        ),
        spec(
            "workspace.next-tab",
            "Next Tab",
            "Workspace",
            vec![bind(
                Scope::Workspace,
                Chord::new(Mods::CTRL, Key::Tab),
                CommandId("workspace.next-tab"),
            )],
        ),
        spec(
            "search.global",
            "Search Vault",
            "Workspace",
            vec![bind(
                Scope::Workspace,
                Chord::new(Mods::CTRL_SHIFT, Key::Char('f')),
                CommandId("search.global"),
            )],
        ),
        spec(
            "help.shortcuts",
            "Keyboard Shortcuts",
            "Help",
            vec![bind(
                Scope::Workspace,
                Chord::ctrl('/'),
                CommandId("help.shortcuts"),
            )],
        ),
        // -- markdown editor --------------------------------------------------
        spec(
            "editor.undo",
            "Undo",
            "Editor",
            vec![bind(md, Chord::ctrl('z'), CommandId("editor.undo"))],
        ),
        spec(
            "editor.redo",
            "Redo",
            "Editor",
            vec![bind(
                md,
                Chord::new(Mods::CTRL_SHIFT, Key::Char('z')),
                CommandId("editor.redo"),
            )],
        ),
        spec(
            "editor.save",
            "Save",
            "Editor",
            vec![bind(md, Chord::ctrl('s'), CommandId("editor.save"))],
        ),
        spec(
            "editor.find",
            "Find in Note",
            "Editor",
            vec![bind(md, Chord::ctrl('f'), CommandId("editor.find"))],
        ),
        spec(
            "editor.select-all",
            "Select All",
            "Editor",
            vec![bind(md, Chord::ctrl('a'), CommandId("editor.select-all"))],
        ),
        spec(
            "editor.copy",
            "Copy",
            "Editor",
            vec![bind(md, Chord::ctrl('c'), CommandId("editor.copy"))],
        ),
        spec(
            "editor.cut",
            "Cut",
            "Editor",
            vec![bind(md, Chord::ctrl('x'), CommandId("editor.cut"))],
        ),
        spec(
            "note.backlinks",
            "Backlinks",
            "Editor",
            vec![bind(
                md,
                Chord::new(Mods::CTRL_SHIFT, Key::Char('b')),
                CommandId("note.backlinks"),
            )],
        ),
        spec("note.outline-panel", "Outline Panel", "Editor", vec![]),
        spec("editor.toggle-bold", "Bold", "Editor", vec![]),
        spec("editor.toggle-italic", "Italic", "Editor", vec![]),
        spec("editor.toggle-code", "Inline Code", "Editor", vec![]),
        spec("editor.heading-cycle", "Heading Cycle", "Editor", vec![]),
        spec(
            "editor.heading-1",
            "Heading 1",
            "Editor",
            vec![bind(
                md,
                Chord::new(Mods::CTRL, Key::Char('1')),
                CommandId("editor.heading-1"),
            )],
        ),
        spec(
            "editor.heading-2",
            "Heading 2",
            "Editor",
            vec![bind(
                md,
                Chord::new(Mods::CTRL, Key::Char('2')),
                CommandId("editor.heading-2"),
            )],
        ),
        spec(
            "editor.heading-3",
            "Heading 3",
            "Editor",
            vec![bind(
                md,
                Chord::new(Mods::CTRL, Key::Char('3')),
                CommandId("editor.heading-3"),
            )],
        ),
        spec(
            "editor.heading-4",
            "Heading 4",
            "Editor",
            vec![bind(
                md,
                Chord::new(Mods::CTRL, Key::Char('4')),
                CommandId("editor.heading-4"),
            )],
        ),
        spec(
            "editor.heading-5",
            "Heading 5",
            "Editor",
            vec![bind(
                md,
                Chord::new(Mods::CTRL, Key::Char('5')),
                CommandId("editor.heading-5"),
            )],
        ),
        spec(
            "editor.heading-6",
            "Heading 6",
            "Editor",
            vec![bind(
                md,
                Chord::new(Mods::CTRL, Key::Char('6')),
                CommandId("editor.heading-6"),
            )],
        ),
        spec(
            "editor.toggle-bullet",
            "Toggle Bullet List",
            "Editor",
            vec![],
        ),
        spec(
            "editor.toggle-checkbox",
            "Toggle Checkbox",
            "Editor",
            vec![bind(
                md,
                Chord::new(Mods::CTRL, Key::Enter),
                CommandId("editor.toggle-checkbox"),
            )],
        ),
        spec(
            "editor.toggle-wikilink",
            "Toggle Wikilink",
            "Editor",
            vec![],
        ),
        // -- pdf editor -------------------------------------------------------
        // The other half of v2's BUG-A pair: same chord, different scope.
        spec(
            "pdf.zoom-input",
            "Set Zoom Level",
            "PDF",
            vec![bind(pdf, Chord::ctrl('z'), CommandId("pdf.zoom-input"))],
        ),
        spec(
            "pdf.zoom-in",
            "Zoom In",
            "PDF",
            vec![bind(pdf, Chord::ctrl('='), CommandId("pdf.zoom-in"))],
        ),
        spec(
            "pdf.zoom-out",
            "Zoom Out",
            "PDF",
            vec![bind(pdf, Chord::ctrl('-'), CommandId("pdf.zoom-out"))],
        ),
        spec(
            "pdf.go-to-page",
            "Go to Page",
            "PDF",
            vec![bind(pdf, Chord::ctrl('g'), CommandId("pdf.go-to-page"))],
        ),
        spec(
            "pdf.find",
            "Find in PDF",
            "PDF",
            vec![bind(pdf, Chord::ctrl('f'), CommandId("pdf.find"))],
        ),
        spec(
            "pdf.toc",
            "Table of Contents",
            "PDF",
            vec![bind(pdf, Chord::ctrl('t'), CommandId("pdf.toc"))],
        ),
        spec(
            "pdf.highlight",
            "Highlight Selection",
            "PDF",
            vec![bind(pdf, Chord::ctrl('h'), CommandId("pdf.highlight"))],
        ),
        spec(
            "pdf.annotation-note",
            "Edit Annotation Note",
            "PDF",
            vec![bind(
                pdf,
                Chord::ctrl('n'),
                CommandId("pdf.annotation-note"),
            )],
        ),
        spec("pdf.previous-page", "Previous Page", "PDF", vec![]),
        spec("pdf.next-page", "Next Page", "PDF", vec![]),
        spec(
            "pdf.back",
            "Back (Jump History)",
            "PDF",
            vec![bind(
                pdf,
                Chord::new(Mods::ALT, Key::Left),
                CommandId("pdf.back"),
            )],
        ),
        spec(
            "pdf.forward",
            "Forward (Jump History)",
            "PDF",
            vec![bind(
                pdf,
                Chord::new(Mods::ALT, Key::Right),
                CommandId("pdf.forward"),
            )],
        ),
        spec(
            "pdf.copy-selection",
            "Copy Selection",
            "PDF",
            vec![bind(pdf, Chord::ctrl('c'), CommandId("pdf.copy-selection"))],
        ),
        // Palette-only: no chord, reachable via ctrl+shift+p.
        spec(
            "pdf.annotations-export",
            "Export Annotations (Markdown)",
            "PDF",
            vec![],
        ),
        spec(
            "pdf.highlight-color",
            "Cycle Highlight Color",
            "PDF",
            vec![],
        ),
        spec(
            "pdf.annotation-link-note",
            "Open Linked Note for Highlight",
            "PDF",
            vec![],
        ),
        spec(
            "pdf.annotations-orphans",
            "Orphaned Annotations Report",
            "PDF",
            vec![],
        ),
        spec("pdf.fit-width", "Fit Width", "PDF", vec![]),
        spec("pdf.fit-page", "Fit Page", "PDF", vec![]),
        spec("pdf.toc-panel", "TOC Panel", "PDF", vec![]),
        spec("pdf.annotations-panel", "Annotations Panel", "PDF", vec![]),
        spec("pdf.highlight-and-note", "Highlight + Note", "PDF", vec![]),
        // -- overlays ---------------------------------------------------------
        spec(
            "overlay.close",
            "Dismiss Overlay",
            "Overlay",
            vec![bind(
                Scope::Overlay,
                Chord::new(Mods::NONE, Key::Escape),
                CommandId("overlay.close"),
            )],
        ),
        spec(
            "overlay.confirm",
            "Confirm",
            "Overlay",
            vec![bind(
                Scope::Overlay,
                Chord::new(Mods::NONE, Key::Enter),
                CommandId("overlay.confirm"),
            )],
        ),
    ];

    for s in table {
        reg.register(s)?;
    }
    Ok(reg)
}

fn spec(
    id: &'static str,
    title: &'static str,
    category: &'static str,
    bindings: Vec<Binding>,
) -> CommandSpec {
    CommandSpec {
        id: CommandId(id),
        title,
        category,
        bindings,
    }
}

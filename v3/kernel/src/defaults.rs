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
            "note.backlinks",
            "Backlinks",
            "Editor",
            vec![bind(
                md,
                Chord::new(Mods::CTRL_SHIFT, Key::Char('b')),
                CommandId("note.backlinks"),
            )],
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

//! Headless CLI modes — what CI runs. Real guarantees live here:
//!
//! - `dump_shortcuts` prints the shortcuts table *generated from the command
//!   registry* — the single source of truth; docs/SHORTCUTS.md is produced
//!   by this, never edited by hand.
//! - `palette` exercises the registry-backed palette query.
//! - `demo` walks the BUG-A/BUG-C scenario end to end on the real kernel.

use std::process::ExitCode;

use md_kernel::input::{Chord, EditorKind};
use md_kernel::{CommandRegistry, Keymap, SplitAxis, Workspace};

pub fn dump_shortcuts(registry: &CommandRegistry) {
    println!("# Keyboard Shortcuts");
    println!();
    println!("Generated from the command registry by `md-editor --dump-shortcuts`.");
    println!("Do not edit by hand — change `kernel/src/defaults.rs` instead.");
    println!();
    println!("| Command | Title | Category | Scope | Chord |");
    println!("|---|---|---|---|---|");
    for spec in registry.specs() {
        if spec.bindings.is_empty() {
            println!(
                "| `{}` | {} | {} | — | — |",
                spec.id, spec.title, spec.category
            );
        }
        for b in &spec.bindings {
            println!(
                "| `{}` | {} | {} | {:?} | `{}` |",
                spec.id, spec.title, spec.category, b.scope, b.chord
            );
        }
    }
}

pub fn palette(registry: &CommandRegistry, query: &str) {
    for spec in registry.palette(query) {
        println!("{:<28} {} ({})", spec.id.0, spec.title, spec.category);
    }
}

/// Walk the regression scenarios on the live kernel; non-zero exit if any
/// expectation fails. A smoke check, not a substitute for the test suites.
pub fn demo(keymap: &Keymap) -> ExitCode {
    let mut ws = Workspace::new();
    let steps: Vec<(&str, bool)> = vec![
        (
            "open note standalone",
            ws.open("notes/research.md", EditorKind::Markdown).is_ok(),
        ),
        // BUG-C: PDF as a sibling tab, no split.
        (
            "open pdf as tab, still one pane",
            ws.open("papers/paper.pdf", EditorKind::Pdf).is_ok() && ws.panes.pane_count() == 1,
        ),
        // BUG-A: ctrl+z is zoom-input while the PDF is focused…
        (
            "ctrl+z → pdf.zoom-input",
            ws.handle_key(keymap, Chord::ctrl('z')).map(|c| c.0) == Some("pdf.zoom-input"),
        ),
        // …and explicit split is available as a layout choice.
        (
            "explicit split",
            ws.open_in_new_split(
                "notes/other.md",
                EditorKind::Markdown,
                SplitAxis::Horizontal,
            )
            .is_ok()
                && ws.panes.pane_count() == 2,
        ),
        (
            "ctrl+z → editor.undo after focus switch",
            ws.handle_key(keymap, Chord::ctrl('z')).map(|c| c.0) == Some("editor.undo"),
        ),
    ];
    let mut ok = true;
    for (name, passed) in steps {
        println!("{} {name}", if passed { "ok " } else { "FAIL" });
        ok &= passed;
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

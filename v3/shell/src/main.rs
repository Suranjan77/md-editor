//! md3 shell. Startup builds the default registry and keymap; a binding
//! conflict makes the process exit non-zero (plan §3.1: conflicts detected
//! at startup). Modes:
//!
//! - default / `<vault-dir>`: the iced GUI (ADR-0100) over the given vault
//!   (current directory if omitted).
//! - `--dump-shortcuts` prints the shortcuts table *generated from the
//!   command registry* — the single source of truth; docs/V3_SHORTCUTS.md is
//!   produced by this, never edited by hand.
//! - `--palette <query>` exercises the registry-backed palette.
//! - `--demo` walks the BUG-A/BUG-C scenario end to end on the real kernel,
//!   headless (used by CI).

mod gui;

use std::process::ExitCode;

use md3_kernel::defaults::default_registry;
use md3_kernel::input::{Chord, EditorKind};
use md3_kernel::{CommandRegistry, Keymap, SplitAxis, Workspace};

fn main() -> ExitCode {
    let registry = match default_registry() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("md3: command registry is invalid: {e}");
            return ExitCode::FAILURE;
        }
    };
    let keymap = match registry.keymap() {
        Ok(k) => k,
        Err(e) => {
            eprintln!("md3: keymap conflict: {e}");
            return ExitCode::FAILURE;
        }
    };

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--dump-shortcuts") => dump_shortcuts(&registry),
        Some("--palette") => palette(&registry, args.get(1).map(String::as_str).unwrap_or("")),
        Some("--demo") => return demo(&keymap),
        Some("--help") | Some("-h") => {
            println!("usage: md3-shell [<vault-dir> | --dump-shortcuts | --palette <query> | --demo]");
        }
        first => {
            let root = std::path::PathBuf::from(first.unwrap_or("."));
            let root = match root.canonicalize() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("md3: vault {}: {e}", root.display());
                    return ExitCode::FAILURE;
                }
            };
            if let Err(e) = gui::run(registry, keymap, root) {
                eprintln!("md3: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}

fn dump_shortcuts(registry: &CommandRegistry) {
    println!("# Shortcuts (v3)");
    println!();
    println!("Generated from the command registry by `md3-shell --dump-shortcuts`.");
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

fn palette(registry: &CommandRegistry, query: &str) {
    for spec in registry.palette(query) {
        println!("{:<28} {} ({})", spec.id.0, spec.title, spec.category);
    }
}

/// Walk the v2 bug scenarios on the live kernel; non-zero exit if any
/// expectation fails. A smoke check, not a substitute for the test suites.
fn demo(keymap: &Keymap) -> ExitCode {
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

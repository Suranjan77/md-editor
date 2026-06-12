//! Annotation niceties (impl plan P5.3) at the shell layer: copy-selection,
//! highlight color cycling, linked notes, and the orphaned-annotations
//! report — all driven windowlessly. None of these need pdfium: identity
//! hashing is byte-level and the selection/pick state is injectable
//! (`PdfSession` fields are the documented test seam).

use std::path::Path;

use md3_kernel::defaults::default_registry;
use md3_kernel::input::{Chord, Key, Mods};
use md3_shell::gui::keys::KeyEvent;
use md3_shell::gui::overlay::Overlay;
use md3_shell::gui::session::PdfSelection;
use md3_shell::gui::{Message, Shell};
use tempfile::TempDir;

fn chord(s: &str) -> Chord {
    match Chord::parse(s) {
        Ok(c) => c,
        Err(e) => panic!("bad chord `{s}`: {e}"),
    }
}

fn press(shell: &mut Shell, s: &str) {
    let _ = shell.update(Message::Key(KeyEvent {
        chord: Some(chord(s)),
        text: None,
    }));
}

fn type_text(shell: &mut Shell, text: &str) {
    for c in text.chars() {
        let _ = shell.update(Message::Key(KeyEvent {
            chord: Some(Chord::new(Mods::NONE, Key::Char(c.to_ascii_lowercase()))),
            text: Some(c.to_string()),
        }));
    }
}

fn new_shell(root: &Path) -> Shell {
    let registry = match default_registry() {
        Ok(r) => r,
        Err(e) => panic!("registry: {e}"),
    };
    let keymap = match registry.keymap() {
        Ok(k) => k,
        Err(e) => panic!("keymap: {e}"),
    };
    Shell::new(registry, keymap, root.to_path_buf())
}

fn open_via_quick_open(shell: &mut Shell, name: &str) {
    press(shell, "ctrl+p");
    type_text(shell, name);
    press(shell, "enter");
}

fn run_via_palette(shell: &mut Shell, command_title: &str) {
    press(shell, "ctrl+shift+p");
    type_text(shell, command_title);
    press(shell, "enter");
}

fn vault_with_pdf() -> TempDir {
    let dir = match TempDir::new() {
        Ok(d) => d,
        Err(e) => panic!("tempdir: {e}"),
    };
    if let Err(e) = std::fs::write(
        dir.path().join("paper.pdf"),
        b"%PDF-not-really, but hashable bytes",
    ) {
        panic!("write fake pdf: {e}");
    }
    dir
}

/// Drop a selection into the focused PDF session (the canvas drag's end
/// state), then `ctrl+h` persists and auto-picks it.
fn highlight_injected_selection(shell: &mut Shell) {
    match shell.focused_pdf_session_mut() {
        Some(session) => {
            session.selection = Some(PdfSelection {
                page: 0,
                anchor: (10.0, 10.0),
                quads: vec![md3_pdf::SelRect {
                    x0: 10.0,
                    y0: 10.0,
                    x1: 90.0,
                    y1: 22.0,
                }],
                text: "hello world".to_string(),
            });
        }
        None => panic!("no focused pdf session"),
    }
    press(shell, "ctrl+h");
}

#[test]
fn copy_without_a_selection_is_a_friendly_message() {
    let dir = vault_with_pdf();
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");
    press(&mut shell, "ctrl+c");
    assert!(
        shell.status().contains("select text first"),
        "status: {}",
        shell.status()
    );
}

#[test]
fn copy_reports_the_copied_length() {
    let dir = vault_with_pdf();
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");
    match shell.focused_pdf_session_mut() {
        Some(session) => {
            session.selection = Some(PdfSelection {
                page: 0,
                anchor: (0.0, 0.0),
                quads: Vec::new(),
                text: "hello world".to_string(),
            });
        }
        None => panic!("no focused pdf session"),
    }
    press(&mut shell, "ctrl+c");
    assert_eq!(
        shell.status(),
        "11 chars copied",
        "the clipboard task carries the text; the status reports it"
    );
}

#[test]
fn highlight_color_cycles_through_the_palette_and_persists() {
    let dir = vault_with_pdf();
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");
    highlight_injected_selection(&mut shell);

    let color_at = |shell: &Shell| match shell.focused_pdf() {
        Some(s) => s.annotations[0].color.clone(),
        None => panic!("pdf session lost"),
    };
    assert_eq!(color_at(&shell), "#ffd866", "new highlights use entry 0");

    run_via_palette(&mut shell, "cycle highlight");
    assert_eq!(color_at(&shell), "#a9dc76", "stepped to entry 1");
    run_via_palette(&mut shell, "cycle highlight");
    assert_eq!(color_at(&shell), "#78dce8", "stepped to entry 2");
}

#[test]
fn linked_note_is_created_recorded_and_opened() {
    let dir = vault_with_pdf();
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");
    highlight_injected_selection(&mut shell);

    run_via_palette(&mut shell, "linked note");
    assert!(
        dir.path().join("paper-notes.md").exists(),
        "first use creates the sibling note"
    );
    let focused_note = match shell.focused_md() {
        Some(s) => s.rel_path.clone(),
        None => panic!("the linked note is not the focused document"),
    };
    assert_eq!(
        focused_note, "paper-notes.md",
        "the note opened and focused"
    );
    assert!(
        shell.status().contains("linked note"),
        "status: {}",
        shell.status()
    );

    // The annotation remembers its note across a reopen of the PDF tab.
    open_via_quick_open(&mut shell, "paper.pdf");
    let linked = match shell.focused_pdf() {
        Some(s) => s.annotations[0].linked_note.clone(),
        None => panic!("pdf session lost"),
    };
    assert_eq!(linked.as_deref(), Some("paper-notes.md"));
}

#[test]
fn orphan_report_lists_documents_whose_bytes_changed() {
    let dir = vault_with_pdf();
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");
    highlight_injected_selection(&mut shell);

    // Nothing orphaned while the bytes still match.
    run_via_palette(&mut shell, "orphaned annotations");
    assert!(shell.overlay().is_none());
    assert!(
        shell.status().contains("no orphaned annotations"),
        "status: {}",
        shell.status()
    );

    // Edit the PDF bytes: new identity, old annotations orphaned.
    if let Err(e) = std::fs::write(dir.path().join("paper.pdf"), b"%PDF-different bytes now") {
        panic!("rewrite pdf: {e}");
    }
    run_via_palette(&mut shell, "orphaned annotations");
    match shell.overlay() {
        Some(Overlay::OrphanReport { rows }) => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].0, "paper.pdf", "last seen path names the doc");
            assert_eq!(rows[0].1, "1 annotations");
        }
        other => panic!("expected the orphan report, got {other:?}"),
    }
    press(&mut shell, "escape");
    assert!(shell.overlay().is_none(), "esc dismisses the report");
}

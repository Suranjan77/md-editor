//! The shared overlay hit list (quick-open / palette / search / pdf-find /
//! toc): the list is no longer truncated to a 12-row window — every row is
//! rendered (the card scrolls) and the selection is clamped to the rows
//! actually displayed, so the row enter picks is always a row on screen.
//! (User-reported: the TOC overlay could not be scrolled, and ↓ kept
//! "selecting" rows the view never showed.)

use std::path::Path;

use md3_kernel::defaults::default_registry;
use md3_kernel::input::{Chord, Key, Mods};
use md3_shell::gui::keys::KeyEvent;
use md3_shell::gui::overlay::{self, Overlay};
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

/// A vault with `n` notes named `note-00.md` … (scan order == sorted order).
fn vault(n: usize) -> TempDir {
    let dir = match TempDir::new() {
        Ok(d) => d,
        Err(e) => panic!("tempdir: {e}"),
    };
    for i in 0..n {
        let path = dir.path().join(format!("note-{i:02}.md"));
        if let Err(e) = std::fs::write(&path, format!("# note {i}\n")) {
            panic!("write note: {e}");
        }
    }
    dir
}

fn selected_of(shell: &Shell) -> usize {
    match shell.overlay() {
        Some(
            Overlay::QuickOpen { selected, .. }
            | Overlay::Palette { selected, .. }
            | Overlay::Search { selected, .. }
            | Overlay::PdfFind { selected, .. }
            | Overlay::PdfToc { selected, .. },
        ) => *selected,
        other => panic!("expected a list overlay, got {other:?}"),
    }
}

#[test]
fn quick_open_lists_every_match_not_a_truncated_window() {
    let dir = vault(30);
    let mut shell = new_shell(dir.path());
    press(&mut shell, "ctrl+p");

    let registry = match default_registry() {
        Ok(r) => r,
        Err(e) => panic!("registry: {e}"),
    };
    let files: Vec<String> = (0..30).map(|i| format!("note-{i:02}.md")).collect();
    let overlay = match shell.overlay() {
        Some(ov) => ov,
        None => panic!("quick-open did not open"),
    };
    let rows = overlay::list_rows(overlay, &registry, &files);
    assert_eq!(rows.len(), 30, "all matches are rendered, not the first 12");
}

#[test]
fn down_clamps_to_the_last_displayed_row() {
    let dir = vault(5);
    let mut shell = new_shell(dir.path());
    press(&mut shell, "ctrl+p");
    for _ in 0..40 {
        press(&mut shell, "down");
    }
    assert_eq!(
        selected_of(&shell),
        4,
        "selection stops at the last row the list shows"
    );
    // Narrowing the query re-clamps: `note-03` leaves a single match.
    type_text(&mut shell, "note-03");
    assert_eq!(selected_of(&shell), 0, "shrunken list pulls selection back");
}

#[test]
fn enter_picks_a_row_beyond_the_old_12_row_cap() {
    let dir = vault(20);
    let mut shell = new_shell(dir.path());
    press(&mut shell, "ctrl+p");
    for _ in 0..15 {
        press(&mut shell, "down");
    }
    press(&mut shell, "enter");
    assert!(shell.overlay().is_none(), "confirm closes the overlay");
    assert!(
        shell.status().starts_with("note-15.md"),
        "the 16th row opened (status: {})",
        shell.status()
    );
}

#[test]
fn palette_selection_clamps_to_the_command_count() {
    let dir = vault(1);
    let mut shell = new_shell(dir.path());
    press(&mut shell, "ctrl+shift+p");
    let registry = match default_registry() {
        Ok(r) => r,
        Err(e) => panic!("registry: {e}"),
    };
    let total = registry.palette("").len();
    for _ in 0..total + 20 {
        press(&mut shell, "down");
    }
    assert_eq!(
        selected_of(&shell),
        total - 1,
        "selection never leaves the displayed command list"
    );
}

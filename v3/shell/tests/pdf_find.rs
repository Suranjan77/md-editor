//! `pdf.find` at the shell layer (plan §3.3 "PDF search"): the overlay
//! routes through the kernel like every other surface, matches recompute
//! live over cached glyph geometry, and confirming a hit scrolls to it and
//! plants it as the live selection (so `ctrl+h` chains).
//!
//! The guard-rail tests run everywhere; the end-to-end match/jump flow
//! needs real glyphs and is feature-gated like the reading suite.

use std::path::Path;

use md3_kernel::defaults::default_registry;
use md3_kernel::input::{Chord, Key, Mods};
use md3_shell::gui::keys::KeyEvent;
#[cfg(feature = "pdfium")]
use md3_shell::gui::overlay::Overlay;
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

/// A vault containing `paper.pdf` (the real multipage fixture when asked)
/// and a markdown note.
fn vault(real_fixture: bool) -> TempDir {
    let dir = match TempDir::new() {
        Ok(d) => d,
        Err(e) => panic!("tempdir: {e}"),
    };
    let target = dir.path().join("paper.pdf");
    if real_fixture {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests-fixtures/pdf/multipage-outline.pdf");
        if let Err(e) = std::fs::copy(&fixture, &target) {
            panic!("copy fixture: {e}");
        }
    } else if let Err(e) = std::fs::write(&target, b"%PDF-not-really") {
        panic!("write fake pdf: {e}");
    }
    if let Err(e) = std::fs::write(dir.path().join("note.md"), "a note") {
        panic!("write note: {e}");
    }
    dir
}

// ------------------------------------------------------------ always-on --

#[test]
fn pdf_find_without_a_pdf_focus_is_a_friendly_message() {
    let dir = vault(false);
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "note.md");
    // ctrl+f in markdown scope is editor.find, so reach pdf.find the way a
    // user without a PDF would: through the command palette.
    press(&mut shell, "ctrl+shift+p");
    type_text(&mut shell, "find in pdf");
    press(&mut shell, "enter");
    assert!(shell.overlay().is_none(), "no overlay without a pdf");
    assert!(
        shell.status().contains("no pdf focused"),
        "status: {}",
        shell.status()
    );
}

#[test]
fn pdf_find_on_an_unrenderable_pdf_does_not_open_the_overlay() {
    let dir = vault(false);
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");
    press(&mut shell, "ctrl+f");
    assert!(shell.overlay().is_none(), "no overlay without pages");
    assert!(
        shell.status().contains("find:"),
        "status explains: {}",
        shell.status()
    );
}

// -------------------------------------------------------- pdfium-gated --

#[cfg(feature = "pdfium")]
#[test]
fn typing_in_pdf_find_lists_hits_and_enter_jumps_to_the_match() {
    let dir = vault(true);
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");
    if shell.focused_pdf().is_none_or(|s| s.layout.is_none()) {
        eprintln!("skipping: libpdfium not available");
        return;
    }

    press(&mut shell, "ctrl+f");
    assert!(
        matches!(shell.overlay(), Some(Overlay::PdfFind { .. })),
        "ctrl+f in pdf scope opens the find overlay"
    );

    // Derive a needle from the last page's real glyphs so the test tracks
    // the fixture: its first four characters, queried in the wrong case to
    // pin case-insensitivity.
    let session = match shell.focused_pdf() {
        Some(s) => s,
        None => panic!("pdf session lost"),
    };
    let last_page = match session.chars.keys().max().copied() {
        Some(p) if p > 0 => p,
        _ => panic!("fixture should yield glyphs on more than one page"),
    };
    let needle: String = session.chars[&last_page]
        .iter()
        .take(4)
        .map(|c| c.ch.to_ascii_uppercase())
        .collect();
    type_text(&mut shell, &needle);

    let (hit_page, hit_count) = match shell.overlay() {
        Some(Overlay::PdfFind { hits, .. }) if !hits.is_empty() => {
            // The needle exists on the last page by construction; it may
            // match earlier pages too. Walk the selection to it.
            let i = hits.iter().position(|h| h.page == last_page);
            let i = match i {
                Some(i) => i,
                None => panic!("no hit on page {last_page}"),
            };
            for _ in 0..i {
                press(&mut shell, "down");
            }
            (last_page, hits_len(&shell))
        }
        other => panic!("expected live hits, got {other:?}"),
    };
    assert!(hit_count >= 1);

    press(&mut shell, "enter");
    assert!(shell.overlay().is_none(), "confirm closes the overlay");
    let session = match shell.focused_pdf() {
        Some(s) => s,
        None => panic!("pdf session lost"),
    };
    let sel = match &session.selection {
        Some(s) => s,
        None => panic!("jump plants the match as the live selection"),
    };
    assert_eq!(sel.page, hit_page);
    assert!(!sel.quads.is_empty(), "match has paintable quads");
    assert!(session.scroll > 0.0, "scrolled down to a later page");
    assert!(
        shell.status().contains("match on p."),
        "status: {}",
        shell.status()
    );
}

#[cfg(feature = "pdfium")]
fn hits_len(shell: &Shell) -> usize {
    match shell.overlay() {
        Some(Overlay::PdfFind { hits, .. }) => hits.len(),
        _ => 0,
    }
}

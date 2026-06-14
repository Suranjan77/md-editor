//! PDF reading UX at the shell layer (plan §3.3 M2): continuous scroll over
//! a real fixture, driven keyboard-only through `Shell::update` — paging
//! keys scroll the strip, go-to-page and zoom go through their overlays,
//! tiles actually render. Runs only with the `pdfium` feature; skips (not
//! fails) when libpdfium isn't bound, like the engine's own suite.
#![cfg(feature = "pdfium")]

use std::path::Path;

use md_kernel::defaults::default_registry;
use md_kernel::input::{Chord, Key, Mods};
use md_shell::gui::keys::KeyEvent;
use md_shell::gui::{Message, Shell};
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

/// Vault with the multipage fixture copied in; the shell has it open and
/// focused. Returns `None` (skip) when pdfium isn't available.
fn shell_with_fixture() -> Option<(TempDir, Shell)> {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests-fixtures/pdf/multipage-outline.pdf");
    let dir = match TempDir::new() {
        Ok(d) => d,
        Err(e) => panic!("tempdir: {e}"),
    };
    let target = dir.path().join("paper.pdf");
    if let Err(e) = std::fs::copy(&fixture, &target) {
        panic!("copy fixture: {e}");
    }
    let registry = match default_registry() {
        Ok(r) => r,
        Err(e) => panic!("registry: {e}"),
    };
    let keymap = match registry.keymap() {
        Ok(k) => k,
        Err(e) => panic!("keymap: {e}"),
    };
    let mut shell = Shell::new(registry, keymap, dir.path().to_path_buf());
    press(&mut shell, "ctrl+p");
    type_text(&mut shell, "paper.pdf");
    press(&mut shell, "enter");

    let loaded = shell.focused_pdf().is_some_and(|s| s.layout.is_some());
    if !loaded {
        eprintln!("skipping: libpdfium not available");
        return None;
    }
    Some((dir, shell))
}

fn pdf(shell: &Shell) -> &md_shell::gui::session::PdfSession {
    match shell.focused_pdf() {
        Some(s) => s,
        None => panic!("no focused pdf session"),
    }
}

#[test]
fn opening_a_pdf_loads_geometry_and_renders_visible_tiles() {
    let Some((_dir, shell)) = shell_with_fixture() else {
        return;
    };
    let session = pdf(&shell);
    assert!(session.page_count() > 1, "fixture is multipage");
    assert_eq!(session.current_page(), 0);
    assert!(!session.tiles.is_empty(), "visible tiles rendered on open");
    assert!(session.cache.used_bytes() > 0);
    let status = shell.status();
    assert!(
        status.contains("p. 1/") && status.contains("100%"),
        "page pill in status: {status}"
    );
}

#[test]
fn paging_keys_scroll_the_continuous_strip() {
    let Some((_dir, mut shell)) = shell_with_fixture() else {
        return;
    };
    press(&mut shell, "pagedown");
    assert!(pdf(&shell).scroll > 0.0, "pagedown scrolls");

    // End: bottom of the document; the pill shows the last page.
    press(&mut shell, "end");
    let session = pdf(&shell);
    let last = session.page_count();
    assert_eq!(session.current_page() + 1, last, "end lands on p. {last}");

    press(&mut shell, "home");
    let session = pdf(&shell);
    assert_eq!(session.scroll, 0.0, "home returns to the top");
    assert_eq!(session.current_page(), 0);
}

#[test]
fn wheel_scroll_message_moves_and_renders() {
    let Some((_dir, mut shell)) = shell_with_fixture() else {
        return;
    };
    let tab = match shell.workspace().focused_tab() {
        Some(t) => t,
        None => panic!("no focused tab"),
    };
    let _ = shell.update(Message::PdfScrolled {
        tab,
        dy: 500.0,
        viewport: (900.0, 700.0),
    });
    let session = pdf(&shell);
    assert!((session.scroll - 500.0).abs() < f32::EPSILON);
    assert_eq!(session.viewport, (900.0, 700.0));
    // Over-scroll clamps to the strip's end.
    let _ = shell.update(Message::PdfScrolled {
        tab,
        dy: f32::MAX,
        viewport: (900.0, 700.0),
    });
    let session = pdf(&shell);
    let layout = match &session.layout {
        Some(l) => l,
        None => panic!("layout lost"),
    };
    assert!((session.scroll - layout.max_scroll(700.0)).abs() < 1.0);
}

#[test]
fn go_to_page_overlay_jumps_and_zoom_overlay_re_anchors() {
    let Some((_dir, mut shell)) = shell_with_fixture() else {
        return;
    };
    press(&mut shell, "ctrl+g");
    type_text(&mut shell, "3");
    press(&mut shell, "enter");
    let session = pdf(&shell);
    assert_eq!(session.current_page(), 2, "go-to-page is 1-based");
    let anchor = session.current_page();

    press(&mut shell, "ctrl+z"); // pdf scope: zoom input (BUG-A pair)
    type_text(&mut shell, "150");
    press(&mut shell, "enter");
    let session = pdf(&shell);
    assert!((session.zoom - 1.5).abs() < f32::EPSILON);
    assert_eq!(
        session.current_page(),
        anchor,
        "zoom keeps the current page anchored"
    );
    assert!(
        session
            .tiles
            .keys()
            .any(|k| k.bucket == md_pdf::zoom_bucket(1.5)),
        "tiles rendered in the new zoom bucket"
    );
    let status = shell.status();
    assert!(status.contains("150%"), "zoom pill updated: {status}");
}

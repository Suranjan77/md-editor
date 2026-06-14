//! `pdf.toc` at the shell layer (plan §3.3 "TOC with section tracking"):
//! ctrl+t lists the outline as a filterable jump list, enter goes to the
//! section's page, and the status pill tracks the current section.

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
    dir
}

// ------------------------------------------------------------ always-on --

#[test]
fn toc_without_an_outline_is_a_friendly_message() {
    let dir = vault(false);
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");
    press(&mut shell, "ctrl+t");
    assert!(shell.overlay().is_none(), "no overlay without an outline");
    assert!(
        shell.status().contains("no table of contents"),
        "status: {}",
        shell.status()
    );
}

// -------------------------------------------------------- pdfium-gated --

#[cfg(feature = "pdfium")]
#[test]
fn toc_lists_sections_enter_jumps_and_the_pill_tracks_the_section() {
    let dir = vault(true);
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");
    if shell.focused_pdf().is_none_or(|s| s.layout.is_none()) {
        eprintln!("skipping: libpdfium not available");
        return;
    }
    let outline = match shell.focused_pdf() {
        Some(s) => s.outline.clone(),
        None => panic!("pdf session lost"),
    };
    assert!(!outline.is_empty(), "fixture has an outline");

    press(&mut shell, "ctrl+t");
    let entries = match shell.overlay() {
        Some(Overlay::PdfToc { entries, .. }) => entries.clone(),
        other => panic!("expected the toc overlay, got {other:?}"),
    };
    assert_eq!(entries.len(), outline.len());

    // Pick a section that is not on page 0, by walking the selection down.
    let target = match entries.iter().position(|(_, page)| *page > 0) {
        Some(i) => i,
        None => panic!("fixture outline has later-page sections"),
    };
    for _ in 0..target {
        press(&mut shell, "down");
    }
    press(&mut shell, "enter");
    assert!(shell.overlay().is_none(), "confirm closes the overlay");
    assert!(
        shell.status().starts_with("§ "),
        "status names the section: {}",
        shell.status()
    );

    let session = match shell.focused_pdf() {
        Some(s) => s,
        None => panic!("pdf session lost"),
    };
    assert_eq!(
        session.current_page() as u32,
        entries[target].1,
        "jumped to the section's page"
    );
    // Section tracking: the pill's source reports the section we're in.
    let section = match session.current_section() {
        Some(s) => s.to_string(),
        None => panic!("a page inside the outline has a current section"),
    };
    assert_eq!(
        format!("§ {section}"),
        shell.status().to_string(),
        "tracked section is the jumped-to section"
    );

    // The jump landed in the history: alt+left returns to where we were,
    // alt+right replays the jump (plan §3.3 back/forward).
    press(&mut shell, "alt+left");
    let page_after_back = match shell.focused_pdf() {
        Some(s) => s.current_page() as u32,
        None => panic!("pdf session lost"),
    };
    assert_eq!(page_after_back, 0, "back returns to the pre-jump position");
    press(&mut shell, "alt+right");
    let page_after_forward = match shell.focused_pdf() {
        Some(s) => s.current_page() as u32,
        None => panic!("pdf session lost"),
    };
    assert_eq!(page_after_forward, entries[target].1, "forward replays");
    // Exhausted history is a message, not a scroll.
    press(&mut shell, "alt+right");
    assert!(
        shell.status().contains("nothing to go forward"),
        "status: {}",
        shell.status()
    );
}

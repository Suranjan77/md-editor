//! PDF link clicks and right-clicks (preview popup) at the shell layer
//! (plan Phase 1).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use md_kernel::defaults::default_registry;
use md_kernel::input::{Chord, Key, Mods};
use md_shell::gui::keys::KeyEvent;
#[cfg(feature = "pdfium")]
use md_shell::gui::overlay::Overlay;
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

fn new_shell(root: &Path) -> Shell {
    unsafe {
        std::env::set_var("MD3_TEST_MODE", "1");
    }
    let registry = match default_registry() {
        Ok(r) => r,
        Err(e) => panic!("registry: {e}"),
    };
    let keymap = match registry.keymap() {
        Ok(k) => k,
        Err(e) => panic!("keymap: {e}"),
    };
    Shell::new(registry, keymap, root.to_path_buf()).0
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
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests-fixtures/pdf/internal-links.pdf");
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
fn right_click_on_nothing_is_inert() {
    let dir = vault(false);
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");

    let tab = shell.workspace().focused_tab().unwrap();
    // Send a right click. Since the PDF is a fake, there's no layout and no links.
    let _ = shell.update(Message::PdfRightClick {
        tab,
        pos: (100.0, 100.0),
        abs_pos: (100.0, 100.0),
        viewport: (800.0, 600.0),
    });

    assert!(
        shell.overlay().is_none(),
        "no overlay opens on right click on nothing"
    );
}

// -------------------------------------------------------- pdfium-gated --

#[cfg(feature = "pdfium")]
#[test]
fn right_click_on_an_internal_link_opens_the_preview_and_esc_closes() {
    let dir = vault(true);
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");

    let tab = shell.workspace().focused_tab().unwrap();
    let viewport = (800.0, 600.0);

    // Trigger link loading by sending a dummy MouseDown
    let _ = shell.update(Message::PdfMouseDown {
        tab,
        pos: (0.0, 0.0),
        viewport,
    });

    let session = shell.focused_pdf().unwrap();
    if session.layout.is_none() {
        eprintln!("skipping: libpdfium not available");
        return;
    }

    // Find the link targetting page 1 (second page).
    let links = session
        .links
        .get(&0)
        .expect("page 0 links should be loaded");
    let link = links
        .iter()
        .find(|l| {
            if let Some((dest_page, _)) = l.dest {
                dest_page == 1
            } else {
                false
            }
        })
        .expect("fixture has link targeting page 1");

    let layout = session.layout.as_ref().unwrap();
    let page = layout.placed_pages(session.scroll, viewport)[0];
    let zoom = layout.zoom();
    let center_pt = (
        page.x + (link.rect.x0 + link.rect.x1) / 2.0 * zoom,
        page.y + (link.rect.y0 + link.rect.y1) / 2.0 * zoom,
    );

    // Send right click
    let _ = shell.update(Message::PdfRightClick {
        tab,
        pos: center_pt,
        abs_pos: center_pt,
        viewport,
    });

    // Check overlay
    let overlay = shell.overlay().expect("Overlay should be open");
    assert!(
        matches!(overlay, Overlay::PdfLinkPreview { dest_page: 1, .. }),
        "Expected Overlay::PdfLinkPreview, got {:?}",
        overlay
    );

    // Escape closes overlay
    press(&mut shell, "escape");
    assert!(shell.overlay().is_none(), "escape closes overlay");
}

#[cfg(feature = "pdfium")]
#[test]
fn left_click_navigates_and_alt_left_returns() {
    let dir = vault(true);
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");

    let tab = shell.workspace().focused_tab().unwrap();
    let viewport = (800.0, 600.0);

    // Trigger link loading
    let _ = shell.update(Message::PdfMouseDown {
        tab,
        pos: (0.0, 0.0),
        viewport,
    });

    let session = shell.focused_pdf().unwrap();
    if session.layout.is_none() {
        eprintln!("skipping: libpdfium not available");
        return;
    }

    let links = session
        .links
        .get(&0)
        .expect("page 0 links should be loaded");
    let link = links
        .iter()
        .find(|l| {
            if let Some((dest_page, _)) = l.dest {
                dest_page == 1
            } else {
                false
            }
        })
        .expect("fixture has link targeting page 1");

    let layout = session.layout.as_ref().unwrap();
    let page = layout.placed_pages(session.scroll, viewport)[0];
    let zoom = layout.zoom();
    let center_pt = (
        page.x + (link.rect.x0 + link.rect.x1) / 2.0 * zoom,
        page.y + (link.rect.y0 + link.rect.y1) / 2.0 * zoom,
    );

    // Left click on link
    let _ = shell.update(Message::PdfMouseDown {
        tab,
        pos: center_pt,
        viewport,
    });

    assert_eq!(
        shell.focused_pdf().unwrap().current_page(),
        1,
        "left click on link should navigate to page 1"
    );

    // alt+left returns
    press(&mut shell, "alt+left");
    assert_eq!(
        shell.focused_pdf().unwrap().current_page(),
        0,
        "alt+left should return to page 0"
    );
}

#[cfg(feature = "pdfium")]
#[test]
fn enter_in_preview_navigates() {
    let dir = vault(true);
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");

    let tab = shell.workspace().focused_tab().unwrap();
    let viewport = (800.0, 600.0);

    // Trigger link loading
    let _ = shell.update(Message::PdfMouseDown {
        tab,
        pos: (0.0, 0.0),
        viewport,
    });

    let session = shell.focused_pdf().unwrap();
    if session.layout.is_none() {
        eprintln!("skipping: libpdfium not available");
        return;
    }

    let links = session
        .links
        .get(&0)
        .expect("page 0 links should be loaded");
    let link = links
        .iter()
        .find(|l| {
            if let Some((dest_page, _)) = l.dest {
                dest_page == 1
            } else {
                false
            }
        })
        .expect("fixture has link targeting page 1");

    let layout = session.layout.as_ref().unwrap();
    let page = layout.placed_pages(session.scroll, viewport)[0];
    let zoom = layout.zoom();
    let center_pt = (
        page.x + (link.rect.x0 + link.rect.x1) / 2.0 * zoom,
        page.y + (link.rect.y0 + link.rect.y1) / 2.0 * zoom,
    );

    // Right click to open preview
    let _ = shell.update(Message::PdfRightClick {
        tab,
        pos: center_pt,
        abs_pos: center_pt,
        viewport,
    });

    assert!(shell.overlay().is_some());

    // Press enter
    press(&mut shell, "enter");

    assert!(shell.overlay().is_none(), "overlay should close on confirm");
    assert_eq!(
        shell.focused_pdf().unwrap().current_page(),
        1,
        "confirming preview should navigate to page 1"
    );
}

#[cfg(feature = "pdfium")]
#[test]
fn left_click_on_uri_link_updates_status_line_and_does_not_panic() {
    let dir = vault(true);
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "paper.pdf");

    let tab = shell.workspace().focused_tab().unwrap();
    let viewport = (800.0, 600.0);

    // Trigger link loading
    let _ = shell.update(Message::PdfMouseDown {
        tab,
        pos: (0.0, 0.0),
        viewport,
    });

    let session = shell.focused_pdf().unwrap();
    if session.layout.is_none() {
        eprintln!("skipping: libpdfium not available");
        return;
    }

    let links = session
        .links
        .get(&0)
        .expect("page 0 links should be loaded");

    // Find the link targeting a URI
    let link = links
        .iter()
        .find(|l| l.uri.is_some())
        .expect("fixture has link targeting a URI");

    let layout = session.layout.as_ref().unwrap();
    let page = layout.placed_pages(session.scroll, viewport)[0];
    let zoom = layout.zoom();
    let center_pt = (
        page.x + (link.rect.x0 + link.rect.x1) / 2.0 * zoom,
        page.y + (link.rect.y0 + link.rect.y1) / 2.0 * zoom,
    );

    // Left click on link
    let _ = shell.update(Message::PdfMouseDown {
        tab,
        pos: center_pt,
        viewport,
    });

    assert_eq!(
        shell.status(),
        "link: https://example.com",
        "left click on URI link should update status to link: https://example.com"
    );
}

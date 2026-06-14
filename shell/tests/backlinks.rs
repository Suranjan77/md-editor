//! `note.backlinks` at the shell layer (plan §3.4 link graph UI):
//! ctrl+shift+b on a focused note lists its referrers from the wikilink
//! graph as a filterable jump list; enter opens the referrer.

use std::path::Path;

use md_kernel::defaults::default_registry;
use md_kernel::input::{Chord, Key, Mods};
use md_shell::gui::keys::KeyEvent;
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

/// hub.md is linked from alpha.md (plain) and beta.md (aliased); gamma.md
/// links elsewhere.
fn vault() -> TempDir {
    let dir = match TempDir::new() {
        Ok(d) => d,
        Err(e) => panic!("tempdir: {e}"),
    };
    let write = |name: &str, body: &str| {
        if let Err(e) = std::fs::write(dir.path().join(name), body) {
            panic!("write {name}: {e}");
        }
    };
    write("hub.md", "# hub\n");
    write("alpha.md", "see [[hub]]\n");
    write("beta.md", "also [[hub|the hub]] here\n");
    write("gamma.md", "unrelated [[alpha]]\n");
    dir
}

#[test]
fn backlinks_lists_referrers_and_enter_opens_one() {
    let dir = vault();
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "hub.md");

    press(&mut shell, "ctrl+shift+b");
    let referrers = match shell.overlay() {
        Some(Overlay::Backlinks { referrers, .. }) => referrers.clone(),
        other => panic!("expected the backlinks overlay, got {other:?}"),
    };
    assert_eq!(
        referrers,
        vec!["alpha.md".to_string(), "beta.md".to_string()],
        "plain and aliased links count; unrelated notes don't"
    );

    // Second row (beta.md), via the same down/enter path as every overlay.
    press(&mut shell, "down");
    press(&mut shell, "enter");
    assert!(shell.overlay().is_none(), "confirm closes the overlay");
    assert!(
        shell.status().starts_with("beta.md"),
        "the referrer opened (status: {})",
        shell.status()
    );
}

#[test]
fn backlinks_filter_narrows_and_confirm_picks_the_shown_row() {
    let dir = vault();
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "hub.md");

    press(&mut shell, "ctrl+shift+b");
    type_text(&mut shell, "alp");
    press(&mut shell, "enter");
    assert!(
        shell.status().starts_with("alpha.md"),
        "filtered row opened (status: {})",
        shell.status()
    );
}

#[test]
fn no_backlinks_is_a_status_message_not_an_overlay() {
    let dir = vault();
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "gamma.md");

    press(&mut shell, "ctrl+shift+b");
    assert!(shell.overlay().is_none(), "nothing to list");
    assert!(
        shell.status().contains("no backlinks"),
        "status: {}",
        shell.status()
    );
}

#[test]
fn backlinks_on_a_pdf_is_inert() {
    let dir = vault();
    if let Err(e) = std::fs::write(dir.path().join("doc.pdf"), b"%PDF-not-really") {
        panic!("write pdf: {e}");
    }
    let mut shell = new_shell(dir.path());
    open_via_quick_open(&mut shell, "doc.pdf");
    press(&mut shell, "ctrl+shift+b");
    assert!(
        shell.overlay().is_none(),
        "md-scoped chord cannot fire in pdf scope (BUG-A discipline)"
    );
}

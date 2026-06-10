//! BUG-A regression suite (M1 gate, plan §5).
//!
//! v2 symptom: Ctrl+Z opened the PDF "go to page/zoom" input instead of
//! undoing, because a global keyboard listener and a widget-internal binding
//! both claimed the chord with no arbitration.
//!
//! v3 contract: one keymap, scope-stack resolution derived from focus, modal
//! overlays fence off inner scopes, and conflicts fail statically — this file
//! IS the "conflict CI" from plan §3.1.

use md3_kernel::defaults::default_registry;
use md3_kernel::input::{Binding, Chord, EditorKind, Key, Mods, Scope};
use md3_kernel::{CommandId, Keymap, KeymapError, SplitAxis, Workspace};

fn setup() -> (Workspace, Keymap) {
    let reg = match default_registry() {
        Ok(r) => r,
        Err(e) => panic!("default registry must build: {e}"),
    };
    let keymap = match reg.keymap() {
        Ok(k) => k,
        Err(e) => panic!("default keymap must be conflict-free: {e}"),
    };
    (Workspace::new(), keymap)
}

fn open(ws: &mut Workspace, path: &str, kind: EditorKind) {
    if let Err(e) = ws.open(path, kind) {
        panic!("open {path}: {e}");
    }
}

#[test]
fn ctrl_z_undoes_in_a_markdown_editor() {
    let (mut ws, keymap) = setup();
    open(&mut ws, "notes/research.md", EditorKind::Markdown);
    assert_eq!(
        ws.handle_key(&keymap, Chord::ctrl('z')),
        Some(CommandId("editor.undo"))
    );
}

#[test]
fn ctrl_z_is_zoom_input_only_when_a_pdf_is_focused() {
    let (mut ws, keymap) = setup();
    open(&mut ws, "papers/paper.pdf", EditorKind::Pdf);
    assert_eq!(
        ws.handle_key(&keymap, Chord::ctrl('z')),
        Some(CommandId("pdf.zoom-input"))
    );
}

#[test]
fn focus_switch_in_a_split_flips_what_ctrl_z_means() {
    // The exact v2 failure scenario: markdown and PDF open side by side.
    let (mut ws, keymap) = setup();
    let md_tab = match ws.open("notes/research.md", EditorKind::Markdown) {
        Ok(t) => t,
        Err(e) => panic!("{e}"),
    };
    let pdf_tab =
        match ws.open_in_new_split("papers/paper.pdf", EditorKind::Pdf, SplitAxis::Horizontal) {
            Ok(t) => t,
            Err(e) => panic!("{e}"),
        };

    assert_eq!(ws.focused_tab(), Some(pdf_tab));
    assert_eq!(
        ws.handle_key(&keymap, Chord::ctrl('z')),
        Some(CommandId("pdf.zoom-input"))
    );

    if let Err(e) = ws.focus_tab(md_tab) {
        panic!("{e}");
    }
    assert_eq!(
        ws.handle_key(&keymap, Chord::ctrl('z')),
        Some(CommandId("editor.undo"))
    );
}

#[test]
fn modal_overlay_fences_editor_bindings() {
    // While a go-to-page overlay is open, Ctrl+Z must reach NEITHER the
    // editor nor the PDF surface — unbound chords under a modal go to the
    // overlay's text input as raw input (resolve = None).
    let (mut ws, keymap) = setup();
    open(&mut ws, "papers/paper.pdf", EditorKind::Pdf);
    ws.open_overlay("go-to-page");

    assert_eq!(ws.handle_key(&keymap, Chord::ctrl('z')), None);
    // Overlay-scope bindings work…
    assert_eq!(
        ws.handle_key(&keymap, Chord::new(Mods::NONE, Key::Escape)),
        Some(CommandId("overlay.close"))
    );
    // …and Global stays reachable even under a modal.
    assert_eq!(
        ws.handle_key(&keymap, Chord::ctrl('q')),
        Some(CommandId("app.quit"))
    );

    ws.close_overlay();
    assert_eq!(
        ws.handle_key(&keymap, Chord::ctrl('z')),
        Some(CommandId("pdf.zoom-input"))
    );
}

#[test]
fn unbound_chords_resolve_to_raw_input() {
    let (mut ws, keymap) = setup();
    open(&mut ws, "notes/research.md", EditorKind::Markdown);
    assert_eq!(
        ws.handle_key(&keymap, Chord::new(Mods::NONE, Key::Char('x'))),
        None
    );
}

#[test]
fn conflict_enumeration_default_keymap_has_no_conflicts() {
    // Plan §3.1: "conflicts are statically detected at startup and in CI
    // (test enumerates the keymap)". Building the keymap enumerates every
    // (scope, chord) pair and fails on any duplicate.
    let (_, keymap) = setup();
    assert!(!keymap.is_empty());
    // And the detector itself works: inject the v2 collision into ONE scope.
    let collision = Keymap::from_bindings([
        Binding::new(
            Scope::Editor(EditorKind::Markdown),
            Chord::ctrl('z'),
            CommandId("editor.undo"),
        ),
        Binding::new(
            Scope::Editor(EditorKind::Markdown),
            Chord::ctrl('z'),
            CommandId("pdf.zoom-input"),
        ),
    ]);
    assert!(matches!(collision, Err(KeymapError::Conflict { .. })));
}

#[test]
fn same_chord_in_sibling_editor_scopes_is_legal_by_design() {
    let (_, keymap) = setup();
    // Both rows exist in the same table without conflicting.
    let rows = keymap.bindings();
    let undo = rows
        .iter()
        .any(|b| b.command == CommandId("editor.undo") && b.chord == Chord::ctrl('z'));
    let zoom = rows
        .iter()
        .any(|b| b.command == CommandId("pdf.zoom-input") && b.chord == Chord::ctrl('z'));
    assert!(undo && zoom);
}

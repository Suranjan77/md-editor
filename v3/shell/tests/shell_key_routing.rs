//! Shell-level routing regression suite: drives the *real* iced update loop
//! (`App::update`) with keyboard messages only — exactly what the window
//! produces — and asserts on kernel state. This is BUG-A and BUG-C pinned at
//! the shell layer: a regression in the wiring (not just the kernel) fails
//! here, windowlessly.

use md3_kernel::input::{Chord, EditorKind, Key, Mods};
use md3_shell::app::{App, Message, OverlayUi};

fn chord(s: &str) -> Chord {
    match Chord::parse(s) {
        Ok(c) => c,
        Err(e) => panic!("bad chord `{s}`: {e}"),
    }
}

fn press(app: &mut App, s: &str) {
    let _ = app.update(Message::Key(chord(s)));
}

/// Type printable characters the way the subscription delivers them: one
/// unmodified chord per character.
fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        let key = if c == ' ' { Key::Space } else { Key::Char(c) };
        let _ = app.update(Message::Key(Chord::new(Mods::NONE, key)));
    }
}

/// Boot state: notes/welcome.md open and focused in a single pane.
fn app() -> App {
    let (app, _task) = App::new();
    assert_eq!(
        app.workspace().focused_editor_kind(),
        Some(EditorKind::Markdown)
    );
    app
}

#[test]
fn quick_open_pdf_via_keyboard_never_splits() {
    // BUG-C at the shell: ctrl+p, type a pdf path, enter. The PDF must open
    // as a sibling tab in the same pane — never a forced split.
    let mut app = app();
    press(&mut app, "ctrl+p");
    assert!(matches!(app.overlay_ui(), OverlayUi::QuickOpen { .. }));

    type_text(&mut app, "papers/attention.pdf");
    press(&mut app, "enter");

    assert_eq!(app.overlay_ui(), &OverlayUi::None);
    assert_eq!(
        app.workspace().panes.pane_count(),
        1,
        "opening a PDF must not split"
    );
    assert_eq!(app.workspace().focused_editor_kind(), Some(EditorKind::Pdf));
    let pane = &app.workspace().panes.panes()[0];
    assert_eq!(pane.tabs().len(), 2, "markdown and pdf are sibling tabs");
    assert!(app.status().contains("opened papers/attention.pdf"));
}

#[test]
fn ctrl_z_routes_by_focused_editor_kind() {
    // BUG-A at the shell: the same physical chord lands on different
    // commands purely by focus — no flags, no widget bindings.
    let mut app = app();

    press(&mut app, "ctrl+z");
    assert_eq!(app.last_command().map(|c| c.0), Some("editor.undo"));
    assert_eq!(app.overlay_ui(), &OverlayUi::None);

    press(&mut app, "ctrl+p");
    type_text(&mut app, "papers/attention.pdf");
    press(&mut app, "enter");

    press(&mut app, "ctrl+z");
    assert_eq!(app.last_command().map(|c| c.0), Some("pdf.zoom-input"));
    assert!(matches!(app.overlay_ui(), OverlayUi::Zoom { .. }));
}

#[test]
fn overlay_is_a_modal_fence_for_keys_and_commands() {
    let mut app = app();
    press(&mut app, "ctrl+p");
    type_text(&mut app, "papers/attention.pdf");
    press(&mut app, "enter");
    press(&mut app, "ctrl+z"); // zoom overlay opens
    assert!(matches!(app.overlay_ui(), OverlayUi::Zoom { .. }));

    // Under the fence, editor/workspace chords must not leak through:
    // ctrl+z resolves to nothing, ctrl+w must not close a tab.
    let tabs_before = app.workspace().panes.panes()[0].tabs().len();
    press(&mut app, "ctrl+z");
    press(&mut app, "ctrl+w");
    assert_eq!(app.last_command().map(|c| c.0), Some("pdf.zoom-input"));
    assert!(matches!(app.overlay_ui(), OverlayUi::Zoom { .. }));
    assert_eq!(app.workspace().panes.panes()[0].tabs().len(), tabs_before);

    // Digits are raw text into the overlay buffer; enter confirms.
    type_text(&mut app, "150");
    assert_eq!(
        app.overlay_ui(),
        &OverlayUi::Zoom {
            digits: "150".to_string()
        }
    );
    press(&mut app, "enter");
    assert_eq!(app.overlay_ui(), &OverlayUi::None);
    assert!(app.status().contains("zoom 150%"));

    // The derived scope stack is back to the editor.
    assert_eq!(app.workspace().focused_editor_kind(), Some(EditorKind::Pdf));
}

#[test]
fn escape_dismisses_any_overlay() {
    let mut app = app();
    press(&mut app, "ctrl+shift+p");
    assert!(matches!(app.overlay_ui(), OverlayUi::Palette { .. }));
    press(&mut app, "escape");
    assert_eq!(app.overlay_ui(), &OverlayUi::None);
    assert_eq!(app.last_command().map(|c| c.0), Some("overlay.close"));
}

#[test]
fn palette_typing_filters_and_enter_runs_the_selection() {
    // Keyboard-only palette flow: ctrl+shift+p, type "split", enter. The
    // palette is the registry; the selection dispatches through the same
    // CommandBus a key chord uses — proven by the split actually happening.
    let mut app = app();
    press(&mut app, "ctrl+shift+p");
    type_text(&mut app, "split");
    assert_eq!(
        app.overlay_ui(),
        &OverlayUi::Palette {
            query: "split".to_string(),
            selected: 0
        }
    );
    press(&mut app, "enter");

    assert_eq!(app.overlay_ui(), &OverlayUi::None);
    assert_eq!(
        app.last_command().map(|c| c.0),
        Some("workspace.split-right")
    );
    assert_eq!(app.workspace().panes.pane_count(), 2);
}

#[test]
fn explicit_split_duplicates_focus_and_close_collapses() {
    let mut app = app();
    press(&mut app, "ctrl+\\");
    assert_eq!(app.workspace().panes.pane_count(), 2);
    // Both panes view the same document (shared by DocumentId).
    assert_eq!(app.workspace().docs.len(), 1);

    press(&mut app, "ctrl+w");
    assert_eq!(
        app.workspace().panes.pane_count(),
        1,
        "empty split collapses"
    );
    assert!(
        app.workspace().focused_tab().is_some(),
        "focus lands back on the surviving pane"
    );
}

#[test]
fn ctrl_tab_cycles_tabs_within_the_focused_pane() {
    let mut app = app();
    press(&mut app, "ctrl+p");
    type_text(&mut app, "papers/attention.pdf");
    press(&mut app, "enter");
    assert_eq!(app.workspace().focused_editor_kind(), Some(EditorKind::Pdf));

    press(&mut app, "ctrl+tab");
    assert_eq!(
        app.workspace().focused_editor_kind(),
        Some(EditorKind::Markdown)
    );
    press(&mut app, "ctrl+tab");
    assert_eq!(app.workspace().focused_editor_kind(), Some(EditorKind::Pdf));
}

#[test]
fn unresolved_chords_outside_overlays_are_inert() {
    // Raw text with no overlay open: today there is no buffer widget, so the
    // shell must do nothing at all (no status change, no command).
    let mut app = app();
    let status = app.status().to_string();
    type_text(&mut app, "hello");
    press(&mut app, "f7");
    assert_eq!(app.status(), status);
    assert_eq!(app.last_command(), None);
}

//! Shell-level routing regression suite: drives the *real* iced update loop
//! (`App::update`) with keyboard messages only — exactly what the window
//! produces — and asserts on kernel state. This is BUG-A and BUG-C pinned at
//! the shell layer: a regression in the wiring (not just the kernel) fails
//! here, windowlessly.

use md3_kernel::input::{Chord, EditorKind, Key, Mods};
use md3_shell::app::{App, Message, OverlayUi};
use md3_shell::keys::KeyPress;

fn chord(s: &str) -> Chord {
    match Chord::parse(s) {
        Ok(c) => c,
        Err(e) => panic!("bad chord `{s}`: {e}"),
    }
}

fn press(app: &mut App, s: &str) {
    let _ = app.update(Message::Key(KeyPress::chord(chord(s))));
}

/// Type printable characters the way the subscription delivers them: one
/// press per character, carrying both the lowercased chord and the verbatim
/// produced text.
fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        let key = if c == ' ' {
            Key::Space
        } else {
            Key::Char(c.to_ascii_lowercase())
        };
        let mods = Mods {
            shift: c.is_ascii_uppercase(),
            ..Mods::NONE
        };
        let _ = app.update(Message::Key(KeyPress {
            chord: Some(Chord::new(mods, key)),
            text: Some(c.to_string()),
        }));
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
fn unresolved_function_keys_are_inert() {
    let mut app = app();
    let status = app.status().to_string();
    press(&mut app, "f7");
    assert_eq!(app.status(), status);
    assert_eq!(app.last_command(), None);
}

// ----- the markdown surface is a real buffer --------------------------------------

fn focused_text(app: &App) -> String {
    match app.focused_buffer() {
        Some(b) => b.text(),
        None => panic!("focused tab has no buffer"),
    }
}

#[test]
fn typing_edits_the_focused_buffer_case_preserved() {
    let mut app = app();
    let original = focused_text(&app);
    type_text(&mut app, "Hi!");
    let now = focused_text(&app);
    assert!(
        now.starts_with("Hi!"),
        "typed text lands at the caret: {now:?}"
    );
    assert_eq!(now.len(), original.len() + 3);
    assert!(
        app.focused_buffer().is_some_and(|b| b.is_dirty()),
        "editing marks the buffer dirty"
    );
}

#[test]
fn ctrl_z_actually_undoes_and_ctrl_shift_z_redoes() {
    let mut app = app();
    let original = focused_text(&app);
    type_text(&mut app, "ab");

    press(&mut app, "ctrl+z");
    press(&mut app, "ctrl+z");
    assert_eq!(focused_text(&app), original, "two keystrokes, two undos");
    assert_eq!(app.last_command().map(|c| c.0), Some("editor.undo"));

    press(&mut app, "ctrl+shift+z");
    assert!(
        focused_text(&app).starts_with('a'),
        "redo replays the first keystroke"
    );
}

#[test]
fn select_all_then_typing_replaces_the_note() {
    let mut app = app();
    press(&mut app, "ctrl+a");
    assert_eq!(app.last_command().map(|c| c.0), Some("editor.select-all"));
    type_text(&mut app, "fresh");
    assert_eq!(focused_text(&app), "fresh");
    press(&mut app, "ctrl+z");
    assert_ne!(focused_text(&app), "fresh", "replace-all is one undo step");
}

#[test]
fn enter_and_backspace_edit_through_chords() {
    let mut app = app();
    press(&mut app, "ctrl+a");
    type_text(&mut app, "one");
    press(&mut app, "enter");
    type_text(&mut app, "two");
    assert_eq!(focused_text(&app), "one\ntwo");
    press(&mut app, "backspace");
    assert_eq!(focused_text(&app), "one\ntw");
}

#[test]
fn typing_at_a_pdf_neither_edits_nor_panics() {
    let mut app = app();
    let md_text = focused_text(&app);
    press(&mut app, "ctrl+p");
    type_text(&mut app, "papers/attention.pdf");
    press(&mut app, "enter");
    assert_eq!(app.workspace().focused_editor_kind(), Some(EditorKind::Pdf));

    type_text(&mut app, "stray keys");
    assert!(
        app.focused_buffer().is_none(),
        "a pdf tab has no text buffer"
    );

    press(&mut app, "ctrl+tab"); // back to markdown
    assert_eq!(
        focused_text(&app),
        md_text,
        "stray keys at the pdf never reached the markdown buffer"
    );
}

#[test]
fn split_panes_share_one_buffer_per_document() {
    let mut app = app();
    press(&mut app, "ctrl+\\");
    assert_eq!(app.workspace().panes.pane_count(), 2);
    type_text(&mut app, "X");
    // Same DocumentId in both panes — one buffer, both views see the edit.
    assert!(focused_text(&app).starts_with('X'));
    assert_eq!(app.workspace().docs.len(), 1);
}

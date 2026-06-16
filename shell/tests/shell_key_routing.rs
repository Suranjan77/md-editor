//! Shell-level routing regression suite: drives the *real* iced update loop
//! (`Shell::update`) with keyboard messages only — exactly what the window
//! produces — and asserts on kernel state. This is BUG-A and BUG-C pinned at
//! the shell layer: a regression in the wiring (not just the kernel) fails
//! here, windowlessly. The vault is a throwaway directory; files open through
//! the same quick-open flow a user types.

use md_kernel::defaults::default_registry;
use md_kernel::input::{Chord, EditorKind, Key, Mods};
use md_shell::gui::keys::KeyEvent;
use md_shell::gui::overlay::Overlay;
use md_shell::gui::{Message, Shell};
use tempfile::TempDir;

const WELCOME: &str = "# Welcome\n\nfirst line\nsecond line\n";

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

/// Type printable characters the way the subscription delivers them: one
/// press per character, carrying both the lowercased chord and the verbatim
/// produced text.
fn type_text(shell: &mut Shell, text: &str) {
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
        let _ = shell.update(Message::Key(KeyEvent {
            chord: Some(Chord::new(mods, key)),
            text: Some(c.to_string()),
        }));
    }
}

/// A throwaway vault with a markdown note and a (stub) PDF, plus a booted
/// shell over it. Nothing is open yet — opening goes through quick-open.
fn vault() -> (TempDir, Shell) {
    let dir = match TempDir::new() {
        Ok(d) => d,
        Err(e) => panic!("tempdir: {e}"),
    };
    let root = dir.path();
    for (path, bytes) in [
        ("notes/welcome.md", WELCOME.as_bytes()),
        ("papers/attention.pdf", b"%PDF-1.4 stub".as_slice()),
    ] {
        let abs = root.join(path);
        if let Some(parent) = abs.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            panic!("mkdir {}: {e}", parent.display());
        }
        if let Err(e) = std::fs::write(&abs, bytes) {
            panic!("write {path}: {e}");
        }
    }
    let registry = match default_registry() {
        Ok(r) => r,
        Err(e) => panic!("registry: {e}"),
    };
    let keymap = match registry.keymap() {
        Ok(k) => k,
        Err(e) => panic!("keymap: {e}"),
    };
    let shell = Shell::new(registry, keymap, root.to_path_buf()).0;
    (dir, shell)
}

/// Quick-open a vault file end to end: ctrl+p, type the path, enter.
fn quick_open(shell: &mut Shell, path: &str) {
    press(shell, "ctrl+p");
    assert!(
        matches!(shell.overlay(), Some(Overlay::QuickOpen { .. })),
        "ctrl+p must open quick-open"
    );
    type_text(shell, path);
    press(shell, "enter");
    assert!(shell.overlay().is_none(), "enter must close quick-open");
}

/// Boot state most tests want: the welcome note open and focused.
fn shell_with_note() -> (TempDir, Shell) {
    let (dir, mut shell) = vault();
    quick_open(&mut shell, "notes/welcome.md");
    assert_eq!(
        shell.workspace().focused_editor_kind(),
        Some(EditorKind::Markdown)
    );
    (dir, shell)
}

fn focused_text(shell: &Shell) -> String {
    match shell.focused_md() {
        Some(s) => s.doc.buffer().text(),
        None => panic!("focused tab has no markdown session"),
    }
}

#[test]
fn quick_open_pdf_via_keyboard_never_splits() {
    // BUG-C at the shell: ctrl+p, pick a pdf, enter. The PDF must open as a
    // sibling tab in the same pane — never a forced split.
    let (_dir, mut shell) = shell_with_note();
    quick_open(&mut shell, "papers/attention.pdf");

    assert_eq!(
        shell.workspace().panes.pane_count(),
        1,
        "opening a PDF must not split"
    );
    assert_eq!(
        shell.workspace().focused_editor_kind(),
        Some(EditorKind::Pdf)
    );
    let pane = &shell.workspace().panes.panes()[0];
    assert_eq!(pane.tabs().len(), 2, "markdown and pdf are sibling tabs");
    assert!(shell.focused_pdf().is_some(), "pdf session exists");
}

#[test]
fn ctrl_z_routes_by_focused_editor_kind() {
    // BUG-A at the shell: the same physical chord lands on different
    // commands purely by focus — no flags, no widget bindings.
    let (_dir, mut shell) = shell_with_note();

    press(&mut shell, "ctrl+z");
    assert_eq!(shell.last_command().map(|c| c.0), Some("editor.undo"));
    assert!(shell.overlay().is_none());

    quick_open(&mut shell, "papers/attention.pdf");

    press(&mut shell, "ctrl+z");
    assert_eq!(shell.last_command().map(|c| c.0), Some("pdf.zoom-input"));
    assert!(matches!(shell.overlay(), Some(Overlay::PdfZoom { .. })));
}

#[test]
fn overlay_is_a_modal_fence_for_keys_and_commands() {
    let (_dir, mut shell) = shell_with_note();
    quick_open(&mut shell, "papers/attention.pdf");
    press(&mut shell, "ctrl+z"); // zoom overlay opens
    assert!(matches!(shell.overlay(), Some(Overlay::PdfZoom { .. })));

    // Under the fence, editor/workspace chords must not leak through:
    // ctrl+z resolves to nothing, ctrl+w must not close a tab.
    let tabs_before = shell.workspace().panes.panes()[0].tabs().len();
    press(&mut shell, "ctrl+z");
    press(&mut shell, "ctrl+w");
    assert_eq!(shell.last_command().map(|c| c.0), Some("pdf.zoom-input"));
    assert!(matches!(shell.overlay(), Some(Overlay::PdfZoom { .. })));
    assert_eq!(shell.workspace().panes.panes()[0].tabs().len(), tabs_before);

    // Digits are raw text into the overlay buffer; enter confirms and the
    // zoom factor actually changes on the session.
    type_text(&mut shell, "150");
    match shell.overlay() {
        Some(Overlay::PdfZoom { input }) => assert_eq!(input, "150"),
        other => panic!("zoom overlay lost: {other:?}"),
    }
    press(&mut shell, "enter");
    assert!(shell.overlay().is_none());
    match shell.focused_pdf() {
        Some(session) => assert!((session.zoom - 1.5).abs() < f32::EPSILON),
        None => panic!("pdf session lost"),
    }

    // The derived scope stack is back to the pdf editor.
    assert_eq!(
        shell.workspace().focused_editor_kind(),
        Some(EditorKind::Pdf)
    );
}

#[test]
fn escape_dismisses_any_overlay() {
    let (_dir, mut shell) = shell_with_note();
    press(&mut shell, "ctrl+shift+p");
    assert!(matches!(shell.overlay(), Some(Overlay::Palette { .. })));
    press(&mut shell, "escape");
    assert!(shell.overlay().is_none());
    assert_eq!(shell.last_command().map(|c| c.0), Some("overlay.close"));
}

#[test]
fn palette_typing_filters_and_enter_runs_the_selection() {
    // Keyboard-only palette flow: ctrl+shift+p, type "split", enter. The
    // palette is the registry; the selection runs through the same command
    // dispatch a key chord uses — proven by the split actually happening.
    let (_dir, mut shell) = shell_with_note();
    press(&mut shell, "ctrl+shift+p");
    type_text(&mut shell, "split");
    match shell.overlay() {
        Some(Overlay::Palette { input, selected }) => {
            assert_eq!(input, "split");
            assert_eq!(*selected, 0);
        }
        other => panic!("palette lost: {other:?}"),
    }
    press(&mut shell, "enter");

    assert!(shell.overlay().is_none());
    assert_eq!(
        shell.last_command().map(|c| c.0),
        Some("workspace.split-right")
    );
    assert_eq!(shell.workspace().panes.pane_count(), 2);
}

#[test]
fn explicit_split_duplicates_focus_and_close_collapses() {
    let (_dir, mut shell) = shell_with_note();
    press(&mut shell, "ctrl+\\");
    assert_eq!(shell.workspace().panes.pane_count(), 2);
    // Both panes view the same document (shared by DocumentId).
    assert_eq!(shell.workspace().docs.len(), 1);

    press(&mut shell, "ctrl+w");
    assert_eq!(
        shell.workspace().panes.pane_count(),
        1,
        "empty split collapses"
    );
    assert!(
        shell.workspace().focused_tab().is_some(),
        "focus lands back on the surviving pane"
    );
}

#[test]
fn ctrl_tab_cycles_tabs_within_the_focused_pane() {
    let (_dir, mut shell) = shell_with_note();
    quick_open(&mut shell, "papers/attention.pdf");
    assert_eq!(
        shell.workspace().focused_editor_kind(),
        Some(EditorKind::Pdf)
    );

    press(&mut shell, "ctrl+tab");
    assert_eq!(
        shell.workspace().focused_editor_kind(),
        Some(EditorKind::Markdown)
    );
    press(&mut shell, "ctrl+tab");
    assert_eq!(
        shell.workspace().focused_editor_kind(),
        Some(EditorKind::Pdf)
    );
}

#[test]
fn unresolved_function_keys_are_inert() {
    let (_dir, mut shell) = shell_with_note();
    let status = shell.status().to_string();
    let last = shell.last_command();
    press(&mut shell, "f7");
    assert_eq!(shell.status(), status);
    assert_eq!(shell.last_command(), last);
}

// ----- the markdown surface is a real engine document -----------------------------

#[test]
fn typing_edits_the_focused_buffer_case_preserved() {
    let (_dir, mut shell) = shell_with_note();
    let original = focused_text(&shell);
    type_text(&mut shell, "Hi!");
    let now = focused_text(&shell);
    assert!(
        now.starts_with("Hi!"),
        "typed text lands at the caret: {now:?}"
    );
    assert_eq!(now.len(), original.len() + 3);
    assert!(
        shell
            .focused_md()
            .is_some_and(|s| s.doc.buffer().is_dirty()),
        "editing marks the buffer dirty"
    );
}

#[test]
fn ctrl_z_actually_undoes_and_ctrl_shift_z_redoes() {
    let (_dir, mut shell) = shell_with_note();
    let original = focused_text(&shell);
    type_text(&mut shell, "ab");

    press(&mut shell, "ctrl+z");
    press(&mut shell, "ctrl+z");
    assert_eq!(focused_text(&shell), original, "undo restores the note");
    assert_eq!(shell.last_command().map(|c| c.0), Some("editor.undo"));

    press(&mut shell, "ctrl+shift+z");
    assert!(
        focused_text(&shell).starts_with('a'),
        "redo replays the typing run"
    );
}

#[test]
fn select_all_then_typing_replaces_the_note() {
    let (_dir, mut shell) = shell_with_note();
    press(&mut shell, "ctrl+a");
    assert_eq!(shell.last_command().map(|c| c.0), Some("editor.select-all"));
    type_text(&mut shell, "fresh");
    assert_eq!(focused_text(&shell), "fresh");
    press(&mut shell, "ctrl+z");
    assert_ne!(
        focused_text(&shell),
        "fresh",
        "replace-all is one undo step"
    );
}

#[test]
fn enter_and_backspace_edit_through_chords() {
    let (_dir, mut shell) = shell_with_note();
    press(&mut shell, "ctrl+a");
    type_text(&mut shell, "one");
    press(&mut shell, "enter");
    type_text(&mut shell, "two");
    assert_eq!(focused_text(&shell), "one\ntwo");
    press(&mut shell, "backspace");
    assert_eq!(focused_text(&shell), "one\ntw");
}

#[test]
fn typing_at_a_pdf_neither_edits_nor_panics() {
    let (_dir, mut shell) = shell_with_note();
    let md_text = focused_text(&shell);
    quick_open(&mut shell, "papers/attention.pdf");
    assert_eq!(
        shell.workspace().focused_editor_kind(),
        Some(EditorKind::Pdf)
    );

    type_text(&mut shell, "stray keys");
    assert!(
        shell.focused_md().is_none(),
        "a pdf tab has no markdown session"
    );

    press(&mut shell, "ctrl+tab"); // back to markdown
    assert_eq!(
        focused_text(&shell),
        md_text,
        "stray keys at the pdf never reached the markdown buffer"
    );
}

#[test]
fn split_panes_share_one_buffer_per_document() {
    let (_dir, mut shell) = shell_with_note();
    press(&mut shell, "ctrl+\\");
    assert_eq!(shell.workspace().panes.pane_count(), 2);
    type_text(&mut shell, "X");
    // Same DocumentId in both panes — one engine document, both views see
    // the edit.
    assert!(focused_text(&shell).starts_with('X'));
    assert_eq!(shell.workspace().docs.len(), 1);
}

#[test]
fn editor_save_writes_through_the_vault() {
    let (dir, mut shell) = shell_with_note();
    press(&mut shell, "ctrl+a");
    type_text(&mut shell, "saved body");
    press(&mut shell, "ctrl+s");
    assert_eq!(shell.last_command().map(|c| c.0), Some("editor.save"));
    let on_disk = match std::fs::read_to_string(dir.path().join("notes/welcome.md")) {
        Ok(t) => t,
        Err(e) => panic!("read saved note: {e}"),
    };
    assert_eq!(on_disk, "saved body");
    assert!(
        shell
            .focused_md()
            .is_some_and(|s| !s.doc.buffer().is_dirty()),
        "save clears the dirty flag"
    );
}

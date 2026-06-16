//! Integration tests for study tracker wiring (plan Phase 4).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use md_kernel::defaults::default_registry;
use md_kernel::input::Chord;
use md_shell::gui::keys::KeyEvent;
use md_shell::gui::tracker_view::{TrackerMessage, TrackerTab};
use md_shell::gui::{Message, Shell, ToastKind};
use std::path::Path;
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

fn new_shell(root: &Path) -> Shell {
    let registry = match default_registry() {
        Ok(r) => r,
        Err(e) => panic!("registry: {e}"),
    };
    let keymap = match registry.keymap() {
        Ok(k) => k,
        Err(e) => panic!("keymap: {e}"),
    };
    Shell::new_with_tracker_db(
        registry,
        keymap,
        root.to_path_buf(),
        root.join(".md-editor/tracker.db"),
    ).0
}

#[test]
fn tracker_toggle_and_persistence() {
    let dir = TempDir::new().unwrap();

    // 1. Fresh shell starts with tracker closed
    let mut shell = new_shell(dir.path());
    assert!(!shell.tracker_open());

    // 2. Pressing ctrl+shift+t toggles it open
    press(&mut shell, "ctrl+shift+t");
    assert!(shell.tracker_open());

    // 3. Quitting and restoring saves/loads the state
    press(&mut shell, "ctrl+q");
    drop(shell);

    let shell = new_shell(dir.path());
    assert!(shell.tracker_open());
}

#[test]
fn tracker_manual_log_and_delete() {
    let dir = TempDir::new().unwrap();
    let mut shell = new_shell(dir.path());

    // Open tracker
    press(&mut shell, "ctrl+shift+t");

    // Log a manual session
    let _ = shell.update(Message::Tracker(TrackerMessage::ManualDateChanged(
        "2026-06-12".to_string(),
    )));
    let _ = shell.update(Message::Tracker(TrackerMessage::ManualHoursChanged(
        "2.5".to_string(),
    )));
    let _ = shell.update(Message::Tracker(TrackerMessage::ManualNotesChanged(
        "Reading books".to_string(),
    )));
    let _ = shell.update(Message::Tracker(TrackerMessage::ManualAdd));

    assert!(shell.toasts().iter().any(|toast| {
        toast.kind == ToastKind::Success && toast.message == "tracker: session logged manually"
    }));

    // Verify state: we can query the database directly or toggle tabs
    let _ = shell.update(Message::Tracker(TrackerMessage::TabSelected(
        TrackerTab::Log,
    )));
    press(&mut shell, "ctrl+q");
    drop(shell);

    // Reopen and check if log is preserved and tab is restored
    let mut shell = new_shell(dir.path());
    assert!(shell.tracker_open());

    // Delete session (first session has ID 1)
    let _ = shell.update(Message::Tracker(TrackerMessage::SessionDelete(1)));
    assert_eq!(shell.status(), "tracker: session deleted");
}

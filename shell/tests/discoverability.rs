use std::path::Path;

use md_kernel::defaults::default_registry;
use md_kernel::input::{Chord, Key, Mods};
use md_shell::gui::keys::KeyEvent;
use md_shell::gui::overlay::{self, Overlay};
use md_shell::gui::welcome::welcome_rows;
use md_shell::gui::{Message, Shell};

fn shell(root: &Path) -> Shell {
    let registry = match default_registry() {
        Ok(registry) => registry,
        Err(e) => panic!("registry: {e}"),
    };
    let keymap = match registry.keymap() {
        Ok(keymap) => keymap,
        Err(e) => panic!("keymap: {e}"),
    };
    Shell::new(registry, keymap, root.to_path_buf()).0
}

fn press(shell: &mut Shell, chord: &str) {
    let chord = match Chord::parse(chord) {
        Ok(chord) => chord,
        Err(e) => panic!("chord: {e}"),
    };
    let _ = shell.update(Message::Key(KeyEvent {
        chord: Some(chord),
        text: None,
    }));
}

fn type_text(shell: &mut Shell, text: &str) {
    for ch in text.chars() {
        let _ = shell.update(Message::Key(KeyEvent {
            chord: Some(Chord::new(Mods::NONE, Key::Char(ch))),
            text: Some(ch.to_string()),
        }));
    }
}

#[test]
fn welcome_rows_resolve_to_registered_commands_with_chords() {
    let registry = match default_registry() {
        Ok(registry) => registry,
        Err(e) => panic!("registry: {e}"),
    };
    let rows = welcome_rows(&registry);
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|row| registry.get(row.command).is_some()));
    assert!(rows.iter().all(|row| row.chord.is_some()));
}

#[test]
fn shortcut_help_lists_filters_and_runs_registry_commands() {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(e) => panic!("tempdir: {e}"),
    };
    let mut shell = shell(dir.path());
    assert!(shell.tree_open(), "fresh vault exposes file panel");

    press(&mut shell, "ctrl+/");
    let rows = match shell.overlay() {
        Some(overlay @ Overlay::Help { .. }) => {
            overlay::list_rows(overlay, &default_registry().unwrap_or_default(), &[])
        }
        other => panic!("expected help overlay, got {other:?}"),
    };
    let registry_len = match default_registry() {
        Ok(registry) => registry.len(),
        Err(e) => panic!("registry: {e}"),
    };
    assert_eq!(rows.len(), registry_len);

    type_text(&mut shell, "toggle file");
    let filtered = match shell.overlay() {
        Some(overlay @ Overlay::Help { .. }) => {
            let registry = match default_registry() {
                Ok(registry) => registry,
                Err(e) => panic!("registry: {e}"),
            };
            overlay::list_rows(overlay, &registry, &[])
        }
        other => panic!("expected filtered help overlay, got {other:?}"),
    };
    assert_eq!(filtered.len(), 1);
    press(&mut shell, "enter");
    assert!(!shell.tree_open(), "enter ran workspace.toggle-files");
    assert_eq!(
        shell.last_command().map(|command| command.0),
        Some("workspace.toggle-files")
    );
}

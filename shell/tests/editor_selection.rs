#![allow(clippy::unwrap_used)]
//! Mouse-drag selection and clipboard wiring for the Markdown editor.

use std::path::Path;
use tempfile::TempDir;

use md_kernel::CommandId;
use md_kernel::defaults::default_registry;
use md_shell::gui::{Message, Shell};

fn new_shell(root: &Path) -> Shell {
    let registry = default_registry().unwrap();
    let keymap = registry.keymap().unwrap();
    let mut s = Shell::new(registry, keymap, root.to_path_buf()).0; s.sync_file_loads = true; s
}

fn open(content: &str) -> (TempDir, Shell) {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("note.md"), content).unwrap();
    let mut shell = new_shell(dir.path());
    let _ = shell.update(Message::TreeFileClicked("note.md".to_string()));
    (dir, shell)
}

#[test]
fn drag_select_sets_the_selection_text() {
    let (_dir, mut shell) = open("hello world");
    let tab = shell.workspace().focused_tab().unwrap();

    let _ = shell.update(Message::EditorDragSelect {
        tab,
        anchor: 0,
        head: 5,
    });

    let selected = shell.focused_md().unwrap().doc.buffer().selected_text();
    assert_eq!(selected.as_deref(), Some("hello"));
}

#[test]
fn reverse_multiline_drag_normalizes_the_selection() {
    let (_dir, mut shell) = open("first\nsecond\nthird");
    let tab = shell.workspace().focused_tab().unwrap();

    let _ = shell.update(Message::EditorDragSelect {
        tab,
        anchor: 13,
        head: 3,
    });

    let selected = shell.focused_md().unwrap().doc.buffer().selected_text();
    assert_eq!(selected.as_deref(), Some("st\nsecond\n"));
}

#[test]
fn copy_keeps_selection_cut_is_undoable() {
    let (_dir, mut shell) = open("hello world");
    let tab = shell.workspace().focused_tab().unwrap();
    let _ = shell.update(Message::EditorDragSelect {
        tab,
        anchor: 6,
        head: 11,
    });

    // Copy leaves the buffer untouched.
    let _ = shell.update(Message::RunCommand(CommandId("editor.copy")));
    assert_eq!(
        shell.focused_md().unwrap().doc.buffer().text(),
        "hello world"
    );

    // Cut removes the selected run.
    let _ = shell.update(Message::RunCommand(CommandId("editor.cut")));
    assert_eq!(shell.focused_md().unwrap().doc.buffer().text(), "hello ");

    let _ = shell.update(Message::RunCommand(CommandId("editor.undo")));
    assert_eq!(
        shell.focused_md().unwrap().doc.buffer().text(),
        "hello world"
    );
}

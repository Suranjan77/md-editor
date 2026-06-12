//! Integration tests for the vault file-browser left panel (plan Phase 2.5).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use md3_kernel::defaults::default_registry;
use md3_kernel::input::{Chord, EditorKind, Key, Mods};
use md3_shell::gui::keys::KeyEvent;
use md3_shell::gui::overlay::{NamePurpose, Overlay};
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

fn run(shell: &mut Shell, id: &'static str) {
    let _ = shell.update(Message::RunCommand(md3_kernel::CommandId(id)));
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

fn open(shell: &mut Shell, name: &str) {
    press(shell, "ctrl+p");
    type_text(shell, name);
    press(shell, "enter");
}

fn write(root: &Path, rel: &str, body: &str) {
    let abs = root.join(rel);
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    if let Err(e) = std::fs::write(&abs, body) {
        panic!("write {rel}: {e}");
    }
}

fn vault() -> TempDir {
    let dir = TempDir::new().unwrap();
    write(dir.path(), "alpha.md", "# alpha\n");
    write(dir.path(), "beta.md", "# beta\n");
    write(dir.path(), "papers/attention.pdf", "%PDF-1.4 stub");
    dir
}

fn focused_path(shell: &Shell) -> Option<String> {
    let tab = shell.workspace().focused_tab()?;
    let (_, tab) = shell.workspace().panes.find_tab(tab)?;
    shell
        .workspace()
        .docs
        .get(tab.document)
        .map(|d| d.path.clone())
}

#[test]
fn ctrl_b_toggles_and_persists() {
    let dir = vault();

    // 1. Fresh shell starts with file tree open for discoverability.
    let mut shell = new_shell(dir.path());
    assert!(shell.tree_open());

    // 2. Pressing ctrl+b toggles it closed.
    press(&mut shell, "ctrl+b");
    assert!(!shell.tree_open());

    // 3. Quitting and restoring saves/loads the state.
    press(&mut shell, "ctrl+q");
    drop(shell);

    let mut shell = new_shell(dir.path());
    assert!(!shell.tree_open());

    // 4. Pressing ctrl+b toggles it open, and that persists too.
    press(&mut shell, "ctrl+b");
    assert!(shell.tree_open());

    press(&mut shell, "ctrl+q");
    drop(shell);

    let shell = new_shell(dir.path());
    assert!(shell.tree_open());
}

#[test]
fn clicking_a_file_row_opens_it_in_the_focused_pane() {
    let dir = vault();
    let mut shell = new_shell(dir.path());

    // Click alpha.md in the tree.
    let _ = shell.update(Message::TreeFileClicked("alpha.md".to_string()));
    assert_eq!(
        shell.workspace().focused_editor_kind(),
        Some(EditorKind::Markdown)
    );
    assert_eq!(focused_path(&shell), Some("alpha.md".to_string()));

    // Click beta.md in the tree.
    let _ = shell.update(Message::TreeFileClicked("beta.md".to_string()));
    assert_eq!(focused_path(&shell), Some("beta.md".to_string()));
}

#[test]
fn ctrl_b_works_from_pdf_focus() {
    let dir = vault();
    let mut shell = new_shell(dir.path());

    // Open a PDF so focus changes to PDF view.
    open(&mut shell, "papers/attention.pdf");
    assert_eq!(
        shell.workspace().focused_editor_kind(),
        Some(EditorKind::Pdf)
    );

    // Toggle tree and ensure it works while PDF has focus.
    assert!(shell.tree_open());
    press(&mut shell, "ctrl+b");
    assert!(!shell.tree_open());

    press(&mut shell, "ctrl+b");
    assert!(shell.tree_open());
}

#[test]
fn create_rename_repair_and_delete_flow() {
    let dir = vault();
    write(dir.path(), "target.md", "# target\n");
    write(dir.path(), "referrer.md", "See [[target]].\n");
    let mut shell = new_shell(dir.path());

    run(&mut shell, "file.new-note");
    type_text(&mut shell, "created");
    press(&mut shell, "enter");
    assert!(dir.path().join("created.md").is_file());
    assert_eq!(focused_path(&shell).as_deref(), Some("created.md"));

    let _ = shell.update(Message::TreeFileClicked("target.md".to_string()));
    let original_id = shell
        .workspace()
        .panes
        .find_tab(shell.workspace().focused_tab().expect("focused tab"))
        .map(|(_, tab)| tab.document)
        .expect("open target document");
    run(&mut shell, "file.rename");
    for _ in 0.."target.md".len() {
        press(&mut shell, "backspace");
    }
    type_text(&mut shell, "renamed");
    press(&mut shell, "enter");

    assert!(!dir.path().join("target.md").exists());
    assert!(dir.path().join("renamed.md").is_file());
    assert_eq!(focused_path(&shell).as_deref(), Some("renamed.md"));
    let renamed_id = shell
        .workspace()
        .panes
        .find_tab(shell.workspace().focused_tab().expect("focused tab"))
        .map(|(_, tab)| tab.document)
        .expect("renamed target document");
    assert_eq!(renamed_id, original_id, "rename keeps DocumentId stable");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("referrer.md")).ok(),
        Some("See [[renamed]].\n".to_string())
    );

    run(&mut shell, "file.delete");
    press(&mut shell, "enter");
    assert!(!dir.path().join("renamed.md").exists());
    assert!(
        shell.workspace().docs.get(original_id).is_none(),
        "deleting open file closes affected tabs and document"
    );
}

#[test]
fn context_menu_routes_commands_and_sidebar_width_persists() {
    let dir = vault();
    let mut shell = new_shell(dir.path());

    let _ = shell.update(Message::TreeContextRequested {
        rel_path: "alpha.md".to_string(),
        is_dir: false,
    });
    let _ = shell.update(Message::TreeContextCommand(md3_kernel::CommandId(
        "file.new-note",
    )));
    assert!(matches!(
        shell.overlay(),
        Some(Overlay::NameInput {
            purpose: NamePurpose::NewNote { parent },
            ..
        }) if parent.is_empty()
    ));
    press(&mut shell, "escape");

    let _ = shell.update(Message::TreeResizeStarted);
    let _ = shell.update(Message::TreeResized(900.0));
    let _ = shell.update(Message::TreeResizeFinished);
    assert_eq!(shell.tree_width(), 480.0);
    drop(shell);

    let shell = new_shell(dir.path());
    assert_eq!(shell.tree_width(), 480.0);
}

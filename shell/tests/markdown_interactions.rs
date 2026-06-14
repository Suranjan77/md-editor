#![allow(clippy::unwrap_used)]

use std::path::Path;

use md3_kernel::defaults::default_registry;
use md3_shell::gui::{Message, Shell};

fn shell(root: &Path) -> Shell {
    unsafe {
        std::env::set_var("MD3_TEST_MODE", "1");
    }
    let registry = default_registry().unwrap();
    let keymap = registry.keymap().unwrap();
    Shell::new(registry, keymap, root.to_path_buf())
}

fn focused_path(shell: &Shell) -> String {
    let tab = shell.workspace().focused_tab().unwrap();
    let (_, tab) = shell.workspace().panes.find_tab(tab).unwrap();
    shell
        .workspace()
        .docs
        .get(tab.document)
        .unwrap()
        .path
        .clone()
}

#[test]
fn checkbox_click_uses_command_and_ctrl_click_follows_links() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("note.md"),
        "- [ ] task\n[site](https://example.com)\n[[target]]\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("target.md"), "# target\n").unwrap();

    let mut shell = shell(dir.path());
    let _ = shell.update(Message::TreeFileClicked("note.md".to_string()));
    let tab = shell.workspace().focused_tab().unwrap();

    let _ = shell.update(Message::EditorClicked {
        tab,
        line: 0,
        col: 0,
        viewport_h: 600.0,
        checkbox: true,
        ctrl: false,
    });
    assert!(
        shell
            .focused_md()
            .unwrap()
            .doc
            .buffer()
            .text()
            .starts_with("- [x] task")
    );

    let _ = shell.update(Message::EditorClicked {
        tab,
        line: 1,
        col: 2,
        viewport_h: 600.0,
        checkbox: false,
        ctrl: true,
    });
    assert_eq!(shell.status(), "link: https://example.com");

    let _ = shell.update(Message::EditorClicked {
        tab,
        line: 2,
        col: 3,
        viewport_h: 600.0,
        checkbox: false,
        ctrl: true,
    });
    assert_eq!(focused_path(&shell), "target.md");
}

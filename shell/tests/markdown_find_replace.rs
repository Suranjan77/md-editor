use std::path::Path;
use tempfile::TempDir;

use md_kernel::CommandId;
use md_kernel::defaults::default_registry;
use md_shell::gui::{Message, Shell};

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

#[test]
fn test_markdown_find_replace_flow() {
    let dir = match TempDir::new() {
        Ok(d) => d,
        Err(e) => panic!("tempdir: {e}"),
    };
    let file_path = dir.path().join("note.md");
    // "hello world hello" -> length is 17
    let content = "hello world hello";
    if let Err(e) = std::fs::write(&file_path, content) {
        panic!("write note.md: {e}");
    }

    let mut shell = new_shell(dir.path());
    let _ = shell.update(Message::TreeFileClicked("note.md".to_string()));

    let tab = match shell.workspace().focused_tab() {
        Some(t) => t,
        None => panic!("no focused tab"),
    };

    // Toggle find panel open.
    let _ = shell.update(Message::RunCommand(CommandId("editor.find")));
    let focused_md = match shell.focused_md() {
        Some(x) => x,
        None => panic!("no focused md session"),
    };
    assert!(focused_md.find_open);

    // Type query "hello"
    let _ = shell.update(Message::MdFindQueryChanged {
        tab,
        query: "hello".to_string(),
    });

    // Check that first "hello" is selected
    let focused_md = match shell.focused_md() {
        Some(x) => x,
        None => panic!("no focused md session"),
    };
    let primary = focused_md.doc.buffer().primary();
    assert_eq!(primary.anchor, 0);
    assert_eq!(primary.head, 5);

    // Click Next
    let _ = shell.update(Message::MdFindNext { tab });
    let focused_md = match shell.focused_md() {
        Some(x) => x,
        None => panic!("no focused md session"),
    };
    let primary = focused_md.doc.buffer().primary();
    assert_eq!(primary.anchor, 12);
    assert_eq!(primary.head, 17);

    // Input replace text "hi"
    let _ = shell.update(Message::MdReplaceTextChanged {
        tab,
        text: "hi".to_string(),
    });

    // Replace the active match ("hello" at 12..17)
    let _ = shell.update(Message::MdReplace { tab });
    let focused_md = match shell.focused_md() {
        Some(x) => x,
        None => panic!("no focused md session"),
    };
    assert_eq!(focused_md.doc.buffer().text(), "hello world hi");

    // Replace All "hello" with "hey"
    let _ = shell.update(Message::MdFindQueryChanged {
        tab,
        query: "hello".to_string(),
    });
    let _ = shell.update(Message::MdReplaceTextChanged {
        tab,
        text: "hey".to_string(),
    });
    let _ = shell.update(Message::MdReplaceAll { tab });
    let focused_md = match shell.focused_md() {
        Some(x) => x,
        None => panic!("no focused md session"),
    };
    assert_eq!(focused_md.doc.buffer().text(), "hey world hi");

    // Close find panel
    let _ = shell.update(Message::MdCloseFind { tab });
    let focused_md = match shell.focused_md() {
        Some(x) => x,
        None => panic!("no focused md session"),
    };
    assert!(!focused_md.find_open);
}

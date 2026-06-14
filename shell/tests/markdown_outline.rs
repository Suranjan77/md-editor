use std::path::Path;
use tempfile::TempDir;

use md3_kernel::CommandId;
use md3_kernel::defaults::default_registry;
use md3_shell::gui::{Message, Shell, drag::PanelKind};

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
fn markdown_outline_toggle_jump_and_resize() {
    let dir = match TempDir::new() {
        Ok(d) => d,
        Err(e) => panic!("tempdir: {e}"),
    };
    let file_path = dir.path().join("note.md");
    let content = "# Heading 1\nsome text\n## Heading 2\nmore text\n";
    if let Err(e) = std::fs::write(&file_path, content) {
        panic!("write note.md: {e}");
    }

    let mut shell = new_shell(dir.path());
    let _ = shell.update(Message::TreeFileClicked("note.md".to_string()));

    // Verify initial state: outline is closed.
    let focused_md = match shell.focused_md() {
        Some(x) => x,
        None => panic!("no focused md session"),
    };
    assert!(!focused_md.outline_open);

    // Toggle outline panel open.
    let _ = shell.update(Message::RunCommand(CommandId("note.outline-panel")));
    let focused_md = match shell.focused_md() {
        Some(x) => x,
        None => panic!("no focused md session"),
    };
    assert!(focused_md.outline_open);
    assert_eq!(focused_md.outline_width, 250.0);

    // Verify headings are extracted.
    let headings = focused_md.doc.headings();
    assert_eq!(headings.len(), 2);
    assert_eq!(headings[0], (1, "Heading 1".to_string(), 0));
    assert_eq!(headings[1], (2, "Heading 2".to_string(), 2));

    // Jump to Heading 2.
    let tab = match shell.workspace().focused_tab() {
        Some(t) => t,
        None => panic!("no focused tab"),
    };
    let _ = shell.update(Message::MdJumpToLine { tab, line: 2 });

    // Verify cursor updated to line 2.
    let focused_md = match shell.focused_md() {
        Some(x) => x,
        None => panic!("no focused md session"),
    };
    let head = focused_md.doc.buffer().primary().head;
    let (line, col) = focused_md.doc.buffer().offset_to_line_col(head);
    assert_eq!(line, 2);
    assert_eq!(col, 0);

    // Resize outline panel.
    let _ = shell.update(Message::PanelResized {
        kind: PanelKind::Outline,
        width: 300.0,
    });
    let focused_md = match shell.focused_md() {
        Some(x) => x,
        None => panic!("no focused md session"),
    };
    assert_eq!(focused_md.outline_width, 300.0);

    // Close outline panel.
    let _ = shell.update(Message::RunCommand(CommandId("note.outline-panel")));
    let focused_md = match shell.focused_md() {
        Some(x) => x,
        None => panic!("no focused md session"),
    };
    assert!(!focused_md.outline_open);
}

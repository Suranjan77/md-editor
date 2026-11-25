//! End-to-end scenarios. Each boots the real app on a fresh vault, drives it,
//! checks the outcome on disk and on screen, and leaves screenshots and
//! telemetry in `target/e2e/`.

use super::{Vault, boot};

/// A small but varied vault: linked notes, every block type, a PDF, a deck.
pub(crate) fn research_vault() -> Vault {
    Vault::new("research")
        .file(
            "Index.md",
            "# Research Index\n\nStart at [[Attention]] and [[Optimizers]].\n\n- [ ] read the Adam paper\n- [x] set up the vault\n\n| Topic | Status |\n|---|---|\n| Attention | drafting |\n| Optimizers | reading |\n",
        )
        .file(
            "notes/Attention.md",
            "# Attention\n\nScaled dot-product attention:\n\n$$\n\\mathrm{softmax}\\left(\\frac{QK^T}{\\sqrt{d_k}}\\right)V\n$$\n\n```python\ndef attention(q, k, v):\n    return softmax(q @ k.T / sqrt(d)) @ v\n```\n\n> Attention is all you need.\n\nBack to [[Index]].\n",
        )
        .file(
            "notes/Optimizers.md",
            "# Optimizers\n\nAdam keeps moving averages of the gradient ($m_t$) and its square ($v_t$).\n\nSee [[Attention]] and the paper in `papers/`.\n",
        )
        .fixture("papers/dummy.pdf", "pdf/dummy.pdf")
        .fixture("slides/deck.pptx", "pptx/fixture.pptx")
}

#[test]
fn smoke_boots_and_renders_a_note() {
    let vault = research_vault();
    let mut app = boot("smoke", &vault, Some("Index.md"));
    app.screenshot("index");
    assert!(
        app.shows("Index.md"),
        "the open note should be named somewhere in the UI"
    );
    let state = app.finish();
    assert_eq!(state.active_path.as_deref(), Some("Index.md"));
}

use std::time::Duration;

use crate::editor::buffer::EditorCommand;
use crate::messages::{EditorMessage, Message, VaultMessage};

fn insert(text: &str) -> Message {
    Message::Editor(EditorMessage::Command(EditorCommand::InsertText(
        text.to_string(),
    )))
}

fn linked_vault(name: &str) -> Vault {
    Vault::new(name)
        .file("notes/A.md", "Links: [[B]]\n")
        .file("notes/B.md", "# B\n\nSelf link: [[B]]\n")
}

/// S4 / H2: renaming the open note while it has unsaved edits keeps the
/// edits, rewrites links to it, and never resurrects the old file.
#[test]
fn s4_rename_open_dirty_note() {
    let vault = linked_vault("s4-rename");
    let mut app = boot("s4-rename", &vault, Some("notes/B.md"));

    app.send_quick(insert("typed "));
    app.send_quick(Message::Vault(VaultMessage::RenameEntryDialog(
        "notes/B.md".to_string(),
    )));
    app.send(Message::Vault(VaultMessage::NameModalSubmit(
        "C.md".to_string(),
    )));
    app.screenshot("renamed");

    assert!(!vault.exists("notes/B.md"), "old file must be gone");
    assert!(vault.read("notes/C.md").contains("typed "), "edits kept");
    let backlinker = vault.read("notes/A.md");
    assert!(
        backlinker.contains("C]]") && !backlinker.contains("[[B]]"),
        "backlink not rewritten: {backlinker:?}"
    );
    // The renamed note's own self-link was rewritten on disk, and the open
    // buffer followed (the clean-buffer invariant checks buffer == disk).
    assert!(!vault.read("notes/C.md").contains("[[B]]"));
    assert_eq!(app.probe().active_path.as_deref(), Some("notes/C.md"));

    // Keep typing: autosave goes to the new name only.
    app.type_in_note("more ");
    app.wait(Duration::from_millis(800));
    app.settle();
    assert!(
        !vault.exists("notes/B.md"),
        "autosave recreated the old file"
    );
    assert!(vault.read("notes/C.md").contains("more "));
    app.finish();
}

/// S4 / H3: deleting the folder holding the open, dirty note detaches it;
/// autosave must not recreate the folder.
#[test]
fn s4_delete_folder_of_open_dirty_note() {
    let vault = linked_vault("s4-delete");
    let mut app = boot("s4-delete", &vault, Some("notes/B.md"));

    app.send_quick(insert("unsaved "));
    app.send(Message::Vault(VaultMessage::DeleteFile(
        "notes".to_string(),
    )));
    app.wait(Duration::from_millis(1000));
    app.settle();

    assert!(!vault.exists("notes"), "the deleted folder came back");
    assert_eq!(app.probe().active_path, None);
    app.finish();
}

/// S15 / H7: switching vault with unsaved edits writes them to the first
/// vault before anything else happens.
#[test]
fn s15_switch_vault_with_dirty_note() {
    let first = linked_vault("s15-first");
    let second = Vault::new("s15-second").file("Other.md", "# Other\n");
    let mut app = boot("s15-switch", &first, Some("notes/B.md"));

    app.send_quick(insert("last words "));
    app.send(Message::Vault(VaultMessage::VaultOpened(Some(
        second.root.to_string_lossy().into_owned(),
    ))));

    assert!(
        first.read("notes/B.md").contains("last words "),
        "edits made just before switching vault were lost"
    );
    let probe = app.finish();
    assert_eq!(
        probe.vault_root.as_deref(),
        Some(second.root.to_string_lossy().as_ref())
    );
}

/// S16 / H8 (first half): an external edit to the open, clean note shows up.
/// The quiet-state invariant (clean buffer == disk) is the oracle.
#[test]
fn s16_external_edit_of_clean_open_note() {
    let vault = linked_vault("s16-clean");
    let mut app = boot("s16-clean", &vault, Some("notes/B.md"));

    std::fs::write(vault.root.join("notes/B.md"), "# B\n\nChanged on disk\n").unwrap();
    app.wait(Duration::from_millis(1500));
    app.settle();

    assert!(app.probe().buffer.contains("Changed on disk"));
    app.finish();
}

/// S16 / H8 (second half): an external edit to the open note while it has
/// unsaved edits must not be silently overwritten by autosave.
#[test]
fn s16_external_edit_of_dirty_open_note() {
    let vault = linked_vault("s16-dirty");
    let mut app = boot("s16-dirty", &vault, Some("notes/B.md"));

    app.send_quick(insert("mine "));
    std::fs::write(vault.root.join("notes/B.md"), "# B\n\nTheirs\n").unwrap();
    app.wait(Duration::from_millis(1500));
    app.settle();
    app.screenshot("after-conflict");

    let probe = app.finish();
    assert!(
        vault.read("notes/B.md").contains("mine "),
        "our edits were not saved"
    );
    let copies: Vec<String> = std::fs::read_dir(vault.root.join("notes"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("B (conflict "))
        .collect();
    assert_eq!(
        copies.len(),
        1,
        "expected one conflict copy, found {copies:?}"
    );
    assert!(
        vault
            .read(&format!("notes/{}", copies[0]))
            .contains("Theirs"),
        "the external edit was lost"
    );
    assert!(probe.toast.is_some_and(|t| t.contains("changed on disk")));
}

fn edit(command: EditorCommand) -> Message {
    Message::Editor(EditorMessage::Command(command))
}

/// S2: typing autosaves; undo and redo are saved too.
#[test]
fn s2_autosave_undo_redo() {
    let vault = linked_vault("s2-undo");
    let mut app = boot("s2-undo", &vault, Some("notes/B.md"));
    let original = vault.read("notes/B.md");

    app.type_in_note("hello ");
    assert!(
        vault.read("notes/B.md").contains("hello "),
        "autosave did not write"
    );
    assert!(!app.probe().dirty);

    app.send(edit(EditorCommand::Undo));
    assert_eq!(vault.read("notes/B.md"), original, "undo not saved");
    app.send(edit(EditorCommand::Redo));
    assert!(
        vault.read("notes/B.md").contains("hello "),
        "redo not saved"
    );
    app.finish();
}

/// S11 / H1: the tracker rejects non-positive and non-finite hours, and the
/// dashboard totals what was accepted.
#[test]
fn s11_tracker_manual_sessions() {
    use crate::messages::ShellMessage;
    let vault = Vault::new("s11-tracker").file("Index.md", "# Index\n");
    let mut app = boot("s11-tracker", &vault, Some("Index.md"));
    app.send(Message::Shell(ShellMessage::TrackerToggle));
    app.send(Message::Shell(ShellMessage::TrackerManualDateChanged(
        "2026-11-20".to_string(),
    )));
    for bad in ["-1", "0", "inf", "NaN", "abc"] {
        app.send(Message::Shell(ShellMessage::TrackerManualHoursChanged(
            bad.to_string(),
        )));
        app.send(Message::Shell(ShellMessage::TrackerManualAdd));
    }
    // Nothing was accepted.
    assert_eq!(
        app.probe().tracker_sessions,
        0,
        "an invalid session was accepted"
    );
    assert!(app.shows("0m"), "the total should read 0m");
    app.send(Message::Shell(ShellMessage::TrackerManualHoursChanged(
        "2.5".to_string(),
    )));
    app.send(Message::Shell(ShellMessage::TrackerManualAdd));
    app.screenshot("tracker");
    assert_eq!(app.probe().tracker_sessions, 1);
    assert!(app.shows("2h 30m"), "total not shown as hours and minutes");
    app.finish();
}

/// S17: a note that is not UTF-8 is refused with a toast, and the note that
/// was open stays open and intact.
#[test]
fn s17_non_utf8_note_is_refused() {
    let vault = linked_vault("s17-utf8");
    std::fs::write(vault.root.join("notes/latin1.md"), [0x63, 0x61, 0x66, 0xe9]).unwrap();
    let mut app = boot("s17-utf8", &vault, Some("notes/B.md"));
    let before = vault.read("notes/B.md");

    app.expect_error_toast();
    app.send(Message::Vault(VaultMessage::SidebarFileClicked(
        "notes/latin1.md".to_string(),
    )));
    let probe = app.probe();
    assert_eq!(probe.active_path.as_deref(), Some("notes/B.md"));
    assert!(probe.toast.is_some_and(|t| t.contains("UTF-8")));
    assert_eq!(vault.read("notes/B.md"), before);
    assert_eq!(
        std::fs::read(vault.root.join("notes/latin1.md")).unwrap(),
        vec![0x63, 0x61, 0x66, 0xe9],
        "the unreadable note was modified"
    );
    app.finish();
}

/// S17: what happens when the open note is read-only on disk.
#[test]
fn s17_read_only_note() {
    use std::os::unix::fs::PermissionsExt;
    let vault = linked_vault("s17-readonly");
    let path = vault.root.join("notes/B.md");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    let mut app = boot("s17-readonly", &vault, Some("notes/B.md"));

    // Past the 400 ms autosave, before the 3 s toast auto-hide.
    app.send_quick(insert("edit "));
    app.wait(Duration::from_millis(700));
    let probe = app.probe();
    app.expect_error_toast();
    let written = vault.read("notes/B.md").contains("edit ");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    // Make the vault removable again.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    app.finish();
    assert!(
        !written,
        "a read-only note was overwritten (mode now {mode:o}); toast {:?}",
        probe.toast
    );
    assert!(
        probe.dirty,
        "the refused edits must stay unsaved in the buffer"
    );
    assert!(probe.buffer.contains("edit "));
    assert!(probe.toast.is_some_and(|t| t.contains("read-only")));
}

//! End-to-end scenarios. Each boots the real app on a fresh vault, drives it,
//! checks the outcome on disk and on screen, and leaves screenshots and
//! telemetry in `target/e2e/`.

use super::{Vault, boot};

/// A small but varied vault: linked notes, every block type, a PDF.
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

fn search(message: crate::messages::SearchMessage) -> Message {
    Message::Search(message)
}

/// BUG-05: Replace walks every match in order, never skipping one, and in
/// regex mode expands capture groups like Replace All.
#[test]
fn s6b_replace_one_visits_every_match_and_expands_captures() {
    use crate::messages::SearchMessage;

    let vault = Vault::new("replace").file("n.md", "x1 x2 x3 x4\n");
    let mut app = boot("s6b-replace", &vault, Some("n.md"));
    app.send(search(SearchMessage::QueryChanged("x".to_string())));
    app.wait(Duration::from_millis(300));
    app.send(search(SearchMessage::ReplaceChanged("yy".to_string())));
    for _ in 0..4 {
        app.send(search(SearchMessage::ReplaceOne));
    }
    assert_eq!(app.probe().buffer, "yy1 yy2 yy3 yy4\n");

    // A replacement containing the query moves on past itself.
    app.send(search(SearchMessage::QueryChanged("y".to_string())));
    app.wait(Duration::from_millis(300));
    app.send(search(SearchMessage::ReplaceChanged("yz".to_string())));
    app.send(search(SearchMessage::ReplaceOne));
    app.send(search(SearchMessage::ReplaceOne));
    assert_eq!(app.probe().buffer, "yzyz1 yy2 yy3 yy4\n");

    app.send(search(SearchMessage::RegexToggled(true)));
    app.send(search(SearchMessage::QueryChanged(r"y+(\d)".to_string())));
    app.wait(Duration::from_millis(300));
    app.send(search(SearchMessage::ReplaceChanged("<$1>".to_string())));
    app.send(search(SearchMessage::ReplaceOne));
    app.send(search(SearchMessage::ReplaceOne));
    assert_eq!(app.probe().buffer, "yzyz1 <2> <3> yy4\n");
    app.finish();
}

/// BUG-09: a two-letter vault search finds the note, and opening the hit
/// shows the query in the find bar with the match on its line selected.
#[test]
fn s5b_vault_search_hit_opens_at_the_match() {
    use crate::messages::SearchMessage;

    let vault = Vault::new("search-jump")
        .file("Index.md", "# Index\n")
        .file(
            "notes/ml.md",
            "# ML notes\n\nintro\n\nmore\n\nWhy ML works.\n",
        );
    let mut app = boot("s5b-search-jump", &vault, Some("Index.md"));
    app.send(search(SearchMessage::GlobalSearchOpen));
    app.send(search(SearchMessage::QueryChanged("works".to_string())));
    let mut results = Vec::new();
    for _ in 0..30 {
        app.wait(Duration::from_millis(100));
        app.settle();
        results = app.probe().search_results;
        if !results.is_empty() {
            break;
        }
    }
    assert_eq!(results, ["notes/ml.md"]);

    app.send(search(SearchMessage::ResultClicked(
        "notes/ml.md".to_string(),
    )));
    let probe = app.probe();
    assert_eq!(probe.active_path.as_deref(), Some("notes/ml.md"));
    assert!(
        probe.file_search_visible,
        "find bar not opened with the query"
    );
    assert_eq!(probe.cursor_line, 6, "caret not on the matching line");

    // Two letters are enough to search the vault.
    app.send(search(SearchMessage::GlobalSearchOpen));
    app.send(search(SearchMessage::QueryChanged("ML".to_string())));
    let mut found = false;
    for _ in 0..30 {
        app.wait(Duration::from_millis(100));
        app.settle();
        if app
            .probe()
            .search_results
            .iter()
            .any(|p| p == "notes/ml.md")
        {
            found = true;
            break;
        }
    }
    assert!(found, "two-letter query found nothing");
    app.finish();
}

/// `[[` suggests notes and inserts the chosen link (over an auto-paired
/// `]]`); `/` at a line start inserts a block; Escape and moving away close.
#[test]
fn s3b_inline_completion_for_links_and_blocks() {
    use crate::editor::buffer::{EditorCommand, Movement};
    use crate::messages::EditorMessage;

    let vault = research_vault();
    let mut app = boot("s3b-completion", &vault, Some("Index.md"));
    app.send(edit(EditorCommand::MoveCursor {
        movement: Movement::DocEnd,
        extend: false,
    }));
    app.send(insert("\nSee [[opt"));
    assert_eq!(
        app.probe().completion.first().map(String::as_str),
        Some("Optimizers")
    );
    app.redraw();
    app.screenshot("link-popup");
    app.send(Message::Editor(EditorMessage::CompletionAccept));
    let probe = app.probe();
    assert!(
        probe.buffer.ends_with("See [[Optimizers]]"),
        "{}",
        probe.buffer
    );
    assert!(probe.completion.is_empty());

    // An auto-paired `]]` after the caret is replaced, not doubled.
    app.send(insert(" [[]]"));
    for _ in 0..2 {
        app.send(edit(EditorCommand::MoveCursor {
            movement: Movement::Left,
            extend: false,
        }));
    }
    app.send(insert("Atte"));
    assert!(!app.probe().completion.is_empty());
    app.send(Message::Editor(EditorMessage::CompletionAccept));
    assert!(app.probe().buffer.ends_with("[[Optimizers]] [[Attention]]"));

    // `/` at a line start: blocks, narrowed as you type; Escape closes.
    app.send(insert("\n/tab"));
    assert_eq!(
        app.probe().completion.first().map(String::as_str),
        Some("Table")
    );
    app.redraw();
    app.screenshot("slash-popup");
    app.send(Message::Editor(EditorMessage::CompletionClose));
    assert!(app.probe().completion.is_empty());
    app.send(insert("l"));
    assert_eq!(
        app.probe().completion.first().map(String::as_str),
        Some("Table")
    );
    app.send(Message::Editor(EditorMessage::CompletionAccept));
    assert!(app.probe().buffer.contains("\n| Column 1 | Column 2 |"));

    // Moving the caret away closes the popup without typing.
    app.send(insert("\n[[In"));
    assert!(!app.probe().completion.is_empty());
    app.send(edit(EditorCommand::MoveCursor {
        movement: Movement::Home,
        extend: false,
    }));
    assert!(app.probe().completion.is_empty());
    app.finish();
}

fn png(width: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(width, 1, image::Rgba([9, 9, 9, 255]))
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encode png");
    bytes
}

/// Images decode off the UI thread; a missing one shows once the file
/// appears; `img.png` in two folders is each note's own image.
#[test]
fn s14b_note_images_load_refresh_and_stay_per_folder() {
    let vault = Vault::new("note-images")
        .file("a/note.md", "# A\n\n![x](img.png)\n")
        .file("b/note.md", "# B\n\n![x](img.png)\n");
    std::fs::write(vault.root.join("b/img.png"), png(7)).unwrap();
    let mut app = boot("s14b-note-images", &vault, Some("a/note.md"));
    assert!(
        app.probe().images.is_empty(),
        "a/img.png does not exist yet"
    );

    std::fs::write(vault.root.join("a/img.png"), png(3)).unwrap();
    app.send(Message::Vault(VaultMessage::VaultFilesChanged(vec![
        "a/img.png".to_string(),
    ])));
    assert_eq!(app.probe().images, [("img.png".to_string(), 3.0)]);

    app.send(Message::Vault(VaultMessage::SidebarFileClicked(
        "b/note.md".to_string(),
    )));
    assert_eq!(
        app.probe().images,
        [("img.png".to_string(), 7.0)],
        "b's note showed a's image"
    );
    app.finish();
}

/// A `pdf://…&crop=` image renders the region live from the PDF, beside an
/// ordinary link to the same PDF, and a crop of a missing PDF just fails
/// quietly (no error toast, not retried).
#[test]
fn s18_pdf_crop_renders_inline_in_a_note() {
    let crop = "pdf://papers/dummy.pdf?page=1&crop=60,60,300,120";
    let missing = "pdf://papers/gone.pdf?page=1&crop=0,0,100,100";
    let vault = research_vault().file(
        "notes/Reading.md",
        &format!(
            "# Reading\n\n![dummy, p. 1]({crop})\n\nSee [the paper](pdf://papers/dummy.pdf?page=1).\n\n![gone]({missing})\n"
        ),
    );
    let mut app = boot("s18-pdf-crop", &vault, Some("notes/Reading.md"));
    app.screenshot("crop");
    let probe = app.probe();
    let width = probe
        .images
        .iter()
        .find(|(key, _)| key == crop)
        .map(|(_, width)| *width)
        .expect("the crop rendered into the note's images");
    assert_eq!(width, 400.0, "shown at the crop's size in CSS px");
    assert!(probe.images.iter().all(|(key, _)| key != missing));
    app.finish();
}

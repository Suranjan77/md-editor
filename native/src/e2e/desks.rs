//! End-to-end tests for saved desks and palette Open-to-Side: saving, restoring, relaunch persistence, data safety, and rename/delete propagation.

use super::boot;
use super::scenarios::research_vault;
use crate::app::workspace::{PaneId, SplitAxis};
use crate::editor::buffer::{EditorCommand, Movement};
use crate::messages::{DesksMessage, EditorMessage, Message, ShellMessage, Shortcut, VaultMessage};

fn open(path: &str) -> Message {
    Message::Vault(VaultMessage::SidebarFileClicked(path.to_string()))
}

fn shortcut(s: Shortcut) -> Message {
    Message::KeyboardShortcut(s)
}

fn edit(command: EditorCommand) -> Message {
    Message::Editor(EditorMessage::Command(command))
}

#[test]
fn d1_save_restore_and_persist_two_pane_desk() {
    let vault = research_vault();
    let mut app = boot("d1-desks", &vault, Some("Index.md"));

    // Split with a PDF in Pane B, stacked horizontally, with a custom split ratio.
    app.send(Message::Vault(VaultMessage::OpenToSide(
        "papers/dummy.pdf".to_string(),
    )));
    app.send(shortcut(Shortcut::ToggleSplitAxis));
    app.send(Message::Shell(ShellMessage::WorkspaceMeasured(
        iced::Size::new(900.0, 700.0),
    )));
    app.send(Message::Shell(ShellMessage::SplitViewDragStart));
    app.send(Message::Shell(ShellMessage::SplitViewDragging(
        iced::Point::new(0.0, 0.0),
    )));
    app.send(Message::Shell(ShellMessage::SplitViewDragging(
        iced::Point::new(0.0, 105.0),
    )));
    app.send(Message::Shell(ShellMessage::SplitViewDragEnd));

    let before = app.probe();
    assert!(before.split_view);
    assert_eq!(before.split_axis, SplitAxis::Horizontal);
    assert!((before.split_ratio - 0.65).abs() < 0.02, "{before:?}");
    assert_eq!(before.focused_pane, PaneId::B);
    assert_eq!(before.pdf_path.as_deref(), Some("papers/dummy.pdf"));
    assert_eq!(before.other_pane_path.as_deref(), Some("Index.md"));

    // Open Save Desk dialog, verify empty validation keeps modal open, then save.
    app.send(shortcut(Shortcut::SaveDesk));
    assert!(app.probe().modal_open);
    app.send(Message::Desks(DesksMessage::Save("   ".to_string())));
    assert!(app.probe().modal_open);
    assert!(app.shows("Desk name cannot be empty"));

    app.send(Message::Vault(VaultMessage::NameModalInputChanged(
        "Paper Study".to_string(),
    )));
    app.send(Message::Shell(ShellMessage::OverlaySubmitCurrent));
    let after_save = app.probe();
    assert!(!after_save.modal_open);
    assert_eq!(after_save.saved_desks, vec!["Paper Study"]);

    // Verify Desks dialog displays the saved desk summary.
    app.send(shortcut(Shortcut::SwitchDesk));
    assert!(app.probe().modal_open);
    assert!(app.shows("Paper Study"));
    assert!(app.shows("Split · 2 tabs"));
    app.screenshot("desks-dialog");
    app.send(shortcut(Shortcut::Escape));
    assert!(!app.probe().modal_open);

    // Change the workspace completely: unsplit and open another note.
    app.send(shortcut(Shortcut::Unsplit));
    app.send(Message::Shell(ShellMessage::SplitViewReset));
    app.send(open("notes/Optimizers.md"));
    let changed = app.probe();
    assert!(!changed.split_view);
    assert_eq!(changed.active_path.as_deref(), Some("notes/Optimizers.md"));

    // Switch back to the saved desk via the Command Palette.
    app.send(Message::Shell(ShellMessage::CommandPaletteOpen));
    app.send(Message::Shell(ShellMessage::CommandPaletteQueryChanged(
        "Paper Study".to_string(),
    )));
    app.send(Message::Shell(ShellMessage::OverlaySubmitCurrent));
    let restored = app.probe();
    assert!(!restored.palette_open);
    assert!(restored.split_view);
    assert_eq!(restored.split_axis, SplitAxis::Horizontal);
    assert!((restored.split_ratio - 0.65).abs() < 0.02, "{restored:?}");
    assert_eq!(restored.focused_pane, PaneId::B);
    assert_eq!(restored.pdf_path.as_deref(), Some("papers/dummy.pdf"));
    assert_eq!(restored.other_pane_path.as_deref(), Some("Index.md"));

    // Persist across a fresh boot on the same vault/data dir.
    let mut app = app.restart("d1-desks-relaunch", None);
    app.settle();
    let relaunched = app.probe();
    assert_eq!(relaunched.saved_desks, vec!["Paper Study"]);
    assert!(relaunched.split_view);
    assert_eq!(relaunched.split_axis, SplitAxis::Horizontal);
    assert!((relaunched.split_ratio - 0.65).abs() < 0.02);
    app.finish();
}

#[test]
fn d2_switch_desk_flushes_dirty_or_refuses_on_write_failure() {
    use std::os::unix::fs::PermissionsExt;

    let vault = research_vault();
    let mut app = boot("d2-safety", &vault, Some("Index.md"));
    app.send(Message::Desks(DesksMessage::Save("Index Desk".to_string())));

    app.send(open("notes/Attention.md"));
    app.send(Message::Desks(DesksMessage::Save(
        "Attention Desk".to_string(),
    )));

    // Dirty edit without waiting for autosave -> switching desks flushes it to disk first.
    app.send_quick(edit(EditorCommand::MoveCursor {
        movement: Movement::DocEnd,
        extend: false,
    }));
    app.send_quick(edit(EditorCommand::InsertText(
        "\nFlushed before desk switch.".to_string(),
    )));
    assert!(app.probe().dirty);

    app.send(Message::Desks(DesksMessage::Switch(
        "Index Desk".to_string(),
    )));
    assert_eq!(app.probe().active_path.as_deref(), Some("Index.md"));
    assert!(
        vault
            .read("notes/Attention.md")
            .contains("Flushed before desk switch."),
        "switching desks must flush dirty edits"
    );

    // Now switch back, make the file read-only, dirty it, and verify switch is refused.
    app.send(Message::Desks(DesksMessage::Switch(
        "Attention Desk".to_string(),
    )));
    let attention_path = vault.root.join("notes/Attention.md");
    std::fs::set_permissions(&attention_path, std::fs::Permissions::from_mode(0o444)).unwrap();

    app.send_quick(edit(EditorCommand::InsertText("UNSAVED ".to_string())));
    assert!(app.probe().dirty);
    app.expect_error_toast();
    app.send(Message::Desks(DesksMessage::Switch(
        "Index Desk".to_string(),
    )));

    let refused = app.probe();
    assert_eq!(refused.active_path.as_deref(), Some("notes/Attention.md"));
    assert!(refused.dirty);
    assert!(refused.buffer.contains("UNSAVED "));
    assert!(
        refused
            .toast
            .as_deref()
            .is_some_and(|t| t.contains("Not switching desks"))
    );

    // Restore write permission so Vault::drop can clean up cleanly.
    std::fs::set_permissions(&attention_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    app.send(shortcut(Shortcut::Save));
    app.finish();
}

#[test]
fn d3_rename_and_delete_propagate_to_saved_desks() {
    let vault = research_vault();
    let mut app = boot("d3-propagate", &vault, Some("Index.md"));
    app.send(Message::Vault(VaultMessage::OpenToSide(
        "notes/Attention.md".to_string(),
    )));
    app.send(Message::Desks(DesksMessage::Save("Pair".to_string())));

    // Rename Pane B's file and delete Pane A's file.
    app.send(Message::Vault(VaultMessage::RenameEntryDialog(
        "notes/Attention.md".to_string(),
    )));
    app.send(Message::Vault(VaultMessage::NameModalSubmit(
        "AttentionRenamed.md".to_string(),
    )));
    app.send(Message::Vault(VaultMessage::DeleteFile(
        "Index.md".to_string(),
    )));

    // Move away, unsplit, then restore the saved desk.
    app.send(shortcut(Shortcut::Unsplit));
    app.send(open("notes/Optimizers.md"));
    app.send(Message::Desks(DesksMessage::Switch("Pair".to_string())));

    let probe = app.probe();
    assert!(probe.split_view);
    assert_eq!(probe.focused_pane, PaneId::B);
    assert_eq!(
        probe.active_path.as_deref(),
        Some("notes/AttentionRenamed.md")
    );
    assert_eq!(probe.other_pane_path, None);

    // Deleting the saved desk removes it from the list.
    app.send(Message::Desks(DesksMessage::Delete("Pair".to_string())));
    assert!(app.probe().saved_desks.is_empty());
    app.finish();
}

#[test]
fn d5_pane_b_undo_history_survives_desk_switch_and_pane_close() {
    let vault = research_vault();
    let mut app = boot("d5-undo", &vault, Some("Index.md"));
    app.send(Message::Vault(VaultMessage::OpenToSide(
        "notes/Attention.md".to_string(),
    )));

    // Edit Attention.md in Pane B, building undo history, then save a desk.
    let original = app.probe().buffer;
    app.send(edit(EditorCommand::MoveCursor {
        movement: Movement::DocEnd,
        extend: false,
    }));
    app.send(edit(EditorCommand::InsertText(
        "\nUndoable line in Pane B.".to_string(),
    )));
    assert!(app.probe().buffer.contains("Undoable line in Pane B."));
    app.send(Message::Desks(DesksMessage::Save("Two Panes".to_string())));

    // Switch to an unsplit desk on another note, then switch back to Two Panes.
    app.send(shortcut(Shortcut::Unsplit));
    app.send(open("notes/Optimizers.md"));
    app.send(Message::Desks(DesksMessage::Save("Single".to_string())));

    app.send(Message::Desks(DesksMessage::Switch(
        "Two Panes".to_string(),
    )));
    assert_eq!(app.probe().focused_pane, PaneId::B);
    assert_eq!(
        app.probe().active_path.as_deref(),
        Some("notes/Attention.md")
    );

    // Undo in Pane B must revert the edit made before switching desks.
    app.send(edit(EditorCommand::Undo));
    assert_eq!(app.probe().buffer, original);
    app.finish();
}

#[test]
fn d6_daily_desk_rollover_and_synthesis_brief() {
    let vault = research_vault()
        .file(
            "daily/2020-01-01.md",
            "# 2020-01-01\n\n- [ ] Roll me over\n- [x] Already done\n",
        )
        .file(
            "notes/Questions.md",
            "# Questions\n\n> [!question] Why does attention scale quadratically?\n",
        );

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let daily_rel = format!("daily/{today}.md");

    let mut app = boot("d6-daily", &vault, Some("Index.md"));
    app.send(shortcut(Shortcut::DailyNote));

    let probe = app.probe();
    assert_eq!(probe.active_path.as_deref(), Some(daily_rel.as_str()));
    assert!(probe.buffer.contains("- [ ] Roll me over"));
    assert!(!probe.buffer.contains("Already done"));
    assert!(probe.buffer.contains("## Synthesis Brief"));
    assert!(
        probe
            .buffer
            .contains("Why does attention scale quadratically?")
    );
    assert!(probe.buffer.contains("[[notes/Questions|Questions]]"));

    // Type user content, switch away, and press Ctrl+D again -> must not overwrite!
    app.send(edit(EditorCommand::InsertText(
        "Daily reflection line.".to_string(),
    )));
    app.send(open("Index.md"));
    app.send(shortcut(Shortcut::DailyNote));

    let revisited = app.probe();
    assert_eq!(revisited.active_path.as_deref(), Some(daily_rel.as_str()));
    assert!(revisited.buffer.contains("Daily reflection line."));
    app.finish();
}

#[test]
fn d7_study_tracker_sidecar_sync_across_databases() {
    let vault = research_vault();
    let mut app = boot("d7-tracker-1", &vault, Some("Index.md"));

    // Log a manual study session in DataDir 1.
    app.send(Message::Shell(ShellMessage::TrackerToggle));
    app.send(Message::Shell(ShellMessage::TrackerManualDateChanged(
        "2026-11-26".to_string(),
    )));
    app.send(Message::Shell(ShellMessage::TrackerManualHoursChanged(
        "1.5".to_string(),
    )));
    app.send(Message::Shell(ShellMessage::TrackerManualNotesChanged(
        "Synced via sidecar".to_string(),
    )));
    app.send(Message::Shell(ShellMessage::TrackerManualAdd));
    assert_eq!(app.probe().tracker_sessions, 1);
    app.finish();

    let sidecar = vault.root.join(".md-editor").join("study-sessions.jsonl");
    assert!(
        sidecar.is_file(),
        "study-sessions.jsonl must exist after saving a session"
    );
    assert!(
        std::fs::read_to_string(&sidecar)
            .unwrap()
            .contains("Synced via sidecar")
    );

    // Clear SQLite tracker tables to simulate a brand-new empty DataDir 2.
    {
        let fresh_db = md_editor_core::state::AppState::new();
        let db = fresh_db.db.lock().unwrap();
        db.execute_batch("DELETE FROM tracker_sessions; DELETE FROM tracker_sidecar_sync;")
            .unwrap();
    }

    let mut app2 = boot("d7-tracker-2", &vault, None);
    app2.send(Message::Shell(ShellMessage::TrackerToggle));
    let probe2 = app2.probe();
    assert_eq!(probe2.tracker_sessions, 1);
    assert_eq!(
        probe2.tracker_last_notes.as_deref(),
        Some("Synced via sidecar")
    );
    assert!(app2.shows("Synced via sidecar"));
    app2.finish();
}

#[test]
fn d4_command_palette_open_to_side() {
    let vault = research_vault();
    let mut app = boot("d4-palette-side", &vault, Some("Index.md"));

    app.send(Message::Shell(ShellMessage::CommandPaletteOpen));
    app.send(Message::Shell(ShellMessage::CommandPaletteQueryChanged(
        "Optimizers".to_string(),
    )));
    app.send(Message::Shell(ShellMessage::CommandPaletteOpenToSide));

    let probe = app.probe();
    assert!(!probe.palette_open);
    assert!(probe.split_view);
    assert_eq!(probe.focused_pane, PaneId::B);
    assert_eq!(probe.active_path.as_deref(), Some("notes/Optimizers.md"));
    assert_eq!(probe.other_pane_path.as_deref(), Some("Index.md"));
    app.finish();
}

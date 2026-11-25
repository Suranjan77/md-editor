//! Workspace flows: document tabs (feature plan §1.5), Settings applied live (theme, editor typography, excluded folders) and context menus opening where the pointer is.

use std::time::Duration;

use super::scenarios::research_vault;
use iced_program::Program;

use super::{Session, Vault, boot};
use crate::app::MdEditor;
use crate::design::{ContentMeasure, ThemeMode};
use crate::editor::buffer::EditorCommand;
use crate::messages::{
    EditorMessage, Message, SettingsMessage, Shortcut, TabsMessage, VaultMessage,
};

fn open(path: &str) -> Message {
    Message::Vault(VaultMessage::SidebarFileClicked(path.to_string()))
}

fn type_text(text: &str) -> Message {
    Message::Editor(EditorMessage::Command(EditorCommand::InsertText(
        text.to_string(),
    )))
}

fn tab(message: TabsMessage) -> Message {
    Message::Tabs(message)
}

fn settings(message: SettingsMessage) -> Message {
    Message::Settings(message)
}

const NOTES: [&str; 3] = ["Index.md", "notes/Attention.md", "notes/Optimizers.md"];

/// Open three notes and a PDF, in that order.
fn open_four<P: Program<State = MdEditor, Message = Message> + 'static>(app: &mut Session<P>) {
    for path in NOTES {
        app.send(open(path));
    }
    app.send(open("papers/dummy.pdf"));
}

/// Three notes and a PDF open as four tabs; switching between them flushes
/// edits, and a note's undo history survives a round trip through the PDF.
#[test]
fn tabs_switch_flush_and_keep_undo_history() {
    let vault = research_vault();
    let mut app = boot("tabs-switch", &vault, None);
    open_four(&mut app);
    let probe = app.probe();
    assert_eq!(
        probe.open_tabs,
        vec![NOTES[0], NOTES[1], NOTES[2], "papers/dummy.pdf"]
    );
    assert_eq!(probe.active_tab.as_deref(), Some("papers/dummy.pdf"));
    app.screenshot("four-tabs");

    app.send(tab(TabsMessage::SelectOrdinal(2)));
    assert_eq!(app.probe().active_path.as_deref(), Some(NOTES[1]));
    app.send_quick(type_text("typed "));
    app.send(tab(TabsMessage::SelectOrdinal(4)));
    assert_eq!(app.probe().active_tab.as_deref(), Some("papers/dummy.pdf"));
    assert!(
        vault.read(NOTES[1]).contains("typed "),
        "edits were not flushed when switching tabs"
    );

    app.send(tab(TabsMessage::SelectOrdinal(2)));
    app.send(Message::Editor(EditorMessage::Command(EditorCommand::Undo)));
    assert!(
        !app.probe().buffer.contains("typed "),
        "undo history did not survive switching tabs"
    );

    // Ctrl+Tab goes back to the most recently used tab, the PDF.
    app.send(tab(TabsMessage::MruNext));
    assert_eq!(app.probe().active_tab.as_deref(), Some("papers/dummy.pdf"));
    app.finish();
}

/// Closing a dirty note's tab writes it first; Ctrl+Shift+T brings it back.
#[test]
fn tabs_close_flushes_and_reopen_restores() {
    let vault = research_vault();
    let mut app = boot("tabs-close", &vault, None);
    app.send(open(NOTES[0]));
    app.send(open(NOTES[1]));
    app.send_quick(type_text("unsaved "));
    app.send(tab(TabsMessage::CloseActive));

    let probe = app.probe();
    assert_eq!(probe.open_tabs, vec![NOTES[0]]);
    assert_eq!(probe.active_path.as_deref(), Some(NOTES[0]));
    assert!(
        vault.read(NOTES[1]).contains("unsaved "),
        "closing a dirty tab lost its edits"
    );

    app.send(tab(TabsMessage::Reopen));
    let probe = app.probe();
    assert_eq!(probe.open_tabs, vec![NOTES[0], NOTES[1]]);
    assert_eq!(probe.active_path.as_deref(), Some(NOTES[1]));
    assert!(probe.buffer.contains("unsaved "));
    app.finish();
}

/// Renaming an open note renames its tab and keeps the buffer.
#[test]
fn tabs_follow_a_rename() {
    let vault = research_vault();
    let mut app = boot("tabs-rename", &vault, None);
    app.send(open(NOTES[0]));
    app.send(open(NOTES[2]));
    app.send_quick(type_text("kept "));
    app.send_quick(Message::Vault(VaultMessage::RenameEntryDialog(
        NOTES[2].to_string(),
    )));
    app.send(Message::Vault(VaultMessage::NameModalSubmit(
        "Adam.md".to_string(),
    )));

    let probe = app.probe();
    assert_eq!(probe.open_tabs, vec![NOTES[0], "notes/Adam.md"]);
    assert_eq!(probe.active_path.as_deref(), Some("notes/Adam.md"));
    assert!(probe.buffer.contains("kept "));
    app.finish();
}

/// Relaunching restores the vault's tabs and which one was active.
#[test]
fn tabs_are_restored_on_relaunch() {
    let vault = research_vault();
    let mut app = boot("tabs-relaunch", &vault, None);
    open_four(&mut app);
    app.send(tab(TabsMessage::SelectOrdinal(2)));
    let before = app.probe();

    let mut app = app.restart("tabs-relaunch-2", None);
    app.settle();
    let after = app.probe();
    assert_eq!(after.open_tabs, before.open_tabs);
    assert_eq!(after.active_tab.as_deref(), Some(NOTES[1]));
    assert_eq!(after.active_path.as_deref(), Some(NOTES[1]));
    app.finish();
}

/// Switching to Light repaints the editor in the light tokens; editor font
/// size and line width apply immediately.
#[test]
fn settings_apply_theme_and_editor_typography_live() {
    let vault = research_vault();
    let mut app = boot("settings-live", &vault, Some(NOTES[1]));

    app.send(settings(SettingsMessage::ThemeModeSelected(
        ThemeMode::Light,
    )));
    let probe = app.probe();
    assert!(probe.light_theme);
    let light = app.screenshot("light");

    app.send(settings(SettingsMessage::EditorFontSizeChanged(20.0)));
    app.send(settings(SettingsMessage::ContentMeasureSelected(
        ContentMeasure::Narrow,
    )));
    assert_eq!(app.probe().editor_font_size, 20.0);
    let larger = app.screenshot("light-20px-narrow");
    assert_ne!(
        std::fs::read(&light).unwrap(),
        std::fs::read(&larger).unwrap(),
        "changing the editor font size did not repaint the note"
    );

    app.send(settings(SettingsMessage::ThemeModeSelected(
        ThemeMode::Dark,
    )));
    assert!(!app.probe().light_theme);
    app.send(settings(SettingsMessage::Toggle));
    app.screenshot("settings-page");
    app.finish();
}

/// A folder named in Settings → Files disappears from the tree and the index
/// once applied, and comes back when removed.
#[test]
fn excluded_folders_apply_to_the_vault() {
    let vault: Vault = research_vault().file("drafts/wip.md", "# wip\n");
    let mut app = boot("excluded", &vault, Some(NOTES[0]));
    assert!(app.probe().entries.iter().any(|e| e == "drafts/wip.md"));

    app.send(settings(SettingsMessage::ExcludedFoldersChanged(
        "drafts".to_string(),
    )));
    app.send(settings(SettingsMessage::ExcludedFoldersApply));
    app.settle();
    assert!(
        !app.probe().entries.iter().any(|e| e.starts_with("drafts")),
        "excluded folder still listed"
    );

    app.send(settings(SettingsMessage::ExcludedFoldersChanged(
        String::new(),
    )));
    app.send(settings(SettingsMessage::ExcludedFoldersApply));
    app.settle();
    assert!(app.probe().entries.iter().any(|e| e == "drafts/wip.md"));
    app.finish();
}

/// Right-clicking a sidebar row opens its menu at the pointer, not at a
/// remembered position; Escape closes it.
#[test]
fn context_menus_open_at_the_pointer() {
    let vault = research_vault();
    let mut app = boot("context-menu", &vault, Some(NOTES[0]));

    app.right_click_at(80.0, 103.0);
    let at = app.probe().context_menu.expect("sidebar context menu");
    assert!(
        (at.0 - 80.0).abs() < 1.0 && (at.1 - 103.0).abs() < 1.0,
        "menu opened at {at:?}"
    );
    app.screenshot("sidebar-menu");
    app.send(Message::KeyboardShortcut(Shortcut::Escape));
    assert!(app.probe().context_menu.is_none());

    // The editor's menu, far from the first click.
    app.right_click_at(700.0, 400.0);
    let at = app.probe().context_menu.expect("editor context menu");
    assert!((at.0 - 700.0).abs() < 1.0 && (at.1 - 400.0).abs() < 1.0);
    app.wait(Duration::from_millis(10));
    app.finish();
}

/// Closing the palette or find hands the keyboard back to the note, so typing
/// continues where it was; F6 into the main region does the same.
#[test]
fn keyboard_returns_to_the_note_after_overlays() {
    let vault = research_vault();
    let mut app = boot("focus-return", &vault, Some(NOTES[2]));

    app.send(Message::Shell(
        crate::messages::ShellMessage::CommandPaletteOpen,
    ));
    app.send(Message::KeyboardShortcut(Shortcut::Escape));
    app.type_text("Q");
    assert!(
        app.probe().buffer.contains('Q'),
        "typing after closing the palette did not reach the note"
    );

    app.send(Message::KeyboardShortcut(Shortcut::Search));
    app.send(Message::KeyboardShortcut(Shortcut::Escape));
    app.type_text("W");
    assert!(
        app.probe().buffer.contains('W'),
        "find did not return focus"
    );

    // F6 away to the toolbar and around back to the main region.
    for _ in 0..4 {
        app.send(Message::KeyboardShortcut(Shortcut::CycleFocusNext));
    }
    app.type_text("E");
    assert!(
        app.probe().buffer.contains('E'),
        "F6 into the note did not focus it"
    );
    app.finish();
}

/// A 2×2 PNG written outside any vault, as a file the user picks or drops.
fn external_png(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("md-editor-e2e-drop-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("create drop dir");
    let path = dir.join(name);
    image::RgbaImage::from_pixel(2, 2, image::Rgba([20, 120, 220, 255]))
        .save(&path)
        .expect("write png");
    path
}

/// "Insert image…": the picked files become attachments and image lines; with
/// the beside-note setting they land in `notes/attachments/`.
#[test]
fn picked_images_follow_the_attachments_setting() {
    let vault = research_vault();
    let mut app = boot("insert-image", &vault, Some(NOTES[1]));
    app.send(settings(SettingsMessage::AttachmentsModeSelected(
        crate::messages::AttachmentsMode::BesideNote,
    )));
    let picked = external_png("figure.png");
    app.send(Message::Editor(EditorMessage::ImagesPicked(vec![picked])));
    app.settle();

    assert!(
        vault.exists("notes/attachments/figure.png"),
        "picked image not stored beside the note"
    );
    assert!(app.probe().buffer.contains("](attachments/figure.png)"));
    app.finish();
}

/// Dropping a file over the sidebar imports it into the vault and never
/// overwrites a file of the same name; dropping over the note inserts it.
#[test]
fn dropped_files_import_into_the_tree_or_the_note() {
    let vault = research_vault();
    let mut app = boot("drop", &vault, Some(NOTES[0]));
    let dropped = external_png("dropped.png");

    app.send(Message::Shell(crate::messages::ShellMessage::CursorMoved(
        100.0, 150.0,
    )));
    app.send(Message::Shell(crate::messages::ShellMessage::FileDropped(
        dropped.clone(),
    )));
    assert!(vault.exists("dropped.png"), "sidebar drop did not import");
    let before = std::fs::read(vault.root.join("dropped.png")).unwrap();

    // The same name again is refused, not overwritten.
    std::fs::write(&dropped, b"not the same bytes").unwrap();
    app.send(Message::Shell(crate::messages::ShellMessage::FileDropped(
        dropped.clone(),
    )));
    app.expect_error_toast();
    assert_eq!(
        std::fs::read(vault.root.join("dropped.png")).unwrap(),
        before
    );

    // Over the note: inserted as an image line.
    let over_note = external_png("inline.png");
    app.send(Message::Shell(crate::messages::ShellMessage::CursorMoved(
        700.0, 400.0,
    )));
    app.send(Message::Shell(crate::messages::ShellMessage::FileDropped(
        over_note,
    )));
    app.settle();
    assert!(app.probe().buffer.contains("inline.png"));
    app.finish();
}

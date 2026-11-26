//! Two-pane workspace scenarios: Note+Note and PDF+PDF side by side, focus and saving, one file per pane, unsplitting, and restoring the panes on relaunch.

use super::scenarios::research_vault;
use super::{Vault, boot};
use crate::app::workspace::PaneId;
use crate::editor::buffer::{EditorCommand, Movement};
use crate::messages::{EditorMessage, Message, ShellMessage, Shortcut, VaultMessage};

fn open(path: &str) -> Message {
    Message::Vault(VaultMessage::SidebarFileClicked(path.to_string()))
}

fn shortcut(s: Shortcut) -> Message {
    Message::KeyboardShortcut(s)
}

fn edit(command: EditorCommand) -> Message {
    Message::Editor(EditorMessage::Command(command))
}

fn type_at_end(text: &str) -> [Message; 2] {
    [
        edit(EditorCommand::MoveCursor {
            movement: Movement::DocEnd,
            extend: false,
        }),
        edit(EditorCommand::InsertText(text.to_string())),
    ]
}

/// Two notes side by side: each pane edits its own; leaving a pane saves
/// it; a file open in one pane is never opened again in the other.
#[test]
fn p1_two_notes_side_by_side() {
    let vault = research_vault();
    let mut app = boot("p1-two-notes", &vault, Some("Index.md"));
    app.send(open("notes/Attention.md"));

    // Move Attention to a new pane beside Index.
    app.send(shortcut(Shortcut::MoveTabToOtherPane));
    let probe = app.probe();
    assert!(probe.split_view);
    assert_eq!(probe.focused_pane, PaneId::B);
    assert_eq!(probe.active_path.as_deref(), Some("notes/Attention.md"));
    assert_eq!(probe.open_tabs, ["notes/Attention.md"]);
    assert_eq!(probe.other_pane_path.as_deref(), Some("Index.md"));

    for message in type_at_end("\nWritten in pane B.") {
        app.send_quick(message);
    }
    assert!(app.probe().dirty);
    app.send_quick(shortcut(Shortcut::FocusOtherPane));
    let probe = app.probe();
    assert_eq!(probe.focused_pane, PaneId::A);
    assert_eq!(probe.active_path.as_deref(), Some("Index.md"));
    assert!(
        vault
            .read("notes/Attention.md")
            .contains("Written in pane B."),
        "the note was not saved when its pane lost focus"
    );

    for message in type_at_end("\nWritten in pane A.") {
        app.send(message);
    }

    // Asking for the file the other pane has focuses it there.
    app.send(open("notes/Attention.md"));
    let probe = app.probe();
    assert_eq!(probe.focused_pane, PaneId::B);
    assert_eq!(probe.active_path.as_deref(), Some("notes/Attention.md"));
    assert_eq!(probe.open_tabs, ["notes/Attention.md"], "opened twice");
    assert!(vault.read("Index.md").contains("Written in pane A."));

    app.send(shortcut(Shortcut::ToggleSplitAxis));
    app.screenshot("stacked");
    app.send(shortcut(Shortcut::ToggleSplitAxis));
    app.screenshot("side-by-side");

    // Unsplit from pane B: its tabs join pane A's, nothing is lost.
    app.send(Message::Shell(ShellMessage::ClosePane(PaneId::B)));
    let probe = app.probe();
    assert!(!probe.split_view);
    assert_eq!(probe.focused_pane, PaneId::A);
    let mut tabs = probe.open_tabs.clone();
    tabs.sort();
    assert_eq!(tabs, ["Index.md", "notes/Attention.md"]);
    app.finish();
}

/// The same PDF cannot be in both panes, but two different PDFs can sit side
/// by side, each with its own page and zoom.
#[test]
fn p2_two_pdfs_side_by_side() {
    let vault = research_vault().fixture("papers/large.pdf", "pdf/large-500-pages.pdf");
    let mut app = boot("p2-two-pdfs", &vault, Some("papers/dummy.pdf"));
    app.send(Message::Vault(VaultMessage::OpenToSide(
        "papers/large.pdf".to_string(),
    )));
    let probe = app.probe();
    assert!(probe.split_view);
    assert_eq!(probe.focused_pane, PaneId::B);
    assert_eq!(probe.pdf_path.as_deref(), Some("papers/large.pdf"));
    assert_eq!(probe.other_pane_path.as_deref(), Some("papers/dummy.pdf"));
    super::flows::wait_for(&mut app, "pages of the second PDF", |p| {
        p.pdf_pages == 500 && p.pdf_rendered > 0
    });
    app.send(Message::Pdf(crate::messages::PdfMessage::ZoomChanged(1.5)));
    assert!((app.probe().pdf_zoom - 1.5).abs() < 0.01);

    app.send(shortcut(Shortcut::FocusOtherPane));
    let probe = app.probe();
    assert_eq!(probe.pdf_path.as_deref(), Some("papers/dummy.pdf"));
    assert!(probe.pdf_rendered > 0, "the first PDF kept its pages");
    assert!(
        (probe.pdf_zoom - 1.5).abs() > 0.01,
        "zoom is per pane, not shared"
    );
    app.screenshot("two-pdfs");
    app.finish();
}

/// A split session comes back split: both panes' tabs and the axis.
#[test]
fn p3_panes_are_restored_on_relaunch() {
    let vault: Vault = research_vault();
    let mut app = boot("p3-panes-relaunch", &vault, Some("Index.md"));
    app.send(open("notes/Optimizers.md"));
    app.send(shortcut(Shortcut::MoveTabToOtherPane));
    app.send(shortcut(Shortcut::ToggleSplitAxis));
    app.send(shortcut(Shortcut::FocusOtherPane));
    let before = app.probe();
    assert!(before.split_view);
    assert_eq!(before.active_path.as_deref(), Some("Index.md"));

    let mut app = app.restart("p3-panes-relaunch-2", None);
    app.settle();
    let after = app.probe();
    assert!(after.split_view, "the split was not restored");
    let mut shown = [
        after.active_path.clone().unwrap_or_default(),
        after.other_pane_path.clone().unwrap_or_default(),
    ];
    shown.sort();
    assert_eq!(shown, ["Index.md", "notes/Optimizers.md"]);
    app.finish();
}

/// Switching theme repaints the note in the pane without focus too: its
/// text was left in the old theme's colours (light grey on a light page).
#[test]
fn p4_theme_switch_repaints_both_panes() {
    use crate::design::{ActiveTheme, ThemeMode};
    use crate::messages::SettingsMessage;

    let vault = research_vault();
    let mut app = boot("p4-theme-both-panes", &vault, Some("Index.md"));
    app.send(Message::Vault(VaultMessage::OpenToSide(
        "papers/dummy.pdf".to_string(),
    )));
    let probe = app.probe();
    assert_eq!(probe.focused_pane, PaneId::B);
    assert_eq!(probe.other_pane_path.as_deref(), Some("Index.md"));
    assert_eq!(probe.other_pane_theme, Some(ActiveTheme::Dark));

    app.send(Message::Settings(SettingsMessage::ThemeModeSelected(
        ThemeMode::Light,
    )));
    assert_eq!(
        app.probe().other_pane_theme,
        Some(ActiveTheme::Light),
        "the note beside the PDF kept the dark theme's colours"
    );
    app.screenshot("light");
    app.send(Message::Settings(SettingsMessage::ThemeModeSelected(
        ThemeMode::Dark,
    )));
    assert_eq!(app.probe().other_pane_theme, Some(ActiveTheme::Dark));
    app.finish();
}

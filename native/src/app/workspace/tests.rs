//! Tests for `workspace`.

use super::*;

fn split_app() -> MdEditor {
    let mut app = MdEditor::new_with_startup_file(None).0;
    app.workspace.other = Some(PaneDocs::new(PaneId::B));
    app.active_path = Some("a.md".into());
    if let Some(other) = app.workspace.other.as_mut() {
        other.active_path = Some("b.md".into());
    }
    app
}

#[test]
fn with_pane_swaps_the_other_panes_state_in_and_back() {
    let mut app = split_app();
    let seen = app.with_pane(PaneId::B, |app| {
        (
            app.active_path.clone(),
            current_pane(),
            app.workspace.current,
        )
    });
    assert_eq!(seen, (Some("b.md".into()), PaneId::B, PaneId::B));
    assert_eq!(app.active_path.as_deref(), Some("a.md"));
    assert_eq!(
        (current_pane(), app.workspace.current),
        (PaneId::A, PaneId::A)
    );
    assert_eq!(
        app.parked().and_then(|d| d.active_path.as_deref()),
        Some("b.md")
    );
}

#[test]
fn the_outline_panel_stays_open_whichever_pane_is_swapped_in() {
    let mut app = split_app();
    app.editor.toc_visible = true;
    assert!(app.with_pane(PaneId::B, |app| app.editor.toc_visible));
}

#[test]
fn a_result_for_a_closed_pane_runs_in_the_one_that_is_left() {
    let mut app = MdEditor::new_with_startup_file(None).0;
    app.active_path = Some("a.md".into());
    let _ = app.update(Message::ToPane(
        PaneId::B,
        Box::new(Message::Shell(crate::messages::ShellMessage::ShowToast(
            "indexed".into(),
        ))),
    ));
    assert_eq!(app.ui.toast.as_deref(), Some("indexed"));
}

#[test]
fn scrolling_a_pane_does_not_take_focus_but_touching_it_does() {
    let mut app = split_app();
    let scroll = Message::Editor(crate::messages::EditorMessage::Scrolled {
        y: 120.0,
        viewport_width: 400.0,
        viewport_height: 300.0,
    });
    let _ = app.update(Message::FromPane(PaneId::B, Box::new(scroll)));
    assert_eq!(app.workspace.focused, PaneId::A);
    assert_eq!(
        app.parked().map(|d| d.editor.scroll_y),
        Some(120.0),
        "the scroll landed in pane B"
    );

    let _ = app.update(Message::FromPane(
        PaneId::B,
        Box::new(Message::Editor(crate::messages::EditorMessage::ToggleTOC)),
    ));
    assert_eq!(app.workspace.focused, PaneId::B);
    assert_eq!(app.active_path.as_deref(), Some("b.md"), "B is swapped in");
}

#[test]
fn tasks_are_addressed_to_their_pane_once() {
    let toggle = || Message::Editor(crate::messages::EditorMessage::ToggleTOC);
    let addressed = Message::ToPane(PaneId::A, Box::new(toggle()));
    assert!(matches!(
        address(PaneId::B, addressed),
        Message::ToPane(PaneId::A, _)
    ));
    assert!(matches!(
        address(PaneId::B, toggle()),
        Message::ToPane(PaneId::B, _)
    ));
}

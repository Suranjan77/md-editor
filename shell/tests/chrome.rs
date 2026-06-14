use std::path::Path;

use md_kernel::CommandId;
use md_kernel::defaults::default_registry;
use md_kernel::input::{Chord, EditorKind};
use md_kernel::pane::{Layout, SplitAxis};
use md_shell::gui::keys::KeyEvent;
use md_shell::gui::menu::{self, MenuId};
use md_shell::gui::{Message, Shell};

fn shell(root: &Path) -> Shell {
    let registry = match default_registry() {
        Ok(registry) => registry,
        Err(e) => panic!("registry: {e}"),
    };
    let keymap = match registry.keymap() {
        Ok(keymap) => keymap,
        Err(e) => panic!("keymap: {e}"),
    };
    Shell::new(registry, keymap, root.to_path_buf())
}

#[test]
fn menus_disable_commands_for_the_wrong_surface() {
    let registry = match default_registry() {
        Ok(registry) => registry,
        Err(e) => panic!("registry: {e}"),
    };
    let model = menu::menu_model(&registry, Some(EditorKind::Markdown), true);
    let pdf = model
        .iter()
        .find(|group| group.id == MenuId::Pdf)
        .unwrap_or_else(|| panic!("PDF menu missing"));
    assert!(
        pdf.items
            .iter()
            .filter(|item| item.command != CommandId("pdf.annotations-orphans"))
            .all(|item| !item.enabled)
    );
}

#[test]
fn escape_closes_menu_through_overlay_fence() {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(e) => panic!("tempdir: {e}"),
    };
    let mut shell = shell(dir.path());
    let _ = shell.update(Message::MenuToggled(MenuId::File));
    assert_eq!(shell.open_menu(), Some(MenuId::File));
    let escape = match Chord::parse("escape") {
        Ok(chord) => chord,
        Err(e) => panic!("escape: {e}"),
    };
    let _ = shell.update(Message::Key(KeyEvent {
        chord: Some(escape),
        text: None,
    }));
    assert_eq!(shell.open_menu(), None);
}

#[test]
fn zoom_commands_change_pdf_zoom_without_an_overlay() {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(e) => panic!("tempdir: {e}"),
    };
    if let Err(e) = std::fs::write(dir.path().join("paper.pdf"), b"%PDF-fake") {
        panic!("fixture: {e}");
    }
    let mut shell = shell(dir.path());
    let _ = shell.update(Message::TreeFileClicked("paper.pdf".to_string()));
    shell.inject_pdf_session_layout(md_pdf::DocLayout::new(vec![(612.0, 792.0)], 1.0, 16.0));

    let _ = shell.update(Message::RunCommand(CommandId("pdf.zoom-in")));
    assert_eq!(shell.focused_pdf().map(|session| session.zoom), Some(1.25));
    let _ = shell.update(Message::RunCommand(CommandId("pdf.zoom-out")));
    assert_eq!(shell.focused_pdf().map(|session| session.zoom), Some(1.0));
    assert!(shell.overlay().is_none());
}

#[test]
fn floating_pdf_controls_target_their_tab() {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(e) => panic!("tempdir: {e}"),
    };
    if let Err(e) = std::fs::write(dir.path().join("paper.pdf"), b"%PDF-fake") {
        panic!("fixture: {e}");
    }
    let mut shell = shell(dir.path());
    let _ = shell.update(Message::TreeFileClicked("paper.pdf".to_string()));
    let tab = shell
        .workspace()
        .focused_tab()
        .unwrap_or_else(|| panic!("pdf tab"));
    shell.inject_pdf_session_layout(md_pdf::DocLayout::new(vec![(612.0, 792.0); 3], 1.0, 16.0));

    let _ = shell.update(Message::PdfCommand {
        tab,
        command: CommandId("pdf.next-page"),
    });
    assert_eq!(
        shell.focused_pdf().map(|session| session.current_page()),
        Some(1)
    );
    let _ = shell.update(Message::PdfCommand {
        tab,
        command: CommandId("pdf.previous-page"),
    });
    assert_eq!(
        shell.focused_pdf().map(|session| session.current_page()),
        Some(0)
    );
}

#[test]
fn tab_close_and_pane_controls_drive_workspace_state() {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(e) => panic!("tempdir: {e}"),
    };
    for name in ["alpha.md", "beta.md"] {
        if let Err(e) = std::fs::write(dir.path().join(name), format!("# {name}\n")) {
            panic!("fixture {name}: {e}");
        }
    }
    let mut shell = shell(dir.path());
    let _ = shell.update(Message::TreeFileClicked("alpha.md".to_string()));
    let alpha = shell
        .workspace()
        .focused_tab()
        .unwrap_or_else(|| panic!("alpha tab"));
    let _ = shell.update(Message::TreeFileClicked("beta.md".to_string()));
    assert_eq!(shell.workspace().panes.pane_count(), 1);

    let _ = shell.update(Message::TabCloseClicked(alpha));
    assert!(shell.workspace().panes.find_tab(alpha).is_none());

    let pane = shell.workspace().panes.first_pane();
    let _ = shell.update(Message::PaneCommand {
        pane,
        command: CommandId("workspace.split-down"),
    });
    assert_eq!(shell.workspace().panes.pane_count(), 2);
    assert!(matches!(
        shell.workspace().panes.layout(),
        Layout::Split {
            axis: SplitAxis::Vertical,
            ..
        }
    ));

    let second = shell.workspace().panes.panes()[1].id;
    let _ = shell.update(Message::PaneCommand {
        pane: second,
        command: CommandId("workspace.close-pane"),
    });
    assert_eq!(shell.workspace().panes.pane_count(), 1);
}

#[test]
fn dragged_split_ratio_persists() {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(e) => panic!("tempdir: {e}"),
    };
    if let Err(e) = std::fs::write(dir.path().join("alpha.md"), "# alpha\n") {
        panic!("fixture: {e}");
    }
    let mut app = shell(dir.path());
    let _ = app.update(Message::TreeFileClicked("alpha.md".to_string()));
    let pane = app.workspace().panes.first_pane();
    let _ = app.update(Message::PaneCommand {
        pane,
        command: CommandId("workspace.split-right"),
    });
    let _ = app.update(Message::SplitRatioDragged {
        path: Vec::new(),
        ratio: 0.72,
    });
    let _ = app.update(Message::SplitRatioDragFinished);
    drop(app);

    let restored = shell(dir.path());
    match restored.workspace().panes.layout() {
        Layout::Split { ratio, .. } => assert!((ratio - 0.72).abs() < f32::EPSILON),
        Layout::Pane(_) => panic!("split should restore"),
    }
}

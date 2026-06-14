//! Session restore at the shell layer (plan §5 M2): a second `Shell` over
//! the same vault comes back with the previous layout, view state, and
//! focus — driven windowlessly through `Shell::update` like every other
//! shell suite. The pdfium-gated case proves "resumed at p. N".

use std::path::Path;

use md3_kernel::defaults::default_registry;
use md3_kernel::input::{Chord, Key, Mods};
use md3_shell::gui::keys::KeyEvent;
use md3_shell::gui::{Message, Shell};
use tempfile::TempDir;

fn chord(s: &str) -> Chord {
    match Chord::parse(s) {
        Ok(c) => c,
        Err(e) => panic!("bad chord `{s}`: {e}"),
    }
}

fn press(shell: &mut Shell, s: &str) {
    let _ = shell.update(Message::Key(KeyEvent {
        chord: Some(chord(s)),
        text: None,
    }));
}

fn type_text(shell: &mut Shell, text: &str) {
    for c in text.chars() {
        let _ = shell.update(Message::Key(KeyEvent {
            chord: Some(Chord::new(Mods::NONE, Key::Char(c.to_ascii_lowercase()))),
            text: Some(c.to_string()),
        }));
    }
}

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

fn open(shell: &mut Shell, name: &str) {
    press(shell, "ctrl+p");
    type_text(shell, name);
    press(shell, "enter");
}

fn write(root: &Path, rel: &str, body: &str) {
    if let Err(e) = std::fs::write(root.join(rel), body) {
        panic!("write {rel}: {e}");
    }
}

fn vault() -> TempDir {
    let dir = match TempDir::new() {
        Ok(d) => d,
        Err(e) => panic!("tempdir: {e}"),
    };
    write(
        dir.path(),
        "alpha.md",
        "# alpha\n\nfirst note body\nline three\n",
    );
    write(dir.path(), "beta.md", "# beta\n\nsecond note body\n");
    dir
}

fn focused_path(shell: &Shell) -> Option<String> {
    let tab = shell.workspace().focused_tab()?;
    let (_, tab) = shell.workspace().panes.find_tab(tab)?;
    shell
        .workspace()
        .docs
        .get(tab.document)
        .map(|d| d.path.clone())
}

#[test]
fn layout_views_and_focus_survive_a_restart() {
    let dir = vault();

    let mut first = new_shell(dir.path());
    open(&mut first, "alpha.md");
    press(&mut first, "ctrl+\\"); // split right: alpha twice, side by side
    open(&mut first, "beta.md");
    // Move beta's caret somewhere recognizable (raw arrows).
    press(&mut first, "down");
    press(&mut first, "down");
    press(&mut first, "right");
    press(&mut first, "ctrl+q"); // quit saves the session
    drop(first);

    let second = new_shell(dir.path());
    let ws = second.workspace();
    assert_eq!(ws.panes.pane_count(), 2, "split restored");
    assert_eq!(
        focused_path(&second).as_deref(),
        Some("beta.md"),
        "focus restored"
    );
    let md = match second.focused_md() {
        Some(s) => s,
        None => panic!("focused markdown session missing"),
    };
    let head = md.doc.buffer().primary().head;
    let caret = md.doc.buffer().offset_to_line_col(head);
    assert_eq!(caret, (2, 1), "caret restored to line 3, col 1");
    // The other pane holds alpha.md.
    let all_paths: Vec<String> = ws
        .panes
        .panes()
        .iter()
        .flat_map(|p| p.tabs())
        .filter_map(|t| ws.docs.get(t.document).map(|d| d.path.clone()))
        .collect();
    assert!(all_paths.contains(&"alpha.md".to_string()), "{all_paths:?}");
}

#[test]
fn split_ratio_round_trips() {
    let dir = vault();
    let mut first = new_shell(dir.path());
    open(&mut first, "alpha.md");
    press(&mut first, "ctrl+\\");
    open(&mut first, "beta.md");
    press(&mut first, "ctrl+q");
    drop(first);

    let second = new_shell(dir.path());
    match second.workspace().panes.layout() {
        md3_kernel::pane::Layout::Split { ratio, .. } => {
            assert!((ratio - 0.5).abs() < f32::EPSILON);
        }
        md3_kernel::pane::Layout::Pane(_) => panic!("expected a restored split"),
    }
}

#[test]
fn reduce_motion_setting_round_trips() {
    let dir = vault();
    let mut first = new_shell(dir.path());
    press(&mut first, "ctrl+,");
    let _ = first.update(Message::SettingsReduceMotionChanged(true));
    let _ = first.update(Message::SettingsSave);
    drop(first);

    let mut second = new_shell(dir.path());
    press(&mut second, "ctrl+,");
    match second.overlay() {
        Some(md3_shell::gui::overlay::Overlay::Settings { reduce_motion, .. }) => {
            assert!(*reduce_motion);
        }
        _ => panic!("settings overlay missing"),
    }
}

#[test]
fn theme_is_instance_local_and_round_trips() {
    let light_dir = vault();
    let dark_dir = vault();
    let mut light = new_shell(light_dir.path());
    let dark = new_shell(dark_dir.path());

    press(&mut light, "ctrl+,");
    let _ = light.update(Message::SettingsThemeChanged("light".to_string()));
    let _ = light.update(Message::SettingsSave);

    assert_eq!(light.theme_name(), "light");
    assert_eq!(dark.theme_name(), "dark");
    assert_ne!(
        light.theme_tokens().bg_primary,
        dark.theme_tokens().bg_primary,
        "one shell's theme must not mutate another shell"
    );
    drop(light);

    let restored = new_shell(light_dir.path());
    assert_eq!(restored.theme_name(), "light");
    assert_eq!(
        restored.theme_tokens().bg_primary,
        md3_shell::gui::tokens::light().bg_primary
    );
}

#[test]
fn vanished_files_are_skipped_and_hollow_splits_collapse() {
    let dir = vault();
    let mut first = new_shell(dir.path());
    open(&mut first, "alpha.md");
    press(&mut first, "ctrl+\\"); // split duplicates alpha into pane B
    open(&mut first, "beta.md"); // B: [alpha, beta]
    press(&mut first, "ctrl+tab"); // cycle B's focus back to alpha
    press(&mut first, "ctrl+w"); // close alpha-in-B — B: [beta] only
    assert_eq!(first.workspace().panes.pane_count(), 2);
    press(&mut first, "ctrl+q");
    drop(first);

    // beta.md disappears between sessions.
    if let Err(e) = std::fs::remove_file(dir.path().join("beta.md")) {
        panic!("remove: {e}");
    }
    let second = new_shell(dir.path());
    let ws = second.workspace();
    assert_eq!(ws.panes.pane_count(), 1, "hollow split collapsed");
    assert_eq!(focused_path(&second).as_deref(), Some("alpha.md"));
}

#[test]
fn fresh_vault_starts_empty_and_a_quit_session_restores_empty() {
    let dir = vault();
    let shell = new_shell(dir.path());
    assert!(shell.workspace().focused_tab().is_none(), "no session yet");
    assert!(shell.tree_open(), "file tree opens on first run");
    drop(shell);

    // Quit without opening anything: the saved session is an empty layout.
    let mut first = new_shell(dir.path());
    press(&mut first, "ctrl+q");
    drop(first);
    let second = new_shell(dir.path());
    assert!(second.workspace().focused_tab().is_none());
    assert_eq!(second.workspace().panes.pane_count(), 1);
}

#[test]
fn saved_closed_file_tree_stays_closed() {
    let dir = vault();
    let mut first = new_shell(dir.path());
    assert!(first.tree_open());
    press(&mut first, "ctrl+b");
    assert!(!first.tree_open());
    press(&mut first, "ctrl+q");
    drop(first);

    let second = new_shell(dir.path());
    assert!(!second.tree_open());
}

#[test]
fn closing_a_tab_persists_without_quitting() {
    let dir = vault();
    let mut first = new_shell(dir.path());
    open(&mut first, "alpha.md");
    open(&mut first, "beta.md");
    press(&mut first, "ctrl+w"); // close beta — saves the session itself
    drop(first); // no quit: simulates a hard stop after the close

    let second = new_shell(dir.path());
    assert_eq!(focused_path(&second).as_deref(), Some("alpha.md"));
    let tabs: usize = second
        .workspace()
        .panes
        .panes()
        .iter()
        .map(|p| p.tabs().len())
        .sum();
    assert_eq!(tabs, 1, "closed tab stays closed");
}

#[cfg(feature = "pdfium")]
#[test]
fn pdf_scroll_and_zoom_resume_at_page_n() {
    let dir = vault();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests-fixtures/pdf/multipage-outline.pdf");
    if let Err(e) = std::fs::copy(&fixture, dir.path().join("paper.pdf")) {
        panic!("copy fixture: {e}");
    }

    let mut first = new_shell(dir.path());
    open(&mut first, "paper.pdf");
    if first.focused_pdf().is_none_or(|s| s.layout.is_none()) {
        eprintln!("skipping: libpdfium not available");
        return;
    }
    press(&mut first, "ctrl+g");
    type_text(&mut first, "3");
    press(&mut first, "enter");
    press(&mut first, "ctrl+z");
    type_text(&mut first, "150");
    press(&mut first, "enter");
    let scroll = match first.focused_pdf() {
        Some(s) => s.scroll,
        None => panic!("pdf session lost"),
    };
    press(&mut first, "ctrl+q");
    drop(first);

    let second = new_shell(dir.path());
    let session = match second.focused_pdf() {
        Some(s) => s,
        None => panic!("pdf not refocused"),
    };
    assert!((session.zoom - 1.5).abs() < f32::EPSILON, "zoom restored");
    assert!(
        (session.scroll - scroll).abs() < 1.0,
        "scroll restored: {} vs {}",
        session.scroll,
        scroll
    );
    assert_eq!(session.current_page(), 2, "back on page 3");
    assert!(
        second.status().contains("resumed at p. 3"),
        "status announces the resume: {}",
        second.status()
    );
}

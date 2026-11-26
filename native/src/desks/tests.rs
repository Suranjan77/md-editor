//! Unit tests for `desks`: CRUD, JSON round-trip and corruption resilience, SQLite persistence, and rename/delete propagation.

use super::*;
use crate::tabs::{TabKind, TabView};

fn sample_tabs_json(paths: &[&str], active: Option<&str>) -> String {
    let mut tabs = Tabs::new();
    for &path in paths {
        if let Some(kind) = TabKind::from_path(path) {
            tabs.open(path, kind, false);
        }
    }
    if let Some(act) = active
        && let Some(id) = tabs.find_by_path(act).map(|t| t.id)
    {
        tabs.activate(id);
    }
    tabs.to_json()
}

fn sample_desk(name: &str, split: bool) -> Desk {
    let mut tabs_a = Tabs::new();
    tabs_a.open("notes/Intro.md", TabKind::Note, false);
    let img = tabs_a.open("assets/fig.png", TabKind::Image, true);
    if let Some(tab) = tabs_a.get_mut(img) {
        tab.view = TabView::Image {
            zoom: 1.5,
            fit: false,
        };
    }

    let pane_b_tabs = split.then(|| {
        let mut tabs_b = Tabs::new();
        let pdf = tabs_b.open("papers/attention.pdf", TabKind::Pdf, false);
        if let Some(tab) = tabs_b.get_mut(pdf) {
            tab.view = TabView::Pdf {
                page: 3,
                zoom: 1.75,
                fit_to_width: false,
                fit_fills_pane: false,
                scroll_y: 420.0,
            };
        }
        tabs_b.to_json()
    });

    Desk {
        name: name.to_string(),
        pane_a_tabs: tabs_a.to_json(),
        pane_b_tabs,
        axis: SplitAxis::Vertical,
        split_ratio: 0.6,
        focused: if split { PaneId::B } else { PaneId::A },
        sidebar_visible: true,
        backlinks_visible: false,
        toc_visible: true,
    }
}

#[test]
fn crud_save_overwrite_case_insensitive_and_delete() {
    let mut store = DeskStore::default();
    assert!(store.list().is_empty());

    assert!(store.save(sample_desk("   ", false)).is_err());

    store.save(sample_desk("  Paper Review  ", true)).unwrap();
    store.save(sample_desk("Writing", false)).unwrap();
    assert_eq!(store.list().len(), 2);
    assert_eq!(store.list()[0].name, "Writing");
    assert_eq!(store.list()[1].name, "Paper Review");
    assert_eq!(store.list()[1].summary(), "Split · 3 tabs");
    assert_eq!(store.list()[0].summary(), "1 pane · 2 tabs");

    // Overwriting with a different case replaces the entry and moves it to top.
    let mut updated = sample_desk("paper review", false);
    updated.split_ratio = 0.99; // clamped to 0.8
    store.save(updated).unwrap();
    assert_eq!(store.list().len(), 2);
    assert_eq!(store.list()[0].name, "paper review");
    assert_eq!(store.list()[0].split_ratio, 0.8);
    assert!(store.get("PAPER REVIEW").is_some());

    assert!(store.delete("WRITING"));
    assert!(!store.delete("missing"));
    assert_eq!(store.list().len(), 1);
}

#[test]
fn json_round_trip_sqlite_persistence_and_corrupt_json_resilience() {
    let mut store = DeskStore::default();
    store.save(sample_desk("Research", true)).unwrap();
    let json = store.to_json();

    let parsed = DeskStore::from_json(&json);
    assert_eq!(parsed, store);

    // Corrupt JSON never panics and falls back to an empty store.
    assert!(DeskStore::from_json("{not valid json").list().is_empty());
    assert!(DeskStore::from_json("").list().is_empty());

    // SQLite persistence is scoped per vault root under `desks:<vault_root>`.
    let state = md_editor_core::state::AppState::new_in_memory();
    store.persist(&state, "/vault/alpha");
    let loaded_alpha = DeskStore::load(&state, "/vault/alpha");
    assert_eq!(loaded_alpha, store);
    assert!(DeskStore::load(&state, "/vault/beta").list().is_empty());

    // Corrupt row in SQLite loads as empty store.
    md_editor_core::config::set_sys_config(&state, "desks:/vault/corrupt", "[[bad").unwrap();
    assert!(DeskStore::load(&state, "/vault/corrupt").list().is_empty());
}

#[test]
fn rename_propagation_updates_file_and_folder_prefixes() {
    let mut store = DeskStore::default();
    let mut desk = sample_desk("Study", true);
    desk.pane_a_tabs = sample_tabs_json(
        &["notes/sub/a.md", "notes/other.md"],
        Some("notes/sub/a.md"),
    );
    desk.pane_b_tabs = Some(sample_tabs_json(
        &["papers/p1.pdf", "notes/sub/b.md"],
        Some("notes/sub/b.md"),
    ));
    store.save(desk).unwrap();

    // Folder rename updates descendants in both panes, active_path, and recent_paths.
    assert!(store.rename_path("notes/sub", "archive/old"));
    let saved = store.get("Study").unwrap();
    let mut restored_a = Tabs::new();
    assert!(restored_a.restore_from_json(&saved.pane_a_tabs, |_| true));
    assert_eq!(
        restored_a.active_tab().map(|t| t.path.as_str()),
        Some("archive/old/a.md")
    );
    assert!(restored_a.find_by_path("notes/other.md").is_some());

    let mut restored_b = Tabs::new();
    assert!(restored_b.restore_from_json(saved.pane_b_tabs.as_deref().unwrap(), |_| true));
    assert_eq!(
        restored_b.active_tab().map(|t| t.path.as_str()),
        Some("archive/old/b.md")
    );

    // Single-file rename updates exact path without touching siblings.
    assert!(store.rename_path("papers/p1.pdf", "papers/renamed.pdf"));
    let saved = store.get("Study").unwrap();
    let mut restored_b = Tabs::new();
    assert!(restored_b.restore_from_json(saved.pane_b_tabs.as_deref().unwrap(), |_| true));
    assert!(restored_b.find_by_path("papers/renamed.pdf").is_some());
    assert!(!store.rename_path("does/not/exist.md", "nope.md"));
}

#[test]
fn delete_propagation_prunes_file_and_folder_prefixes() {
    let mut store = DeskStore::default();
    let mut desk = sample_desk("Focus", true);
    desk.pane_a_tabs = sample_tabs_json(&["notes/keep.md", "notes/drop.md"], Some("notes/drop.md"));
    desk.pane_b_tabs = Some(sample_tabs_json(
        &["dir/a.pdf", "dir/sub/b.md"],
        Some("dir/a.pdf"),
    ));
    store.save(desk).unwrap();

    // Delete single file -> drops it and promotes remaining tab as active.
    assert!(store.remove_deleted_path("notes/drop.md"));
    let saved = store.get("Focus").unwrap();
    let mut restored_a = Tabs::new();
    assert!(restored_a.restore_from_json(&saved.pane_a_tabs, |_| true));
    assert_eq!(restored_a.len(), 1);
    assert_eq!(
        restored_a.active_tab().map(|t| t.path.as_str()),
        Some("notes/keep.md")
    );

    // Delete folder -> prunes every descendant in Pane B while keeping split snapshot valid.
    assert!(store.remove_deleted_path("dir"));
    let saved = store.get("Focus").unwrap();
    assert_eq!(saved.tab_count(), 1);
    let mut restored_b = Tabs::new();
    assert!(!restored_b.restore_from_json(saved.pane_b_tabs.as_deref().unwrap(), |_| true));
    assert!(restored_b.is_empty());
    assert!(!store.remove_deleted_path("missing.md"));
}

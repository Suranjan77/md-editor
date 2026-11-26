//! Unit tests for `Tabs`: open/focus/background, close, MRU, reopen stack, disambiguation, rename/delete and session persistence.

use super::*;

#[test]
fn opening_focuses_existing_or_inserts_to_right_of_active() {
    let mut tabs = Tabs::new();
    let a = tabs.open("a.md", TabKind::Note, false);
    let b = tabs.open("b.md", TabKind::Note, false);
    assert_eq!(tabs.active_id(), Some(b));

    // Activate `a`, then open `c`: `c` lands right after `a` (before `b`).
    tabs.activate(a);
    let c = tabs.open("c.pdf", TabKind::Pdf, false);
    let paths: Vec<&str> = tabs.list().iter().map(|t| t.path.as_str()).collect();
    assert_eq!(paths, vec!["a.md", "c.pdf", "b.md"]);
    assert_eq!(tabs.active_id(), Some(c));

    // Re-opening `b.md` focuses the existing tab instead of duplicating.
    let again = tabs.open("b.md", TabKind::Note, false);
    assert_eq!(again, b);
    assert_eq!(tabs.len(), 3);
}

#[test]
fn background_open_preserves_active_tab() {
    let mut tabs = Tabs::new();
    let a = tabs.open("a.md", TabKind::Note, false);
    let bg = tabs.open("b.md", TabKind::Note, true);
    assert_ne!(a, bg);
    assert_eq!(tabs.active_id(), Some(a));
    assert_eq!(tabs.len(), 2);
}

#[test]
fn close_activates_mru_and_reopen_stack_caps_at_ten() {
    let mut tabs = Tabs::new();
    let a = tabs.open("a.md", TabKind::Note, false);
    let b = tabs.open("b.md", TabKind::Note, false);
    let c = tabs.open("c.md", TabKind::Note, false);
    tabs.activate(a);
    tabs.activate(c);

    // Closing `c` returns to `a` (most recently active before `c`), not `b`.
    tabs.close(c);
    assert_eq!(tabs.active_id(), Some(a));

    let reopened = tabs.reopen_closed().expect("reopen c");
    assert_eq!(tabs.get(reopened).map(|t| t.path.as_str()), Some("c.md"));
    assert_eq!(tabs.active_id(), Some(reopened));

    // Close 12 tabs: reopen stack keeps only the 10 most recent.
    let _ = b;
    for i in 0..12 {
        let id = tabs.open(&format!("n{i}.md"), TabKind::Note, false);
        tabs.close(id);
    }
    let mut count = 0;
    while tabs.reopen_closed().is_some() {
        count += 1;
    }
    assert_eq!(count, 10);
}

#[test]
fn duplicate_names_show_parent_folder() {
    let mut tabs = Tabs::new();
    let root = tabs.open("Index.md", TabKind::Note, false);
    let sub = tabs.open("notes/Index.md", TabKind::Note, false);
    let unique = tabs.open("notes/Other.md", TabKind::Note, false);

    assert_eq!(tabs.label_for(tabs.get(root).unwrap()), "Index — /");
    assert_eq!(tabs.label_for(tabs.get(sub).unwrap()), "Index — notes/");
    assert_eq!(tabs.label_for(tabs.get(unique).unwrap()), "Other");
}

#[test]
fn mru_of_kind_tracks_split_pane_candidates() {
    let mut tabs = Tabs::new();
    let n1 = tabs.open("n1.md", TabKind::Note, false);
    let p1 = tabs.open("p1.pdf", TabKind::Pdf, false);
    let p2 = tabs.open("p2.pdf", TabKind::Pdf, false);
    tabs.activate(n1);

    assert_eq!(tabs.mru_of_kind(TabKind::Note).map(|t| t.id), Some(n1));
    assert_eq!(tabs.mru_of_kind(TabKind::Pdf).map(|t| t.id), Some(p2));
    tabs.activate(p1);
    assert_eq!(tabs.mru_of_kind(TabKind::Pdf).map(|t| t.id), Some(p1));
}

#[test]
fn ordinals_step_and_reorder() {
    let mut tabs = Tabs::new();
    let a = tabs.open("1.md", TabKind::Note, false);
    let _b = tabs.open("2.md", TabKind::Note, false);
    let c = tabs.open("3.md", TabKind::Note, false);

    assert_eq!(tabs.select_ordinal(1), Some(a));
    // Ctrl+9 always jumps to the last tab, even when fewer than 9 tabs are open.
    assert_eq!(tabs.select_ordinal(9), Some(c));

    tabs.move_active_by(-2);
    assert_eq!(tabs.list()[0].id, c);

    assert_eq!(
        tabs.step_strip(1)
            .map(|id| tabs.get(id).unwrap().path.as_str()),
        Some("1.md")
    );
}

#[test]
fn rename_delete_and_session_round_trip() {
    let mut tabs = Tabs::new();
    tabs.open("notes/old.md", TabKind::Note, false);
    let pdf = tabs.open("papers/doc.pdf", TabKind::Pdf, false);
    if let Some(tab) = tabs.get_mut(pdf) {
        tab.view = TabView::Pdf {
            page: 4,
            zoom: 1.75,
            fit_to_width: false,
            fit_fills_pane: false,
            scroll_y: 320.0,
        };
    }

    tabs.rename_path("notes", "archive");
    assert!(tabs.find_by_path("archive/old.md").is_some());

    let json = tabs.to_json();
    let mut restored = Tabs::new();
    assert!(restored.restore_from_json(&json, |_| true));
    assert_eq!(restored.len(), 2);
    assert_eq!(
        restored.active_tab().map(|t| t.path.as_str()),
        Some("papers/doc.pdf")
    );
    assert_eq!(
        restored.active_tab().map(|t| &t.view),
        Some(&TabView::Pdf {
            page: 4,
            zoom: 1.75,
            fit_to_width: false,
            fit_fills_pane: false,
            scroll_y: 320.0,
        })
    );

    restored.sync_missing(|p| p != "archive/old.md");
    assert!(restored.find_by_path("archive/old.md").unwrap().missing);

    restored.remove_deleted_path("archive");
    assert_eq!(restored.len(), 1);
}

#[test]
fn restore_skips_tabs_whose_saved_view_no_longer_parses() {
    // A session saved while `.pptx` decks still opened in tabs.
    let raw = r#"{"tabs":[
        {"path":"a.md","view":"Note"},
        {"path":"deck.pptx","view":{"Deck":{"zoom":1.0,"scroll_x":0.0,"scroll_y":0.0}}},
        {"path":"b.md","view":"Note"}
    ],"active_path":"b.md","recent_paths":[]}"#;
    let mut tabs = Tabs::new();
    assert!(tabs.restore_from_json(raw, |_| true));
    let paths: Vec<_> = tabs.list().iter().map(|t| t.path.as_str()).collect();
    assert_eq!(paths, ["a.md", "b.md"]);
    assert_eq!(tabs.active_tab().map(|t| t.path.as_str()), Some("b.md"));
}

#[test]
fn from_path_excludes_sketches_and_keeps_regular_svgs_as_images() {
    assert_eq!(TabKind::from_path("notes/diagram.ink.svg"), None);
    assert_eq!(TabKind::from_path("notes/diagram.INK.SVG"), None);
    assert_eq!(TabKind::from_path("notes/diagram.ink"), None);
    assert_eq!(
        TabKind::from_path("assets/diagram.svg"),
        Some(TabKind::Image)
    );
}

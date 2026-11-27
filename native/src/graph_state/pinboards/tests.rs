//! Unit tests for saved graph pinboards: CRUD, JSON resilience, and rename/delete propagation.

use std::collections::BTreeMap;

use super::*;
use crate::graph_state::GraphScope;

fn sample_board(name: &str) -> Pinboard {
    let mut pinned_positions = BTreeMap::new();
    pinned_positions.insert("notes/A.md".into(), PinPosition { x: 10.0, y: -20.0 });
    pinned_positions.insert("notes/sub/B.md".into(), PinPosition { x: 45.0, y: 80.0 });
    Pinboard {
        name: name.to_string(),
        pinned_positions,
        scope: GraphScope::Local { depth: 2 },
        show_pdfs: true,
        show_sketches: true,
        show_missing: false,
        show_orphans: false,
        pan: (15.0, -30.0),
        zoom: 1.5,
    }
}

#[test]
fn crud_save_overwrite_case_insensitive_and_delete() {
    let mut store = PinboardStore::default();
    assert!(store.save(sample_board("   ")).is_err());
    store.save(sample_board("  Thesis Map  ")).unwrap();
    store.save(sample_board("Cluster")).unwrap();
    assert_eq!(store.list().len(), 2);
    assert_eq!(store.list()[0].name, "Cluster");

    let mut updated = sample_board("thesis map");
    updated.zoom = 999.0; // clamped to MAX_ZOOM
    store.save(updated).unwrap();
    assert_eq!(store.list().len(), 2);
    assert_eq!(store.list()[0].name, "thesis map");
    assert!((store.list()[0].zoom - MAX_ZOOM).abs() < 0.001);

    assert!(store.delete("THESIS MAP"));
    assert!(!store.delete("nonexistent"));
    assert_eq!(store.list().len(), 1);
}

#[test]
fn json_round_trip_and_corrupt_resilience() {
    let state = md_editor_core::state::AppState::new_in_memory();
    let mut store = PinboardStore::default();
    store.save(sample_board("Alpha")).unwrap();
    store.persist(&state, "/tmp/vault-one");

    let loaded = PinboardStore::load(&state, "/tmp/vault-one");
    assert_eq!(loaded, store);

    let corrupt = PinboardStore::from_json("{not valid json");
    assert!(corrupt.list().is_empty());
}

#[test]
fn rename_and_delete_propagate_to_pinboard_paths() {
    let mut store = PinboardStore::default();
    store.save(sample_board("Map")).unwrap();

    assert!(store.rename_path("notes/A.md", "notes/A-renamed.md"));
    assert!(store.rename_path("notes/sub", "archive"));

    let board = store.get("Map").unwrap();
    assert!(board.pinned_positions.contains_key("notes/A-renamed.md"));
    assert!(board.pinned_positions.contains_key("archive/B.md"));
    assert!(!board.pinned_positions.contains_key("notes/A.md"));
    assert!(!board.pinned_positions.contains_key("notes/sub/B.md"));

    assert!(store.remove_deleted_path("archive"));
    assert!(
        !store
            .get("Map")
            .unwrap()
            .pinned_positions
            .contains_key("archive/B.md")
    );
    assert!(
        store
            .get("Map")
            .unwrap()
            .pinned_positions
            .contains_key("notes/A-renamed.md")
    );
}

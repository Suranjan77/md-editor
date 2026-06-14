//! Integration tests for TrackerStore in md-vault.

use md_vault::tracker::{StudySession, TrackerStore};

#[test]
#[allow(clippy::unwrap_used)]
fn test_tracker_store_integration() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("tracker.db");

    let mut store = TrackerStore::open(&db_path).unwrap();
    assert_eq!(store.schema_version().unwrap(), 1);

    // Save a few sessions with different dates to test sorting order
    store
        .save_session(&StudySession {
            id: 0,
            date: "2026-06-12 09:00:00".to_string(),
            hours: 1.0,
            activity_type: "Study".to_string(),
            phase: "Phase 1".to_string(),
            notes: None,
        })
        .unwrap();

    store
        .save_session(&StudySession {
            id: 0,
            date: "2026-06-12 10:00:00".to_string(),
            hours: 2.5,
            activity_type: "Coding".to_string(),
            phase: "Phase 2".to_string(),
            notes: Some("Second note".to_string()),
        })
        .unwrap();

    store
        .save_session(&StudySession {
            id: 0,
            date: "2026-06-12 08:00:00".to_string(),
            hours: 0.5,
            activity_type: "Reading".to_string(),
            phase: "Phase 3".to_string(),
            notes: None,
        })
        .unwrap();

    assert_eq!(store.get_total_hours().unwrap(), 4.0);

    // Check sessions are sorted DESC by date (latest first)
    let sessions = store.get_sessions().unwrap();
    assert_eq!(sessions.len(), 3);
    assert_eq!(sessions[0].date, "2026-06-12 10:00:00");
    assert_eq!(sessions[1].date, "2026-06-12 09:00:00");
    assert_eq!(sessions[2].date, "2026-06-12 08:00:00");

    // Test updates
    let mut session_to_update = sessions[0].clone();
    session_to_update.hours = 3.0;
    session_to_update.notes = Some("Updated note".to_string());
    store.save_session(&session_to_update).unwrap();

    let updated_sessions = store.get_sessions().unwrap();
    assert_eq!(updated_sessions[0].hours, 3.0);
    assert_eq!(updated_sessions[0].notes.as_deref(), Some("Updated note"));
    assert_eq!(store.get_total_hours().unwrap(), 4.5);

    // Test KV updates
    store.set_kv("tracker_config", "{\"test\": true}").unwrap();
    let kvs = store.get_kv().unwrap();
    assert_eq!(kvs.len(), 1);
    assert_eq!(kvs[0].key, "tracker_config");
    assert_eq!(kvs[0].value, "{\"test\": true}");
}

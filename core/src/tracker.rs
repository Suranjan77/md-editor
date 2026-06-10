//! Legacy tracker module (P2.T4): types live in `domain::session`, the public
//! entry point is `application::TrackerService`. The free functions below are
//! the service's internals; external callers must use the service.

use crate::database::tracker_repository;
use crate::state::AppState;

pub use crate::domain::session::{StudySession, TrackerKv};

pub(crate) fn save_session(state: &AppState, session: StudySession) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    tracker_repository::save_session(&db, session)
}

pub(crate) fn delete_session(state: &AppState, id: i64) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    tracker_repository::delete_session(&db, id)
}

pub(crate) fn get_sessions(state: &AppState) -> Result<Vec<StudySession>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    tracker_repository::get_sessions(&db)
}

pub(crate) fn get_total_hours(state: &AppState) -> Result<f32, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    tracker_repository::get_total_hours(&db)
}

pub(crate) fn get_kv(state: &AppState) -> Result<Vec<TrackerKv>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    tracker_repository::get_kv(&db)
}

pub(crate) fn set_kv(state: &AppState, key: &str, value: &str) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    tracker_repository::set_kv(&db, key, value)
}

#[cfg(test)]
mod tracker_scale_tests {
    use crate::state::AppState;
    use crate::tracker::{
        StudySession, get_kv, get_sessions, get_total_hours, save_session, set_kv,
    };

    #[test]
    fn test_study_tracker_massive_sessions() {
        let state =
            AppState::try_new_in_memory().expect("in-memory application state should initialize");

        // 1. Bulk insert 500 StudySessions with various dates, activities, and notes
        let mut expected_sessions = Vec::new();
        let mut expected_total_hours = 0.0f32;

        for i in 0..500 {
            // Construct varying dates to test sorting (Date format: YYYY-MM-DD HH:MM:SS)
            // We alternate dates to test descending sort correctness
            let day = 1 + (i % 28);
            let month = 1 + (i % 12);
            let date_str = format!("2026-{:02}-{:02} 12:00:{:02}", month, day, i % 60);

            let hours = 0.5f32 * (1 + (i % 8)) as f32;
            expected_total_hours += hours;

            let session = StudySession {
                id: 0, // database auto-increments
                date: date_str,
                hours,
                activity_type: format!("Activity_{}", i % 5),
                phase: format!("Phase_{}", i % 3),
                notes: if i % 2 == 0 {
                    Some(format!("Detailed notes for session {}", i))
                } else {
                    None
                },
            };

            save_session(&state, session.clone()).expect("Failed to save session");
            expected_sessions.push(session);
        }

        // 2. Retrieve sessions and assert they are sorted by date DESC
        let retrieved = get_sessions(&state).expect("Failed to get sessions");
        assert_eq!(retrieved.len(), 500);

        // Verify sorting order: retrieved[j].date >= retrieved[j+1].date
        for j in 0..499 {
            assert!(
                retrieved[j].date >= retrieved[j + 1].date,
                "Sessions not sorted descending by date! index {}: {} vs index {}: {}",
                j,
                retrieved[j].date,
                j + 1,
                retrieved[j + 1].date
            );
        }

        // 3. Verify total aggregate hours calculation
        let calculated_hours = get_total_hours(&state).expect("Failed to get total hours");
        let diff = (calculated_hours - expected_total_hours).abs();
        assert!(
            diff < 0.01,
            "Aggregate hours sum mismatch: expected {}, got {}",
            expected_total_hours,
            calculated_hours
        );

        // 4. Test massive Tracker KV store (500 entries)
        for i in 0..500 {
            let key = format!("kv_key_{:03}", i);
            let val = format!("kv_val_{}", i);
            set_kv(&state, &key, &val).expect("Failed to set tracker KV");
        }

        let kv_entries = get_kv(&state).expect("Failed to get tracker KV entries");
        assert_eq!(kv_entries.len(), 500);

        // Verify KV entries are sorted by key in ascending order
        for j in 0..499 {
            assert!(
                kv_entries[j].key < kv_entries[j + 1].key,
                "Tracker KV entries not sorted ascending by key! index {}: {} vs index {}: {}",
                j,
                kv_entries[j].key,
                j + 1,
                kv_entries[j + 1].key
            );
        }
    }
}

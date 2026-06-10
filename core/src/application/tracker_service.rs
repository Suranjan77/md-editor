//! Study-tracker service: the only public entry point for tracker
//! persistence. Owns access to the tracker repository (private to core).

use crate::domain::session::{StudySession, TrackerKv};
use crate::state::AppState;

/// Façade over tracker persistence (sessions, totals, key/value store).
pub struct TrackerService<'a> {
    state: &'a AppState,
}

impl<'a> TrackerService<'a> {
    /// Create a service borrowing the shared application state.
    pub const fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    /// Persist a session (insert; `id` is auto-incremented).
    pub fn save_session(&self, session: StudySession) -> Result<(), String> {
        crate::tracker::save_session(self.state, session)
    }

    /// Delete a session by row id.
    pub fn delete_session(&self, id: i64) -> Result<(), String> {
        crate::tracker::delete_session(self.state, id)
    }

    /// All sessions, newest first.
    pub fn sessions(&self) -> Result<Vec<StudySession>, String> {
        crate::tracker::get_sessions(self.state)
    }

    /// Sum of hours across all sessions.
    pub fn total_hours(&self) -> Result<f32, String> {
        crate::tracker::get_total_hours(self.state)
    }

    /// All tracker key/value entries, sorted by key.
    pub fn kv_entries(&self) -> Result<Vec<TrackerKv>, String> {
        crate::tracker::get_kv(self.state)
    }

    /// Upsert one tracker key/value entry.
    pub fn set_kv(&self, key: &str, value: &str) -> Result<(), String> {
        crate::tracker::set_kv(self.state, key, value)
    }
}

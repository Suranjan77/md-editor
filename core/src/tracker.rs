use crate::database::tracker_repository;
use crate::state::AppState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudySession {
    pub id: i64,
    pub date: String,
    pub hours: f32,
    pub activity_type: String,
    pub phase: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerKv {
    pub key: String,
    pub value: String,
}

pub fn save_session(state: &AppState, session: StudySession) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    tracker_repository::save_session(&db, session)
}

pub fn delete_session(state: &AppState, id: i64) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    tracker_repository::delete_session(&db, id)
}

pub fn get_sessions(state: &AppState) -> Result<Vec<StudySession>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    tracker_repository::get_sessions(&db)
}

pub fn get_total_hours(state: &AppState) -> Result<f32, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    tracker_repository::get_total_hours(&db)
}

pub fn get_kv(state: &AppState) -> Result<Vec<TrackerKv>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    tracker_repository::get_kv(&db)
}

pub fn set_kv(state: &AppState, key: &str, value: &str) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    tracker_repository::set_kv(&db, key, value)
}

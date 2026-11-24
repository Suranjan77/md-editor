//! Study-tracker persistence: sessions and key/value progress.

use crate::state::{AppState, StateResult};
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

pub fn save_session(state: &AppState, session: StudySession) -> StateResult<()> {
    let db = state.db.lock()?;
    db.execute(
        "INSERT INTO tracker_sessions (date, hours, activity_type, phase, notes) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![session.date, session.hours, session.activity_type, session.phase, session.notes],
    )?;
    Ok(())
}

pub fn delete_session(state: &AppState, id: i64) -> StateResult<()> {
    let db = state.db.lock()?;
    db.execute(
        "DELETE FROM tracker_sessions WHERE id = ?1",
        rusqlite::params![id],
    )?;
    Ok(())
}

pub fn get_sessions(state: &AppState) -> StateResult<Vec<StudySession>> {
    let db = state.db.lock()?;
    let mut stmt = db.prepare("SELECT id, date, hours, activity_type, phase, notes FROM tracker_sessions ORDER BY date DESC")
        ?;

    let rows = stmt.query_map([], |row| {
        Ok(StudySession {
            id: row.get(0)?,
            date: row.get(1)?,
            hours: row.get(2)?,
            activity_type: row.get(3)?,
            phase: row.get(4)?,
            notes: row.get(5)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        match row {
            Ok(r) => results.push(r),
            // A malformed row is skipped but logged — silently dropping
            // tracker sessions would hide data corruption.
            Err(e) => log::warn!("Skipping malformed tracker_sessions row: {e}"),
        }
    }
    Ok(results)
}

pub fn get_total_hours(state: &AppState) -> StateResult<f32> {
    let db = state.db.lock()?;
    let mut stmt = db.prepare("SELECT SUM(hours) FROM tracker_sessions")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        Ok(row.get(0).unwrap_or(0.0))
    } else {
        Ok(0.0)
    }
}

pub fn get_kv(state: &AppState) -> StateResult<Vec<TrackerKv>> {
    let db = state.db.lock()?;
    let mut stmt = db.prepare("SELECT key, value FROM tracker_kv ORDER BY key")?;

    let rows = stmt.query_map([], |row| {
        Ok(TrackerKv {
            key: row.get(0)?,
            value: row.get(1)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows.flatten() {
        results.push(row);
    }
    Ok(results)
}

pub fn set_kv(state: &AppState, key: &str, value: &str) -> StateResult<()> {
    let db = state.db.lock()?;
    db.execute(
        "INSERT INTO tracker_kv (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

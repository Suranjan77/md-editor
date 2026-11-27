//! Study-tracker persistence: sessions and key/value progress.

use crate::state::{AppState, StateResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudySession {
    pub id: i64,
    #[serde(default)]
    pub uid: String,
    pub date: String,
    pub hours: f32,
    pub activity_type: String,
    pub phase: String,
    pub notes: Option<String>,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerKv {
    pub key: String,
    pub value: String,
}

fn now_epoch_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn save_session(state: &AppState, mut session: StudySession) -> StateResult<()> {
    if session.uid.trim().is_empty() {
        session.uid = uuid::Uuid::new_v4().simple().to_string();
    }
    if session.updated_at == 0 {
        session.updated_at = now_epoch_secs();
    }
    let scope = state.current_tracker_scope()?;
    {
        let db = state.db.lock()?;
        db.execute(
            "INSERT INTO tracker_sessions
                (date, hours, activity_type, phase, notes, uid, updated_at, vault_scope)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                session.date,
                session.hours,
                session.activity_type,
                session.phase,
                session.notes,
                session.uid,
                session.updated_at,
                scope,
            ],
        )?;
    }
    state.export_tracker_sidecar();
    Ok(())
}

pub fn delete_session(state: &AppState, id: i64) -> StateResult<()> {
    {
        let db = state.db.lock()?;
        db.execute(
            "DELETE FROM tracker_sessions WHERE id = ?1",
            rusqlite::params![id],
        )?;
    }
    state.export_tracker_sidecar();
    Ok(())
}

pub fn get_sessions(state: &AppState) -> StateResult<Vec<StudySession>> {
    let scope = state.current_tracker_scope()?;
    let db = state.db.lock()?;
    let mut stmt = db.prepare(
        "SELECT id, COALESCE(uid, ''), date, hours, activity_type, phase, notes, COALESCE(updated_at, 0)
         FROM tracker_sessions
         WHERE vault_scope = ?1
         ORDER BY date DESC, id DESC",
    )?;

    let rows = stmt.query_map([scope], |row| {
        Ok(StudySession {
            id: row.get(0)?,
            uid: row.get(1)?,
            date: row.get(2)?,
            hours: row.get(3)?,
            activity_type: row.get(4)?,
            phase: row.get(5)?,
            notes: row.get(6)?,
            updated_at: row.get(7)?,
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
    let scope = state.current_tracker_scope()?;
    let db = state.db.lock()?;
    let mut stmt = db.prepare("SELECT SUM(hours) FROM tracker_sessions WHERE vault_scope = ?1")?;
    let mut rows = stmt.query([scope])?;
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

use tauri::State;
use serde::{Deserialize, Serialize};

use crate::commands::AppState;

#[derive(Serialize, Deserialize, Debug)]
pub struct TrackerSession {
    pub id: Option<i64>,
    pub date: String,
    pub hours: f64,
    pub activity_type: String,
    pub phase: String,
    pub notes: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TrackerActivity {
    pub id: Option<i64>,
    pub r#type: String,
    pub text: String,
    pub time: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TrackerKV {
    pub key: String,
    pub value: String,
}

#[tauri::command]
pub fn get_tracker_sessions(state: State<'_, AppState>) -> Result<Vec<TrackerSession>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = db.prepare("SELECT id, date, hours, activity_type, phase, notes FROM tracker_sessions ORDER BY date DESC, id DESC").map_err(|e| e.to_string())?;
    
    let rows = stmt.query_map([], |row| {
        Ok(TrackerSession {
            id: row.get(0)?,
            date: row.get(1)?,
            hours: row.get(2)?,
            activity_type: row.get(3)?,
            phase: row.get(4)?,
            notes: row.get(5)?,
        })
    }).map_err(|e| e.to_string())?;

    let mut sessions = Vec::new();
    for row in rows {
        sessions.push(row.map_err(|e| e.to_string())?);
    }
    Ok(sessions)
}

#[tauri::command]
pub fn add_tracker_session(session: TrackerSession, state: State<'_, AppState>) -> Result<i64, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.execute(
        "INSERT INTO tracker_sessions (date, hours, activity_type, phase, notes) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![session.date, session.hours, session.activity_type, session.phase, session.notes],
    ).map_err(|e| e.to_string())?;
    Ok(db.last_insert_rowid())
}

#[tauri::command]
pub fn delete_tracker_session(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.execute("DELETE FROM tracker_sessions WHERE id = ?1", rusqlite::params![id]).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_tracker_activities(state: State<'_, AppState>) -> Result<Vec<TrackerActivity>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = db.prepare("SELECT id, type, text, time FROM tracker_activity ORDER BY time DESC LIMIT 50").map_err(|e| e.to_string())?;
    
    let rows = stmt.query_map([], |row| {
        Ok(TrackerActivity {
            id: row.get(0)?,
            r#type: row.get(1)?,
            text: row.get(2)?,
            time: row.get(3)?,
        })
    }).map_err(|e| e.to_string())?;

    let mut activities = Vec::new();
    for row in rows {
        activities.push(row.map_err(|e| e.to_string())?);
    }
    Ok(activities)
}

#[tauri::command]
pub fn add_tracker_activity(activity: TrackerActivity, state: State<'_, AppState>) -> Result<i64, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.execute(
        "INSERT INTO tracker_activity (type, text, time) VALUES (?1, ?2, ?3)",
        rusqlite::params![activity.r#type, activity.text, activity.time],
    ).map_err(|e| e.to_string())?;
    Ok(db.last_insert_rowid())
}

#[tauri::command]
pub fn get_tracker_kv(state: State<'_, AppState>) -> Result<Vec<TrackerKV>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = db.prepare("SELECT key, value FROM tracker_kv").map_err(|e| e.to_string())?;
    
    let rows = stmt.query_map([], |row| {
        Ok(TrackerKV {
            key: row.get(0)?,
            value: row.get(1)?,
        })
    }).map_err(|e| e.to_string())?;

    let mut kvs = Vec::new();
    for row in rows {
        kvs.push(row.map_err(|e| e.to_string())?);
    }
    Ok(kvs)
}

#[tauri::command]
pub fn set_tracker_kv(key: String, value: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.execute(
        "INSERT INTO tracker_kv (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

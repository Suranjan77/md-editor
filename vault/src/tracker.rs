//! Study tracker persistence: study sessions and KV status stored in the
//! global settings sidecar database.

use crate::error::VaultError;
use crate::migrations;
use rusqlite::Connection;
use std::path::Path;

const COMPONENT: &str = "tracker";
const MIGRATIONS: &[&str] = &["CREATE TABLE tracker_sessions (
        id            INTEGER PRIMARY KEY AUTOINCREMENT,
        date          TEXT NOT NULL,
        hours         REAL NOT NULL,
        activity_type TEXT NOT NULL,
        phase         TEXT NOT NULL,
        notes         TEXT
    ) STRICT;
    CREATE TABLE tracker_kv (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    ) STRICT;"];

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StudySession {
    pub id: i64,
    pub date: String,
    pub hours: f32,
    pub activity_type: String,
    pub phase: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrackerKv {
    pub key: String,
    pub value: String,
}

pub struct TrackerStore {
    conn: Connection,
}

impl TrackerStore {
    /// Open (or create) the database on `db_path`.
    pub fn open(db_path: &Path) -> Result<Self, VaultError> {
        let mut conn = Connection::open(db_path)?;
        migrations::run(&mut conn, COMPONENT, MIGRATIONS)?;
        Ok(TrackerStore { conn })
    }

    pub fn open_in_memory() -> Result<Self, VaultError> {
        let mut conn = Connection::open_in_memory()?;
        migrations::run(&mut conn, COMPONENT, MIGRATIONS)?;
        Ok(TrackerStore { conn })
    }

    pub fn schema_version(&self) -> Result<u32, VaultError> {
        migrations::version(&self.conn, COMPONENT)
    }

    pub fn save_session(&mut self, session: &StudySession) -> Result<i64, VaultError> {
        if session.id == 0 {
            self.conn.execute(
                "INSERT INTO tracker_sessions (date, hours, activity_type, phase, notes)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    session.date,
                    session.hours,
                    session.activity_type,
                    session.phase,
                    session.notes
                ],
            )?;
            Ok(self.conn.last_insert_rowid())
        } else {
            self.conn.execute(
                "UPDATE tracker_sessions SET date = ?1, hours = ?2, activity_type = ?3, phase = ?4, notes = ?5
                 WHERE id = ?6",
                rusqlite::params![
                    session.date,
                    session.hours,
                    session.activity_type,
                    session.phase,
                    session.notes,
                    session.id
                ],
            )?;
            Ok(session.id)
        }
    }

    pub fn delete_session(&mut self, id: i64) -> Result<(), VaultError> {
        self.conn.execute(
            "DELETE FROM tracker_sessions WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    }

    pub fn get_sessions(&self) -> Result<Vec<StudySession>, VaultError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, date, hours, activity_type, phase, notes
             FROM tracker_sessions
             ORDER BY date DESC",
        )?;
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
        let mut sessions = Vec::new();
        for s in rows {
            sessions.push(s?);
        }
        Ok(sessions)
    }

    pub fn get_total_hours(&self) -> Result<f32, VaultError> {
        let mut stmt = self
            .conn
            .prepare("SELECT SUM(hours) FROM tracker_sessions")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(row.get(0).unwrap_or(0.0))
        } else {
            Ok(0.0)
        }
    }

    pub fn get_kv(&self) -> Result<Vec<TrackerKv>, VaultError> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM tracker_kv ORDER BY key")?;
        let rows = stmt.query_map([], |row| {
            Ok(TrackerKv {
                key: row.get(0)?,
                value: row.get(1)?,
            })
        })?;
        let mut entries = Vec::new();
        for entry in rows {
            entries.push(entry?);
        }
        Ok(entries)
    }

    pub fn set_kv(&mut self, key: &str, value: &str) -> Result<(), VaultError> {
        self.conn.execute(
            "INSERT INTO tracker_kv (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_tracker_store_flow() {
        let mut store = TrackerStore::open_in_memory().unwrap();
        assert_eq!(store.get_total_hours().unwrap(), 0.0);

        let id1 = store
            .save_session(&StudySession {
                id: 0,
                date: "2026-06-01 12:00:00".to_string(),
                hours: 1.5,
                activity_type: "Reading".to_string(),
                phase: "Math".to_string(),
                notes: Some("Intro".to_string()),
            })
            .unwrap();

        let id2 = store
            .save_session(&StudySession {
                id: 0,
                date: "2026-06-02 12:00:00".to_string(),
                hours: 2.0,
                activity_type: "Coding".to_string(),
                phase: "Systems".to_string(),
                notes: None,
            })
            .unwrap();

        assert_eq!(store.get_total_hours().unwrap(), 3.5);

        let list = store.get_sessions().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, id2);
        assert_eq!(list[1].id, id1);

        store.delete_session(id1).unwrap();
        assert_eq!(store.get_total_hours().unwrap(), 2.0);

        store.set_kv("test_key", "test_val").unwrap();
        let kvs = store.get_kv().unwrap();
        assert_eq!(kvs.len(), 1);
        assert_eq!(kvs[0].key, "test_key");
        assert_eq!(kvs[0].value, "test_val");
    }
}

use rusqlite::Connection;

use crate::tracker::{StudySession, TrackerKv};

pub(crate) fn save_session(db: &Connection, session: StudySession) -> Result<(), String> {
    db.execute(
        "INSERT INTO tracker_sessions (date, hours, activity_type, phase, notes)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            session.date,
            session.hours,
            session.activity_type,
            session.phase,
            session.notes
        ],
    )
    .map(|_| ())
    .map_err(|err| err.to_string())
}

pub(crate) fn delete_session(db: &Connection, id: i64) -> Result<(), String> {
    db.execute(
        "DELETE FROM tracker_sessions WHERE id = ?1",
        rusqlite::params![id],
    )
    .map(|_| ())
    .map_err(|err| err.to_string())
}

pub(crate) fn get_sessions(db: &Connection) -> Result<Vec<StudySession>, String> {
    let mut stmt = db
        .prepare(
            "SELECT id, date, hours, activity_type, phase, notes
             FROM tracker_sessions
             ORDER BY date DESC",
        )
        .map_err(|err| err.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(StudySession {
                id: row.get(0)?,
                date: row.get(1)?,
                hours: row.get(2)?,
                activity_type: row.get(3)?,
                phase: row.get(4)?,
                notes: row.get(5)?,
            })
        })
        .map_err(|err| err.to_string())?;

    let mut sessions = Vec::new();
    for session in rows.flatten() {
        sessions.push(session);
    }
    Ok(sessions)
}

pub(crate) fn get_total_hours(db: &Connection) -> Result<f32, String> {
    let mut stmt = db
        .prepare("SELECT SUM(hours) FROM tracker_sessions")
        .map_err(|err| err.to_string())?;
    let mut rows = stmt.query([]).map_err(|err| err.to_string())?;

    if let Some(row) = rows.next().map_err(|err| err.to_string())? {
        Ok(row.get(0).unwrap_or(0.0))
    } else {
        Ok(0.0)
    }
}

pub(crate) fn get_kv(db: &Connection) -> Result<Vec<TrackerKv>, String> {
    let mut stmt = db
        .prepare("SELECT key, value FROM tracker_kv ORDER BY key")
        .map_err(|err| err.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(TrackerKv {
                key: row.get(0)?,
                value: row.get(1)?,
            })
        })
        .map_err(|err| err.to_string())?;

    let mut entries = Vec::new();
    for entry in rows.flatten() {
        entries.push(entry);
    }
    Ok(entries)
}

pub(crate) fn set_kv(db: &Connection, key: &str, value: &str) -> Result<(), String> {
    db.execute(
        "INSERT INTO tracker_kv (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )
    .map(|_| ())
    .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{delete_session, get_kv, get_sessions, get_total_hours, save_session, set_kv};
    use crate::database::initialize;
    use crate::tracker::StudySession;

    #[test]
    fn sessions_are_ordered_totaled_and_deleted() {
        let db = initialized_database();
        save_session(&db, session("2026-06-01", 1.5, Some("first")))
            .expect("first session should save");
        save_session(&db, session("2026-06-03", 2.25, None)).expect("second session should save");

        let sessions = get_sessions(&db).expect("sessions should load");
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].date, "2026-06-03");
        assert_eq!(sessions[1].notes.as_deref(), Some("first"));
        assert_eq!(get_total_hours(&db).expect("total should load"), 3.75);

        delete_session(&db, sessions[0].id).expect("session should delete");
        let remaining = get_sessions(&db).expect("remaining sessions should load");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].date, "2026-06-01");
    }

    #[test]
    fn empty_session_total_is_zero() {
        let db = initialized_database();

        assert_eq!(get_total_hours(&db).expect("empty total should load"), 0.0);
    }

    #[test]
    fn tracker_kv_is_sorted_and_upserted() {
        let db = initialized_database();
        set_kv(&db, "beta", "2").expect("beta should save");
        set_kv(&db, "alpha", "1").expect("alpha should save");
        set_kv(&db, "beta", "updated").expect("beta should update");

        let entries = get_kv(&db).expect("tracker values should load");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "alpha");
        assert_eq!(entries[1].key, "beta");
        assert_eq!(entries[1].value, "updated");
    }

    fn initialized_database() -> Connection {
        let db = Connection::open_in_memory().expect("memory database should open");
        initialize(&db).expect("database should initialize");
        db
    }

    fn session(date: &str, hours: f32, notes: Option<&str>) -> StudySession {
        StudySession {
            id: 0,
            date: date.to_string(),
            hours,
            activity_type: "Reading".to_string(),
            phase: "Study".to_string(),
            notes: notes.map(str::to_string),
        }
    }
}

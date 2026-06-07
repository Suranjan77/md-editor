use rusqlite::{Connection, Error};

pub(crate) fn get(db: &Connection, key: &str) -> Result<Option<String>, String> {
    let mut stmt = db
        .prepare("SELECT value FROM settings WHERE key = ?1")
        .map_err(|err| err.to_string())?;

    match stmt.query_row([key], |row| row.get(0)) {
        Ok(value) => Ok(Some(value)),
        Err(Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

pub(crate) fn set(db: &Connection, key: &str, value: &str) -> Result<(), String> {
    db.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )
    .map(|_| ())
    .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{get, set};
    use crate::database::initialize;

    #[test]
    fn setting_round_trip_and_upsert() {
        let db = initialized_database();

        assert_eq!(
            get(&db, "theme").expect("missing setting should load"),
            None
        );

        set(&db, "theme", "light").expect("setting should save");
        assert_eq!(
            get(&db, "theme").expect("setting should load"),
            Some("light".to_string())
        );

        set(&db, "theme", "dark").expect("setting should update");
        assert_eq!(
            get(&db, "theme").expect("updated setting should load"),
            Some("dark".to_string())
        );
    }

    fn initialized_database() -> Connection {
        let db = Connection::open_in_memory().expect("memory database should open");
        initialize(&db).expect("database should initialize");
        db
    }
}

use crate::state::AppState;

/// Get a configuration value by key.
pub fn get_sys_config(state: &AppState, key: &str) -> Result<Option<String>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = db
        .prepare("SELECT value FROM settings WHERE key = ?1")
        .map_err(|e| e.to_string())?;

    let maybe_val: Result<String, _> = stmt.query_row([key], |row| row.get(0));

    match maybe_val {
        Ok(val) => Ok(Some(val)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Set a configuration value by key (upsert).
pub fn set_sys_config(state: &AppState, key: &str, value: &str) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Read a single settings value without constructing a full [`AppState`].
///
/// Window geometry has to be known *before* the window is created, which is
/// before the app (and its `AppState`) exists. This opens the settings
/// database on its own for that one lookup and closes it again; every other
/// read goes through [`get_sys_config`].
pub fn read_startup_value(key: &str) -> Option<String> {
    let db = rusqlite::Connection::open(crate::state::settings_db_path()).ok()?;
    db.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        rusqlite::params![key],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

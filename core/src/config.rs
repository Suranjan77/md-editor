use crate::database::settings_repository;
use crate::state::AppState;

/// Get a configuration value by key.
pub fn get_sys_config(state: &AppState, key: &str) -> Result<Option<String>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    settings_repository::get(&db, key)
}

/// Set a configuration value by key (upsert).
pub fn set_sys_config(state: &AppState, key: &str, value: &str) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    settings_repository::set(&db, key, value)
}

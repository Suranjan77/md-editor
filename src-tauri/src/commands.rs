use std::path::PathBuf;
use std::sync::Mutex;

use tauri::State;

use rusqlite::Connection;

use crate::file_index::FileIndex;
use crate::fs_commands;
use crate::ipc_types::FileEntry;

pub struct AppState {
    pub vault_root: Mutex<Option<PathBuf>>,
    pub file_index: Mutex<FileIndex>,
    pub db: Mutex<Connection>,
}

impl AppState {
    pub fn new() -> Self {
        let mut db_path = PathBuf::from("md_editor_settings.sqlite");
        if let Ok(mut exe_path) = std::env::current_exe() {
            exe_path.pop(); // Remove the executable file name, leaving the portable directory
            exe_path.push("md_editor_settings.sqlite");
            db_path = exe_path;
        }

        let db = Connection::open(&db_path).expect("Failed to open local sqlite database");
        db.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )
        .expect("Failed to initialize settings table");

        AppState {
            vault_root: Mutex::new(None),
            file_index: Mutex::new(FileIndex::new(PathBuf::new())),
            db: Mutex::new(db),
        }
    }
}

#[tauri::command]
pub fn open_file(path: String, state: State<'_, AppState>) -> Result<Vec<u8>, String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = fs_commands::resolve_vault_path(vault_root, &path);

    if fs_commands::is_image(&abs_path.extension().unwrap().to_str().unwrap()) {
        let content = fs_commands::read_image(&abs_path)?;
        return Ok(content);
    } else {
        let content = fs_commands::read_file(&abs_path)?;
        // Update wikilink index
        let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
        index.update_file(&abs_path, &content);
        return Ok(content.into_bytes());
    }
}

#[tauri::command]
pub fn save_file(path: String, content: String, state: State<'_, AppState>) -> Result<(), String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = fs_commands::resolve_vault_path(vault_root, &path);
    fs_commands::write_file(&abs_path, &content)?;

    // Update wikilink index
    let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
    index.update_file(&abs_path, &content);

    Ok(())
}

#[tauri::command]
pub fn create_file(path: String, state: State<'_, AppState>) -> Result<(), String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = fs_commands::resolve_vault_path(vault_root, &path);
    fs_commands::create_file(&abs_path)
}

#[tauri::command]
pub fn delete_file(path: String, state: State<'_, AppState>) -> Result<(), String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = fs_commands::resolve_vault_path(vault_root, &path);

    let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
    index.remove_file(&abs_path);

    fs_commands::delete_file(&abs_path)
}

#[tauri::command]
pub fn list_vault(state: State<'_, AppState>) -> Result<Vec<FileEntry>, String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    fs_commands::list_vault(vault_root)
}

#[tauri::command]
pub fn set_vault_root(path: String, state: State<'_, AppState>) -> Result<Vec<FileEntry>, String> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(format!("Not a directory: {}", path));
    }

    {
        let mut vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
        *vault_root = Some(root.clone());
    }

    {
        let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
        *index = FileIndex::new(root.clone());
    }

    fs_commands::list_vault(&root)
}

#[tauri::command]
pub fn get_backlinks(path: String, state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = fs_commands::resolve_vault_path(vault_root, &path);

    let index = state.file_index.lock().map_err(|e| e.to_string())?;
    let backlinks = index.get_backlinks(&abs_path);

    Ok(backlinks
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect())
}

#[tauri::command]
pub fn get_sys_config(key: String, state: State<'_, AppState>) -> Result<Option<String>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = db
        .prepare("SELECT value FROM settings WHERE key = ?1")
        .map_err(|e| e.to_string())?;

    // Attempt to map the row explicitly
    let maybe_val: Result<String, _> = stmt.query_row([&key], |row| row.get(0));

    match maybe_val {
        Ok(val) => Ok(Some(val)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub fn set_sys_config(
    key: String,
    value: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![&key, &value],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

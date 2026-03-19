/**
 * Simplified Rust commands — pure file I/O + backlinks.
 * No piece table, no tree-sitter, no AST diffs.
 * CodeMirror 6 handles all document state on the frontend.
 */
use std::path::PathBuf;
use std::sync::Mutex;

use tauri::State;

use crate::file_index::FileIndex;
use crate::fs_commands;
use crate::ipc_types::FileEntry;

pub struct AppState {
    pub vault_root: Mutex<Option<PathBuf>>,
    pub file_index: Mutex<FileIndex>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            vault_root: Mutex::new(None),
            file_index: Mutex::new(FileIndex::new(PathBuf::new())),
        }
    }
}

#[tauri::command]
pub fn open_file(path: String, state: State<'_, AppState>) -> Result<String, String> {
    let vault_root = state.vault_root.lock().map_err(|e| e.to_string())?;
    let vault_root = vault_root.as_ref().ok_or("No vault root set")?;
    let abs_path = fs_commands::resolve_vault_path(vault_root, &path);
    let content = fs_commands::read_file(&abs_path)?;

    // Update wikilink index
    let mut index = state.file_index.lock().map_err(|e| e.to_string())?;
    index.update_file(&abs_path, &content);

    Ok(content)
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

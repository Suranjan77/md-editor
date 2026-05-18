use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::file_index::FileIndex;
use crate::pdf::{PdfRenderer, PdfState};

/// Application-wide shared state.
/// Wrap in `Arc<AppState>` when sharing across threads.
pub struct AppState {
    pub vault_root: Mutex<Option<PathBuf>>,
    pub file_index: Mutex<FileIndex>,
    pub db: Mutex<Connection>,
    pub pdf_state: Mutex<PdfState>,
    pub pdf_renderer: Option<PdfRenderer>,
}

impl AppState {
    pub fn new() -> Self {
        let db_path = settings_db_path();
        let db = Connection::open(&db_path).expect("Failed to open local sqlite database");

        db.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )
        .expect("Failed to initialize settings table");

        db.execute(
            "CREATE TABLE IF NOT EXISTS tracker_sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                date TEXT NOT NULL,
                hours REAL NOT NULL,
                activity_type TEXT NOT NULL,
                phase TEXT NOT NULL,
                notes TEXT
            )",
            [],
        )
        .expect("Failed to create tracker_sessions");

        db.execute(
            "CREATE TABLE IF NOT EXISTS tracker_activity (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                type TEXT NOT NULL,
                text TEXT NOT NULL,
                time TEXT NOT NULL
            )",
            [],
        )
        .expect("Failed to create tracker_activity");

        db.execute(
            "CREATE TABLE IF NOT EXISTS tracker_kv (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )
        .expect("Failed to create tracker_kv");

        db.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS file_search USING fts5(
                path,
                content
            )",
            [],
        )
        .expect("Failed to create file_search fts table");

        AppState {
            vault_root: Mutex::new(None),
            file_index: Mutex::new(FileIndex::new(PathBuf::new())),
            db: Mutex::new(db),
            pdf_state: Mutex::new(PdfState::new()),
            pdf_renderer: PdfRenderer::new().ok(),
        }
    }

    pub fn new_in_memory() -> Self {
        let db = Connection::open_in_memory().expect("Failed to open memory sqlite database");

        db.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )
        .expect("Failed to initialize settings table");

        db.execute(
            "CREATE TABLE IF NOT EXISTS tracker_sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                date TEXT NOT NULL,
                hours REAL NOT NULL,
                activity_type TEXT NOT NULL,
                phase TEXT NOT NULL,
                notes TEXT
            )",
            [],
        )
        .expect("Failed to create tracker_sessions");

        db.execute(
            "CREATE TABLE IF NOT EXISTS tracker_activity (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                type TEXT NOT NULL,
                text TEXT NOT NULL,
                time TEXT NOT NULL
            )",
            [],
        )
        .expect("Failed to create tracker_activity");

        db.execute(
            "CREATE TABLE IF NOT EXISTS tracker_kv (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )
        .expect("Failed to create tracker_kv");

        db.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS file_search USING fts5(
                path,
                content
            )",
            [],
        )
        .expect("Failed to create file_search fts table");

        AppState {
            vault_root: Mutex::new(None),
            file_index: Mutex::new(FileIndex::new(PathBuf::new())),
            db: Mutex::new(db),
            pdf_state: Mutex::new(PdfState::new()),
            pdf_renderer: None,
        }
    }
}

fn settings_db_path() -> PathBuf {
    let mut dir = config_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("Failed to create config directory {}: {err}", dir.display());
        return PathBuf::from("md_editor_settings.sqlite");
    }
    dir.push("md_editor_settings.sqlite");
    dir
}

fn config_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.to_path_buf();
        }
    }

    PathBuf::from(".")
}

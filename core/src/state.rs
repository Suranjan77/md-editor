use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::file_index::FileIndex;
use crate::pdf::{PdfState, PdfRenderer};

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
        let mut db_path = PathBuf::from("md_editor_settings.sqlite");
        if let Ok(mut exe_path) = std::env::current_exe() {
            exe_path.pop(); // Remove the executable file name
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
}

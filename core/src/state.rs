use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::file_index::FileIndex;
use crate::pdf::{
    PdfAnnotation, PdfAnnotationColor, PdfAnnotationKind, PdfRect, PdfRenderer, PdfState,
    PdfTextRange,
};

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
        init_schema(&db).expect("Failed to initialize database schema");

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
        init_schema(&db).expect("Failed to initialize database schema");

        AppState {
            vault_root: Mutex::new(None),
            file_index: Mutex::new(FileIndex::new(PathBuf::new())),
            db: Mutex::new(db),
            pdf_state: Mutex::new(PdfState::new()),
            pdf_renderer: None,
        }
    }

    pub fn save_pdf_document(
        &self,
        document_id: &str,
        vault_relative_path: &str,
        file_size: u64,
        modified_at: Option<i64>,
    ) -> Result<(), String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        db.execute(
            "INSERT INTO pdf_documents (document_id, vault_relative_path, file_size, modified_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(document_id) DO UPDATE SET
                vault_relative_path = excluded.vault_relative_path,
                file_size = excluded.file_size,
                modified_at = excluded.modified_at,
                updated_at = excluded.updated_at",
            rusqlite::params![
                document_id,
                vault_relative_path,
                file_size as i64,
                modified_at,
                now
            ],
        )
        .map_err(|e| format!("Failed to save pdf document: {e}"))?;

        Ok(())
    }

    pub fn get_pdf_path_by_id(&self, document_id: &str) -> Result<Option<String>, String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        let mut stmt = db
            .prepare("SELECT vault_relative_path FROM pdf_documents WHERE document_id = ?1")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query([document_id]).map_err(|e| e.to_string())?;
        if let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let path: String = row.get(0).map_err(|e| e.to_string())?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    pub fn save_pdf_annotation(&self, ann: &PdfAnnotation) -> Result<(), String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        let ranges_json = serde_json::to_string(&ann.ranges)
            .map_err(|e| format!("Failed to serialize ranges: {e}"))?;
        let rects_json = serde_json::to_string(&ann.rects)
            .map_err(|e| format!("Failed to serialize rects: {e}"))?;

        db.execute(
            "INSERT INTO pdf_annotations (
                id, document_id, page_index, kind, color, selected_text,
                ranges_json, rects_json, note, linked_note_path, markdown_anchor,
                created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO UPDATE SET
                color = excluded.color,
                selected_text = excluded.selected_text,
                ranges_json = excluded.ranges_json,
                rects_json = excluded.rects_json,
                note = excluded.note,
                linked_note_path = excluded.linked_note_path,
                markdown_anchor = excluded.markdown_anchor,
                updated_at = excluded.updated_at",
            rusqlite::params![
                ann.id,
                ann.document_id,
                ann.page_index as i32,
                ann.kind.as_str(),
                ann.color.as_str(),
                ann.selected_text,
                ranges_json,
                rects_json,
                ann.note,
                ann.linked_note_path,
                ann.markdown_anchor,
                ann.created_at,
                ann.updated_at,
            ],
        )
        .map_err(|e| format!("Failed to save pdf annotation: {e}"))?;

        Ok(())
    }

    pub fn delete_pdf_annotation(&self, id: &str) -> Result<(), String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        db.execute("DELETE FROM pdf_annotations WHERE id = ?1", [id])
            .map_err(|e| format!("Failed to delete pdf annotation: {e}"))?;
        Ok(())
    }

    pub fn get_pdf_annotations(
        &self,
        document_id: &str,
        page_index: Option<u16>,
    ) -> Result<Vec<PdfAnnotation>, String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        let query = if page_index.is_some() {
            "SELECT id, document_id, page_index, kind, color, selected_text,
                    ranges_json, rects_json, note, linked_note_path, markdown_anchor,
                    created_at, updated_at
             FROM pdf_annotations
             WHERE document_id = ?1 AND page_index = ?2
             ORDER BY created_at ASC"
        } else {
            "SELECT id, document_id, page_index, kind, color, selected_text,
                    ranges_json, rects_json, note, linked_note_path, markdown_anchor,
                    created_at, updated_at
             FROM pdf_annotations
             WHERE document_id = ?1
             ORDER BY created_at ASC"
        };

        let mut stmt = db.prepare(query).map_err(|e| e.to_string())?;
        let mut rows = if let Some(page) = page_index {
            stmt.query(rusqlite::params![document_id, page as i32])
                .map_err(|e| e.to_string())?
        } else {
            stmt.query(rusqlite::params![document_id])
                .map_err(|e| e.to_string())?
        };

        let mut annotations = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let id: String = row.get(0).map_err(|e| e.to_string())?;
            let doc_id: String = row.get(1).map_err(|e| e.to_string())?;
            let page_idx: i32 = row.get(2).map_err(|e| e.to_string())?;
            let kind_str: String = row.get(3).map_err(|e| e.to_string())?;
            let color_str: String = row.get(4).map_err(|e| e.to_string())?;
            let selected_text: String = row.get(5).map_err(|e| e.to_string())?;
            let ranges_json: String = row.get(6).map_err(|e| e.to_string())?;
            let rects_json: String = row.get(7).map_err(|e| e.to_string())?;
            let note: Option<String> = row.get(8).map_err(|e| e.to_string())?;
            let linked_note_path: Option<String> = row.get(9).map_err(|e| e.to_string())?;
            let markdown_anchor: Option<String> = row.get(10).map_err(|e| e.to_string())?;
            let created_at: i64 = row.get(11).map_err(|e| e.to_string())?;
            let updated_at: i64 = row.get(12).map_err(|e| e.to_string())?;

            let kind = PdfAnnotationKind::from_str(&kind_str)?;
            let color = PdfAnnotationColor::from_str(&color_str)?;
            let ranges: Vec<PdfTextRange> = serde_json::from_str(&ranges_json)
                .map_err(|e| format!("Failed to parse ranges JSON: {e}"))?;
            let rects: Vec<PdfRect> = serde_json::from_str(&rects_json)
                .map_err(|e| format!("Failed to parse rects JSON: {e}"))?;

            annotations.push(PdfAnnotation {
                id,
                document_id: doc_id,
                page_index: page_idx as u16,
                kind,
                color,
                selected_text,
                ranges,
                rects,
                note,
                linked_note_path,
                markdown_anchor,
                created_at,
                updated_at,
            });
        }

        Ok(annotations)
    }
}

/// Current on-disk schema version. Bump this and add a corresponding arm in
/// [`apply_migrations`] whenever the schema changes.
const SCHEMA_VERSION: i64 = 1;

/// Create all tables/indexes (idempotent) and run version migrations.
///
/// Shared by both the on-disk and in-memory constructors so the schema lives
/// in exactly one place.
fn init_schema(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS tracker_sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            date TEXT NOT NULL,
            hours REAL NOT NULL,
            activity_type TEXT NOT NULL,
            phase TEXT NOT NULL,
            notes TEXT
        );

        CREATE TABLE IF NOT EXISTS tracker_activity (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            type TEXT NOT NULL,
            text TEXT NOT NULL,
            time TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS tracker_kv (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS file_search USING fts5(
            path,
            content
        );

        CREATE TABLE IF NOT EXISTS pdf_documents (
            document_id TEXT PRIMARY KEY,
            vault_relative_path TEXT NOT NULL,
            file_size INTEGER NOT NULL,
            modified_at INTEGER,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS pdf_documents_vault_relative_path
            ON pdf_documents(vault_relative_path);

        CREATE TABLE IF NOT EXISTS pdf_annotations (
            id TEXT PRIMARY KEY,
            document_id TEXT NOT NULL,
            page_index INTEGER NOT NULL,
            kind TEXT NOT NULL,
            color TEXT NOT NULL,
            selected_text TEXT NOT NULL,
            ranges_json TEXT NOT NULL,
            rects_json TEXT NOT NULL,
            note TEXT,
            linked_note_path TEXT,
            markdown_anchor TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS pdf_annotations_document_page
            ON pdf_annotations(document_id, page_index);
        CREATE INDEX IF NOT EXISTS pdf_annotations_document_linked_note
            ON pdf_annotations(document_id, linked_note_path)
            WHERE linked_note_path IS NOT NULL AND linked_note_path != '';
        CREATE INDEX IF NOT EXISTS pdf_annotations_linked_note
            ON pdf_annotations(linked_note_path)
            WHERE linked_note_path IS NOT NULL AND linked_note_path != '';",
    )?;

    apply_migrations(db)?;
    Ok(())
}

/// Apply incremental migrations based on `PRAGMA user_version`.
///
/// New schema changes should be expressed as additive steps here, each
/// bumping `user_version`, rather than editing the base DDL above (which only
/// runs for fresh databases via `IF NOT EXISTS`).
fn apply_migrations(db: &Connection) -> rusqlite::Result<()> {
    let mut version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    // Example shape for future migrations:
    // if version < 2 {
    //     db.execute_batch("ALTER TABLE ...;")?;
    //     version = 2;
    // }

    if version < SCHEMA_VERSION {
        version = SCHEMA_VERSION;
    }

    db.execute_batch(&format!("PRAGMA user_version = {version};"))?;
    Ok(())
}

fn settings_db_path() -> PathBuf {
    let mut dir = data_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("Failed to create data directory {}: {err}", dir.display());
        return PathBuf::from("md_editor_settings.sqlite");
    }
    dir.push("md_editor_settings.sqlite");
    dir
}

/// Per-user data directory for the settings database.
///
/// Prefers the platform data directory (XDG `$XDG_DATA_HOME` or
/// `~/.local/share` on Linux) so the app works when installed to a read-only
/// location. Falls back to the executable directory, then the current
/// directory, for portable/unusual setups.
fn data_dir() -> PathBuf {
    if let Some(base) = platform_data_home() {
        return base.join("md-editor");
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.to_path_buf();
        }
    }
    PathBuf::from(".")
}

fn platform_data_home() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join("Library").join("Application Support"))
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
            })
    }
}

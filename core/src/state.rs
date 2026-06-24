use std::path::{Path, PathBuf};
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
        // WAL keeps writes fast (and would allow concurrent reads if we ever add
        // a second connection); synchronous=NORMAL is the safe, recommended
        // pairing for WAL. Best-effort — fall back silently if unsupported.
        let _ = db.pragma_update(None, "journal_mode", "WAL");
        let _ = db.pragma_update(None, "synchronous", "NORMAL");
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

    /// Cached resolved internal references for a document, or `None` if the
    /// document has not been scanned yet. Keyed by `document_id` (a content
    /// hash), so the cache is valid until the file changes.
    pub fn get_pdf_references(
        &self,
        document_id: &str,
    ) -> Option<Vec<crate::references::ReferenceLink>> {
        let db = self.db.lock().ok()?;
        let json: String = db
            .query_row(
                "SELECT links_json FROM pdf_references WHERE document_id = ?1",
                [document_id],
                |row| row.get(0),
            )
            .ok()?;
        // Stored as (resolver_version, links); ignore entries from an older
        // resolver so an algorithm change doesn't serve stale results.
        let (version, links): (u32, Vec<crate::references::ReferenceLink>) =
            serde_json::from_str(&json).ok()?;
        (version == crate::references::RESOLVER_VERSION).then_some(links)
    }

    /// Store resolved references for a document. Best-effort; the one-time text
    /// scan that produced these is the expensive part, so caching avoids
    /// repeating it on every open.
    pub fn put_pdf_references(
        &self,
        document_id: &str,
        links: &[crate::references::ReferenceLink],
    ) {
        let Ok(db) = self.db.lock() else {
            return;
        };
        let Ok(json) = serde_json::to_string(&(crate::references::RESOLVER_VERSION, links))
        else {
            return;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if let Err(e) = db.execute(
            "INSERT INTO pdf_references (document_id, links_json, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(document_id) DO UPDATE SET
                links_json = excluded.links_json,
                updated_at = excluded.updated_at",
            rusqlite::params![document_id, json, now],
        ) {
            eprintln!("Failed to cache PDF references for {document_id}: {e}");
        }
    }

    /// Return cached extracted text for a PDF if the cache entry's size and
    /// modified-time match the current file (i.e. the PDF is unchanged since it
    /// was last indexed). Returns `None` on any mismatch or error.
    pub fn get_cached_pdf_text(
        &self,
        rel_path: &str,
        file_size: u64,
        modified_at: i64,
    ) -> Option<String> {
        let db = self.db.lock().ok()?;
        db.query_row(
            "SELECT content FROM pdf_text_cache
             WHERE path = ?1 AND file_size = ?2 AND modified_at = ?3",
            rusqlite::params![rel_path, file_size as i64, modified_at],
            |row| row.get::<_, String>(0),
        )
        .ok()
    }

    /// Store extracted PDF text keyed by path + size + modified-time so an
    /// unchanged PDF is not re-extracted on the next vault open. Best-effort.
    pub fn put_cached_pdf_text(
        &self,
        rel_path: &str,
        file_size: u64,
        modified_at: i64,
        content: &str,
    ) {
        let Ok(db) = self.db.lock() else {
            return;
        };
        if let Err(e) = db.execute(
            "INSERT INTO pdf_text_cache (path, file_size, modified_at, content)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET
                file_size = excluded.file_size,
                modified_at = excluded.modified_at,
                content = excluded.content",
            rusqlite::params![rel_path, file_size as i64, modified_at, content],
        ) {
            eprintln!("Failed to cache PDF text for {rel_path}: {e}");
        }
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
            WHERE linked_note_path IS NOT NULL AND linked_note_path != '';

        CREATE TABLE IF NOT EXISTS pdf_text_cache (
            path TEXT PRIMARY KEY,
            file_size INTEGER NOT NULL,
            modified_at INTEGER NOT NULL,
            content TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS pdf_references (
            document_id TEXT PRIMARY KEY,
            links_json TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );",
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

const DB_FILE_NAME: &str = "md_editor_settings.sqlite";

fn settings_db_path() -> PathBuf {
    let mut dir = data_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("Failed to create data directory {}: {err}", dir.display());
        return PathBuf::from(DB_FILE_NAME);
    }
    dir.push(DB_FILE_NAME);

    // One-time migration: earlier versions stored the database next to the
    // executable. If there's no database at the new (XDG) location but a legacy
    // one exists, copy it over so users keep their settings and study history.
    if !dir.exists() {
        if let Some(legacy) = legacy_db_path() {
            migrate_legacy_db(&legacy, &dir);
        }
    }

    dir
}

/// Database location used by versions that stored it next to the executable.
fn legacy_db_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join(DB_FILE_NAME))
}

/// Copy a legacy database (and its WAL sidecars, if any) to `new_path` when the
/// new location is empty and the legacy file exists in a different place. The
/// legacy file is left in place so nothing is lost if the copy is interrupted.
fn migrate_legacy_db(legacy: &Path, new_path: &Path) {
    if legacy == new_path || !legacy.exists() || new_path.exists() {
        return;
    }
    if let Err(e) = std::fs::copy(legacy, new_path) {
        eprintln!("Failed to migrate legacy settings database: {e}");
        return;
    }
    // Best-effort: bring along WAL/SHM sidecars so any not-yet-checkpointed
    // writes survive the move.
    for suffix in ["-wal", "-shm"] {
        let from = sidecar(legacy, suffix);
        if from.exists() {
            let _ = std::fs::copy(&from, sidecar(new_path, suffix));
        }
    }
    eprintln!(
        "Migrated settings database from {} to {}",
        legacy.display(),
        new_path.display()
    );
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_legacy_db_with_sidecars_when_new_location_empty() {
        let base = std::env::temp_dir().join(format!("md_editor_mig_{}", uuid::Uuid::new_v4()));
        let legacy_dir = base.join("legacy");
        let new_dir = base.join("xdg");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::create_dir_all(&new_dir).unwrap();

        let legacy = legacy_dir.join(DB_FILE_NAME);
        let new_path = new_dir.join(DB_FILE_NAME);
        std::fs::write(&legacy, b"DBDATA").unwrap();
        std::fs::write(sidecar(&legacy, "-wal"), b"WAL").unwrap();

        migrate_legacy_db(&legacy, &new_path);

        assert_eq!(std::fs::read(&new_path).unwrap(), b"DBDATA");
        assert_eq!(std::fs::read(sidecar(&new_path, "-wal")).unwrap(), b"WAL");
        // Legacy file is left untouched.
        assert!(legacy.exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn pdf_text_cache_hits_only_when_size_and_mtime_match() {
        let state = AppState::new_in_memory();
        assert!(state.get_cached_pdf_text("a.pdf", 100, 5).is_none());

        state.put_cached_pdf_text("a.pdf", 100, 5, "hello world");
        assert_eq!(
            state.get_cached_pdf_text("a.pdf", 100, 5).as_deref(),
            Some("hello world")
        );
        // A changed size or mtime is a miss (the PDF was modified).
        assert!(state.get_cached_pdf_text("a.pdf", 101, 5).is_none());
        assert!(state.get_cached_pdf_text("a.pdf", 100, 6).is_none());

        // Re-extraction updates the entry in place.
        state.put_cached_pdf_text("a.pdf", 101, 6, "new text");
        assert_eq!(
            state.get_cached_pdf_text("a.pdf", 101, 6).as_deref(),
            Some("new text")
        );
    }

    #[test]
    fn migration_never_overwrites_an_existing_new_db() {
        let base = std::env::temp_dir().join(format!("md_editor_mig_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let legacy = base.join("legacy.sqlite");
        let new_path = base.join("new.sqlite");
        std::fs::write(&legacy, b"OLD").unwrap();
        std::fs::write(&new_path, b"CURRENT").unwrap();

        migrate_legacy_db(&legacy, &new_path);

        assert_eq!(std::fs::read(&new_path).unwrap(), b"CURRENT");
        let _ = std::fs::remove_dir_all(&base);
    }
}

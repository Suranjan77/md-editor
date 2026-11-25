//! `AppState`: the shared SQLite connection, schema and migrations, and vault generation state.

mod error;
mod pdf_store;
pub use error::{StateError, StateResult};
use std::path::{Path, PathBuf};
use std::sync::{
    Mutex, MutexGuard,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

use rusqlite::Connection;

use crate::file_index::FileIndex;
use crate::pdf::PdfRenderer;
#[cfg(test)]
use crate::pdf::{PdfAnnotation, PdfAnnotationColor, PdfAnnotationKind};

/// Application-wide shared state.
/// Wrap in `Arc<AppState>` when sharing across threads.
#[derive(Debug)]
pub struct AppState {
    pub vault_root: Mutex<Option<PathBuf>>,
    pub file_index: Mutex<FileIndex>,
    pub db: Mutex<Connection>,
    pub pdf_renderer: Option<PdfRenderer>,
    pub(crate) vault_generation: AtomicU64,
    pub(crate) vault_publish_lock: Mutex<()>,
    vault_content_revision: AtomicU64,
    vault_mutation_lock: Mutex<()>,
    startup_notice: Mutex<Option<String>>,
}

pub(crate) struct VaultMutationGuard<'a> {
    _lock: MutexGuard<'a, ()>,
    revision: &'a AtomicU64,
}

impl Drop for VaultMutationGuard<'_> {
    fn drop(&mut self) {
        self.revision.fetch_add(1, Ordering::SeqCst);
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        let db_path = settings_db_path();
        Self::new_with_db_path(&db_path)
    }

    fn new_with_db_path(db_path: &Path) -> Self {
        let (db, startup_notice) = match open_and_initialize_db(db_path) {
            Ok(db) => (db, None),
            Err(first_error) => {
                let backup = quarantine_database(db_path).unwrap_or_else(|rename_error| {
                    panic!(
                        "Failed to open settings database ({first_error}); could not preserve it before retrying: {rename_error}"
                    )
                });
                let db = open_and_initialize_db(db_path).unwrap_or_else(|retry_error| {
                    panic!(
                        "Failed to recreate settings database after preserving {}: {retry_error}",
                        backup.display()
                    )
                });
                (
                    db,
                    Some(format!(
                        "Settings database was corrupt and has been reset; the old file was kept as {}",
                        backup.display()
                    )),
                )
            }
        };
        let _ = db.pragma_update(None, "journal_mode", "WAL");
        let _ = db.pragma_update(None, "synchronous", "NORMAL");
        let _ = db.busy_timeout(Duration::from_secs(5));

        AppState {
            vault_root: Mutex::new(None),
            file_index: Mutex::new(FileIndex::new(PathBuf::new())),
            db: Mutex::new(db),
            pdf_renderer: PdfRenderer::new().ok(),
            vault_generation: AtomicU64::new(0),
            vault_publish_lock: Mutex::new(()),
            vault_content_revision: AtomicU64::new(0),
            vault_mutation_lock: Mutex::new(()),
            startup_notice: Mutex::new(startup_notice),
        }
    }

    pub fn new_in_memory() -> Self {
        let db = Connection::open_in_memory().expect("Failed to open memory sqlite database");
        let _ = db.busy_timeout(Duration::from_secs(5));
        init_schema(&db).expect("Failed to initialize database schema");

        AppState {
            vault_root: Mutex::new(None),
            file_index: Mutex::new(FileIndex::new(PathBuf::new())),
            db: Mutex::new(db),
            pdf_renderer: None,
            vault_generation: AtomicU64::new(0),
            vault_publish_lock: Mutex::new(()),
            vault_content_revision: AtomicU64::new(0),
            vault_mutation_lock: Mutex::new(()),
            startup_notice: Mutex::new(None),
        }
    }

    pub fn take_startup_notice(&self) -> Option<String> {
        self.startup_notice.lock().ok()?.take()
    }

    /// Declare the vault the UI intends to use and invalidate older indexing work.
    pub fn begin_vault_change(&self, root: PathBuf) -> StateResult<u64> {
        // Outside the locks: it touches the filesystem (canonicalize).
        let scope = Self::pdf_scope_prefix_for_root(&root);
        let _publish = self.vault_publish_lock.lock()?;
        let _mutation = self.vault_mutation_lock.lock()?;
        {
            // Another vault's rows must not show while this one indexes; the
            // same vault's rows stay, so reopening re-reads only what changed
            // (`vault/indexing.rs`). Rows and their records go together.
            let db = self.db.lock()?;
            let same_vault: bool = db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM vault_file_meta WHERE scope = ?1)
                       AND NOT EXISTS(SELECT 1 FROM vault_file_meta WHERE scope != ?1)",
                    rusqlite::params![scope],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            if !same_vault {
                db.execute_batch("DELETE FROM file_search; DELETE FROM vault_file_meta;")
                    .map_err(|e| format!("Failed to clear previous vault search index: {e}"))?;
            }
        }
        *self.file_index.lock()? = FileIndex::new(root.clone());
        let generation = self
            .vault_generation
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1);
        *self.vault_root.lock()? = Some(root);
        self.vault_content_revision.fetch_add(1, Ordering::SeqCst);
        Ok(generation)
    }

    pub fn current_vault_generation(&self) -> u64 {
        self.vault_generation.load(Ordering::SeqCst)
    }

    pub(crate) fn current_vault_content_revision(&self) -> u64 {
        self.vault_content_revision.load(Ordering::SeqCst)
    }

    pub(crate) fn vault_mutation_guard(&self) -> StateResult<VaultMutationGuard<'_>> {
        Ok(VaultMutationGuard {
            _lock: self.vault_mutation_lock.lock()?,
            revision: &self.vault_content_revision,
        })
    }

    pub(crate) fn vault_publish_mutation_lock(&self) -> StateResult<MutexGuard<'_, ()>> {
        self.vault_mutation_lock.lock().map_err(StateError::from)
    }
}

/// Current on-disk schema version. Bump this and add a corresponding arm in
/// [`apply_migrations`] whenever the schema changes.
const SCHEMA_VERSION: i64 = 3;

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

fn apply_migrations(db: &Connection) -> rusqlite::Result<()> {
    let mut version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    // v2: cache recovered outlines of bookmark-less PDFs, so reopening one
    // does not rescan every page.
    if version < 2 {
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS pdf_outline (
                document_id TEXT PRIMARY KEY,
                outline_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );",
        )?;
    }

    // v3: what the search index last read of each vault file, so reopening a
    // vault re-reads only the files that changed.
    if version < 3 {
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS vault_file_meta (
                scope TEXT NOT NULL,
                path TEXT NOT NULL,
                size INTEGER NOT NULL,
                modified_ns INTEGER NOT NULL,
                links_json TEXT NOT NULL,
                PRIMARY KEY (scope, path)
            );",
        )?;
    }

    if version < SCHEMA_VERSION {
        version = SCHEMA_VERSION;
    }

    db.execute_batch(&format!("PRAGMA user_version = {version};"))?;
    Ok(())
}

const DB_FILE_NAME: &str = "md_editor_settings.sqlite";

fn open_and_initialize_db(path: &Path) -> rusqlite::Result<Connection> {
    let db = Connection::open(path)?;
    init_schema(&db)?;
    Ok(db)
}

fn quarantine_database(path: &Path) -> std::io::Result<PathBuf> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut backup = PathBuf::from(format!("{}.corrupt-{timestamp}", path.display()));
    let mut suffix = 1_u32;
    while backup.exists() {
        backup = PathBuf::from(format!("{}.corrupt-{timestamp}-{suffix}", path.display()));
        suffix += 1;
    }

    std::fs::rename(path, &backup)?;
    for sidecar_suffix in ["-wal", "-shm"] {
        let original_sidecar = sidecar(path, sidecar_suffix);
        if original_sidecar.exists() {
            let backup_sidecar = sidecar(&backup, sidecar_suffix);
            if let Err(error) = std::fs::rename(&original_sidecar, &backup_sidecar) {
                log::warn!(
                    "Failed to preserve settings database sidecar {}: {error}",
                    original_sidecar.display()
                );
            }
        }
    }
    Ok(backup)
}

pub fn settings_db_path() -> PathBuf {
    let mut dir = data_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        log::warn!("Failed to create data directory {}: {err}", dir.display());
        return PathBuf::from(DB_FILE_NAME);
    }
    dir.push(DB_FILE_NAME);

    if !dir.exists()
        && data_dir_override().is_none()
        && let Some(portable) = portable_db_path()
        && portable != dir
    {
        migrate_legacy_db(&portable, &dir);
    }

    dir
}

/// Read the last window geometry without constructing PDF services.
pub fn persisted_window_size() -> Option<(f32, f32)> {
    let db = Connection::open(settings_db_path()).ok()?;
    let read = |key: &str| -> Option<f32> {
        db.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get::<_, String>(0)
        })
        .ok()?
        .parse()
        .ok()
    };
    let width = read("layout_window_width")?;
    let height = read("layout_window_height")?;
    (width >= 640.0 && height >= 480.0).then_some((width, height))
}

fn portable_db_path() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join(DB_FILE_NAME))
}

fn migrate_legacy_db(legacy: &Path, new_path: &Path) {
    if legacy == new_path || !legacy.exists() || new_path.exists() {
        return;
    }
    if let Err(e) = std::fs::copy(legacy, new_path) {
        log::warn!("Failed to migrate legacy settings database: {e}");
        return;
    }
    for suffix in ["-wal", "-shm"] {
        let from = sidecar(legacy, suffix);
        if from.exists() {
            let _ = std::fs::copy(&from, sidecar(new_path, suffix));
        }
    }
    log::info!(
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

/// Overrides the data directory (database and log), and disables migration
/// from the portable location.
pub const DATA_DIR_ENV: &str = "MD_EDITOR_DATA_DIR";

static DATA_DIR_OVERRIDE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Pin the data directory for this process without touching the environment.
pub fn override_data_dir(dir: PathBuf) -> bool {
    DATA_DIR_OVERRIDE.get_or_init(|| dir.clone()) == &dir
}

fn data_dir_override() -> Option<PathBuf> {
    #[cfg(test)]
    DATA_DIR_OVERRIDE.get_or_init(|| {
        std::env::temp_dir().join(format!("md-editor-core-test-data-{}", std::process::id()))
    });
    DATA_DIR_OVERRIDE.get().cloned().or_else(|| {
        std::env::var_os(DATA_DIR_ENV)
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
    })
}

/// Directory holding the settings database.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = data_dir_override() {
        return dir;
    }
    xdg_dir(
        std::env::var_os("XDG_DATA_HOME"),
        std::env::var_os("HOME"),
        ".local/share",
    )
    .map_or_else(|| PathBuf::from("."), |base| base.join(APP_DIR_NAME))
}

/// Directory holding `md-editor.log`.
pub fn log_dir() -> PathBuf {
    if let Some(dir) = data_dir_override() {
        return dir;
    }
    xdg_dir(
        std::env::var_os("XDG_STATE_HOME"),
        std::env::var_os("HOME"),
        ".local/state",
    )
    .map_or_else(data_dir, |base| base.join(APP_DIR_NAME))
}

const APP_DIR_NAME: &str = "md-editor";

fn xdg_dir(
    var: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
    default_rel: &str,
) -> Option<PathBuf> {
    var.map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            home.map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|home| home.join(default_rel))
        })
}

#[cfg(test)]
mod tests;

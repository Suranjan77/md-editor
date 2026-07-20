use std::path::{Path, PathBuf};
use std::sync::{
    Mutex, MutexGuard,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};

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
        // WAL keeps writes fast (and would allow concurrent reads if we ever add
        // a second connection); synchronous=NORMAL is the safe, recommended
        // pairing for WAL. Best-effort — fall back silently if unsupported.
        let _ = db.pragma_update(None, "journal_mode", "WAL");
        let _ = db.pragma_update(None, "synchronous", "NORMAL");
        let _ = db.busy_timeout(Duration::from_secs(5));

        AppState {
            vault_root: Mutex::new(None),
            file_index: Mutex::new(FileIndex::new(PathBuf::new())),
            db: Mutex::new(db),
            pdf_state: Mutex::new(PdfState::new()),
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
            pdf_state: Mutex::new(PdfState::new()),
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

    /// Declare the vault the UI intends to use and invalidate older indexing
    /// work. Publishing an index uses the same lock, making the generation
    /// check and the multi-store update atomic with respect to another switch.
    pub fn begin_vault_change(&self, root: PathBuf) -> Result<u64, String> {
        let _publish = self.vault_publish_lock.lock().map_err(|e| e.to_string())?;
        let _mutation = self.vault_mutation_lock.lock().map_err(|e| e.to_string())?;
        {
            let db = self.db.lock().map_err(|e| e.to_string())?;
            db.execute("DELETE FROM file_search", [])
                .map_err(|e| format!("Failed to clear previous vault search index: {e}"))?;
        }
        *self.file_index.lock().map_err(|e| e.to_string())? = FileIndex::new(root.clone());
        let generation = self
            .vault_generation
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1);
        *self.vault_root.lock().map_err(|e| e.to_string())? = Some(root);
        self.vault_content_revision.fetch_add(1, Ordering::SeqCst);
        Ok(generation)
    }

    pub fn current_vault_generation(&self) -> u64 {
        self.vault_generation.load(Ordering::SeqCst)
    }

    pub(crate) fn current_vault_content_revision(&self) -> u64 {
        self.vault_content_revision.load(Ordering::SeqCst)
    }

    pub(crate) fn vault_mutation_guard(&self) -> Result<VaultMutationGuard<'_>, String> {
        Ok(VaultMutationGuard {
            _lock: self.vault_mutation_lock.lock().map_err(|e| e.to_string())?,
            revision: &self.vault_content_revision,
        })
    }

    pub(crate) fn vault_publish_mutation_lock(&self) -> Result<MutexGuard<'_, ()>, String> {
        self.vault_mutation_lock.lock().map_err(|e| e.to_string())
    }

    /// Stable namespace for PDF persistence belonging to the active vault.
    /// Existing databases used unscoped document IDs; `save_pdf_document`
    /// adopts those rows once when a PDF is first opened after this change.
    pub(crate) fn pdf_scope_prefix(&self) -> Result<Option<String>, String> {
        let root = {
            let guard = self.vault_root.lock().map_err(|e| e.to_string())?;
            guard.clone()
        };
        let Some(root) = root else {
            // In-memory unit tests and non-vault callers retain the legacy raw
            // key behavior. User-facing PDF operations always have a root.
            return Ok(None);
        };
        Ok(Some(Self::pdf_scope_prefix_for_root(&root)))
    }

    pub(crate) fn pdf_scope_prefix_for_root(root: &Path) -> String {
        let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let normalized = canonical.to_string_lossy().replace('\\', "/");
        let mut hasher = Sha256::new();
        hasher.update(normalized.as_bytes());
        format!("{:x}:", hasher.finalize())
    }

    pub(crate) fn pdf_storage_document_id(&self, document_id: &str) -> Result<String, String> {
        Ok(match self.pdf_scope_prefix()? {
            Some(prefix) => format!("{prefix}{document_id}"),
            None => document_id.to_string(),
        })
    }

    pub(crate) fn stored_pdf_document_id_for_vault(
        &self,
        vault_root: &Path,
        rel_path: &str,
        file_size: u64,
        modified_at: Option<i64>,
    ) -> Option<String> {
        let prefix = Self::pdf_scope_prefix_for_root(vault_root);
        let db = self.db.lock().ok()?;
        let storage_id: String = db
            .query_row(
                "SELECT document_id FROM pdf_documents
                 WHERE vault_relative_path = ?1 AND file_size = ?2
                   AND (modified_at = ?3 OR (modified_at IS NULL AND ?3 IS NULL))
                   AND substr(document_id, 1, length(?4)) = ?4
                 ORDER BY updated_at DESC LIMIT 1",
                rusqlite::params![rel_path, file_size as i64, modified_at, &prefix],
                |row| row.get(0),
            )
            .ok()?;
        storage_id.strip_prefix(&prefix).map(str::to_string)
    }

    pub fn save_pdf_document(
        &self,
        document_id: &str,
        vault_relative_path: &str,
        file_size: u64,
        modified_at: Option<i64>,
    ) -> Result<(), String> {
        let scope_prefix = self.pdf_scope_prefix()?;
        // Derive both values from the same root read. Re-reading the active
        // vault here would let a concurrent switch combine one vault's storage
        // namespace with another vault's reconciliation query.
        let storage_document_id = match scope_prefix.as_deref() {
            Some(prefix) => format!("{prefix}{document_id}"),
            None => document_id.to_string(),
        };
        let db = self.db.lock().map_err(|e| e.to_string())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;

        let existing_new: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pdf_documents WHERE document_id = ?1)",
                [&storage_document_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !existing_new {
            // Path is the durable user-facing identity. Prefer a size match,
            // but adopt the newest same-path row even after a replacement so
            // annotations are preserved instead of silently orphaned.
            let previous_id: Option<String> = if let Some(prefix) = scope_prefix.as_deref() {
                tx.query_row(
                    "SELECT document_id FROM pdf_documents
                     WHERE vault_relative_path = ?1 AND document_id != ?2
                       AND (substr(document_id, 1, length(?4)) = ?4
                            OR instr(document_id, ':') = 0)
                     ORDER BY (substr(document_id, 1, length(?4)) = ?4) DESC,
                              (file_size = ?3) DESC, updated_at DESC
                     LIMIT 1",
                    rusqlite::params![
                        vault_relative_path,
                        storage_document_id,
                        file_size as i64,
                        prefix
                    ],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?
            } else {
                tx.query_row(
                    "SELECT document_id FROM pdf_documents
                     WHERE vault_relative_path = ?1 AND document_id != ?2
                     ORDER BY (file_size = ?3) DESC, updated_at DESC
                     LIMIT 1",
                    rusqlite::params![
                        vault_relative_path,
                        storage_document_id,
                        file_size as i64
                    ],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?
            };
            if let Some(previous_id) = previous_id {
                tx.execute(
                    "UPDATE pdf_annotations SET document_id = ?1 WHERE document_id = ?2",
                    rusqlite::params![storage_document_id, previous_id],
                )
                .map_err(|e| format!("Failed to reconcile pdf annotations: {e}"))?;
                tx.execute(
                    "UPDATE pdf_references SET document_id = ?1 WHERE document_id = ?2",
                    rusqlite::params![storage_document_id, previous_id],
                )
                .map_err(|e| format!("Failed to reconcile pdf references: {e}"))?;
                tx.execute(
                    "UPDATE pdf_documents SET document_id = ?1 WHERE document_id = ?2",
                    rusqlite::params![storage_document_id, previous_id],
                )
                .map_err(|e| format!("Failed to reconcile pdf document: {e}"))?;
            }
        }

        tx.execute(
            "INSERT INTO pdf_documents (document_id, vault_relative_path, file_size, modified_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(document_id) DO UPDATE SET
                vault_relative_path = excluded.vault_relative_path,
                file_size = excluded.file_size,
                modified_at = excluded.modified_at,
                updated_at = excluded.updated_at",
            rusqlite::params![
                storage_document_id,
                vault_relative_path,
                file_size as i64,
                modified_at,
                now
            ],
        )
        .map_err(|e| format!("Failed to save pdf document: {e}"))?;

        tx.commit().map_err(|e| e.to_string())?;

        Ok(())
    }

    pub fn get_pdf_path_by_id(&self, document_id: &str) -> Result<Option<String>, String> {
        let storage_document_id = self.pdf_storage_document_id(document_id)?;
        let db = self.db.lock().map_err(|e| e.to_string())?;
        let mut stmt = db
            .prepare("SELECT vault_relative_path FROM pdf_documents WHERE document_id = ?1")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query([storage_document_id])
            .map_err(|e| e.to_string())?;
        if let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let path: String = row.get(0).map_err(|e| e.to_string())?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    pub fn save_pdf_annotation(&self, ann: &PdfAnnotation) -> Result<(), String> {
        let storage_document_id = self.pdf_storage_document_id(&ann.document_id)?;
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
                storage_document_id,
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
        let scope_prefix = self.pdf_scope_prefix()?;
        let db = self.db.lock().map_err(|e| e.to_string())?;
        let deleted = if let Some(prefix) = scope_prefix {
            db.execute(
                "DELETE FROM pdf_annotations
                 WHERE id = ?1 AND substr(document_id, 1, length(?2)) = ?2",
                rusqlite::params![id, prefix],
            )
        } else {
            db.execute("DELETE FROM pdf_annotations WHERE id = ?1", [id])
        };
        deleted
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
        let storage_document_id = self.pdf_storage_document_id(document_id).ok()?;
        let db = self.db.lock().ok()?;
        let json: String = db
            .query_row(
                "SELECT links_json FROM pdf_references WHERE document_id = ?1",
                [storage_document_id],
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
        let Ok(storage_document_id) = self.pdf_storage_document_id(document_id) else {
            return;
        };
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
            rusqlite::params![storage_document_id, json, now],
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
        let key = self.pdf_cache_key(rel_path).ok()?;
        self.get_cached_pdf_text_by_key(&key, file_size, modified_at)
    }

    pub(crate) fn get_cached_pdf_text_for_vault(
        &self,
        vault_root: &Path,
        rel_path: &str,
        file_size: u64,
        modified_at: i64,
    ) -> Option<String> {
        let key = format!("{}{}", Self::pdf_scope_prefix_for_root(vault_root), rel_path);
        self.get_cached_pdf_text_by_key(&key, file_size, modified_at)
    }

    fn get_cached_pdf_text_by_key(
        &self,
        key: &str,
        file_size: u64,
        modified_at: i64,
    ) -> Option<String> {
        let db = self.db.lock().ok()?;
        db.query_row(
            "SELECT content FROM pdf_text_cache
             WHERE path = ?1 AND file_size = ?2 AND modified_at = ?3",
            rusqlite::params![key, file_size as i64, modified_at],
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
        let Ok(key) = self.pdf_cache_key(rel_path) else {
            return;
        };
        self.put_cached_pdf_text_by_key(&key, file_size, modified_at, content);
    }

    pub(crate) fn put_cached_pdf_text_for_vault(
        &self,
        vault_root: &Path,
        rel_path: &str,
        file_size: u64,
        modified_at: i64,
        content: &str,
    ) {
        let key = format!("{}{}", Self::pdf_scope_prefix_for_root(vault_root), rel_path);
        self.put_cached_pdf_text_by_key(&key, file_size, modified_at, content);
    }

    fn pdf_cache_key(&self, rel_path: &str) -> Result<String, String> {
        Ok(match self.pdf_scope_prefix()? {
            Some(prefix) => format!("{prefix}{rel_path}"),
            None => rel_path.to_string(),
        })
    }

    fn put_cached_pdf_text_by_key(
        &self,
        key: &str,
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
            rusqlite::params![key, file_size as i64, modified_at, content],
        ) {
            eprintln!("Failed to cache PDF text for {key}: {e}");
        }
    }

    pub fn get_pdf_annotations(
        &self,
        document_id: &str,
        page_index: Option<u16>,
    ) -> Result<Vec<PdfAnnotation>, String> {
        let storage_document_id = self.pdf_storage_document_id(document_id)?;
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
            stmt.query(rusqlite::params![storage_document_id, page as i32])
                .map_err(|e| e.to_string())?
        } else {
            stmt.query(rusqlite::params![storage_document_id])
                .map_err(|e| e.to_string())?
        };

        let mut annotations = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let id: String = row.get(0).map_err(|e| e.to_string())?;
            let _stored_doc_id: String = row.get(1).map_err(|e| e.to_string())?;
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
                document_id: document_id.to_string(),
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
                eprintln!(
                    "Failed to preserve settings database sidecar {}: {error}",
                    original_sidecar.display()
                );
            }
        }
    }
    Ok(backup)
}

fn settings_db_path() -> PathBuf {
    let mut dir = data_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("Failed to create data directory {}: {err}", dir.display());
        return PathBuf::from(DB_FILE_NAME);
    }
    dir.push(DB_FILE_NAME);

    // One-time migration: an interim version stored the database in the per-user
    // platform data directory. If the portable location (beside the executable)
    // has no database yet but that one does, copy it back so users keep their
    // settings and study history.
    if !dir.exists() {
        if let Some(other) = platform_data_db_path() {
            if other != dir {
                migrate_legacy_db(&other, &dir);
            }
        }
    }

    dir
}

/// Read the last window geometry without constructing PDF services. This is
/// used before Iced creates the first window.
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

/// Database location used by the interim version that stored it in the per-user
/// platform data directory.
fn platform_data_db_path() -> Option<PathBuf> {
    Some(platform_data_home()?.join("md-editor").join(DB_FILE_NAME))
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

/// Directory holding the settings database.
///
/// Portability is a core promise, so the database lives **beside the
/// executable** by default and the whole app travels as one folder. Only when
/// that directory is not writable — e.g. a read-only system install — does it
/// fall back to the per-user platform data directory (`%APPDATA%`,
/// `~/Library/Application Support`, or `$XDG_DATA_HOME`/`~/.local/share`), and
/// finally the current directory.
fn data_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if is_writable_dir(dir) {
                return dir.to_path_buf();
            }
            if let Some(base) = platform_data_home() {
                return base.join("md-editor");
            }
            return dir.to_path_buf();
        }
    }
    if let Some(base) = platform_data_home() {
        return base.join("md-editor");
    }
    PathBuf::from(".")
}

/// Best-effort writability probe: create and delete a temp file in `dir`. Used
/// to decide whether the portable, exe-adjacent location can hold the database
/// before falling back to a per-user directory.
fn is_writable_dir(dir: &Path) -> bool {
    let probe = dir.join(".md_editor_write_test");
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
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
    fn vault_mutation_guard_advances_content_revision() {
        let state = AppState::new_in_memory();
        let before = state.current_vault_content_revision();
        {
            let _mutation = state.vault_mutation_guard().unwrap();
            assert_eq!(state.current_vault_content_revision(), before);
        }
        assert_eq!(
            state.current_vault_content_revision(),
            before.wrapping_add(1)
        );
    }

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
    fn pdf_text_cache_is_isolated_by_vault() {
        let base = std::env::temp_dir().join(format!("md_pdf_cache_{}", uuid::Uuid::new_v4()));
        let first = base.join("first");
        let second = base.join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let state = AppState::new_in_memory();

        state.put_cached_pdf_text_for_vault(&first, "paper.pdf", 10, 1, "first text");
        assert!(
            state
                .get_cached_pdf_text_for_vault(&second, "paper.pdf", 10, 1)
                .is_none()
        );
        state.put_cached_pdf_text_for_vault(&second, "paper.pdf", 10, 1, "second text");
        assert_eq!(
            state
                .get_cached_pdf_text_for_vault(&first, "paper.pdf", 10, 1)
                .as_deref(),
            Some("first text")
        );

        drop(state);
        let _ = std::fs::remove_dir_all(&base);
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

    #[test]
    fn corrupt_database_is_preserved_and_recreated() {
        let base = std::env::temp_dir().join(format!("md_editor_corrupt_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let db_path = base.join(DB_FILE_NAME);
        std::fs::write(&db_path, b"this is not sqlite").unwrap();

        let state = AppState::new_with_db_path(&db_path);

        assert!(db_path.exists());
        assert!(state
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM settings", [], |row| row.get::<_, i64>(0))
            .is_ok());
        let notice = state.take_startup_notice().expect("recovery notice");
        assert!(notice.contains("has been reset"));
        let backup = std::fs::read_dir(&base)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("md_editor_settings.sqlite.corrupt-") && !name.ends_with("-wal"))
            })
            .expect("quarantined database");
        assert_eq!(std::fs::read(&backup).unwrap(), b"this is not sqlite");
        assert!(state.take_startup_notice().is_none());

        drop(state);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn quarantining_database_preserves_sidecars() {
        let base = std::env::temp_dir().join(format!("md_editor_sidecars_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let db_path = base.join(DB_FILE_NAME);
        std::fs::write(&db_path, b"database").unwrap();
        std::fs::write(sidecar(&db_path, "-wal"), b"wal").unwrap();
        std::fs::write(sidecar(&db_path, "-shm"), b"shm").unwrap();

        let backup = quarantine_database(&db_path).unwrap();

        assert_eq!(std::fs::read(&backup).unwrap(), b"database");
        assert_eq!(std::fs::read(sidecar(&backup, "-wal")).unwrap(), b"wal");
        assert_eq!(std::fs::read(sidecar(&backup, "-shm")).unwrap(), b"shm");
        assert!(!db_path.exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn pdf_document_reconciliation_rekeys_existing_annotations() {
        use sha2::{Digest, Sha256};

        let state = AppState::new_in_memory();
        let base = std::env::temp_dir().join(format!("md_editor_pdf_id_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join("paper.pdf");
        let bytes = b"stable pdf bytes";
        std::fs::write(&path, bytes).unwrap();
        let (_, size, mtime) = crate::pdf::compute_provisional_id(&path).unwrap();

        let mut old_hasher = Sha256::new();
        old_hasher.update(bytes);
        old_hasher.update(size.to_be_bytes());
        old_hasher.update(mtime.unwrap_or_default().to_be_bytes());
        let old_id = format!("{:x}", old_hasher.finalize());
        state
            .save_pdf_document(&old_id, "paper.pdf", size, mtime)
            .unwrap();
        state
            .save_pdf_annotation(&PdfAnnotation {
                id: "annotation-1".to_string(),
                document_id: old_id.clone(),
                page_index: 0,
                kind: PdfAnnotationKind::Highlight,
                color: PdfAnnotationColor::Yellow,
                selected_text: "stable".to_string(),
                ranges: vec![],
                rects: vec![],
                note: None,
                linked_note_path: None,
                markdown_anchor: None,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();

        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        let bumped = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        file.set_times(std::fs::FileTimes::new().set_modified(bumped)).unwrap();
        let (new_id, new_size, new_mtime) = crate::pdf::compute_provisional_id(&path).unwrap();
        assert_ne!(old_id, new_id);

        state
            .save_pdf_document(&new_id, "paper.pdf", new_size, new_mtime)
            .unwrap();

        assert!(state.get_pdf_annotations(&old_id, None).unwrap().is_empty());
        let annotations = state.get_pdf_annotations(&new_id, None).unwrap();
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].id, "annotation-1");
        assert_eq!(annotations[0].document_id, new_id);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn pdf_document_reconciliation_prefers_preservation_on_same_path_size_change() {
        let state = AppState::new_in_memory();
        state
            .save_pdf_document("old", "paper.pdf", 10, Some(1))
            .unwrap();
        state
            .save_pdf_annotation(&PdfAnnotation {
                id: "annotation-2".to_string(),
                document_id: "old".to_string(),
                page_index: 0,
                kind: PdfAnnotationKind::Note,
                color: PdfAnnotationColor::Blue,
                selected_text: String::new(),
                ranges: vec![],
                rects: vec![],
                note: Some("keep me".to_string()),
                linked_note_path: None,
                markdown_anchor: None,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();

        state
            .save_pdf_document("new", "paper.pdf", 20, Some(2))
            .unwrap();

        assert_eq!(state.get_pdf_annotations("new", None).unwrap().len(), 1);
    }

    #[test]
    fn pdf_annotations_are_isolated_between_vaults_with_identical_document_ids() {
        let base = std::env::temp_dir().join(format!("md_pdf_scopes_{}", uuid::Uuid::new_v4()));
        let first_root = base.join("first");
        let second_root = base.join("second");
        std::fs::create_dir_all(&first_root).unwrap();
        std::fs::create_dir_all(&second_root).unwrap();
        let state = AppState::new_in_memory();

        let annotation = |id: &str, text: &str| PdfAnnotation {
            id: id.to_string(),
            document_id: "same-content-id".to_string(),
            page_index: 0,
            kind: PdfAnnotationKind::Highlight,
            color: PdfAnnotationColor::Yellow,
            selected_text: text.to_string(),
            ranges: vec![],
            rects: vec![],
            note: None,
            linked_note_path: Some(format!("{text}.md")),
            markdown_anchor: None,
            created_at: 1,
            updated_at: 1,
        };

        *state.vault_root.lock().unwrap() = Some(first_root);
        state
            .save_pdf_document("same-content-id", "paper.pdf", 10, Some(1))
            .unwrap();
        state
            .save_pdf_annotation(&annotation("first-annotation", "first-note"))
            .unwrap();

        *state.vault_root.lock().unwrap() = Some(second_root);
        state
            .save_pdf_document("same-content-id", "paper.pdf", 10, Some(1))
            .unwrap();
        assert!(
            state
                .get_pdf_annotations("same-content-id", None)
                .unwrap()
                .is_empty()
        );
        state
            .save_pdf_annotation(&annotation("second-annotation", "second-note"))
            .unwrap();
        assert_eq!(
            state
                .get_pdf_annotations("same-content-id", None)
                .unwrap()[0]
                .id,
            "second-annotation"
        );

        *state.vault_root.lock().unwrap() = Some(base.join("first"));
        let first_annotations = state
            .get_pdf_annotations("same-content-id", None)
            .unwrap();
        assert_eq!(first_annotations.len(), 1);
        assert_eq!(first_annotations[0].id, "first-annotation");

        let stored_documents: i64 = state
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM pdf_documents", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored_documents, 2);

        drop(state);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn opening_pdf_adopts_legacy_unscoped_annotations_into_current_vault() {
        let base = std::env::temp_dir().join(format!("md_pdf_legacy_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let state = AppState::new_in_memory();
        {
            let db = state.db.lock().unwrap();
            db.execute(
                "INSERT INTO pdf_documents (
                     document_id, vault_relative_path, file_size, modified_at,
                     created_at, updated_at
                 ) VALUES ('legacy-id', 'paper.pdf', 10, 1, 1, 1)",
                [],
            )
            .unwrap();
            db.execute(
                "INSERT INTO pdf_annotations (
                     id, document_id, page_index, kind, color, selected_text,
                     ranges_json, rects_json, note, linked_note_path,
                     markdown_anchor, created_at, updated_at
                 ) VALUES ('legacy-annotation', 'legacy-id', 0, 'Highlight',
                           'Yellow', 'legacy', '[]', '[]', NULL, 'note.md',
                           NULL, 1, 1)",
                [],
            )
            .unwrap();
        }

        *state.vault_root.lock().unwrap() = Some(base.clone());
        state
            .save_pdf_document("legacy-id", "paper.pdf", 10, Some(1))
            .unwrap();

        let annotations = state.get_pdf_annotations("legacy-id", None).unwrap();
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].id, "legacy-annotation");
        assert_eq!(annotations[0].document_id, "legacy-id");
        let unscoped_documents: i64 = state
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM pdf_documents WHERE document_id = 'legacy-id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unscoped_documents, 0);

        drop(state);
        let _ = std::fs::remove_dir_all(&base);
    }
}

use rusqlite::Connection;
use std::fmt;
use std::ops::Deref;
use std::path::{Path, PathBuf};

pub(crate) mod pdf_repository;
pub(crate) mod search_repository;
pub(crate) mod settings_repository;
pub(crate) mod tracker_repository;

#[derive(Debug)]
pub struct DatabaseError {
    operation: &'static str,
    path: Option<PathBuf>,
    detail: String,
}

impl DatabaseError {
    fn open(path: Option<&Path>, error: rusqlite::Error) -> Self {
        Self {
            operation: "open database",
            path: path.map(Path::to_path_buf),
            detail: error.to_string(),
        }
    }

    fn initialize(path: Option<&Path>, detail: String) -> Self {
        Self {
            operation: "initialize database",
            path: path.map(Path::to_path_buf),
            detail,
        }
    }
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.path {
            write!(
                formatter,
                "{} at {}: {}",
                self.operation,
                path.display(),
                self.detail
            )
        } else {
            write!(formatter, "{}: {}", self.operation, self.detail)
        }
    }
}

impl std::error::Error for DatabaseError {}

pub(crate) struct Database {
    connection: Connection,
}

impl Database {
    pub(crate) fn open(path: &Path) -> Result<Self, DatabaseError> {
        let connection =
            Connection::open(path).map_err(|error| DatabaseError::open(Some(path), error))?;
        initialize(&connection).map_err(|detail| DatabaseError::initialize(Some(path), detail))?;
        Ok(Self { connection })
    }

    pub(crate) fn open_in_memory() -> Result<Self, DatabaseError> {
        let connection =
            Connection::open_in_memory().map_err(|error| DatabaseError::open(None, error))?;
        initialize(&connection).map_err(|detail| DatabaseError::initialize(None, detail))?;
        Ok(Self { connection })
    }
}

impl Deref for Database {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}

const CREATE_SETTINGS: &str = "CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
)";

const CREATE_TRACKER_SESSIONS: &str = "CREATE TABLE IF NOT EXISTS tracker_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    date TEXT NOT NULL,
    hours REAL NOT NULL,
    activity_type TEXT NOT NULL,
    phase TEXT NOT NULL,
    notes TEXT
)";

const CREATE_TRACKER_ACTIVITY: &str = "CREATE TABLE IF NOT EXISTS tracker_activity (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    type TEXT NOT NULL,
    text TEXT NOT NULL,
    time TEXT NOT NULL
)";

const CREATE_TRACKER_KV: &str = "CREATE TABLE IF NOT EXISTS tracker_kv (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
)";

const CREATE_FILE_SEARCH: &str = "CREATE VIRTUAL TABLE IF NOT EXISTS file_search USING fts5(
    path,
    content
)";

const CREATE_PDF_TEXT_SEARCH: &str =
    "CREATE VIRTUAL TABLE IF NOT EXISTS pdf_text_search USING fts5(
    path,
    page_index UNINDEXED,
    content
)";

const CREATE_PDF_DOCUMENTS: &str = "CREATE TABLE IF NOT EXISTS pdf_documents (
    document_id TEXT PRIMARY KEY,
    vault_relative_path TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    modified_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
)";

const CREATE_PDF_ANNOTATIONS: &str = "CREATE TABLE IF NOT EXISTS pdf_annotations (
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
    tags_json TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL DEFAULT 'Unresolved',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
)";

const CREATE_PDF_DOCUMENT_PATH_INDEX: &str =
    "CREATE INDEX IF NOT EXISTS pdf_documents_vault_relative_path
     ON pdf_documents(vault_relative_path)";

const CREATE_PDF_ANNOTATION_PAGE_INDEX: &str =
    "CREATE INDEX IF NOT EXISTS pdf_annotations_document_page
     ON pdf_annotations(document_id, page_index)";

const CREATE_PDF_ANNOTATION_DOCUMENT_LINK_INDEX: &str =
    "CREATE INDEX IF NOT EXISTS pdf_annotations_document_linked_note
     ON pdf_annotations(document_id, linked_note_path)
     WHERE linked_note_path IS NOT NULL AND linked_note_path != ''";

const CREATE_PDF_ANNOTATION_LINK_INDEX: &str =
    "CREATE INDEX IF NOT EXISTS pdf_annotations_linked_note
     ON pdf_annotations(linked_note_path)
     WHERE linked_note_path IS NOT NULL AND linked_note_path != ''";

pub(crate) fn initialize(db: &Connection) -> Result<(), String> {
    execute(db, CREATE_SETTINGS, "settings table")?;
    execute(db, CREATE_TRACKER_SESSIONS, "tracker_sessions table")?;
    execute(db, CREATE_TRACKER_ACTIVITY, "tracker_activity table")?;
    execute(db, CREATE_TRACKER_KV, "tracker_kv table")?;
    execute(db, CREATE_FILE_SEARCH, "file_search FTS table")?;
    execute(db, CREATE_PDF_TEXT_SEARCH, "pdf_text_search FTS table")?;
    execute(db, CREATE_PDF_DOCUMENTS, "pdf_documents table")?;
    execute(
        db,
        CREATE_PDF_DOCUMENT_PATH_INDEX,
        "pdf document path index",
    )?;
    execute(db, CREATE_PDF_ANNOTATIONS, "pdf_annotations table")?;

    migrate(db)?;

    execute(
        db,
        CREATE_PDF_ANNOTATION_PAGE_INDEX,
        "pdf annotation page index",
    )?;
    execute(
        db,
        CREATE_PDF_ANNOTATION_DOCUMENT_LINK_INDEX,
        "pdf annotation document linked-note index",
    )?;
    execute(
        db,
        CREATE_PDF_ANNOTATION_LINK_INDEX,
        "pdf annotation note backlink index",
    )?;

    Ok(())
}

fn execute(db: &Connection, sql: &str, object: &str) -> Result<(), String> {
    db.execute(sql, [])
        .map(|_| ())
        .map_err(|err| format!("Failed to initialize {object}: {err}"))
}

fn migrate(db: &Connection) -> Result<(), String> {
    let mut has_tags_json = false;
    let mut has_status = false;

    let mut stmt = db
        .prepare("PRAGMA table_info(pdf_annotations)")
        .map_err(|err| format!("Failed to prepare pragma table_info: {err}"))?;
    let mut rows = stmt
        .query([])
        .map_err(|err| format!("Failed to query pragma table_info: {err}"))?;
    while let Some(row) = rows.next().map_err(|err| err.to_string())? {
        let name: String = row.get(1).map_err(|err| err.to_string())?;
        if name == "tags_json" {
            has_tags_json = true;
        } else if name == "status" {
            has_status = true;
        }
    }

    if !has_tags_json {
        db.execute(
            "ALTER TABLE pdf_annotations ADD COLUMN tags_json TEXT NOT NULL DEFAULT '[]'",
            [],
        )
        .map_err(|err| format!("Failed to add tags_json column: {err}"))?;
    }

    if !has_status {
        db.execute(
            "ALTER TABLE pdf_annotations ADD COLUMN status TEXT NOT NULL DEFAULT 'Unresolved'",
            [],
        )
        .map_err(|err| format!("Failed to add status column: {err}"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::initialize;

    const EXPECTED_TABLES: &[&str] = &[
        "file_search",
        "pdf_annotations",
        "pdf_documents",
        "pdf_text_search",
        "settings",
        "tracker_activity",
        "tracker_kv",
        "tracker_sessions",
    ];

    const EXPECTED_INDEXES: &[&str] = &[
        "pdf_annotations_document_linked_note",
        "pdf_annotations_document_page",
        "pdf_annotations_linked_note",
        "pdf_documents_vault_relative_path",
    ];

    #[test]
    fn fresh_database_has_expected_schema() {
        let db = Connection::open_in_memory().expect("memory database should open");

        initialize(&db).expect("fresh database should initialize");

        assert_eq!(schema_objects(&db, "table"), EXPECTED_TABLES);
        assert_eq!(schema_objects(&db, "index"), EXPECTED_INDEXES);
        assert_eq!(
            table_columns(&db, "pdf_annotations"),
            [
                "id",
                "document_id",
                "page_index",
                "kind",
                "color",
                "selected_text",
                "ranges_json",
                "rects_json",
                "note",
                "linked_note_path",
                "markdown_anchor",
                "tags_json",
                "status",
                "created_at",
                "updated_at",
            ]
        );
    }

    #[test]
    fn legacy_pdf_annotations_migrate_without_losing_data() {
        let db = Connection::open_in_memory().expect("memory database should open");
        db.execute(
            "CREATE TABLE pdf_annotations (
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
            )",
            [],
        )
        .expect("legacy table should be created");
        db.execute(
            "INSERT INTO pdf_annotations (
                id, document_id, page_index, kind, color, selected_text,
                ranges_json, rects_json, note, linked_note_path, markdown_anchor,
                created_at, updated_at
            ) VALUES (
                'ann-1', 'doc-1', 0, 'Highlight', 'Yellow', 'text',
                '[]', '[]', NULL, NULL, NULL, 10, 11
            )",
            [],
        )
        .expect("legacy row should be inserted");

        initialize(&db).expect("legacy database should migrate");

        let migrated: (String, String, String, i64) = db
            .query_row(
                "SELECT id, tags_json, status, updated_at
                 FROM pdf_annotations WHERE id = 'ann-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("migrated row should remain readable");
        assert_eq!(
            migrated,
            (
                "ann-1".to_string(),
                "[]".to_string(),
                "Unresolved".to_string(),
                11
            )
        );
        assert_eq!(schema_objects(&db, "index"), EXPECTED_INDEXES);
    }

    #[test]
    fn initialization_is_idempotent() {
        let db = Connection::open_in_memory().expect("memory database should open");

        initialize(&db).expect("first initialization should succeed");
        initialize(&db).expect("second initialization should succeed");

        assert_eq!(schema_objects(&db, "table"), EXPECTED_TABLES);
        assert_eq!(schema_objects(&db, "index"), EXPECTED_INDEXES);
    }

    fn schema_objects(db: &Connection, object_type: &str) -> Vec<String> {
        let mut stmt = db
            .prepare(
                "SELECT name FROM sqlite_schema
                 WHERE type = ?1
                   AND name NOT LIKE 'sqlite_%'
                   AND name NOT LIKE '%_config'
                   AND name NOT LIKE '%_content'
                   AND name NOT LIKE '%_data'
                   AND name NOT LIKE '%_docsize'
                   AND name NOT LIKE '%_idx'
                 ORDER BY name",
            )
            .expect("schema query should prepare");
        stmt.query_map([object_type], |row| row.get(0))
            .expect("schema query should run")
            .collect::<Result<Vec<_>, _>>()
            .expect("schema names should decode")
    }

    fn table_columns(db: &Connection, table: &str) -> Vec<String> {
        let mut stmt = db
            .prepare(&format!("PRAGMA table_info({table})"))
            .expect("table info query should prepare");
        stmt.query_map([], |row| row.get(1))
            .expect("table info query should run")
            .collect::<Result<Vec<_>, _>>()
            .expect("column names should decode")
    }
}

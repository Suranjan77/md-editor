//! PDF annotations v2 (plan §3.3): highlights/notes keyed by the document's
//! **SHA-256 content hash**, never its path — moving or renaming a file
//! cannot orphan its annotations because the key never mentions where the
//! file lives. The store remembers each hash's last-seen path purely as a
//! human-facing hint (orphan reports, export headers).
//!
//! Schema changes are **numbered migrations** applied in order inside
//! transactions; the ladder is shared-sidecar-friendly (a `migrations` table
//! keyed by component) so the FTS index can adopt it later without clashing.
//!
//! Wire formats: JSON export/import (`serde`) for round-tripping between
//! vaults, and a Markdown summary for humans ("everything I marked in this
//! paper" as a note).

use std::io::Read;
use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::VaultError;
use crate::migrations::{self, now};

/// One highlighted region on a page, in page space (PDF points, origin
/// top-left). A text highlight spanning lines is several quads in one
/// annotation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quad {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// A stored annotation. `doc_hash` is the SHA-256 (lowercase hex) of the
/// PDF's bytes — the identity that survives rename/move.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub id: i64,
    pub doc_hash: String,
    /// 0-based page index.
    pub page: u32,
    pub quads: Vec<Quad>,
    /// `#rrggbb`.
    pub color: String,
    pub note: String,
    /// Vault-relative path of a linked markdown note, if any.
    pub linked_note: Option<String>,
    /// Unix seconds.
    pub created_at: i64,
    pub modified_at: i64,
}

/// Input for [`AnnotationStore::add`] — the store assigns id and timestamps.
#[derive(Debug, Clone)]
pub struct NewAnnotation {
    pub doc_hash: String,
    pub page: u32,
    pub quads: Vec<Quad>,
    pub color: String,
    pub note: String,
    pub linked_note: Option<String>,
}

/// A document the store has seen, for orphan reports: the hash is the truth,
/// the path is the last place that content was observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownDocument {
    pub doc_hash: String,
    pub last_path: String,
    pub annotation_count: usize,
}

// ---------------------------------------------------------------- hashing --

/// SHA-256 of a file's content, lowercase hex — the annotation key. Streams
/// in 64 KiB chunks so a 500-page PDF doesn't need a whole-file buffer.
pub fn document_hash(path: &Path) -> Result<String, VaultError> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).map_err(|e| VaultError::io(path, e))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| VaultError::io(path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

// ------------------------------------------------------------- migrations --

/// Numbered, transactional, append-only. Editing a shipped entry is a bug —
/// add a new one (`migration_ladder_is_append_only` pins the shipped
/// prefix's fingerprint).
const COMPONENT: &str = "annotations";
const MIGRATIONS: &[&str] = &[
    // 1: the annotation table itself, hash-keyed.
    "CREATE TABLE annotations (
        id          INTEGER PRIMARY KEY,
        doc_hash    TEXT NOT NULL,
        page        INTEGER NOT NULL,
        quads       TEXT NOT NULL,
        color       TEXT NOT NULL,
        note        TEXT NOT NULL DEFAULT '',
        linked_note TEXT,
        created_at  INTEGER NOT NULL,
        modified_at INTEGER NOT NULL
    ) STRICT;
    CREATE INDEX annotations_by_doc ON annotations(doc_hash, page);",
    // 2: last-seen path per document hash (orphan reports, export headers).
    "CREATE TABLE annotation_documents (
        doc_hash  TEXT PRIMARY KEY,
        last_path TEXT NOT NULL,
        seen_at   INTEGER NOT NULL
    ) STRICT;",
];

fn migrate(conn: &mut Connection) -> Result<(), VaultError> {
    migrations::run(conn, COMPONENT, MIGRATIONS)
}

// ------------------------------------------------------------------ store --

pub struct AnnotationStore {
    conn: Connection,
}

impl AnnotationStore {
    /// Open (or create) the store at `db_path` — typically the vault's
    /// sidecar database, shared with the FTS index (disjoint tables).
    pub fn open(db_path: &Path) -> Result<AnnotationStore, VaultError> {
        let mut conn = Connection::open(db_path)?;
        migrate(&mut conn)?;
        Ok(AnnotationStore { conn })
    }

    pub fn open_in_memory() -> Result<AnnotationStore, VaultError> {
        let mut conn = Connection::open_in_memory()?;
        migrate(&mut conn)?;
        Ok(AnnotationStore { conn })
    }

    /// Highest applied migration for the annotations component.
    pub fn schema_version(&self) -> Result<u32, VaultError> {
        migrations::version(&self.conn, COMPONENT)
    }

    pub fn add(&mut self, new: NewAnnotation) -> Result<i64, VaultError> {
        let t = now();
        self.conn.execute(
            "INSERT INTO annotations
                 (doc_hash, page, quads, color, note, linked_note, created_at, modified_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                new.doc_hash,
                new.page,
                serde_json::to_string(&new.quads)?,
                new.color,
                new.note,
                new.linked_note,
                t,
                t
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// All annotations of a document, page order then creation order.
    pub fn annotations_for(&self, doc_hash: &str) -> Result<Vec<Annotation>, VaultError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, doc_hash, page, quads, color, note, linked_note,
                    created_at, modified_at
             FROM annotations WHERE doc_hash = ?1 ORDER BY page, id",
        )?;
        let rows = stmt.query_map([doc_hash], row_to_parts)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(parts_to_annotation(row?)?);
        }
        Ok(out)
    }

    pub fn update_note(&mut self, id: i64, note: &str) -> Result<(), VaultError> {
        self.touch(id, "note", note)
    }

    pub fn set_color(&mut self, id: i64, color: &str) -> Result<(), VaultError> {
        self.touch(id, "color", color)
    }

    /// Record the vault-relative path of the annotation's linked note.
    pub fn set_linked_note(&mut self, id: i64, rel_path: &str) -> Result<(), VaultError> {
        self.touch(id, "linked_note", rel_path)
    }

    fn touch(&mut self, id: i64, column: &str, value: &str) -> Result<(), VaultError> {
        // `column` is a compile-time constant from the callers above,
        // never user input.
        let sql = format!("UPDATE annotations SET {column} = ?1, modified_at = ?2 WHERE id = ?3");
        let n = self
            .conn
            .execute(&sql, rusqlite::params![value, now(), id])?;
        if n == 0 {
            return Err(VaultError::Database(rusqlite::Error::QueryReturnedNoRows));
        }
        Ok(())
    }

    pub fn remove(&mut self, id: i64) -> Result<(), VaultError> {
        self.conn
            .execute("DELETE FROM annotations WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Record where a hash's content was last seen. Call on every PDF open;
    /// it is what keeps orphan reports and export headers truthful after the
    /// file moves (the annotations themselves never needed it).
    pub fn record_document(&mut self, doc_hash: &str, rel_path: &str) -> Result<(), VaultError> {
        self.conn.execute(
            "INSERT INTO annotation_documents (doc_hash, last_path, seen_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(doc_hash) DO UPDATE SET last_path = ?2, seen_at = ?3",
            rusqlite::params![doc_hash, rel_path, now()],
        )?;
        Ok(())
    }

    pub fn last_path(&self, doc_hash: &str) -> Result<Option<String>, VaultError> {
        let mut stmt = self
            .conn
            .prepare("SELECT last_path FROM annotation_documents WHERE doc_hash = ?1")?;
        let mut rows = stmt.query([doc_hash])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// Every document that has annotations, with its last-seen path — the
    /// input for an orphan report (caller checks which paths still exist).
    pub fn known_documents(&self) -> Result<Vec<KnownDocument>, VaultError> {
        let mut stmt = self.conn.prepare(
            "SELECT a.doc_hash, COALESCE(d.last_path, ''), COUNT(*)
             FROM annotations a
             LEFT JOIN annotation_documents d ON d.doc_hash = a.doc_hash
             GROUP BY a.doc_hash ORDER BY a.doc_hash",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(KnownDocument {
                doc_hash: r.get(0)?,
                last_path: r.get(1)?,
                annotation_count: r.get::<_, i64>(2)? as usize,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    // -------------------------------------------------------- wire formats --

    /// JSON export of one document's annotations (schema-versioned).
    pub fn export_json(&self, doc_hash: &str) -> Result<String, VaultError> {
        let file = ExportFile {
            version: EXPORT_VERSION,
            doc_hash: doc_hash.to_string(),
            path: self.last_path(doc_hash)?,
            annotations: self
                .annotations_for(doc_hash)?
                .into_iter()
                .map(|a| ExportAnnotation {
                    page: a.page,
                    quads: a.quads,
                    color: a.color,
                    note: a.note,
                    linked_note: a.linked_note,
                    created_at: a.created_at,
                    modified_at: a.modified_at,
                })
                .collect(),
        };
        Ok(serde_json::to_string_pretty(&file)?)
    }

    /// Import a JSON export. Annotations get fresh ids; the export's
    /// `doc_hash` keys them, so importing into another vault that holds the
    /// same PDF (same bytes ⇒ same hash) reattaches them to it. Returns the
    /// number imported.
    pub fn import_json(&mut self, json: &str) -> Result<usize, VaultError> {
        let file: ExportFile = serde_json::from_str(json)?;
        let tx = self.conn.transaction()?;
        for a in &file.annotations {
            tx.execute(
                "INSERT INTO annotations
                     (doc_hash, page, quads, color, note, linked_note, created_at, modified_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    file.doc_hash,
                    a.page,
                    serde_json::to_string(&a.quads)?,
                    a.color,
                    a.note,
                    a.linked_note,
                    a.created_at,
                    a.modified_at
                ],
            )?;
        }
        if let Some(path) = &file.path {
            tx.execute(
                "INSERT INTO annotation_documents (doc_hash, last_path, seen_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(doc_hash) DO UPDATE SET last_path = ?2, seen_at = ?3",
                rusqlite::params![file.doc_hash, path, now()],
            )?;
        }
        tx.commit()?;
        Ok(file.annotations.len())
    }

    /// Human-readable Markdown summary of one document's annotations,
    /// grouped by page — suitable for pasting into a note.
    pub fn export_markdown(&self, doc_hash: &str) -> Result<String, VaultError> {
        let title = self
            .last_path(doc_hash)?
            .unwrap_or_else(|| short_hash(doc_hash));
        let annotations = self.annotations_for(doc_hash)?;
        let mut out = format!("# Annotations — {title}\n");
        let mut current_page = None;
        for a in &annotations {
            if current_page != Some(a.page) {
                current_page = Some(a.page);
                out.push_str(&format!("\n## Page {}\n", a.page + 1));
            }
            out.push_str(&format!("- [{}]", a.color));
            if !a.note.is_empty() {
                out.push(' ');
                out.push_str(&a.note);
            }
            if let Some(linked) = &a.linked_note {
                out.push_str(&format!(" → [[{linked}]]"));
            }
            out.push('\n');
        }
        if annotations.is_empty() {
            out.push_str("\n*(no annotations)*\n");
        }
        Ok(out)
    }
}

const EXPORT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct ExportFile {
    version: u32,
    doc_hash: String,
    /// Last-seen path, as a hint for the importing side.
    path: Option<String>,
    annotations: Vec<ExportAnnotation>,
}

#[derive(Serialize, Deserialize)]
struct ExportAnnotation {
    page: u32,
    quads: Vec<Quad>,
    color: String,
    note: String,
    linked_note: Option<String>,
    created_at: i64,
    modified_at: i64,
}

type AnnotationParts = (
    i64,
    String,
    u32,
    String,
    String,
    String,
    Option<String>,
    i64,
    i64,
);

fn row_to_parts(r: &rusqlite::Row<'_>) -> rusqlite::Result<AnnotationParts> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
    ))
}

fn parts_to_annotation(parts: AnnotationParts) -> Result<Annotation, VaultError> {
    let (id, doc_hash, page, quads, color, note, linked_note, created_at, modified_at) = parts;
    Ok(Annotation {
        id,
        doc_hash,
        page,
        quads: serde_json::from_str(&quads)?,
        color,
        note,
        linked_note,
        created_at,
        modified_at,
    })
}

fn short_hash(hash: &str) -> String {
    format!("sha256:{}…", &hash[..hash.len().min(12)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok<T>(r: Result<T, VaultError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("{e}"),
        }
    }

    fn store() -> AnnotationStore {
        ok(AnnotationStore::open_in_memory())
    }

    fn highlight(doc_hash: &str, page: u32, note: &str) -> NewAnnotation {
        NewAnnotation {
            doc_hash: doc_hash.to_string(),
            page,
            quads: vec![Quad {
                x0: 71.5,
                y0: 120.25,
                x1: 410.0,
                y1: 134.75,
            }],
            color: "#ffd866".to_string(),
            note: note.to_string(),
            linked_note: None,
        }
    }

    #[test]
    fn migrations_apply_once_and_version_is_reported() {
        let store = store();
        assert_eq!(ok(store.schema_version()), MIGRATIONS.len() as u32);
    }

    #[test]
    fn migrations_are_idempotent_across_reopen() {
        let dir = match tempfile::tempdir() {
            Ok(d) => d,
            Err(e) => panic!("tempdir: {e}"),
        };
        let db = dir.path().join("sidecar.db");
        let mut first = ok(AnnotationStore::open(&db));
        ok(first.add(highlight("abc123", 0, "kept across reopen")));
        drop(first);

        // Reopening runs the ladder again — already-applied steps must be
        // skipped and the data untouched.
        let second = ok(AnnotationStore::open(&db));
        assert_eq!(ok(second.schema_version()), MIGRATIONS.len() as u32);
        let anns = ok(second.annotations_for("abc123"));
        assert_eq!(anns.len(), 1);
        assert_eq!(anns[0].note, "kept across reopen");
    }

    #[test]
    fn migration_ladder_is_append_only() {
        // Editing a shipped migration corrupts every existing sidecar (the
        // ladder records versions, not content). If this fails you changed
        // migration 1 or 2 — write migration N+1 instead.
        let fingerprint: usize = MIGRATIONS
            .iter()
            .take(2)
            .map(|m| m.split_whitespace().collect::<String>().len())
            .sum();
        assert_eq!(fingerprint, 378, "shipped migrations must not change");
    }

    #[test]
    fn add_fetch_round_trip_preserves_quads_and_orders_by_page() {
        let mut store = store();
        ok(store.add(highlight("doc-a", 3, "later page")));
        ok(store.add(highlight("doc-a", 1, "earlier page")));
        ok(store.add(highlight("doc-b", 0, "other doc")));

        let anns = ok(store.annotations_for("doc-a"));
        assert_eq!(anns.len(), 2);
        assert_eq!(
            (anns[0].page, anns[0].note.as_str()),
            (1, "earlier page"),
            "page order"
        );
        assert_eq!(anns[1].page, 3);
        assert!((anns[0].quads[0].x0 - 71.5).abs() < f64::EPSILON);
        assert!((anns[0].quads[0].y1 - 134.75).abs() < f64::EPSILON);
        assert!(ok(store.annotations_for("doc-unknown")).is_empty());
    }

    #[test]
    fn update_remove_and_missing_id_errors() {
        let mut store = store();
        let id = ok(store.add(highlight("doc", 0, "first")));
        ok(store.update_note(id, "revised"));
        ok(store.set_color(id, "#ff6188"));
        ok(store.set_linked_note(id, "notes/first.md"));
        let anns = ok(store.annotations_for("doc"));
        assert_eq!(anns[0].note, "revised");
        assert_eq!(anns[0].color, "#ff6188");
        assert_eq!(anns[0].linked_note.as_deref(), Some("notes/first.md"));
        assert!(anns[0].modified_at >= anns[0].created_at);

        ok(store.remove(id));
        assert!(ok(store.annotations_for("doc")).is_empty());
        assert!(store.update_note(9999, "ghost").is_err());
    }

    #[test]
    fn json_export_import_round_trips_into_a_fresh_store() {
        let mut source = store();
        let mut ann = highlight("feedface", 2, "imported");
        ann.linked_note = Some("notes/reading.md".to_string());
        ok(source.add(ann));
        ok(source.record_document("feedface", "papers/p.pdf"));
        let json = ok(source.export_json("feedface"));

        let mut target = store();
        assert_eq!(ok(target.import_json(&json)), 1);
        let anns = ok(target.annotations_for("feedface"));
        assert_eq!(anns.len(), 1);
        assert_eq!(anns[0].note, "imported");
        assert_eq!(anns[0].linked_note.as_deref(), Some("notes/reading.md"));
        assert_eq!(
            ok(target.last_path("feedface")).as_deref(),
            Some("papers/p.pdf"),
            "path hint travels with the export"
        );
        assert!(target.import_json("{ not json").is_err());
    }

    #[test]
    fn markdown_summary_groups_by_page() {
        let mut store = store();
        ok(store.add(highlight("cafe", 0, "intro claim")));
        let mut linked = highlight("cafe", 4, "method detail");
        linked.linked_note = Some("notes/method.md".to_string());
        ok(store.add(linked));
        ok(store.record_document("cafe", "papers/study.pdf"));

        let md = ok(store.export_markdown("cafe"));
        assert!(md.starts_with("# Annotations — papers/study.pdf"));
        assert!(md.contains("## Page 1"));
        assert!(md.contains("- [#ffd866] intro claim"));
        assert!(md.contains("## Page 5"));
        assert!(md.contains("method detail → [[notes/method.md]]"));
    }

    #[test]
    fn known_documents_reports_counts_and_last_paths() {
        let mut store = store();
        ok(store.add(highlight("d1", 0, "")));
        ok(store.add(highlight("d1", 1, "")));
        ok(store.add(highlight("d2", 0, "")));
        ok(store.record_document("d1", "a.pdf"));

        let docs = ok(store.known_documents());
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].doc_hash, "d1");
        assert_eq!(docs[0].annotation_count, 2);
        assert_eq!(docs[0].last_path, "a.pdf");
        assert_eq!(docs[1].last_path, "", "no recorded path yet");
    }
}

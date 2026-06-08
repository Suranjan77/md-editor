use rusqlite::{Connection, Error};

use crate::domain::pdf::{
    PdfAnnotation, PdfAnnotationColor, PdfAnnotationKind, PdfAnnotationStatus, PdfRect,
    PdfTextRange,
};
use crate::state::{CachedPdfTextHit, LinkedPdfAnnotationReference, PdfDocumentReference};

pub(crate) struct PdfDocumentRepository<'a> {
    db: &'a Connection,
}

impl<'a> PdfDocumentRepository<'a> {
    pub(crate) const fn new(db: &'a Connection) -> Self {
        Self { db }
    }

    pub(crate) fn path_by_id(&self, document_id: &str) -> Result<Option<String>, String> {
        match self.db.query_row(
            "SELECT vault_relative_path FROM pdf_documents WHERE document_id = ?1",
            [document_id],
            |row| row.get(0),
        ) {
            Ok(path) => Ok(Some(path)),
            Err(Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    pub(crate) fn save(
        &self,
        document_id: &str,
        vault_path: &str,
        file_size: u64,
        modified_at: Option<i64>,
    ) -> Result<(), String> {
        let existing = self.metadata(vault_path)?;
        let changed = existing.iter().any(|(id, size, mtime)| {
            id != document_id || *size != file_size as i64 || *mtime != modified_at
        });
        if changed {
            self.invalidate(vault_path)?;
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        self.db
            .execute(
                "INSERT INTO pdf_documents (
                    document_id, vault_relative_path, file_size, modified_at, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                 ON CONFLICT(document_id) DO UPDATE SET
                    vault_relative_path = excluded.vault_relative_path,
                    file_size = excluded.file_size,
                    modified_at = excluded.modified_at,
                    updated_at = excluded.updated_at",
                rusqlite::params![document_id, vault_path, file_size as i64, modified_at, now],
            )
            .map(|_| ())
            .map_err(|error| format!("Failed to save pdf document: {error}"))
    }

    pub(crate) fn metadata(
        &self,
        vault_path: &str,
    ) -> Result<Vec<(String, i64, Option<i64>)>, String> {
        let mut statement = self
            .db
            .prepare(
                "SELECT document_id, file_size, modified_at
                 FROM pdf_documents WHERE vault_relative_path = ?1",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([vault_path], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub(crate) fn invalidate(&self, vault_path: &str) -> Result<(), String> {
        let transaction = self.db.unchecked_transaction().map_err(|e| e.to_string())?;
        transaction
            .execute("DELETE FROM pdf_text_search WHERE path = ?1", [vault_path])
            .map_err(|error| format!("Failed to clear stale cached text: {error}"))?;
        transaction
            .execute(
                "DELETE FROM pdf_documents WHERE vault_relative_path = ?1",
                [vault_path],
            )
            .map_err(|error| format!("Failed to clear stale documents: {error}"))?;
        transaction.commit().map_err(|error| error.to_string())
    }

    pub(crate) fn has_cached_text(&self, vault_path: &str) -> Result<bool, String> {
        self.db
            .query_row(
                "SELECT COUNT(*) FROM pdf_text_search WHERE path = ?1",
                [vault_path],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count > 0)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn save_page_text(
        &self,
        vault_path: &str,
        page_index: u16,
        content: &str,
    ) -> Result<(), String> {
        let transaction = self.db.unchecked_transaction().map_err(|e| e.to_string())?;
        transaction
            .execute(
                "DELETE FROM pdf_text_search WHERE path = ?1 AND page_index = ?2",
                rusqlite::params![vault_path, page_index as i64],
            )
            .map_err(|error| format!("Failed to clear cached PDF text: {error}"))?;
        transaction
            .execute(
                "INSERT INTO pdf_text_search (path, page_index, content) VALUES (?1, ?2, ?3)",
                rusqlite::params![vault_path, page_index as i64, content],
            )
            .map_err(|error| format!("Failed to cache PDF text: {error}"))?;
        transaction.commit().map_err(|error| error.to_string())
    }

    pub(crate) fn search_cached_text(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<CachedPdfTextHit>, String> {
        let fts_query = format!("*{}*", query.replace('"', ""));
        let mut statement = self
            .db
            .prepare(
                "SELECT path, page_index, content
                 FROM pdf_text_search
                 WHERE content MATCH ?1
                 LIMIT ?2",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(rusqlite::params![fts_query, limit as i64], |row| {
                Ok(CachedPdfTextHit {
                    vault_path: row.get(0)?,
                    page_index: row.get(1)?,
                    content: row.get(2)?,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub(crate) fn references(&self) -> Result<Vec<PdfDocumentReference>, String> {
        let mut statement = self
            .db
            .prepare("SELECT document_id, vault_relative_path FROM pdf_documents")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok(PdfDocumentReference {
                    document_id: row.get(0)?,
                    vault_path: row.get(1)?,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }
}

pub(crate) struct PdfAnnotationRepository<'a> {
    db: &'a Connection,
}

impl<'a> PdfAnnotationRepository<'a> {
    pub(crate) const fn new(db: &'a Connection) -> Self {
        Self { db }
    }

    pub(crate) fn exists(&self, annotation_id: &str) -> Result<bool, String> {
        let mut statement = self
            .db
            .prepare("SELECT 1 FROM pdf_annotations WHERE id = ?1")
            .map_err(|error| error.to_string())?;
        statement
            .exists([annotation_id])
            .map_err(|error| error.to_string())
    }

    pub(crate) fn save(&self, annotation: &PdfAnnotation) -> Result<(), String> {
        let ranges_json = serde_json::to_string(&annotation.ranges)
            .map_err(|error| format!("Failed to serialize ranges: {error}"))?;
        let rects_json = serde_json::to_string(&annotation.rects)
            .map_err(|error| format!("Failed to serialize rects: {error}"))?;
        let tags_json = serde_json::to_string(&annotation.tags)
            .map_err(|error| format!("Failed to serialize tags: {error}"))?;

        self.db
            .execute(
                "INSERT INTO pdf_annotations (
                    id, document_id, page_index, kind, color, selected_text,
                    ranges_json, rects_json, note, linked_note_path, markdown_anchor,
                    tags_json, status, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                 ON CONFLICT(id) DO UPDATE SET
                    color = excluded.color,
                    selected_text = excluded.selected_text,
                    ranges_json = excluded.ranges_json,
                    rects_json = excluded.rects_json,
                    note = excluded.note,
                    linked_note_path = excluded.linked_note_path,
                    markdown_anchor = excluded.markdown_anchor,
                    tags_json = excluded.tags_json,
                    status = excluded.status,
                    updated_at = excluded.updated_at",
                rusqlite::params![
                    annotation.id,
                    annotation.document_id,
                    annotation.page_index as i32,
                    annotation.kind.as_str(),
                    annotation.color.as_str(),
                    annotation.selected_text,
                    ranges_json,
                    rects_json,
                    annotation.note,
                    annotation.linked_note_path,
                    annotation.markdown_anchor,
                    tags_json,
                    annotation.status.as_str(),
                    annotation.created_at,
                    annotation.updated_at,
                ],
            )
            .map(|_| ())
            .map_err(|error| format!("Failed to save pdf annotation: {error}"))
    }

    pub(crate) fn delete(&self, annotation_id: &str) -> Result<(), String> {
        self.db
            .execute("DELETE FROM pdf_annotations WHERE id = ?1", [annotation_id])
            .map(|_| ())
            .map_err(|error| format!("Failed to delete pdf annotation: {error}"))
    }

    pub(crate) fn linked_references(&self) -> Result<Vec<LinkedPdfAnnotationReference>, String> {
        let mut statement = self
            .db
            .prepare(
                "SELECT id, document_id, page_index, linked_note_path
                 FROM pdf_annotations
                 WHERE linked_note_path IS NOT NULL AND linked_note_path != ''",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok(LinkedPdfAnnotationReference {
                    annotation_id: row.get(0)?,
                    document_id: row.get(1)?,
                    page_index: row.get(2)?,
                    linked_note_path: row.get(3)?,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub(crate) fn list(
        &self,
        document_id: &str,
        page_index: Option<u16>,
    ) -> Result<Vec<PdfAnnotation>, String> {
        let query = if page_index.is_some() {
            "SELECT id, document_id, page_index, kind, color, selected_text,
                    ranges_json, rects_json, note, linked_note_path, markdown_anchor,
                    created_at, updated_at, tags_json, status
             FROM pdf_annotations
             WHERE document_id = ?1 AND page_index = ?2
             ORDER BY created_at ASC"
        } else {
            "SELECT id, document_id, page_index, kind, color, selected_text,
                    ranges_json, rects_json, note, linked_note_path, markdown_anchor,
                    created_at, updated_at, tags_json, status
             FROM pdf_annotations
             WHERE document_id = ?1
             ORDER BY created_at ASC"
        };

        let mut statement = self.db.prepare(query).map_err(|e| e.to_string())?;
        let mut rows = if let Some(page) = page_index {
            statement
                .query(rusqlite::params![document_id, page as i32])
                .map_err(|e| e.to_string())?
        } else {
            statement
                .query(rusqlite::params![document_id])
                .map_err(|e| e.to_string())?
        };

        let mut annotations = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let kind = row
                .get::<_, String>(3)
                .map_err(|e| e.to_string())?
                .parse::<PdfAnnotationKind>()?;
            let color = row
                .get::<_, String>(4)
                .map_err(|e| e.to_string())?
                .parse::<PdfAnnotationColor>()?;
            let ranges = serde_json::from_str::<Vec<PdfTextRange>>(
                &row.get::<_, String>(6).map_err(|e| e.to_string())?,
            )
            .map_err(|e| format!("Failed to parse ranges JSON: {e}"))?;
            let rects = serde_json::from_str::<Vec<PdfRect>>(
                &row.get::<_, String>(7).map_err(|e| e.to_string())?,
            )
            .map_err(|e| format!("Failed to parse rects JSON: {e}"))?;
            let tags = serde_json::from_str::<Vec<String>>(
                &row.get::<_, String>(13).map_err(|e| e.to_string())?,
            )
            .map_err(|e| format!("Failed to parse tags JSON: {e}"))?;
            let status = row
                .get::<_, String>(14)
                .map_err(|e| e.to_string())?
                .parse::<PdfAnnotationStatus>()?;

            annotations.push(PdfAnnotation {
                id: row.get(0).map_err(|e| e.to_string())?,
                document_id: row.get(1).map_err(|e| e.to_string())?,
                page_index: row.get::<_, i32>(2).map_err(|e| e.to_string())? as u16,
                kind,
                color,
                selected_text: row.get(5).map_err(|e| e.to_string())?,
                ranges,
                rects,
                note: row.get(8).map_err(|e| e.to_string())?,
                linked_note_path: row.get(9).map_err(|e| e.to_string())?,
                markdown_anchor: row.get(10).map_err(|e| e.to_string())?,
                tags,
                status,
                created_at: row.get(11).map_err(|e| e.to_string())?,
                updated_at: row.get(12).map_err(|e| e.to_string())?,
            });
        }
        Ok(annotations)
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{PdfAnnotationRepository, PdfDocumentRepository};
    use crate::database::initialize;
    use crate::domain::pdf::{
        PdfAnnotation, PdfAnnotationColor, PdfAnnotationKind, PdfAnnotationStatus,
    };

    #[test]
    fn page_text_save_replaces_same_page() {
        let db = initialized_database();
        let repository = PdfDocumentRepository::new(&db);

        repository
            .save_page_text("paper.pdf", 2, "first")
            .expect("first page text should save");
        repository
            .save_page_text("paper.pdf", 2, "second")
            .expect("second page text should replace first");

        let content: String = db
            .query_row(
                "SELECT content FROM pdf_text_search WHERE path = 'paper.pdf' AND page_index = 2",
                [],
                |row| row.get(0),
            )
            .expect("page text should load");
        assert_eq!(content, "second");
    }

    #[test]
    fn annotation_save_exists_and_delete_round_trip() {
        let db = initialized_database();
        let repository = PdfAnnotationRepository::new(&db);
        let annotation = PdfAnnotation {
            id: "ann-1".to_string(),
            document_id: "doc-1".to_string(),
            page_index: 0,
            kind: PdfAnnotationKind::Highlight,
            color: PdfAnnotationColor::Yellow,
            selected_text: "text".to_string(),
            ranges: Vec::new(),
            rects: Vec::new(),
            note: None,
            linked_note_path: None,
            markdown_anchor: None,
            tags: Vec::new(),
            status: PdfAnnotationStatus::Unresolved,
            created_at: 1,
            updated_at: 1,
        };

        repository
            .save(&annotation)
            .expect("annotation should save");
        assert!(repository.exists("ann-1").expect("existence should load"));
        repository
            .delete("ann-1")
            .expect("annotation should delete");
        assert!(!repository.exists("ann-1").expect("existence should reload"));
    }

    fn initialized_database() -> Connection {
        let db = Connection::open_in_memory().expect("memory database should open");
        initialize(&db).expect("database should initialize");
        db
    }
}

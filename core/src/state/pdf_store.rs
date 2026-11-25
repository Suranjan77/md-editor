//! PDF sidecar persistence on `AppState`: document scoping, annotations, references, and text cache.

use std::path::Path;

use rusqlite::{OptionalExtension, Transaction};
use sha2::{Digest, Sha256};

use super::{AppState, StateResult};
use crate::pdf::{PdfAnnotation, PdfAnnotationColor, PdfAnnotationKind, PdfRect, PdfTextRange};

impl AppState {
    /// Stable namespace for PDF persistence belonging to the active vault.
    pub(crate) fn pdf_scope_prefix(&self) -> StateResult<Option<String>> {
        let root = {
            let guard = self.vault_root.lock()?;
            guard.clone()
        };
        let Some(root) = root else {
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

    pub(crate) fn pdf_storage_document_id(&self, document_id: &str) -> StateResult<String> {
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
    ) -> StateResult<()> {
        let scope_prefix = self.pdf_scope_prefix()?;
        let storage_document_id = match scope_prefix.as_deref() {
            Some(prefix) => format!("{prefix}{document_id}"),
            None => document_id.to_string(),
        };
        let db = self.db.lock()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let tx = db.unchecked_transaction()?;
        reconcile_previous_pdf_document(
            &tx,
            &storage_document_id,
            vault_relative_path,
            file_size,
            scope_prefix.as_deref(),
        )?;

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

        tx.commit()?;
        Ok(())
    }

    pub fn get_pdf_path_by_id(&self, document_id: &str) -> StateResult<Option<String>> {
        let storage_document_id = self.pdf_storage_document_id(document_id)?;
        let db = self.db.lock()?;
        let mut stmt =
            db.prepare("SELECT vault_relative_path FROM pdf_documents WHERE document_id = ?1")?;
        let mut rows = stmt.query([storage_document_id])?;
        if let Some(row) = rows.next()? {
            let path: String = row.get(0)?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    pub fn save_pdf_annotation(&self, ann: &PdfAnnotation) -> StateResult<()> {
        let storage_document_id = self.pdf_storage_document_id(&ann.document_id)?;
        let db = self.db.lock()?;
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

    pub fn delete_pdf_annotation(&self, id: &str) -> StateResult<()> {
        let scope_prefix = self.pdf_scope_prefix()?;
        let db = self.db.lock()?;
        let deleted = if let Some(prefix) = scope_prefix {
            db.execute(
                "DELETE FROM pdf_annotations
                 WHERE id = ?1 AND substr(document_id, 1, length(?2)) = ?2",
                rusqlite::params![id, prefix],
            )
        } else {
            db.execute("DELETE FROM pdf_annotations WHERE id = ?1", [id])
        };
        deleted.map_err(|e| format!("Failed to delete pdf annotation: {e}"))?;
        Ok(())
    }

    /// Cached resolved internal references for a document, or `None` if not scanned yet.
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
        let (version, links): (u32, Vec<crate::references::ReferenceLink>) =
            serde_json::from_str(&json).ok()?;
        (version == crate::references::RESOLVER_VERSION).then_some(links)
    }

    /// Cached recovered outline of a bookmark-less document, or `None` if not
    /// recovered yet (or recovered by an older [`crate::pdf::OUTLINE_VERSION`]).
    pub fn get_pdf_outline(&self, document_id: &str) -> Option<Vec<crate::pdf::TocEntry>> {
        let storage_document_id = self.pdf_storage_document_id(document_id).ok()?;
        let db = self.db.lock().ok()?;
        let json: String = db
            .query_row(
                "SELECT outline_json FROM pdf_outline WHERE document_id = ?1",
                [storage_document_id],
                |row| row.get(0),
            )
            .ok()?;
        let (version, outline): (u32, Vec<crate::pdf::TocEntry>) =
            serde_json::from_str(&json).ok()?;
        (version == crate::pdf::OUTLINE_VERSION).then_some(outline)
    }

    /// Store a recovered outline for a document. Best-effort.
    pub fn put_pdf_outline(&self, document_id: &str, outline: &[crate::pdf::TocEntry]) {
        let Ok(storage_document_id) = self.pdf_storage_document_id(document_id) else {
            return;
        };
        let Ok(db) = self.db.lock() else {
            return;
        };
        let Ok(json) = serde_json::to_string(&(crate::pdf::OUTLINE_VERSION, outline)) else {
            return;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if let Err(e) = db.execute(
            "INSERT INTO pdf_outline (document_id, outline_json, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(document_id) DO UPDATE SET
                outline_json = excluded.outline_json,
                updated_at = excluded.updated_at",
            rusqlite::params![storage_document_id, json, now],
        ) {
            log::warn!("Failed to cache pdf outline: {e}");
        }
    }

    /// Store resolved references for a document. Best-effort.
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
        let Ok(json) = serde_json::to_string(&(crate::references::RESOLVER_VERSION, links)) else {
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
            log::warn!("Failed to cache PDF references for {document_id}: {e}");
        }
    }

    /// Return cached extracted text for a PDF if size and mtime match.
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
        let key = format!(
            "{}{}",
            Self::pdf_scope_prefix_for_root(vault_root),
            rel_path
        );
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

    /// Store extracted PDF text keyed by path + size + modified-time. Best-effort.
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
        let key = format!(
            "{}{}",
            Self::pdf_scope_prefix_for_root(vault_root),
            rel_path
        );
        self.put_cached_pdf_text_by_key(&key, file_size, modified_at, content);
    }

    fn pdf_cache_key(&self, rel_path: &str) -> StateResult<String> {
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
            log::warn!("Failed to cache PDF text for {key}: {e}");
        }
    }

    pub fn get_pdf_annotations(
        &self,
        document_id: &str,
        page_index: Option<u16>,
    ) -> StateResult<Vec<PdfAnnotation>> {
        let storage_document_id = self.pdf_storage_document_id(document_id)?;
        let db = self.db.lock()?;
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

        let mut stmt = db.prepare(query)?;
        let mut rows = if let Some(page) = page_index {
            stmt.query(rusqlite::params![storage_document_id, page as i32])?
        } else {
            stmt.query(rusqlite::params![storage_document_id])?
        };

        let mut annotations = Vec::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let _stored_doc_id: String = row.get(1)?;
            let page_idx: i32 = row.get(2)?;
            let kind_str: String = row.get(3)?;
            let color_str: String = row.get(4)?;
            let selected_text: String = row.get(5)?;
            let ranges_json: String = row.get(6)?;
            let rects_json: String = row.get(7)?;
            let note: Option<String> = row.get(8)?;
            let linked_note_path: Option<String> = row.get(9)?;
            let markdown_anchor: Option<String> = row.get(10)?;
            let created_at: i64 = row.get(11)?;
            let updated_at: i64 = row.get(12)?;

            let kind = PdfAnnotationKind::parse(&kind_str)?;
            let color = PdfAnnotationColor::parse(&color_str)?;
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

fn reconcile_previous_pdf_document(
    tx: &Transaction<'_>,
    storage_document_id: &str,
    vault_relative_path: &str,
    file_size: u64,
    scope_prefix: Option<&str>,
) -> StateResult<()> {
    let existing_new: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pdf_documents WHERE document_id = ?1)",
        [storage_document_id],
        |row| row.get(0),
    )?;
    if existing_new {
        return Ok(());
    }
    let previous_id: Option<String> = if let Some(prefix) = scope_prefix {
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
        .optional()?
    } else {
        tx.query_row(
            "SELECT document_id FROM pdf_documents
             WHERE vault_relative_path = ?1 AND document_id != ?2
             ORDER BY (file_size = ?3) DESC, updated_at DESC
             LIMIT 1",
            rusqlite::params![vault_relative_path, storage_document_id, file_size as i64],
            |row| row.get(0),
        )
        .optional()?
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
            "UPDATE pdf_outline SET document_id = ?1 WHERE document_id = ?2",
            rusqlite::params![storage_document_id, previous_id],
        )
        .map_err(|e| format!("Failed to reconcile pdf outline: {e}"))?;
        tx.execute(
            "UPDATE pdf_documents SET document_id = ?1 WHERE document_id = ?2",
            rusqlite::params![storage_document_id, previous_id],
        )
        .map_err(|e| format!("Failed to reconcile pdf document: {e}"))?;
    }
    Ok(())
}

//! Portable PDF highlights: `<vault>/.md-editor/annotations/<document id>.jsonl`, merged with SQLite.
//!
//! SQLite stays the working store; each PDF's highlights are also written into
//! the vault, one JSON object per line in a fixed order, so Git or Syncthing
//! carries them to other machines with clean line diffs. The document id is a
//! hash of the PDF's content, the same on every machine.
//!
//! Every local change rewrites the document's file. Opening a vault or a PDF
//! reconciles the file with SQLite:
//!
//! - file and SQLite agree: nothing to do;
//! - the file is unchanged since this machine last wrote or read it: SQLite is
//!   newer (a write failed), so the file is rewritten;
//! - otherwise the file changed elsewhere and the two are merged against the
//!   ids both last agreed on (`pdf_sidecar_sync`): an id that disappeared on
//!   one side was deleted there; an id new on one side was added there; an id
//!   on both sides keeps its later `updated_at`.
//!
//! A missing file is rewritten, never read as "everything was deleted".

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};

use super::{AppState, StateResult};
use crate::pdf::PdfAnnotation;

/// The vault folder MD Editor keeps its portable files in. Dot-prefixed, so
/// the vault walk, the file tree and the watcher skip it.
pub const SIDECAR_DIR: &str = ".md-editor";
const ANNOTATIONS_DIR: &str = "annotations";
const EXTENSION: &str = "jsonl";
/// Two machines adding highlights to the same PDF append different lines:
/// Git's union merge keeps both instead of reporting a conflict. A same-id
/// edit on both sides leaves two lines, and the later one wins on import.
const GITATTRIBUTES: &str =
    "# Written by MD Editor. Highlight files merge line by line.\n*.jsonl merge=union\n";

/// What the last reconcile of one document found: the file's hash and the ids
/// it held, which is the base the next three-way merge compares against.
struct SyncRecord {
    content_hash: String,
    ids: BTreeSet<String>,
}

impl AppState {
    /// Reconcile every highlight file in the vault, and write one for each
    /// document that has highlights but no file yet. Returns the number of
    /// documents whose highlights in SQLite changed.
    pub fn sync_pdf_sidecars(&self) -> StateResult<usize> {
        let Some(root) = self.vault_root.lock()?.clone() else {
            return Ok(0);
        };
        let mut ids: BTreeSet<String> = self.annotated_document_ids()?.into_iter().collect();
        if let Ok(entries) = std::fs::read_dir(annotations_dir(&root)) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some(EXTENSION) {
                    continue;
                }
                if let Some(id) = path.file_stem().and_then(|s| s.to_str())
                    && is_document_id(id)
                {
                    ids.insert(id.to_string());
                }
            }
        }
        let mut changed = 0;
        for id in ids {
            match self.sync_pdf_sidecar(&id) {
                Ok(true) => changed += 1,
                Ok(false) => {}
                Err(e) => log::warn!("Could not sync highlights of PDF {id}: {e}"),
            }
        }
        Ok(changed)
    }

    /// Reconcile one document's highlight file with SQLite (see the module
    /// docs). Returns whether SQLite's highlights for it changed.
    pub fn sync_pdf_sidecar(&self, document_id: &str) -> StateResult<bool> {
        let Some(root) = self.vault_root.lock()?.clone() else {
            return Ok(false);
        };
        if !is_document_id(document_id) {
            return Ok(false);
        }
        let path = sidecar_path(&root, document_id);
        let local = self.get_pdf_annotations(document_id, None)?;
        let file = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if !local.is_empty() {
                    self.write_sidecar(&root, document_id, &local)?;
                }
                return Ok(false);
            }
            Err(e) => return Err(format!("Failed to read {}: {e}", path.display()).into()),
        };
        let local_text = serialize(&local);
        let record = self.sync_record(document_id)?;
        if file == local_text {
            if record
                .as_ref()
                .is_none_or(|r| r.content_hash != hash(&file))
            {
                self.put_sync_record(document_id, &file, &local)?;
            }
            return Ok(false);
        }
        if record
            .as_ref()
            .is_some_and(|r| r.content_hash == hash(&file))
        {
            // Only SQLite moved since the last sync.
            self.write_sidecar(&root, document_id, &local)?;
            return Ok(false);
        }

        let remote = parse(document_id, &file);
        let merged = merge(&local, &remote, record.as_ref().map(|r| &r.ids));
        let changed = serialize(&merged) != local_text;
        if changed {
            self.replace_pdf_annotations(document_id, &merged)?;
            log::info!(
                "Merged highlights of PDF {document_id} from {}: {} here, {} in the file, {} now",
                path.display(),
                local.len(),
                remote.len(),
                merged.len()
            );
        }
        self.write_sidecar(&root, document_id, &merged)?;
        Ok(changed)
    }

    /// Rewrite one document's highlight file from SQLite after a local change.
    /// Best-effort: SQLite already holds the change, and the next sync retries.
    pub(crate) fn export_pdf_sidecar(&self, document_id: &str) {
        let result = (|| -> StateResult<()> {
            let Some(root) = self.vault_root.lock()?.clone() else {
                return Ok(());
            };
            if !is_document_id(document_id) {
                return Ok(());
            }
            let local = self.get_pdf_annotations(document_id, None)?;
            let path = sidecar_path(&root, document_id);
            if local.is_empty() && !path.exists() {
                return Ok(());
            }
            self.write_sidecar(&root, document_id, &local)
        })();
        if let Err(e) = result {
            log::warn!("Could not write the highlight file of PDF {document_id}: {e}");
        }
    }

    /// Rewrite the files of every document with highlights; unchanged files
    /// are left alone. After a rename rewrote linked note paths in SQLite.
    pub(crate) fn export_all_pdf_sidecars(&self) {
        match self.annotated_document_ids() {
            Ok(ids) => ids.iter().for_each(|id| self.export_pdf_sidecar(id)),
            Err(e) => log::warn!("Could not list highlighted PDFs: {e}"),
        }
    }

    /// The highlights moved to a new document id (the PDF's content changed):
    /// the old file goes, so another machine does not bring them back under it.
    pub(crate) fn move_pdf_sidecar(&self, old_document_id: &str, new_document_id: &str) {
        if let Ok(Some(root)) = self.vault_root.lock().map(|r| r.clone())
            && is_document_id(old_document_id)
        {
            let old = sidecar_path(&root, old_document_id);
            if old.exists()
                && let Err(e) = std::fs::remove_file(&old)
            {
                log::warn!("Could not remove {}: {e}", old.display());
            }
        }
        if let (Ok(old), Ok(db)) = (
            self.pdf_storage_document_id(old_document_id),
            self.db.lock(),
        ) {
            let _ = db.execute("DELETE FROM pdf_sidecar_sync WHERE document_id = ?1", [old]);
        }
        self.export_pdf_sidecar(new_document_id);
    }

    fn write_sidecar(
        &self,
        root: &Path,
        document_id: &str,
        annotations: &[PdfAnnotation],
    ) -> StateResult<()> {
        let text = serialize(annotations);
        let path = sidecar_path(root, document_id);
        let unchanged = std::fs::read_to_string(&path).is_ok_and(|current| current == text);
        if !unchanged {
            crate::vault::write_file(&path, &text)
                .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
        }
        let attributes = root.join(SIDECAR_DIR).join(".gitattributes");
        if !attributes.exists()
            && let Err(e) = crate::vault::write_file(&attributes, GITATTRIBUTES)
        {
            log::warn!("Could not write {}: {e}", attributes.display());
        }
        self.put_sync_record(document_id, &text, annotations)
    }

    /// Ids (unscoped) of this vault's documents that have highlights.
    fn annotated_document_ids(&self) -> StateResult<Vec<String>> {
        let Some(prefix) = self.pdf_scope_prefix()? else {
            return Ok(Vec::new());
        };
        let db = self.db.lock()?;
        let mut stmt = db.prepare(
            "SELECT DISTINCT document_id FROM pdf_annotations
             WHERE substr(document_id, 1, length(?1)) = ?1",
        )?;
        let ids = stmt
            .query_map([&prefix], |row| row.get::<_, String>(0))?
            .filter_map(Result::ok)
            .filter_map(|id| id.strip_prefix(&prefix).map(str::to_string))
            .collect();
        Ok(ids)
    }

    fn sync_record(&self, document_id: &str) -> StateResult<Option<SyncRecord>> {
        let storage_id = self.pdf_storage_document_id(document_id)?;
        let db = self.db.lock()?;
        let row: Option<(String, String)> = db
            .query_row(
                "SELECT content_hash, ids_json FROM pdf_sidecar_sync WHERE document_id = ?1",
                [storage_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(row.map(|(content_hash, ids_json)| SyncRecord {
            content_hash,
            ids: serde_json::from_str(&ids_json).unwrap_or_default(),
        }))
    }

    fn put_sync_record(
        &self,
        document_id: &str,
        text: &str,
        annotations: &[PdfAnnotation],
    ) -> StateResult<()> {
        let storage_id = self.pdf_storage_document_id(document_id)?;
        let ids: BTreeSet<&str> = annotations.iter().map(|a| a.id.as_str()).collect();
        let ids_json =
            serde_json::to_string(&ids).map_err(|e| format!("Failed to serialize ids: {e}"))?;
        let db = self.db.lock()?;
        db.execute(
            "INSERT INTO pdf_sidecar_sync (document_id, content_hash, ids_json)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(document_id) DO UPDATE SET
                content_hash = excluded.content_hash,
                ids_json = excluded.ids_json",
            rusqlite::params![storage_id, hash(text), ids_json],
        )?;
        Ok(())
    }

    /// Make `annotations` the document's whole set of highlights.
    fn replace_pdf_annotations(
        &self,
        document_id: &str,
        annotations: &[PdfAnnotation],
    ) -> StateResult<()> {
        let storage_id = self.pdf_storage_document_id(document_id)?;
        let db = self.db.lock()?;
        let tx = db.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM pdf_annotations WHERE document_id = ?1",
            [&storage_id],
        )?;
        for ann in annotations {
            super::pdf_store::upsert_annotation(&tx, &storage_id, ann)?;
        }
        tx.commit()?;
        Ok(())
    }
}

fn annotations_dir(root: &Path) -> PathBuf {
    root.join(SIDECAR_DIR).join(ANNOTATIONS_DIR)
}

pub(crate) fn sidecar_path(root: &Path, document_id: &str) -> PathBuf {
    annotations_dir(root).join(format!("{document_id}.{EXTENSION}"))
}

/// Document ids are lowercase SHA-256 hex. Anything else (a stray file in the
/// folder) is ignored, and can never name a path outside it.
fn is_document_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// One line per highlight, oldest first, so a new highlight is a new line at
/// the end and an edit changes one line.
fn serialize(annotations: &[PdfAnnotation]) -> String {
    let mut sorted: Vec<&PdfAnnotation> = annotations.iter().collect();
    sorted.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
    let mut out = String::new();
    for ann in sorted {
        match serde_json::to_string(ann) {
            Ok(line) => {
                out.push_str(&line);
                out.push('\n');
            }
            Err(e) => log::warn!("Could not serialize highlight {}: {e}", ann.id),
        }
    }
    out
}

/// Read a highlight file. Unreadable lines are skipped and logged; an id seen
/// twice (a union merge of a same-id edit) keeps its later version.
fn parse(document_id: &str, text: &str) -> Vec<PdfAnnotation> {
    let mut by_id: BTreeMap<String, PdfAnnotation> = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<PdfAnnotation>(line) {
            Ok(mut ann) => {
                ann.document_id = document_id.to_string();
                keep_later(&mut by_id, ann);
            }
            Err(e) => log::warn!(
                "Skipping line {} of the highlights of PDF {document_id}: {e}",
                n + 1
            ),
        }
    }
    by_id.into_values().collect()
}

fn keep_later(by_id: &mut BTreeMap<String, PdfAnnotation>, ann: PdfAnnotation) {
    match by_id.get(&ann.id) {
        Some(existing) if existing.updated_at >= ann.updated_at => {}
        _ => {
            by_id.insert(ann.id.clone(), ann);
        }
    }
}

/// Three-way merge of this machine's highlights with the file's, against the
/// ids both held at the last sync. Without a base (first sync on this
/// machine) nothing counts as deleted.
fn merge(
    local: &[PdfAnnotation],
    remote: &[PdfAnnotation],
    base: Option<&BTreeSet<String>>,
) -> Vec<PdfAnnotation> {
    let in_base = |id: &str| base.is_some_and(|b| b.contains(id));
    let local_ids: BTreeSet<&str> = local.iter().map(|a| a.id.as_str()).collect();
    let remote_ids: BTreeSet<&str> = remote.iter().map(|a| a.id.as_str()).collect();
    // The file's version goes in first so it wins a tie: a rename on another
    // machine rewrites linked note paths without touching `updated_at`.
    let mut by_id = BTreeMap::new();
    for ann in remote {
        // Gone from SQLite but synced before: deleted here.
        if local_ids.contains(ann.id.as_str()) || !in_base(&ann.id) {
            keep_later(&mut by_id, ann.clone());
        }
    }
    for ann in local {
        // Gone from the file but synced before: deleted on another machine.
        if remote_ids.contains(ann.id.as_str()) || !in_base(&ann.id) {
            keep_later(&mut by_id, ann.clone());
        }
    }
    by_id.into_values().collect()
}

#[cfg(test)]
mod tests;

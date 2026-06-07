use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::database;
use crate::file_index::FileIndex;
use crate::pdf::{
    PdfAnnotation, PdfAnnotationColor, PdfAnnotationKind, PdfAnnotationStatus, PdfRect,
    PdfRenderer, PdfTextRange,
};

/// Application-wide shared state.
/// Wrap in `Arc<AppState>` when sharing across threads.
pub struct AppState {
    pub(crate) vault_root: Mutex<Option<PathBuf>>,
    pub(crate) file_index: Mutex<FileIndex>,
    pub(crate) db: Mutex<Connection>,
    pdf_renderer: Option<PdfRenderer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedPdfTextHit {
    pub vault_path: String,
    pub page_index: u16,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfDocumentReference {
    pub document_id: String,
    pub vault_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedPdfAnnotationReference {
    pub annotation_id: String,
    pub document_id: String,
    pub page_index: u16,
    pub linked_note_path: String,
}

impl AppState {
    pub fn pdf_renderer(&self) -> Option<&PdfRenderer> {
        self.pdf_renderer.as_ref()
    }

    pub fn vault_root_path(&self) -> Result<Option<PathBuf>, String> {
        self.vault_root
            .lock()
            .map(|vault_root| vault_root.clone())
            .map_err(|err| err.to_string())
    }

    pub fn vault_paths_are_linked(&self, first: &str, second: &str) -> bool {
        let Ok(index) = self.file_index.lock() else {
            return false;
        };
        let Ok(vault_root) = self.vault_root.lock() else {
            return false;
        };
        let Some(vault_root) = vault_root.as_ref() else {
            return false;
        };

        let first = crate::vault::resolve_vault_path(vault_root, first);
        let second = crate::vault::resolve_vault_path(vault_root, second);
        index
            .outgoing
            .get(&first)
            .is_some_and(|paths| paths.contains(&second))
            || index
                .incoming
                .get(&first)
                .is_some_and(|paths| paths.contains(&second))
    }

    pub fn update_file_index_targets(
        &self,
        vault_path: &str,
        targets: &[String],
    ) -> Result<(), String> {
        let vault_root = self
            .vault_root_path()?
            .ok_or_else(|| "No vault root set".to_string())?;
        let abs_path = crate::vault::resolve_vault_path(&vault_root, vault_path);
        let mut index = self.file_index.lock().map_err(|err| err.to_string())?;
        index.update_file_targets(&abs_path, targets.iter().map(String::as_str));
        Ok(())
    }

    pub fn rebuild_file_index_with_targets(
        &self,
        vault_root: &std::path::Path,
        files: Vec<(PathBuf, Vec<String>)>,
    ) -> Result<(), String> {
        let mut index = self.file_index.lock().map_err(|err| err.to_string())?;
        *index = FileIndex::new(vault_root.to_path_buf());
        for (abs_path, targets) in files {
            index.update_file_targets(&abs_path, targets.iter().map(String::as_str));
        }
        Ok(())
    }

    pub fn new() -> Self {
        let db_path = settings_db_path();
        let db = Connection::open(&db_path).expect("Failed to open local sqlite database");
        // Public constructor predates fallible startup API; preserve its panic contract.
        database::initialize(&db).expect("Failed to initialize local sqlite database");

        AppState {
            vault_root: Mutex::new(None),
            file_index: Mutex::new(FileIndex::new(PathBuf::new())),
            db: Mutex::new(db),
            pdf_renderer: PdfRenderer::new().ok(),
        }
    }

    pub fn new_in_memory() -> Self {
        let db = Connection::open_in_memory().expect("Failed to open memory sqlite database");
        // Public constructor predates fallible startup API; preserve its panic contract.
        database::initialize(&db).expect("Failed to initialize memory sqlite database");

        AppState {
            vault_root: Mutex::new(None),
            file_index: Mutex::new(FileIndex::new(PathBuf::new())),
            db: Mutex::new(db),
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

        // Detect if any existing document record for this path has mismatched metadata
        let mut check_stmt = db
            .prepare(
                "SELECT document_id, file_size, modified_at FROM pdf_documents
                 WHERE vault_relative_path = ?1",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = check_stmt
            .query([vault_relative_path])
            .map_err(|e| e.to_string())?;
        let mut changed = false;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let db_id: String = row.get(0).map_err(|e| e.to_string())?;
            let db_size: i64 = row.get(1).map_err(|e| e.to_string())?;
            let db_mtime: Option<i64> = row.get(2).map_err(|e| e.to_string())?;
            if db_id != document_id || db_size != file_size as i64 || db_mtime != modified_at {
                changed = true;
                break;
            }
        }

        if changed {
            // Invalidate cached page text and remove conflicting documents for this path
            db.execute(
                "DELETE FROM pdf_text_search WHERE path = ?1",
                rusqlite::params![vault_relative_path],
            )
            .map_err(|e| format!("Failed to clear stale cached text: {e}"))?;
            db.execute(
                "DELETE FROM pdf_documents WHERE vault_relative_path = ?1",
                rusqlite::params![vault_relative_path],
            )
            .map_err(|e| format!("Failed to clear stale documents: {e}"))?;
        }

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

    pub fn validate_and_invalidate_pdf_cache(
        &self,
        vault_relative_path: &str,
    ) -> Result<bool, String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;

        // 1. Resolve path
        let vault_root = self.vault_root.lock().map_err(|e| e.to_string())?;
        let Some(ref root) = *vault_root else {
            return Ok(false);
        };
        let abs_path = root.join(vault_relative_path);

        // 2. Get disk metadata
        let metadata = match std::fs::metadata(&abs_path) {
            Ok(m) => m,
            Err(_) => {
                // File does not exist on disk, clear cache and document records
                db.execute(
                    "DELETE FROM pdf_text_search WHERE path = ?1",
                    rusqlite::params![vault_relative_path],
                )
                .map_err(|e| e.to_string())?;
                db.execute(
                    "DELETE FROM pdf_documents WHERE vault_relative_path = ?1",
                    rusqlite::params![vault_relative_path],
                )
                .map_err(|e| e.to_string())?;
                return Ok(false);
            }
        };
        let disk_size = metadata.len() as i64;
        let disk_mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);

        // 3. Query DB for matching document
        let mut stmt = db
            .prepare(
                "SELECT file_size, modified_at FROM pdf_documents
                 WHERE vault_relative_path = ?1",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query([vault_relative_path])
            .map_err(|e| e.to_string())?;

        let mut matched = false;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let db_size: i64 = row.get(0).map_err(|e| e.to_string())?;
            let db_mtime: Option<i64> = row.get(1).map_err(|e| e.to_string())?;
            if db_size == disk_size && db_mtime == disk_mtime {
                matched = true;
                break;
            }
        }

        if matched {
            // Check if there actually are cached pages
            let mut count_stmt = db
                .prepare("SELECT COUNT(*) FROM pdf_text_search WHERE path = ?1")
                .map_err(|e| e.to_string())?;
            let count: i64 = count_stmt
                .query_row([vault_relative_path], |r| r.get(0))
                .unwrap_or(0);
            if count > 0 {
                return Ok(true);
            } else {
                return Ok(false);
            }
        }

        // Cache is stale. Invalidate.
        db.execute(
            "DELETE FROM pdf_text_search WHERE path = ?1",
            rusqlite::params![vault_relative_path],
        )
        .map_err(|e| e.to_string())?;
        db.execute(
            "DELETE FROM pdf_documents WHERE vault_relative_path = ?1",
            rusqlite::params![vault_relative_path],
        )
        .map_err(|e| e.to_string())?;

        Ok(false)
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

    pub fn save_pdf_page_text(
        &self,
        vault_relative_path: &str,
        page_index: u16,
        content: &str,
    ) -> Result<(), String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        db.execute(
            "DELETE FROM pdf_text_search WHERE path = ?1 AND page_index = ?2",
            rusqlite::params![vault_relative_path, page_index as i64],
        )
        .map_err(|e| format!("Failed to clear cached PDF text: {e}"))?;
        db.execute(
            "INSERT INTO pdf_text_search (path, page_index, content) VALUES (?1, ?2, ?3)",
            rusqlite::params![vault_relative_path, page_index as i64, content],
        )
        .map_err(|e| format!("Failed to cache PDF text: {e}"))?;
        Ok(())
    }

    pub fn search_cached_pdf_text(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<CachedPdfTextHit>, String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        let fts_query = format!("*{}*", query.replace('"', ""));
        let mut stmt = db
            .prepare(
                "SELECT path, page_index, content
                 FROM pdf_text_search
                 WHERE content MATCH ?1
                 LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![fts_query, limit as i64], |row| {
                Ok(CachedPdfTextHit {
                    vault_path: row.get(0)?,
                    page_index: row.get(1)?,
                    content: row.get(2)?,
                })
            })
            .map_err(|e| e.to_string())?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn pdf_document_references(&self) -> Result<Vec<PdfDocumentReference>, String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        let mut stmt = db
            .prepare("SELECT document_id, vault_relative_path FROM pdf_documents")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(PdfDocumentReference {
                    document_id: row.get(0)?,
                    vault_path: row.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn linked_pdf_annotation_references(
        &self,
    ) -> Result<Vec<LinkedPdfAnnotationReference>, String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        let mut stmt = db
            .prepare(
                "SELECT id, document_id, page_index, linked_note_path
                 FROM pdf_annotations
                 WHERE linked_note_path IS NOT NULL AND linked_note_path != ''",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(LinkedPdfAnnotationReference {
                    annotation_id: row.get(0)?,
                    document_id: row.get(1)?,
                    page_index: row.get(2)?,
                    linked_note_path: row.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn pdf_annotation_exists(&self, annotation_id: &str) -> Result<bool, String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        let mut stmt = db
            .prepare("SELECT 1 FROM pdf_annotations WHERE id = ?1")
            .map_err(|e| e.to_string())?;
        stmt.exists([annotation_id]).map_err(|e| e.to_string())
    }

    pub fn save_pdf_annotation(&self, ann: &PdfAnnotation) -> Result<(), String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        let ranges_json = serde_json::to_string(&ann.ranges)
            .map_err(|e| format!("Failed to serialize ranges: {e}"))?;
        let rects_json = serde_json::to_string(&ann.rects)
            .map_err(|e| format!("Failed to serialize rects: {e}"))?;
        let tags_json = serde_json::to_string(&ann.tags)
            .map_err(|e| format!("Failed to serialize tags: {e}"))?;

        db.execute(
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
                tags_json,
                ann.status.as_str(),
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
            let tags_json: String = row.get(13).map_err(|e| e.to_string())?;
            let status_str: String = row.get(14).map_err(|e| e.to_string())?;

            let kind = kind_str.parse::<PdfAnnotationKind>()?;
            let color = color_str.parse::<PdfAnnotationColor>()?;
            let ranges: Vec<PdfTextRange> = serde_json::from_str(&ranges_json)
                .map_err(|e| format!("Failed to parse ranges JSON: {e}"))?;
            let rects: Vec<PdfRect> = serde_json::from_str(&rects_json)
                .map_err(|e| format!("Failed to parse rects JSON: {e}"))?;
            let tags: Vec<String> = serde_json::from_str(&tags_json)
                .map_err(|e| format!("Failed to parse tags JSON: {e}"))?;
            let status = status_str.parse::<PdfAnnotationStatus>()?;

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
                tags,
                status,
                created_at,
                updated_at,
            });
        }

        Ok(annotations)
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

fn settings_db_path() -> PathBuf {
    let mut dir = config_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("Failed to create config directory {}: {err}", dir.display());
        return PathBuf::from("md_editor_settings.sqlite");
    }
    dir.push("md_editor_settings.sqlite");
    dir
}

fn config_dir() -> PathBuf {
    let exe = std::env::current_exe().ok();
    let project_config = directories::ProjectDirs::from("com", "Suranjan77", "md-editor")
        .map(|dirs| dirs.config_dir().to_path_buf());

    config_dir_for(exe.as_deref(), project_config)
}

fn config_dir_for(exe_path: Option<&std::path::Path>, project_config: Option<PathBuf>) -> PathBuf {
    if let Some(exe_path) = exe_path {
        for portable_dir in portable_config_dirs(exe_path) {
            let flag = portable_dir.join("portable.flag");
            let db = portable_dir.join("md_editor_settings.sqlite");
            if flag.exists() || db.exists() {
                return portable_dir;
            }
        }
    }

    project_config
        .or_else(|| {
            exe_path
                .and_then(std::path::Path::parent)
                .map(std::path::Path::to_path_buf)
        })
        .unwrap_or_else(|| PathBuf::from("."))
}

fn portable_config_dirs(exe_path: &std::path::Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let Some(exe_dir) = exe_path.parent() else {
        return dirs;
    };

    dirs.push(exe_dir.to_path_buf());

    if exe_dir.file_name().is_some_and(|name| name == "MacOS")
        && exe_dir
            .parent()
            .and_then(std::path::Path::file_name)
            .is_some_and(|name| name == "Contents")
        && let Some(package_dir) = exe_dir
            .parent()
            .and_then(std::path::Path::parent)
            .and_then(std::path::Path::parent)
    {
        dirs.push(package_dir.to_path_buf());
    }

    dirs
}

#[cfg(test)]
mod tests {
    use super::{config_dir_for, portable_config_dirs};

    fn unique_temp_dir(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("md_editor_state_{name}_{nanos}"))
    }

    #[test]
    fn config_dir_uses_platform_directory_without_portable_marker() {
        let root = unique_temp_dir("platform");
        let exe_dir = root.join("bin");
        let platform_dir = root.join("config");
        std::fs::create_dir_all(&exe_dir).expect("test executable directory should exist");
        let exe = exe_dir.join("md-editor");

        assert_eq!(
            config_dir_for(Some(&exe), Some(platform_dir.clone())),
            platform_dir
        );

        std::fs::remove_dir_all(root).expect("test directory should be removable");
    }

    #[test]
    fn config_dir_uses_executable_directory_for_portable_flag_or_existing_db() {
        for marker in ["portable.flag", "md_editor_settings.sqlite"] {
            let root = unique_temp_dir(marker);
            let exe_dir = root.join("portable");
            std::fs::create_dir_all(&exe_dir).expect("test executable directory should exist");
            std::fs::write(exe_dir.join(marker), []).expect("portable marker should be writable");
            let exe = exe_dir.join("md-editor");

            assert_eq!(
                config_dir_for(Some(&exe), Some(root.join("config"))),
                exe_dir
            );

            std::fs::remove_dir_all(root).expect("test directory should be removable");
        }
    }

    #[test]
    fn macos_bundle_uses_marker_beside_app_without_mutating_bundle() {
        let root = unique_temp_dir("macos_bundle");
        let exe_dir = root.join("MD Editor.app").join("Contents").join("MacOS");
        std::fs::create_dir_all(&exe_dir).expect("bundle executable directory should exist");
        std::fs::write(root.join("portable.flag"), []).expect("portable marker should be writable");
        let exe = exe_dir.join("md-editor");

        assert_eq!(
            portable_config_dirs(&exe),
            [exe_dir, root.clone()].map(std::path::PathBuf::from)
        );
        assert_eq!(
            config_dir_for(Some(&exe), Some(root.join("platform-config"))),
            root
        );
    }
}

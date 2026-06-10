use std::path::PathBuf;
use std::sync::Mutex;

use crate::application::pdf_service::PdfRenderer;
use crate::database::pdf_repository::{PdfAnnotationRepository, PdfDocumentRepository};
use crate::database::{Database, DatabaseError};
use crate::domain::pdf::PdfAnnotation;
use crate::infrastructure::indexer::FileIndex;
/// Application-wide shared state.
/// Wrap in `Arc<AppState>` when sharing across threads.
pub struct AppState {
    pub(crate) vault_root: Mutex<Option<PathBuf>>,
    pub(crate) file_index: Mutex<FileIndex>,
    pub(crate) db: Mutex<Database>,
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

        let (Ok(first), Ok(second)) = (
            crate::vault::resolve_vault_path(vault_root, first),
            crate::vault::resolve_vault_path(vault_root, second),
        ) else {
            return false;
        };
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
        let abs_path = crate::vault::resolve_vault_path(&vault_root, vault_path)?;
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

    pub fn try_new() -> Result<Self, DatabaseError> {
        let db_path = crate::infrastructure::config_store::settings_db_path();
        let db = Database::open(&db_path)?;

        Ok(AppState {
            vault_root: Mutex::new(None),
            file_index: Mutex::new(FileIndex::new(PathBuf::new())),
            db: Mutex::new(db),
            pdf_renderer: PdfRenderer::new().ok(),
        })
    }

    pub fn try_new_in_memory() -> Result<Self, DatabaseError> {
        let db = Database::open_in_memory()?;
        Ok(AppState {
            vault_root: Mutex::new(None),
            file_index: Mutex::new(FileIndex::new(PathBuf::new())),
            db: Mutex::new(db),
            pdf_renderer: None,
        })
    }

    pub fn save_pdf_document(
        &self,
        document_id: &str,
        vault_relative_path: &str,
        file_size: u64,
        modified_at: Option<i64>,
    ) -> Result<(), String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        PdfDocumentRepository::new(&db).save(
            document_id,
            vault_relative_path,
            file_size,
            modified_at,
        )
    }

    pub fn validate_and_invalidate_pdf_cache(
        &self,
        vault_relative_path: &str,
    ) -> Result<bool, String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        let repository = PdfDocumentRepository::new(&db);

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
                repository.invalidate(vault_relative_path)?;
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
        let matched = repository
            .metadata(vault_relative_path)?
            .iter()
            .any(|(_, size, mtime)| *size == disk_size && *mtime == disk_mtime);

        if matched {
            return repository.has_cached_text(vault_relative_path);
        }

        repository.invalidate(vault_relative_path)?;
        Ok(false)
    }

    pub fn get_pdf_path_by_id(&self, document_id: &str) -> Result<Option<String>, String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        PdfDocumentRepository::new(&db).path_by_id(document_id)
    }

    pub fn save_pdf_page_text(
        &self,
        vault_relative_path: &str,
        page_index: u16,
        content: &str,
    ) -> Result<(), String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        PdfDocumentRepository::new(&db).save_page_text(vault_relative_path, page_index, content)
    }

    pub fn search_cached_pdf_text(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<CachedPdfTextHit>, String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        PdfDocumentRepository::new(&db).search_cached_text(query, limit)
    }

    pub fn pdf_document_references(&self) -> Result<Vec<PdfDocumentReference>, String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        PdfDocumentRepository::new(&db).references()
    }

    pub fn linked_pdf_annotation_references(
        &self,
    ) -> Result<Vec<LinkedPdfAnnotationReference>, String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        PdfAnnotationRepository::new(&db).linked_references()
    }

    pub fn pdf_annotation_exists(&self, annotation_id: &str) -> Result<bool, String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        PdfAnnotationRepository::new(&db).exists(annotation_id)
    }

    pub fn save_pdf_annotation(&self, ann: &PdfAnnotation) -> Result<(), String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        PdfAnnotationRepository::new(&db).save(ann)
    }

    pub fn delete_pdf_annotation(&self, id: &str) -> Result<(), String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        PdfAnnotationRepository::new(&db).delete(id)
    }

    pub fn get_pdf_annotations(
        &self,
        document_id: &str,
        page_index: Option<u16>,
    ) -> Result<Vec<PdfAnnotation>, String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        PdfAnnotationRepository::new(&db).list(document_id, page_index)
    }
}

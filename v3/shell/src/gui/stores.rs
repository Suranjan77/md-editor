use super::*;

impl Shell {
    pub(super) fn sidecar_path(&self) -> PathBuf {
        self.vault_root.join(".md3/sidecar.db")
    }

    pub(super) fn open_tracker_store(
        &self,
    ) -> Result<md3_vault::tracker::TrackerStore, md3_vault::error::VaultError> {
        md3_vault::tracker::TrackerStore::open(&self.tracker_db_path)
    }

    pub(super) fn ensure_index(&mut self) {
        if self.index.is_some() {
            return;
        }
        let opened = self
            .open_sidecar_dir()
            .and_then(|_| SearchIndex::open(&self.sidecar_path()))
            .or_else(|_| SearchIndex::open_in_memory());
        match opened {
            Ok(mut index) => {
                if let Err(error) = self.sync_index(&mut index) {
                    self.status = format!("index: {error}");
                }
                self.index = Some(index);
            }
            Err(error) => self.status = format!("index: {error}"),
        }
    }

    pub(super) fn ensure_annotations(&mut self) {
        if self.annotations.is_some() {
            return;
        }
        let opened = self
            .open_sidecar_dir()
            .and_then(|_| AnnotationStore::open(&self.sidecar_path()));
        match opened {
            Ok(store) => self.annotations = Some(store),
            Err(error) => self.status = format!("annotations unavailable: {error}"),
        }
    }

    pub(super) fn ensure_asset_sizes(&mut self) {
        if self.asset_sizes.is_some() {
            return;
        }
        self.asset_sizes = self
            .open_sidecar_dir()
            .and_then(|_| md3_vault::AssetSizeStore::open(&self.sidecar_path()))
            .ok();
    }

    pub(super) fn open_sidecar_dir(&self) -> Result<(), md3_vault::VaultError> {
        let dir = self.vault_root.join(".md3");
        std::fs::create_dir_all(&dir).map_err(|error| md3_vault::VaultError::io(&dir, error))
    }

    pub(super) fn sync_index(&self, index: &mut SearchIndex) -> Result<(), md3_vault::VaultError> {
        #[cfg(feature = "pdfium")]
        {
            if let Some(renderer) = pdf_view::renderer() {
                let extractor = PdfTextExtractor(renderer);
                index.sync_with(&self.vault_root, Some(&extractor))?;
                return Ok(());
            }
        }
        index.sync(&self.vault_root)?;
        Ok(())
    }
}

#[cfg(feature = "pdfium")]
struct PdfTextExtractor(&'static md3_pdf::render::PdfRenderer);

#[cfg(feature = "pdfium")]
impl md3_vault::TextExtractor for PdfTextExtractor {
    fn extract(&self, abs_path: &Path) -> Option<String> {
        let page_count = self.0.page_count(abs_path).ok()?;
        let mut text = String::new();
        for page_index in 0..u32::from(page_count) {
            text.push_str(&self.0.extract_text(abs_path, page_index).ok()?);
            text.push('\n');
        }
        Some(text)
    }
}

use super::*;

impl Shell {
    pub(super) fn schedule_open_pdf_work(&mut self) {
        let worker = self.pdf_worker.clone();
        let root = self.vault_root.clone();
        for session in self.sessions.pdf.values_mut() {
            let abs_path = root.join(&session.rel_path);
            pdf_view::ensure_tiles(session, &abs_path, worker.as_ref());
        }
    }

    pub(super) fn schedule_open_markdown_work(&mut self) {
        let Some(worker) = self.pdf_worker.clone() else {
            return;
        };
        self.ensure_asset_sizes();
        let root = self.vault_root.clone();
        let requests = self
            .sessions
            .md
            .values()
            .flat_map(|session| {
                let document = root.join(&session.rel_path);
                super::markdown_assets::jobs(session, &document)
                    .into_iter()
                    .map(move |job| (session.rel_path.clone(), document.clone(), job))
            })
            .collect::<Vec<_>>();

        let mut cached = std::collections::HashMap::<String, Vec<(String, f32, f32)>>::new();
        for (rel_path, document, job) in requests {
            let key = markdown_job_key(&job).to_string();
            if let Some((width, height)) = self
                .asset_sizes
                .as_ref()
                .and_then(|store| store.get(&rel_path, &key).ok().flatten())
            {
                cached
                    .entry(rel_path.clone())
                    .or_default()
                    .push((key.clone(), width, height));
            }
            if self
                .md_assets_pending
                .insert((document.clone(), key.clone()))
            {
                worker.submit(job);
            }
        }

        for (rel_path, dimensions) in cached {
            if let Some(session) = self
                .sessions
                .md
                .values_mut()
                .find(|session| session.rel_path == rel_path)
            {
                for (key, width, height) in dimensions {
                    super::markdown_assets::apply_dimensions(session, &key, width, height);
                }
                session.doc.remeasure();
            }
        }
    }

    pub(super) fn apply_pdf_worker_output(&mut self, output: worker::PdfJobOutput) {
        use worker::PdfJobOutput;

        match output {
            PdfJobOutput::MarkdownAsset {
                document,
                key,
                handle,
                width,
                height,
            } => {
                self.md_assets_pending
                    .remove(&(document.clone(), key.clone()));
                let root = self.vault_root.clone();
                let Some(session) = self
                    .sessions
                    .md
                    .values_mut()
                    .find(|session| root.join(&session.rel_path) == document)
                else {
                    return;
                };
                let previous = self
                    .asset_sizes
                    .as_ref()
                    .and_then(|store| store.get(&session.rel_path, &key).ok().flatten());
                super::markdown_assets::install_handle(session, &key, handle, width, height);
                if previous.is_none_or(|size| dimensions_changed(size, (width, height))) {
                    session.doc.remeasure();
                }
                if let Some(store) = self.asset_sizes.as_mut() {
                    let _ = store.put(&session.rel_path, &key, width, height);
                }
                return;
            }
            PdfJobOutput::MarkdownAssetFailed {
                document,
                key,
                error,
            } => {
                self.md_assets_pending.remove(&(document, key));
                self.status = format!("asset: {error}");
                return;
            }
            _ => {}
        }

        let path = match &output {
            PdfJobOutput::Tile { path, .. }
            | PdfJobOutput::TileFailed { path, .. }
            | PdfJobOutput::PageGlyphs { path, .. }
            | PdfJobOutput::PageLinks { path, .. } => path,
            PdfJobOutput::MarkdownAsset { .. } | PdfJobOutput::MarkdownAssetFailed { .. } => {
                return;
            }
        }
        .clone();
        let root = self.vault_root.clone();
        let Some(session) = self
            .sessions
            .pdf
            .values_mut()
            .find(|session| root.join(&session.rel_path) == path)
        else {
            return;
        };
        let mut refresh_find = false;
        match output {
            PdfJobOutput::Tile {
                key, handle, bytes, ..
            } => {
                session.tiles_in_flight.remove(&key);
                for evicted in session.cache.insert(key, bytes) {
                    session.tiles.remove(&evicted);
                }
                session.tiles.insert(key, handle);
            }
            PdfJobOutput::TileFailed { key, error, .. } => {
                session.tiles_in_flight.remove(&key);
                session.status = format!("render failed: {error}");
            }
            PdfJobOutput::PageGlyphs { page, chars, .. } => {
                session.chars_pending.remove(&page);
                session.chars.insert(page, chars);
                refresh_find = true;
            }
            PdfJobOutput::PageLinks { page, links, .. } => {
                session.links_pending.remove(&page);
                session.links.insert(page, links);
            }
            PdfJobOutput::MarkdownAsset { .. } | PdfJobOutput::MarkdownAssetFailed { .. } => {
                return;
            }
        }
        if refresh_find {
            self.refresh_open_pdf_find();
        }
    }
}

fn markdown_job_key(job: &worker::PdfJob) -> &str {
    match job {
        worker::PdfJob::MarkdownImage { key, .. } | worker::PdfJob::MarkdownMath { key, .. } => key,
        _ => "",
    }
}

fn dimensions_changed(previous: (f32, f32), current: (f32, f32)) -> bool {
    (previous.0 - current.0).abs() > f32::EPSILON || (previous.1 - current.1).abs() > f32::EPSILON
}

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

    pub(super) fn apply_pdf_worker_output(&mut self, output: worker::PdfJobOutput) {
        use worker::PdfJobOutput;

        let path = match &output {
            PdfJobOutput::Tile { path, .. }
            | PdfJobOutput::TileFailed { path, .. }
            | PdfJobOutput::PageGlyphs { path, .. }
            | PdfJobOutput::PageLinks { path, .. } => path,
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
        }
        if refresh_find {
            self.refresh_open_pdf_find();
        }
    }
}

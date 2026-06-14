use super::*;

impl Shell {
    pub(super) fn open_pdf_find(&mut self) {
        let root = self.vault_root.clone();
        let worker = self.pdf_worker.clone();
        let Some(session) = self.focused_pdf_mut() else {
            self.status = "find: no pdf focused".to_string();
            return;
        };
        if session.layout.is_none() {
            self.status = "find: pdf pages not loaded".to_string();
            return;
        }
        let abs = root.join(&session.rel_path);
        let page_count = session.page_count();
        let limit = if worker.is_some() {
            page_count
        } else {
            page_count.min(200)
        };
        for page_index in 0..limit as u32 {
            pdf_view::request_page_chars(session, &abs, page_index, worker.as_ref());
        }
        self.open_overlay(Overlay::PdfFind {
            input: String::new(),
            selected: 0,
            hits: Vec::new(),
        });
        if worker.is_some() && page_count > 0 {
            self.status = format!("find: loading text from {page_count} pages");
        } else if page_count > 200 {
            self.status = format!("find: searching first 200 of {page_count} pages");
        }
    }

    pub(super) fn refresh_open_pdf_find(&mut self) {
        let Some(Overlay::PdfFind { input, .. }) = self.overlay.as_ref() else {
            return;
        };
        let input = input.clone();
        let hits = self.pdf_find_hits(&input);
        if let Some(Overlay::PdfFind {
            selected,
            hits: current,
            ..
        }) = self.overlay.as_mut()
        {
            *selected = (*selected).min(hits.len().saturating_sub(1));
            *current = hits;
        }
    }

    pub(super) fn open_pdf_toc(&mut self) {
        let Some(session) = self.focused_pdf() else {
            self.status = "toc: no pdf focused".to_string();
            return;
        };
        if session.outline.is_empty() {
            self.status = "this pdf has no table of contents".to_string();
            return;
        }
        let entries = session
            .outline
            .iter()
            .map(|entry| {
                let indent = "  ".repeat(usize::from(entry.depth));
                (format!("{indent}{}", entry.title), entry.page)
            })
            .collect();
        self.open_overlay(Overlay::PdfToc {
            input: String::new(),
            selected: 0,
            entries,
        });
    }

    pub(super) fn pdf_nav_history(&mut self, back: bool) {
        let root = self.vault_root.clone();
        let worker = self.pdf_worker.clone();
        let Some(session) = self.focused_pdf_mut() else {
            self.status = "history: no pdf focused".to_string();
            return;
        };
        let moved = if back {
            session.nav_back()
        } else {
            session.nav_forward()
        };
        if !moved {
            self.status = format!(
                "nothing to go {}",
                if back { "back to" } else { "forward to" }
            );
            return;
        }
        let abs = root.join(&session.rel_path);
        pdf_view::ensure_tiles(session, &abs, worker.as_ref());
        self.sync_status();
    }

    pub(super) fn pdf_find_hits(&self, query: &str) -> Vec<PdfFindHit> {
        const CAP: usize = 100;
        let query = query.trim();
        let mut hits = Vec::new();
        if query.is_empty() {
            return hits;
        }
        let Some(session) = self.focused_pdf() else {
            return hits;
        };
        let mut pages: Vec<u32> = session.chars.keys().copied().collect();
        pages.sort_unstable();
        for page_index in pages {
            let Some(chars) = session.chars.get(&page_index) else {
                continue;
            };
            for range in md3_pdf::select::find(chars, query) {
                let Some(selection) = md3_pdf::select::range_selection(chars, range.clone()) else {
                    continue;
                };
                let context = range.start.saturating_sub(12)..(range.end + 28).min(chars.len());
                hits.push(PdfFindHit {
                    page: page_index,
                    quads: selection.quads,
                    text: selection.text,
                    preview: chars[context].iter().map(|char_box| char_box.ch).collect(),
                });
                if hits.len() >= CAP {
                    return hits;
                }
            }
        }
        hits
    }

    pub(super) fn jump_to_pdf_match(&mut self, hit: &PdfFindHit) {
        let root = self.vault_root.clone();
        let worker = self.pdf_worker.clone();
        let Some(session) = self.focused_pdf_mut() else {
            return;
        };
        let Some(target) = session.layout.as_ref().map(|layout| {
            let y = hit.quads.first().map_or(0.0, |quad| quad.y0);
            let top = layout.page_top(hit.page as usize) + y * layout.zoom();
            (top - session.viewport.1 / 3.0).clamp(0.0, layout.max_scroll(session.viewport.1))
        }) else {
            return;
        };
        session.record_jump();
        session.scroll = target;
        session.selected_annotation = None;
        session.selection = Some(PdfSelection {
            page: hit.page,
            anchor: hit
                .quads
                .first()
                .map_or((0.0, 0.0), |quad| (quad.x0, quad.y0)),
            quads: hit.quads.clone(),
            text: hit.text.clone(),
        });
        let abs = root.join(&session.rel_path);
        pdf_view::ensure_tiles(session, &abs, worker.as_ref());
        self.status = format!("match on p. {} · ctrl+h highlights", hit.page + 1);
    }
}

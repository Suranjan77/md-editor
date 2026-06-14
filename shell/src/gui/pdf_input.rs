use super::*;

impl Shell {
    /// Pick annotation/link under cursor, or anchor text selection.
    pub(super) fn pdf_mouse_down(&mut self, tab: TabId, pos: (f32, f32), viewport: (f32, f32)) {
        let root = self.vault_root.clone();
        let worker = self.pdf_worker.clone();
        let Some(session) = self
            .tab_document(tab)
            .and_then(|doc| self.sessions.pdf.get_mut(&doc))
        else {
            return;
        };
        session.viewport = viewport;
        let abs = root.join(&session.rel_path);
        pdf_view::ensure_tiles(session, &abs, worker.as_ref());
        let hit = session
            .layout
            .as_ref()
            .and_then(|layout| layout.page_at_point(session.scroll, viewport, pos));
        let Some((page_index, point)) = hit else {
            session.selection = None;
            session.selected_annotation = None;
            self.sync_status();
            return;
        };
        let page_index = page_index as u32;
        if let Some(link) = session.link_at(page_index, point) {
            if let Some((dest_page_index, dest_y)) = link.dest {
                session.record_jump();
                session.go_to_page(dest_page_index as usize);
                if let Some(y) = dest_y {
                    let zoom = session.zoom;
                    if let Some(layout) = &session.layout {
                        let max = layout.max_scroll(session.viewport.1);
                        let top = layout.page_top(dest_page_index as usize);
                        session.scroll = (top + y * zoom).clamp(0.0, max);
                    }
                }
                let abs = root.join(&session.rel_path);
                pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                self.status = format!("→ p. {} · alt+left returns", dest_page_index + 1);
                return;
            } else if let Some(uri) = &link.uri {
                self.status = format!("link: {uri}");
                let url = uri.clone();
                if std::env::var("MD3_TEST_MODE").is_err() {
                    std::thread::spawn(move || {
                        let _ = open::that(url);
                    });
                }
                return;
            }
        }
        let picked = session
            .annotation_at(page_index, point)
            .map(|annotation| (annotation.id, annotation.note.clone()));
        if let Some((id, note)) = picked {
            session.selected_annotation = Some(id);
            session.selection = None;
            self.status = if note.is_empty() {
                "highlight · ctrl+n adds a note · delete removes".to_string()
            } else {
                format!("note: {note} · ctrl+n edits · delete removes")
            };
            return;
        }
        session.selected_annotation = None;
        pdf_view::request_page_chars(session, &abs, page_index, worker.as_ref());
        session.selection = Some(PdfSelection {
            page: page_index,
            anchor: point,
            quads: Vec::new(),
            text: String::new(),
        });
        self.sync_status();
    }

    pub(super) fn pdf_right_click(
        &mut self,
        tab: TabId,
        pos: (f32, f32),
        abs_pos: (f32, f32),
        viewport: (f32, f32),
    ) {
        let root = self.vault_root.clone();
        let worker = self.pdf_worker.clone();
        let Some(session) = self
            .tab_document(tab)
            .and_then(|doc| self.sessions.pdf.get_mut(&doc))
        else {
            return;
        };
        session.viewport = viewport;
        let abs = root.join(&session.rel_path);
        pdf_view::ensure_tiles(session, &abs, worker.as_ref());
        let hit = session
            .layout
            .as_ref()
            .and_then(|layout| layout.page_at_point(session.scroll, viewport, pos));
        let Some((page_index, point)) = hit else {
            return;
        };
        let page_index = page_index as u32;

        let on_selection = session.selection.as_ref().is_some_and(|selection| {
            selection.page == page_index
                && selection.quads.iter().any(|quad| {
                    point.0 >= quad.x0
                        && point.0 <= quad.x1
                        && point.1 >= quad.y0
                        && point.1 <= quad.y1
                })
        });

        if on_selection {
            self.pdf_context_menu = Some(PdfContextMenuState { tab, abs_pos });
            self.ws.open_overlay("pdf-context-menu");
            return;
        }

        if let Some(link) = session.link_at(page_index, point) {
            if let Some(uri) = &link.uri {
                self.status = format!("link: {uri}");
            } else if let Some((dest_page_index, dest_y)) = link.dest {
                #[cfg(feature = "pdfium")]
                {
                    if let Some(renderer) = pdf_view::renderer() {
                        match renderer.render_link_preview(&abs, dest_page_index, dest_y) {
                            Ok(rendered) => {
                                let handle = iced::widget::image::Handle::from_rgba(
                                    rendered.width,
                                    rendered.height,
                                    rendered.rgba,
                                );
                                self.open_overlay(Overlay::PdfLinkPreview {
                                    dest_page: dest_page_index,
                                    dest_y,
                                    image: handle,
                                    width: rendered.width,
                                    height: rendered.height,
                                });
                            }
                            Err(error) => {
                                self.status = format!("preview render failed: {error}");
                            }
                        }
                    }
                }
                #[cfg(not(feature = "pdfium"))]
                {
                    let _ = dest_page_index;
                    let _ = dest_y;
                    self.status = "preview: built without pdfium".to_string();
                }
            }
        }
    }

    /// Extend selection from anchor to cursor in anchor page coordinates.
    pub(super) fn pdf_mouse_dragged(&mut self, tab: TabId, pos: (f32, f32), viewport: (f32, f32)) {
        let Some(session) = self
            .tab_document(tab)
            .and_then(|doc| self.sessions.pdf.get_mut(&doc))
        else {
            return;
        };
        let (Some(layout), Some(selection)) = (session.layout.as_ref(), session.selection.as_mut())
        else {
            return;
        };
        let head = layout.point_in_page(session.scroll, viewport, pos, selection.page as usize);
        let chars = session
            .chars
            .get(&selection.page)
            .map_or(&[][..], Vec::as_slice);
        match md_pdf::select::select(chars, selection.anchor, head) {
            Some(text_selection) => {
                selection.quads = text_selection.quads;
                selection.text = text_selection.text;
            }
            None => {
                selection.quads.clear();
                selection.text.clear();
            }
        }
    }
}

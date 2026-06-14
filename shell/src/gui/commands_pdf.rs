use super::*;

impl Shell {
    pub(super) fn run_pdf_command(&mut self, cmd: &str) -> Option<Task<Message>> {
        match cmd {
            "pdf.zoom-input" => self.open_overlay(Overlay::PdfZoom {
                input: String::new(),
            }),
            "pdf.zoom-in" => self.adjust_pdf_zoom(1.25),
            "pdf.zoom-out" => self.adjust_pdf_zoom(0.8),
            "pdf.go-to-page" => self.open_overlay(Overlay::PdfPage {
                input: String::new(),
            }),
            "pdf.previous-page" | "pdf.next-page" => {
                let root = self.vault_root.clone();
                let worker = self.pdf_worker.clone();
                if let Some(session) = self.focused_pdf_mut() {
                    session.record_jump();
                    let current = session.current_page();
                    let page = if cmd == "pdf.next-page" {
                        current.saturating_add(1)
                    } else {
                        current.saturating_sub(1)
                    };
                    session.go_to_page(page);
                    let abs = root.join(&session.rel_path);
                    super::pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                }
            }
            "pdf.find" => self.open_pdf_find(),
            "pdf.toc" => self.open_pdf_toc(),
            "pdf.back" | "pdf.forward" => {
                self.pdf_nav_history(cmd == "pdf.back");
            }
            "pdf.highlight" => self.highlight_selection(),
            "pdf.annotation-note" => {
                match self
                    .focused_pdf()
                    .and_then(super::session::PdfSession::selected_annotation)
                {
                    Some(a) => {
                        let input = a.note.clone();
                        self.open_overlay(Overlay::AnnotationNote { input });
                    }
                    None => self.status = "click a highlight first".to_string(),
                }
            }
            "pdf.annotations-export" => return Some(self.export_annotations()),
            "pdf.copy-selection" => {
                let text = self
                    .focused_pdf()
                    .and_then(|s| s.selection.as_ref())
                    .map(|sel| sel.text.clone())
                    .filter(|t| !t.is_empty());
                return match text {
                    Some(text) => {
                        self.status = format!("{} chars copied", text.chars().count());
                        Some(iced::clipboard::write(text))
                    }
                    None => {
                        self.status = "select text first (drag over the page)".to_string();
                        Some(Task::none())
                    }
                };
            }
            "pdf.highlight-color" => {
                self.cycle_highlight_color();
                return Some(Task::none());
            }
            "pdf.annotation-link-note" => {
                self.link_note_for_annotation();
                return Some(Task::none());
            }
            "pdf.annotations-orphans" => {
                self.orphan_report();
                return Some(Task::none());
            }
            "pdf.fit-width" => {
                let root = self.vault_root.clone();
                let worker = self.pdf_worker.clone();
                if let Some(session) = self.focused_pdf_mut() {
                    session.set_fit_mode(super::session::PdfFitMode::Width);
                    let abs = root.join(&session.rel_path);
                    super::pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                }
                self.save_session();
            }
            "pdf.fit-page" => {
                let root = self.vault_root.clone();
                let worker = self.pdf_worker.clone();
                if let Some(session) = self.focused_pdf_mut() {
                    session.set_fit_mode(super::session::PdfFitMode::Page);
                    let abs = root.join(&session.rel_path);
                    super::pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                }
                self.save_session();
            }
            "pdf.toc-panel" => {
                if let Some(session) = self.focused_pdf_mut() {
                    session.toc_open = !session.toc_open;
                    self.save_session();
                }
            }
            "pdf.annotations-panel" => {
                if let Some(session) = self.focused_pdf_mut() {
                    session.annotations_open = !session.annotations_open;
                    self.save_session();
                }
            }
            "pdf.highlight-and-note" => {
                self.highlight_selection();
                if let Some(session) = self.focused_pdf()
                    && let Some(a) = session.selected_annotation()
                {
                    let input = a.note.clone();
                    self.open_overlay(Overlay::AnnotationNote { input });
                }
            }

            _ => return None,
        }
        Some(Task::none())
    }
}

impl Shell {
    pub(super) fn handle_pdf_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PdfViewportChanged { tab, width, height } => {
                let root = self.vault_root.clone();
                let worker = self.pdf_worker.clone();
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.pdf.get_mut(&d)) {
                    session.set_viewport((width, height));
                    let abs = root.join(&session.rel_path);
                    pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                }
                Task::none()
            }
            Message::PdfScrolled { tab, dy, viewport } => {
                if let Err(e) = self.ws.focus_tab(tab) {
                    self.status = e.to_string();
                }
                let root = self.vault_root.clone();
                let worker = self.pdf_worker.clone();
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.pdf.get_mut(&d)) {
                    session.viewport = viewport;
                    session.scroll_by(dy);
                    let abs = root.join(&session.rel_path);
                    pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                }
                self.sync_status();
                Task::none()
            }
            Message::PdfMouseDown { tab, pos, viewport } => {
                if let Err(e) = self.ws.focus_tab(tab) {
                    self.status = e.to_string();
                }
                self.pdf_mouse_down(tab, pos, viewport);
                Task::none()
            }
            Message::PdfRightClick {
                tab,
                pos,
                abs_pos,
                viewport,
            } => {
                if let Err(e) = self.ws.focus_tab(tab) {
                    self.status = e.to_string();
                }
                self.pdf_right_click(tab, pos, abs_pos, viewport);
                Task::none()
            }
            Message::PdfJumpToPage { tab, page } => {
                if let Err(error) = self.ws.focus_tab(tab) {
                    self.status = error.to_string();
                    return Task::none();
                }
                let root = self.vault_root.clone();
                let worker = self.pdf_worker.clone();
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.pdf.get_mut(&d)) {
                    session.record_jump();
                    session.go_to_page(page);
                    let abs = root.join(&session.rel_path);
                    pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                }
                self.sync_status();
                Task::none()
            }
            Message::PdfJumpToAnnotation { tab, annotation_id } => {
                if let Err(error) = self.ws.focus_tab(tab) {
                    self.status = error.to_string();
                    return Task::none();
                }
                let root = self.vault_root.clone();
                let worker = self.pdf_worker.clone();
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.pdf.get_mut(&d)) {
                    session.selected_annotation = Some(annotation_id);
                    if let Some(ann) = session.selected_annotation() {
                        let target = session.layout.as_ref().map(|layout| {
                            let y = ann.quads.first().map_or(0.0, |q| q.y0 as f32);
                            let top = layout.page_top(ann.page as usize) + y * layout.zoom();
                            (top - session.viewport.1 / 3.0)
                                .clamp(0.0, layout.max_scroll(session.viewport.1))
                        });
                        if let Some(scroll) = target {
                            session.record_jump();
                            session.scroll = scroll;
                            let abs = root.join(&session.rel_path);
                            pdf_view::ensure_tiles(session, &abs, worker.as_ref());
                        }
                    }
                }
                self.sync_status();
                Task::none()
            }
            Message::PdfDeleteAnnotation { tab, annotation_id } => {
                if let Err(error) = self.ws.focus_tab(tab) {
                    self.status = error.to_string();
                    return Task::none();
                }
                self.ensure_annotations();
                let doc = self.tab_document(tab);
                if let (Some(store), Some(session)) = (
                    self.annotations.as_mut(),
                    doc.and_then(|d| self.sessions.pdf.get_mut(&d)),
                ) {
                    if session.selected_annotation == Some(annotation_id) {
                        session.selected_annotation = None;
                    }
                    match store.remove(annotation_id) {
                        Ok(()) => {
                            refresh_annotations(store, session);
                            self.status = "highlight removed".to_string();
                        }
                        Err(e) => self.status = format!("remove failed: {e}"),
                    }
                }
                Task::none()
            }
            Message::PdfEditAnnotationNote { tab, annotation_id } => {
                if let Err(error) = self.ws.focus_tab(tab) {
                    self.status = error.to_string();
                    return Task::none();
                }
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.pdf.get_mut(&d)) {
                    session.selected_annotation = Some(annotation_id);
                    if let Some(ann) = session.selected_annotation() {
                        let input = ann.note.clone();
                        self.open_overlay(Overlay::AnnotationNote { input });
                    }
                }
                Task::none()
            }
            Message::PdfCycleAnnotationColor { tab, annotation_id } => {
                if let Err(error) = self.ws.focus_tab(tab) {
                    self.status = error.to_string();
                    return Task::none();
                }
                self.ensure_annotations();
                let doc = self.tab_document(tab);
                if let (Some(store), Some(session)) = (
                    self.annotations.as_mut(),
                    doc.and_then(|d| self.sessions.pdf.get_mut(&d)),
                ) {
                    session.selected_annotation = Some(annotation_id);
                    if let Some(current) = session.selected_annotation() {
                        let next = match current.color.as_str() {
                            "#f1c40f" => "#e74c3c", // yellow -> red
                            "#e74c3c" => "#2ecc71", // red -> green
                            "#2ecc71" => "#3498db", // green -> blue
                            _ => "#f1c40f",         // blue (or other) -> yellow
                        };
                        match store.set_color(current.id, next) {
                            Ok(()) => {
                                refresh_annotations(store, session);
                                self.status = "color updated".to_string();
                            }
                            Err(e) => self.status = format!("update failed: {e}"),
                        }
                    }
                }
                Task::none()
            }
            Message::PdfContextMenuClosed => {
                self.pdf_context_menu = None;
                Task::none()
            }
            Message::PdfContextMenuCommand { tab, command } => {
                self.pdf_context_menu = None;
                if let Err(error) = self.ws.focus_tab(tab) {
                    self.status = error.to_string();
                    return Task::none();
                }
                self.run_command(command)
            }
            Message::PdfCommand { tab, command } => {
                if let Err(error) = self.ws.focus_tab(tab) {
                    self.status = error.to_string();
                    return Task::none();
                }
                self.run_command(command)
            }
            Message::PdfMouseDragged { tab, pos, viewport } => {
                self.pdf_mouse_dragged(tab, pos, viewport);
                Task::none()
            }
            Message::PdfMouseUp { tab } => {
                let doc = self.tab_document(tab);
                if let Some(session) = doc.and_then(|d| self.sessions.pdf.get_mut(&d)) {
                    match &session.selection {
                        Some(sel) if !sel.text.is_empty() => {
                            self.status = format!(
                                "{} chars selected · ctrl+h highlights",
                                sel.text.chars().count()
                            );
                        }
                        _ => session.selection = None,
                    }
                }
                Task::none()
            }
            Message::PdfWorkerReady(handle) => {
                self.pdf_worker = Some(handle);
                self.schedule_open_pdf_work();
                self.schedule_open_markdown_work();
                Task::none()
            }
            Message::PdfWorker(output) => {
                self.apply_pdf_worker_output(output);
                Task::none()
            }

            _ => Task::none(),
        }
    }
}

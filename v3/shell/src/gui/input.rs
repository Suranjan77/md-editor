use super::*;

impl Shell {
    /// Single keystroke entry point.
    pub(super) fn on_key(&mut self, event: keys::KeyEvent) -> Task<Message> {
        if let Some(chord) = event.chord
            && let Some(command) = self.ws.handle_key(&self.keymap, chord)
        {
            return self.run_command(command);
        }
        self.status.clear();
        if self.overlay.is_some() {
            return self.overlay_raw_input(&event);
        }
        match self.ws.focused_editor_kind() {
            Some(EditorKind::Markdown) => self.editor_raw_input(&event),
            Some(EditorKind::Pdf) => self.pdf_raw_input(&event),
            _ => {}
        }
        self.sync_status();
        Task::none()
    }

    fn editor_raw_input(&mut self, event: &keys::KeyEvent) {
        let Some(session) = self.focused_md_mut() else {
            return;
        };
        let command = match event.chord {
            Some(Chord { mods, key: _ }) if mods.ctrl || mods.alt || mods.meta => None,
            Some(Chord { mods, key }) => {
                let extend = mods.shift;
                match key {
                    Key::Enter => Some(Command::Insert("\n".to_string())),
                    Key::Tab => Some(Command::TableTab { backward: extend }),
                    Key::Backspace => Some(Command::DeleteBackward),
                    Key::Delete => Some(Command::DeleteForward),
                    Key::Up => Some(Command::Move {
                        movement: Movement::Up,
                        extend,
                    }),
                    Key::Down => Some(Command::Move {
                        movement: Movement::Down,
                        extend,
                    }),
                    Key::Left => Some(Command::Move {
                        movement: Movement::Left,
                        extend,
                    }),
                    Key::Right => Some(Command::Move {
                        movement: Movement::Right,
                        extend,
                    }),
                    Key::Home => Some(Command::Move {
                        movement: Movement::Home,
                        extend,
                    }),
                    Key::End => Some(Command::Move {
                        movement: Movement::End,
                        extend,
                    }),
                    Key::PageUp | Key::PageDown => {
                        let direction = if key == Key::PageUp { -0.9 } else { 0.9 };
                        session.scroll_by(session.viewport_h * direction);
                        None
                    }
                    _ => event.text.clone().map(Command::Insert),
                }
            }
            None => event.text.clone().map(Command::Insert),
        };
        if let Some(command) = command {
            session.apply(command);
        }
    }

    fn pdf_raw_input(&mut self, event: &keys::KeyEvent) {
        if event.chord.map(|chord| chord.key) == Some(Key::Delete) {
            self.remove_selected_annotation();
            return;
        }
        let root = self.vault_root.clone();
        let worker = self.pdf_worker.clone();
        let Some(session) = self.focused_pdf_mut() else {
            return;
        };
        let screen = session.viewport.1 * 0.9;
        match event.chord.map(|chord| chord.key) {
            Some(Key::PageDown) => session.scroll_by(screen),
            Some(Key::PageUp) => session.scroll_by(-screen),
            Some(Key::Down) => session.scroll_by(60.0),
            Some(Key::Up) => session.scroll_by(-60.0),
            Some(Key::Right) => session.go_to_page(session.current_page() + 1),
            Some(Key::Left) => session.go_to_page(session.current_page().saturating_sub(1)),
            Some(Key::Home) => session.go_to_page(0),
            Some(Key::End) => session.scroll_by(f32::MAX),
            _ => return,
        }
        let abs = root.join(&session.rel_path);
        pdf_view::ensure_tiles(session, &abs, worker.as_ref());
    }

    fn overlay_raw_input(&mut self, event: &keys::KeyEvent) -> Task<Message> {
        let Some(overlay) = self.overlay.as_mut() else {
            return Task::none();
        };
        match event.chord.map(|chord| chord.key) {
            Some(Key::Backspace) => {
                if let Some(input) = overlay.input_mut() {
                    input.pop();
                }
            }
            Some(Key::Up) => {
                if let Some(selected) = overlay.selected_mut() {
                    *selected = selected.saturating_sub(1);
                }
            }
            Some(Key::Down) => {
                if let Some(selected) = overlay.selected_mut() {
                    *selected += 1;
                }
            }
            _ => {
                if let Some(text) = &event.text
                    && let Some(input) = overlay.input_mut()
                {
                    input.push_str(text);
                }
            }
        }
        let search_query = match self.overlay.as_ref() {
            Some(Overlay::Search { input, .. }) => Some(input.clone()),
            _ => None,
        };
        if let Some(query) = search_query {
            let new_hits = self.search_vault(&query);
            if let Some(Overlay::Search { hits, selected, .. }) = self.overlay.as_mut() {
                *selected = (*selected).min(new_hits.len().saturating_sub(1));
                *hits = new_hits;
            }
        }
        let pdf_query = match self.overlay.as_ref() {
            Some(Overlay::PdfFind { input, .. }) => Some(input.clone()),
            _ => None,
        };
        if let Some(query) = pdf_query {
            let new_hits = self.pdf_find_hits(&query);
            if let Some(Overlay::PdfFind { hits, selected, .. }) = self.overlay.as_mut() {
                *selected = (*selected).min(new_hits.len().saturating_sub(1));
                *hits = new_hits;
            }
        }
        let rows = match self.overlay.as_ref() {
            Some(overlay) => overlay::list_rows(overlay, &self.registry, &self.files).len(),
            None => 0,
        };
        if let Some(selected) = self.overlay.as_mut().and_then(Overlay::selected_mut) {
            *selected = (*selected).min(rows.saturating_sub(1));
            return overlay::snap_selected(rows, *selected);
        }
        Task::none()
    }
}

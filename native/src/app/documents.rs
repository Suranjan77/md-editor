//! Opening, saving and parking documents (notes, PDFs, decks, images) and session persistence.

use super::*;
use crate::messages::ShellMessage;

impl MdEditor {
    pub(super) fn open_file(&mut self, path: &str) -> Task<Message> {
        self.open_file_extended(path, true)
    }

    /// Write the active markdown document to disk if it has unsaved edits.
    ///
    /// The single funnel for every write: `Ctrl+S`, the autosave debounce, and
    /// the flush before a file switch or quit all land here, so durability is
    /// decided in one place rather than at each call site.
    pub(super) fn save_active_document(&mut self) -> SaveOutcome {
        if !self.editor.buffer.dirty {
            self.editor.autosave_pending_since = None;
            return SaveOutcome::NothingToDo;
        }

        // No document is open (an image or PDF is showing), so there is nothing
        // this buffer belongs to. Disarm, or the autosave subscription would
        // poll forever on a pending flag that can never be satisfied.
        let Some(path) = self.active_path.clone() else {
            self.editor.autosave_pending_since = None;
            return SaveOutcome::NothingToDo;
        };

        let content = self.editor.buffer.text();
        let stamp = chrono::Local::now().format("%Y-%m-%d %H-%M-%S").to_string();
        match md_editor_core::vault::save_file_guarded(
            &self.state,
            &path,
            &content,
            self.editor.disk_hash,
            &stamp,
        ) {
            Ok(conflict) => {
                self.editor.disk_hash = Some(md_editor_core::vault::content_hash(&content));
                if let Some(copy) = conflict {
                    self.note_self_write(&copy);
                    self.vault.entries =
                        md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                    self.ui.toast = Some(format!(
                        "{path} was changed on disk while you were editing. Your version is saved; the other is in {copy}"
                    ));
                }
                self.editor.buffer.mark_saved();
                self.editor.autosave_pending_since = None;
                self.invalidate_graph();
                self.autosave_failure_reported = false;
                self.note_self_write(&path);
                self.persist_document_position();
                SaveOutcome::Saved
            }
            Err(err) => {
                // Leave `dirty` set — the edits are still only in memory, so
                // the title keeps its unsaved marker. Restart the debounce
                // rather than leaving it elapsed: otherwise every poll tick
                // would retry a failing write (a full create/write/fsync/rename
                // attempt) ten times a second for the rest of the session.
                self.editor.mark_dirty_now();
                // Once per failure run, like the toast: retries are silent.
                if !self.autosave_failure_reported {
                    log::warn!("Saving {path} failed: {err}");
                }
                SaveOutcome::Failed(err)
            }
        }
    }

    /// Save and park the active document before something replaces it.
    ///
    /// Every path that changes what the editor is showing — opening another
    /// note, an image, or clearing the pane — goes through here, so unsaved
    /// edits and undo history cannot be dropped by a code path that forgot to
    /// think about them. Returns the write error if the document could not be
    /// persisted, in which case the caller must not proceed.
    pub(super) fn leave_active_document(&mut self) -> Result<(), String> {
        if let SaveOutcome::Failed(err) = self.save_active_document() {
            return Err(err.to_string());
        }
        self.park_active_buffer();
        Ok(())
    }

    /// Remember that we just wrote this path, so the vault watcher can ignore
    /// the change notification our own save is about to produce.
    pub(super) fn note_self_write(&mut self, path: &str) {
        let now = std::time::Instant::now();
        self.self_writes
            .retain(|_, at| now.duration_since(*at) < SELF_WRITE_GRACE);
        self.self_writes.insert(path.to_string(), now);
    }

    /// Whether `path` was written by this app moments ago.
    pub(super) fn is_recent_self_write(&self, path: &str) -> bool {
        self.self_writes
            .get(path)
            .is_some_and(|at| at.elapsed() < SELF_WRITE_GRACE)
    }

    /// Record where this session left off: window geometry, and the scroll and
    /// cursor position of the open document.
    ///
    /// Written once on exit rather than on every resize or scroll event, so
    /// dragging a window edge or flicking through a long note does not turn
    /// into a stream of database writes.
    pub(super) fn persist_session(&self) {
        self.persist_window_size();
        self.persist_document_position();
    }

    /// Record the window geometry. Called as the window resizes rather than
    /// only at exit, because the app does not intercept its own close and so
    /// cannot rely on running code on the way out.
    pub(super) fn persist_window_size(&self) {
        let _ = md_editor_core::config::set_sys_config(
            &self.state,
            "window_size",
            &format!("{}x{}", self.ui.window_width, self.ui.window_height),
        );
    }

    /// Record where the reader is in the active document. Rides along with
    /// each save, so it is written on the same schedule as the content it
    /// describes.
    pub(super) fn persist_document_position(&self) {
        let Some(path) = self.active_path.as_deref() else {
            return;
        };
        let _ = md_editor_core::config::set_sys_config(
            &self.state,
            &format!("scroll:{path}"),
            &self.editor.scroll_y.to_string(),
        );
        let _ = md_editor_core::config::set_sys_config(
            &self.state,
            &format!("cursor:{path}"),
            &self.editor.buffer.cursor_offset().to_string(),
        );
    }

    /// Scroll offset stored for `path` by a previous session, if any.
    pub(super) fn stored_scroll_for(&self, path: &str) -> Option<f32> {
        md_editor_core::config::get_sys_config(&self.state, &format!("scroll:{path}"))
            .ok()
            .flatten()
            .and_then(|raw| raw.parse::<f32>().ok())
            .filter(|y| y.is_finite() && *y >= 0.0)
    }

    /// Cursor offset stored for `path` by a previous session, if any.
    pub(super) fn stored_cursor_for(&self, path: &str) -> Option<usize> {
        md_editor_core::config::get_sys_config(&self.state, &format!("cursor:{path}"))
            .ok()
            .flatten()
            .and_then(|raw| raw.parse::<usize>().ok())
    }

    /// Move the active buffer into the retained registry so a later visit to
    /// the same file resumes its undo history.
    pub(super) fn park_active_buffer(&mut self) {
        let Some(path) = self.active_path.clone() else {
            return;
        };
        let outgoing = std::mem::replace(&mut self.editor.buffer, DocBuffer::new());
        let scroll_y = self.editor.scroll_y;
        self.editor.retain_buffer(path, outgoing, scroll_y);
    }

    /// Resolve an internal link target (wikilink / relative markdown link) to a
    /// vault file path. Tries the existing relative-path resolution first, then
    /// falls back to a vault-wide basename lookup so `[[NoteName]]` resolves to
    /// a note living in any subfolder.
    pub(super) fn resolve_internal_link_path(&self, link_path: &str) -> String {
        let mut resolved = resolve_relative_link_path(
            self.vault.root.as_deref(),
            self.active_path.as_deref(),
            link_path,
        );
        if !md_editor_core::vault::is_supported_vault_path(std::path::Path::new(&resolved)) {
            resolved.push_str(".md");
        }
        let exists = self
            .vault
            .root
            .as_deref()
            .map(|root| std::path::Path::new(root).join(&resolved).exists())
            .unwrap_or(false);
        if !exists && let Some(found) = resolve_vault_note_by_name(&self.vault.entries, link_path) {
            return found;
        }
        resolved
    }

    pub(super) fn open_file_extended(&mut self, path: &str, reset_scroll: bool) -> Task<Message> {
        let is_different = self.active_path.as_deref() != Some(path);

        // Read the incoming file *first*. Parking the outgoing buffer before
        // knowing the open will succeed would leave an empty buffer bound to
        // the still-active path, and the next autosave would write that empty
        // buffer over the previous note.
        let incoming = match md_editor_core::vault::open_file(&self.state, path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(content) => content,
                Err(_) => {
                    self.ui.toast = Some(format!(
                        "Cannot open {path}: not valid UTF-8 text (unsupported encoding?)"
                    ));
                    return Task::none();
                }
            },
            Err(err) => {
                self.ui.toast = Some(format!("Cannot open {path}: {err}"));
                return Task::none();
            }
        };

        // Only now is it safe to flush and park what is currently open.
        // Without this, switching files silently discarded both the unsaved
        // edits and the undo stack that could have recovered them.
        if is_different && let Err(err) = self.leave_active_document() {
            // Refuse to navigate away from work we could not persist.
            self.ui.toast = Some(format!(
                "Not switching files: {err}. Your edits are still open here."
            ));
            return Task::none();
        }

        let content = incoming;
        self.editor.disk_hash = Some(md_editor_core::vault::content_hash(&content));
        // Resume the parked buffer when it still matches disk, so
        // undo history and cursor survive a round trip; otherwise
        // the file changed underneath us and a fresh buffer is the
        // only honest option.
        let resumed = self.editor.take_retained_matching(path, &content);
        let resumed_scroll = resumed.as_ref().map(|(_, y)| *y);
        let is_resumed = resumed.is_some();
        self.editor.buffer = resumed
            .map(|(buffer, _)| buffer)
            .unwrap_or_else(|| DocBuffer::from_text(&content));

        // A buffer built fresh from disk starts at the top. If a
        // previous session left a cursor here, put it back.
        if !is_resumed && let Some(offset) = self.stored_cursor_for(path) {
            self.editor.buffer.set_cursor_offset(offset);
        }
        self.editor.autosave_pending_since = None;
        self.editor.buffer_revision = self.editor.buffer_revision.wrapping_add(1);
        self.active_path = Some(path.to_string());
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
        self.active_image_path = None;
        self.active_image = None;
        self.pptx.close();
        self.showing_pdf = false;
        self.active_panel = ActivePanel::Markdown;
        self.editor.toc_entries = views::toc::get_toc(&content);
        self.editor.toc_is_synthetic = false;
        let highlight_task = self.highlight_all();
        self.vault.backlinks =
            md_editor_core::vault::get_mixed_backlinks(&self.state, path).unwrap_or_default();
        if is_different && reset_scroll {
            // Return to where this file was last left, rather than
            // always to the top — finding your place again by hand
            // in a long note is the whole cost this avoids.
            let target_y = resumed_scroll
                .or_else(|| self.stored_scroll_for(path))
                .unwrap_or(0.0);
            self.editor.scroll_y = target_y;
            self.motion.editor_scroll = None;
            let scroll_task = operation::scroll_to(
                iced::advanced::widget::Id::new(EDITOR_SCROLLABLE_ID),
                AbsoluteOffset {
                    x: 0.0,
                    y: target_y,
                },
            );
            return Task::batch(vec![highlight_task, scroll_task]);
        }
        highlight_task
    }

    /// Show a presentation in the viewer. Like an image, a presentation takes
    /// the pane from the open note, so the note is flushed and parked first.
    pub(super) fn open_pptx(&mut self, path: &str) -> Task<Message> {
        if self.vault.root.is_none() {
            self.ui.toast = Some("Open a vault before opening a presentation".to_string());
            return Task::none();
        }
        if let Err(err) = self.leave_active_document() {
            self.ui.toast = Some(format!(
                "Not opening the presentation: {err}. Your edits are still open."
            ));
            return Task::none();
        }
        self.active_path = None;
        self.pdf.active_path = None;
        self.showing_pdf = false;
        self.active_image_path = None;
        self.active_image = None;
        self.active_panel = ActivePanel::Markdown;
        self.editor.toc_entries.clear();
        self.editor.toc_is_synthetic = false;
        self.pdf.toc_entries.clear();
        self.pdf.toc_is_synthetic = false;
        let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
        self.vault.backlinks =
            md_editor_core::vault::get_mixed_backlinks(&self.state, path).unwrap_or_default();
        self.load_pptx(path)
    }

    /// Read and lay out the presentation at `path` off the UI thread.
    pub(super) fn load_pptx(&mut self, path: &str) -> Task<Message> {
        let Some(abs_path) = self.resolve_active_path(path) else {
            return Task::none();
        };
        let generation = self.pptx.begin_load(path);
        Task::perform(
            async move { crate::slides::load_deck(&abs_path).map(Arc::new) },
            move |result| Message::Shell(ShellMessage::PptxLoaded(generation, result)),
        )
    }

    pub(super) fn open_image(&mut self, path: &str) -> Task<Message> {
        let Some(abs_path) = self.resolve_active_path(path) else {
            self.ui.toast = Some("Open a vault before opening an image".to_string());
            return Task::none();
        };

        match image::open(&abs_path) {
            Ok(img) => {
                // Showing an image clears `active_path`, which detaches the
                // open note from the buffer holding it. Flush and park it
                // first, or those edits have nowhere left to be written to.
                if let Err(err) = self.leave_active_document() {
                    self.ui.toast = Some(format!(
                        "Not opening the image: {err}. Your edits are still open."
                    ));
                    return Task::none();
                }

                let (width, height) = img.dimensions();
                let handle = iced::widget::image::Handle::from_rgba(
                    width,
                    height,
                    img.into_rgba8().into_raw(),
                );
                self.active_image_path = Some(path.to_string());
                self.graph.set_active_path(None);
                let _ = md_editor_core::config::set_sys_config(&self.state, "last_file", path);
                self.active_image = Some((handle, width as f32, height as f32));
                self.pdf.active_path = None;
                self.pptx.close();
                self.showing_pdf = false;
                self.active_panel = ActivePanel::Markdown;
                self.editor.toc_entries.clear();
                self.editor.toc_is_synthetic = false;
                self.pdf.toc_entries.clear();
                self.pdf.toc_is_synthetic = false;
                self.vault.backlinks.clear();
            }
            Err(err) => {
                self.ui.toast = Some(format!("Could not open image: {err}"));
            }
        }
        Task::none()
    }

    pub(super) fn resolve_active_path(&self, path: &str) -> Option<std::path::PathBuf> {
        let root = self.vault.root.as_deref()?;
        Some(md_editor_core::vault::resolve_vault_path(
            std::path::Path::new(root),
            path,
        ))
    }

    pub(super) fn restore_scroll_positions(&self) -> Task<Message> {
        let mut tasks = Vec::new();
        // Restore editor scroll position after search bar toggle
        let editor_y = self.editor.scroll_y;
        tasks.push(operation::scroll_to(
            iced::advanced::widget::Id::new(EDITOR_SCROLLABLE_ID),
            AbsoluteOffset {
                x: 0.0,
                y: editor_y,
            },
        ));
        // Restore PDF scroll position after search bar toggle
        if self.pdf.active_path.is_some() {
            let pdf_y = self.pdf.scroll_y;
            tasks.push(operation::scroll_to(
                iced::advanced::widget::Id::new(PDF_SCROLLABLE_ID),
                AbsoluteOffset { x: 0.0, y: pdf_y },
            ));
        }
        Task::batch(tasks)
    }

    pub(super) fn restore_layout(&mut self) {
        let get = |key| {
            md_editor_core::config::get_sys_config(&self.state, key)
                .ok()
                .flatten()
        };
        self.vault.sidebar_visible = get("layout_sidebar_visible")
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.vault.sidebar_visible);
        self.vault.backlinks_visible = get("layout_backlinks_visible")
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.vault.backlinks_visible);
        self.editor.toc_visible = get("layout_toc_visible")
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.editor.toc_visible);
        self.tracker.visible = get("layout_tracker_visible")
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.tracker.visible);
        self.ui.split_view_active = get("layout_split_active")
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.ui.split_view_active);
        self.ui.split_ratio = get("layout_split_ratio")
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(self.ui.split_ratio)
            .clamp(0.25, 0.75);
    }

    pub(super) fn persist_layout(&self) {
        let values = [
            (
                "layout_sidebar_visible",
                self.vault.sidebar_visible.to_string(),
            ),
            (
                "layout_backlinks_visible",
                self.vault.backlinks_visible.to_string(),
            ),
            ("layout_toc_visible", self.editor.toc_visible.to_string()),
            ("layout_tracker_visible", self.tracker.visible.to_string()),
            ("layout_split_active", self.ui.split_view_active.to_string()),
            ("layout_split_ratio", self.ui.split_ratio.to_string()),
        ];
        for (key, value) in values {
            let _ = md_editor_core::config::set_sys_config(&self.state, key, &value);
        }
    }

    /// The open note changed on disk (not by us). A clean buffer follows the
    /// file, keeping the caret; a dirty one is left alone — the next save
    /// notices the change and keeps the disk version as a conflict copy.
    pub(super) fn reload_open_note_from_disk(&mut self) -> Task<Message> {
        let Some(path) = self.active_path.clone() else {
            return Task::none();
        };
        if self.editor.buffer.dirty || self.showing_pdf {
            return Task::none();
        }
        let Ok(bytes) = md_editor_core::vault::open_file(&self.state, &path) else {
            return Task::none();
        };
        let Ok(text) = String::from_utf8(bytes) else {
            return Task::none();
        };
        if text == self.editor.buffer.text() {
            self.editor.disk_hash = Some(md_editor_core::vault::content_hash(&text));
            return Task::none();
        }
        let caret = self.editor.buffer.cursor_offset();
        self.editor.buffer = DocBuffer::from_text(&text);
        self.editor
            .buffer
            .set_cursor_offset(caret.min(text.chars().count()));
        self.editor.disk_hash = Some(md_editor_core::vault::content_hash(&text));
        self.editor.autosave_pending_since = None;
        self.editor.buffer_revision = self.editor.buffer_revision.wrapping_add(1);
        self.editor.toc_entries = views::toc::get_toc(&text);
        log::info!("Reloaded {path}: changed on disk");
        self.highlight_all()
    }
}

//! Pasting, picking and dropping images/files into notes or the vault tree.

use super::*;
use crate::editor::buffer::EditorCommand;
use crate::messages::EditorMessage;
use crate::platform::ClipboardPayload;
use md_editor_core::attachments::{self, AttachmentFolder};

/// Settings key for where attachments go: `root`, `beside-note`, or a folder.
pub(crate) const ATTACHMENTS_FOLDER_KEY: &str = "attachments_folder";

/// Images above this size are stored, with a warning.
const LARGE_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

impl MdEditor {
    /// Ctrl+V in the editor: read the clipboard off the UI thread.
    pub(super) fn paste(&mut self) -> Task<Message> {
        Task::perform(
            super::blocking(crate::platform::read_clipboard),
            |payload| Message::Editor(EditorMessage::Pasted(payload)),
        )
    }

    pub(super) fn pasted(&mut self, payload: ClipboardPayload) -> Task<Message> {
        match payload {
            ClipboardPayload::Other => paste_text(),
            _ if self.active_path.is_none() || self.vault.root.is_none() => {
                self.ui.toast = Some("Open a note in a vault to paste images".to_string());
                Task::none()
            }
            ClipboardPayload::Image {
                width,
                height,
                rgba,
            } => {
                let stamp = chrono::Local::now().format("%Y-%m-%d %H-%M-%S").to_string();
                let stored = attachments::store_bitmap(
                    &self.state,
                    &self.attachment_folder(),
                    self.active_path.as_deref(),
                    width,
                    height,
                    rgba,
                    &stamp,
                );
                match stored {
                    Ok(stored) => self.insert_attachments(vec![stored.rel_path], Vec::new()),
                    Err(err) => {
                        self.ui.toast = Some(format!("Could not paste the image: {err}"));
                        Task::none()
                    }
                }
            }
            ClipboardPayload::Files(files) => self.insert_files(&files),
        }
    }

    /// Open native file picker for `"Insert Image…"` (I3).
    pub(super) fn insert_image_dialog(&mut self) -> Task<Message> {
        if self.active_path.is_none() || self.vault.root.is_none() {
            self.ui.toast = Some("Open a note in a vault to insert images".to_string());
            return Task::none();
        }
        self.ui.context_menu = None;
        Task::perform(
            async {
                let picked = rfd::AsyncFileDialog::new()
                    .set_title("Insert Image")
                    .add_filter(
                        "Images",
                        &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"],
                    )
                    .pick_files()
                    .await;
                picked
                    .unwrap_or_default()
                    .into_iter()
                    .map(|f| f.path().to_path_buf())
                    .collect::<Vec<_>>()
            },
            |files| Message::Editor(EditorMessage::ImagesPicked(files)),
        )
    }

    /// Handle OS file drag-and-drop onto the sidebar or editor (I4).
    pub(super) fn on_file_dropped(&mut self, path: std::path::PathBuf) -> Task<Message> {
        self.ui.hovered_drop_files.clear();
        if self.vault.root.is_none() {
            self.ui.toast = Some("Open a vault before dropping files".to_string());
            return Task::none();
        }

        let sidebar_right = if self.vault.sidebar_visible {
            self.ui.sidebar_width
        } else {
            0.0
        };
        let over_sidebar = self.ui.cursor_pos.0 < sidebar_right;

        if over_sidebar {
            return self.import_dropped_into_sidebar(&path);
        }

        if self.active_path.is_some()
            && (!self.showing_pdf || self.ui.split_view_active)
            && !self.graph.visible
            && !self.tracker.visible
            && !self.ui.settings_visible
        {
            return self.insert_files(&[path]);
        }

        self.import_dropped_into_sidebar(&path)
    }

    fn import_dropped_into_sidebar(&mut self, file: &std::path::Path) -> Task<Message> {
        if !md_editor_core::vault::is_supported_vault_path(file) {
            self.ui.toast = Some("Unsupported file type for vault".to_string());
            return Task::none();
        }
        let Some(file_name) = file.file_name().and_then(|n| n.to_str()) else {
            return Task::none();
        };
        let Ok(bytes) = std::fs::read(file) else {
            self.ui.toast = Some("Could not read dropped file".to_string());
            return Task::none();
        };
        let parent = Self::new_entry_parent(
            self.vault.selected_path.as_deref(),
            &self.vault.entries,
            &self.vault.expanded_folders,
        );
        let rel = match parent {
            Some(dir) if !dir.is_empty() => format!("{}/{file_name}", dir.trim_end_matches('/')),
            _ => file_name.to_string(),
        };
        let Some(root) = self.vault.root.clone() else {
            return Task::none();
        };
        let abs = match md_editor_core::vault::resolve_vault_path_checked(
            std::path::Path::new(&root),
            &rel,
        ) {
            Ok(abs) => abs,
            Err(err) => {
                self.ui.toast = Some(format!("Could not import {file_name}: {err}"));
                return Task::none();
            }
        };
        if abs.exists() {
            // Never overwrite a vault file with a dropped one of the same name.
            self.ui.notify(
                crate::ui::ToastKind::Warning,
                format!("Not imported: {rel} already exists in the vault"),
                false,
            );
            return Task::none();
        }
        match md_editor_core::vault::write_bytes(&abs, &bytes) {
            Ok(()) => {
                self.note_self_write(&rel);
                self.vault.entries =
                    md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
                self.reveal_in_sidebar(&rel);
                self.ui.toast = Some(format!("Added {rel}"));
            }
            Err(err) => {
                self.ui.toast = Some(format!("Could not import {file_name}: {err}"));
            }
        }
        Task::none()
    }

    /// Images are imported (copied into the vault unless already in it);
    /// notes and PDFs already in the vault become wikilinks; anything else is
    /// refused with a toast.
    pub(super) fn insert_files(&mut self, files: &[std::path::PathBuf]) -> Task<Message> {
        let folder = self.attachment_folder();
        let note = self.active_path.clone();
        let root = self.vault.root.clone().map(std::path::PathBuf::from);
        let mut images = Vec::new();
        let mut links = Vec::new();
        let mut skipped = 0;
        let mut large = false;
        for file in files {
            let name = file.to_string_lossy();
            if is_supported_image_path(&name.to_lowercase())
                || name.to_lowercase().ends_with(".svg")
            {
                large |= std::fs::metadata(file).is_ok_and(|m| m.len() > LARGE_IMAGE_BYTES);
                match attachments::import_file(&self.state, &folder, note.as_deref(), file) {
                    Ok(stored) => images.push(stored.rel_path),
                    Err(err) => {
                        self.ui.toast = Some(format!("Could not add {name}: {err}"));
                        return Task::none();
                    }
                }
            } else if let Some(rel) = root
                .as_ref()
                .and_then(|r| file.strip_prefix(r).ok())
                .filter(|_| md_editor_core::vault::is_supported_vault_path(file))
            {
                let target = rel.to_string_lossy().replace('\\', "/");
                let target = target.strip_suffix(".md").unwrap_or(&target);
                links.push(format!("[[{target}]]"));
            } else {
                skipped += 1;
            }
        }
        if images.is_empty() && links.is_empty() {
            self.ui.toast = Some("Only images can be inserted".to_string());
            return Task::none();
        }
        let task = self.insert_attachments(images, links);
        if skipped > 0 {
            self.ui.toast = Some(format!(
                "Inserted what could be; {skipped} file(s) skipped — only images can be inserted"
            ));
        } else if large {
            self.ui.toast = Some("Added a very large image (over 20 MB)".to_string());
        }
        task
    }

    fn insert_attachments(&mut self, images: Vec<String>, links: Vec<String>) -> Task<Message> {
        let note = self.active_path.clone();
        let mut lines: Vec<String> = images
            .iter()
            .map(|rel| attachments::image_markdown(note.as_deref(), rel))
            .collect();
        lines.extend(links);
        if let Some(first) = images.first() {
            self.note_self_write(first);
            self.ui.toast = Some(if images.len() == 1 {
                format!("Added {first}")
            } else {
                format!("Added {} images", images.len())
            });
        }
        self.vault.entries = md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
        self.run_editor_command(EditorCommand::InsertBlock(lines.join("\n")))
    }

    fn attachment_folder(&self) -> AttachmentFolder {
        let setting = md_editor_core::config::get_sys_config(&self.state, ATTACHMENTS_FOLDER_KEY)
            .ok()
            .flatten();
        AttachmentFolder::from_setting(setting.as_deref())
    }
}

fn paste_text() -> Task<Message> {
    iced::clipboard::read().and_then(|text| {
        Task::done(Message::Editor(EditorMessage::Command(
            EditorCommand::InsertText(text),
        )))
    })
}

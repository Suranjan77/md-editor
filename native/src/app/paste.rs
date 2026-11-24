//! Pasting into a note: clipboard bitmaps and image files become attachments in the vault plus an image line; anything else is pasted as text.

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
        Task::perform(async { crate::platform::read_clipboard() }, |payload| {
            Message::Editor(EditorMessage::Pasted(payload))
        })
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

    /// Insert one line per image (and per link) at the caret, as one undo
    /// step, and show the new files in the tree at once.
    fn insert_attachments(&mut self, images: Vec<String>, links: Vec<String>) -> Task<Message> {
        let note = self.active_path.clone();
        let mut lines: Vec<String> = images
            .iter()
            .map(|rel| attachments::image_markdown(note.as_deref(), rel))
            .collect();
        lines.extend(links);
        if let Some(first) = images.first() {
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

/// The ordinary text paste, through iced's clipboard.
fn paste_text() -> Task<Message> {
    iced::clipboard::read().and_then(|text| {
        Task::done(Message::Editor(EditorMessage::Command(
            EditorCommand::InsertText(text),
        )))
    })
}

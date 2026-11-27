//! Files embedded in a PDF: listing them on open, opening one in the system's app (from the cache) and saving one into the vault.

use super::*;
use crate::messages::PdfMessage;
use md_editor_core::attachments::{self, Stored};
use md_editor_core::pdf::PdfAttachmentInfo;

/// Extensions the desktop may run rather than show. An attachment is a
/// stranger's file: these are only ever saved, never handed to `xdg-open`.
const RUNNABLE_EXTENSIONS: &[&str] = &[
    "appimage", "bat", "cmd", "com", "cpl", "desktop", "exe", "hta", "jar", "js", "jse", "lnk",
    "msi", "msp", "pif", "ps1", "py", "reg", "run", "scr", "sh", "vbe", "vbs", "ws", "wsf",
];

/// Whether `name` is a file the system might execute when "opened".
pub(crate) fn is_runnable(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| RUNNABLE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Where `OpenAttachment` writes attachment `index` of the PDF at `abs_pdf`:
/// one folder per PDF and attachment, so two PDFs' `data.csv` never clash.
pub(crate) fn cache_path(abs_pdf: &str, index: u16, name: &str) -> std::path::PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    abs_pdf.hash(&mut hasher);
    index.hash(&mut hasher);
    md_editor_core::state::cache_dir()
        .join("pdf-attachments")
        .join(format!("{:016x}", hasher.finish()))
        .join(attachments::safe_file_name(name, "attachment"))
}

impl MdEditor {
    /// List the embedded files of the PDF just opened (`path` vault-relative).
    pub(super) fn load_pdf_attachments(&self, path: &str, abs_path: String) -> Task<Message> {
        let state = self.state.clone();
        let path = path.to_string();
        Task::perform(
            super::blocking(move || {
                let renderer = state.pdf_renderer.as_ref()?;
                renderer.list_attachments(&abs_path).ok()
            }),
            move |listed| {
                Message::Pdf(PdfMessage::AttachmentsLoaded(
                    path.clone(),
                    listed.unwrap_or_default(),
                ))
            },
        )
    }

    /// `PdfMessage::AttachmentsLoaded`: keep them if the PDF is still open.
    pub(super) fn on_pdf_attachments_loaded(
        &mut self,
        path: String,
        listed: Vec<PdfAttachmentInfo>,
    ) -> Task<Message> {
        if self.pdf.active_path.as_deref() == Some(path.as_str()) {
            self.pdf.attachments = listed;
        }
        Task::none()
    }

    /// `PdfMessage::OpenAttachment`: write it to the cache and open that copy
    /// with the system's default app.
    pub(super) fn on_pdf_open_attachment(
        &mut self,
        path: String,
        attachment: PdfAttachmentInfo,
    ) -> Task<Message> {
        if is_runnable(&attachment.name) {
            self.ui.notify(
                crate::ui::ToastKind::Warning,
                format!(
                    "Not opened: {} could run as a program. Save it to the vault to look at it.",
                    attachment.name
                ),
                false,
            );
            return Task::none();
        }
        let Some(abs_path) = self.resolve_active_path(&path) else {
            return Task::none();
        };
        let abs_path = abs_path.to_string_lossy().to_string();
        let state = self.state.clone();
        Task::perform(
            super::blocking(move || -> Result<(), String> {
                let renderer = state
                    .pdf_renderer
                    .as_ref()
                    .ok_or_else(|| "PDF renderer is unavailable".to_string())?;
                let bytes = renderer
                    .attachment_bytes(&abs_path, attachment.index, &attachment.name)
                    .map_err(|e| e.to_string())?;
                let target = cache_path(&abs_path, attachment.index, &attachment.name);
                if let Some(dir) = target.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                std::fs::write(&target, bytes).map_err(|e| e.to_string())?;
                crate::platform::open_external(&target.to_string_lossy());
                Ok(())
            }),
            |result| result,
        )
        .then(|result| match result {
            Ok(()) => Task::none(),
            Err(err) => Task::done(Message::Pdf(PdfMessage::AttachmentFailed(err))),
        })
    }

    /// `PdfMessage::SaveAttachment`: copy it into the attachments folder
    /// (the setting, read relative to the PDF), deduplicated like a pasted image.
    pub(super) fn on_pdf_save_attachment(
        &mut self,
        path: String,
        attachment: PdfAttachmentInfo,
    ) -> Task<Message> {
        let Some(abs_path) = self.resolve_active_path(&path) else {
            return Task::none();
        };
        let abs_path = abs_path.to_string_lossy().to_string();
        let folder = self.attachment_folder();
        let state = self.state.clone();
        Task::perform(
            super::blocking(move || -> Result<Stored, String> {
                let renderer = state
                    .pdf_renderer
                    .as_ref()
                    .ok_or_else(|| "PDF renderer is unavailable".to_string())?;
                let bytes = renderer
                    .attachment_bytes(&abs_path, attachment.index, &attachment.name)
                    .map_err(|e| e.to_string())?;
                let name = attachments::safe_file_name(&attachment.name, "attachment");
                attachments::store_bytes(&state, &folder, Some(&path), &name, &bytes)
                    .map_err(|e| e.to_string())
            }),
            |result| match result {
                Ok(stored) => Message::Pdf(PdfMessage::AttachmentSaved(stored)),
                Err(err) => Message::Pdf(PdfMessage::AttachmentFailed(err)),
            },
        )
    }

    /// `PdfMessage::AttachmentSaved`: show the file in the sidebar.
    pub(super) fn on_pdf_attachment_saved(&mut self, stored: Stored) -> Task<Message> {
        self.note_self_write(&stored.rel_path);
        self.vault.entries = md_editor_core::vault::list_vault(&self.state).unwrap_or_default();
        self.reveal_in_sidebar(&stored.rel_path);
        self.vault.selected_path = Some(stored.rel_path.clone());
        self.ui.toast = Some(if stored.reused {
            format!("Already in the vault: {}", stored.rel_path)
        } else {
            format!("Saved {}", stored.rel_path)
        });
        Task::none()
    }

    /// `PdfMessage::AttachmentFailed`.
    pub(super) fn on_pdf_attachment_failed(&mut self, err: String) -> Task<Message> {
        self.ui.notify(
            crate::ui::ToastKind::Error,
            format!("Could not read the attachment: {err}"),
            false,
        );
        Task::none()
    }
}

#[cfg(test)]
mod tests;

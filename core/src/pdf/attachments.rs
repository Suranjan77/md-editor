//! Files embedded in a PDF (its `EmbeddedFiles` name tree): listing them and reading one out.

use super::*;

/// One file embedded in a PDF.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdfAttachmentInfo {
    /// Position in the document's attachment list: what
    /// [`PdfRenderer::attachment_bytes`] takes.
    pub index: u16,
    /// The file name the PDF gives it (`Attachment 1`… when it gives none).
    pub name: String,
    /// Size in bytes of the embedded file.
    pub size: usize,
}

/// A worker command about embedded files.
pub(super) enum AttachmentRequest {
    /// List them.
    List(
        String,
        std::sync::mpsc::SyncSender<PdfResult<Vec<PdfAttachmentInfo>>>,
    ),
    /// Read one: path, index, the name it was listed under.
    Bytes(
        String,
        u16,
        String,
        std::sync::mpsc::SyncSender<PdfResult<Vec<u8>>>,
    ),
}

impl super::worker::Worker<'_> {
    pub(super) fn serve_attachments(&mut self, request: AttachmentRequest) {
        match request {
            AttachmentRequest::List(path, resp) => {
                let _ = resp.send(self.open_document(&path).map(list_attachments));
            }
            AttachmentRequest::Bytes(path, index, name, resp) => {
                let _ = resp.send(
                    self.open_document(&path)
                        .and_then(|doc| attachment_bytes(doc, index, &name)),
                );
            }
        }
    }
}

/// Every embedded file of `doc`, in document order.
fn list_attachments(doc: &PdfDocument) -> Vec<PdfAttachmentInfo> {
    doc.attachments()
        .iter()
        .enumerate()
        .map(|(i, attachment)| PdfAttachmentInfo {
            index: i as u16,
            name: display_name(&attachment.name(), i),
            size: attachment.len(),
        })
        .collect()
}

/// The bytes of attachment `index`, provided it is still called `name`: the
/// file may have been replaced on disk since it was listed, and the caller
/// must not save one file under another's name.
fn attachment_bytes(doc: &PdfDocument, index: u16, name: &str) -> PdfResult<Vec<u8>> {
    let attachment = doc
        .attachments()
        .get(index)
        .map_err(|_| PdfError::Other("The PDF no longer has this attachment".to_string()))?;
    if display_name(&attachment.name(), usize::from(index)) != name {
        return Err(PdfError::Other(
            "The PDF changed since its attachments were listed".to_string(),
        ));
    }
    attachment
        .save_to_bytes()
        .map_err(|e| PdfError::Other(e.to_string()))
}

fn display_name(raw: &str, index: usize) -> String {
    let name = raw.trim().trim_matches('\0').trim();
    if name.is_empty() {
        format!("Attachment {}", index + 1)
    } else {
        name.to_string()
    }
}

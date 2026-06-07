use pdfium_render::prelude::*;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub fn compute_provisional_id(path: &Path) -> Result<(String, u64, Option<i64>), String> {
    let metadata =
        std::fs::metadata(path).map_err(|e| format!("Failed to read file metadata: {e}"))?;
    let file_len = metadata.len();
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);

    let mut file = File::open(path).map_err(|e| format!("Failed to open file: {e}"))?;

    // Read up to 1 MiB
    let chunk_size = 1024 * 1024;
    let mut buffer = vec![0u8; chunk_size];
    let bytes_read = file
        .read(&mut buffer)
        .map_err(|e| format!("Failed to read file: {e}"))?;
    buffer.truncate(bytes_read);

    let mut hasher = Sha256::new();
    hasher.update(&buffer);
    hasher.update(file_len.to_be_bytes());
    if let Some(mtime) = modified {
        hasher.update(mtime.to_be_bytes());
    } else {
        hasher.update([0u8; 8]);
    }

    let hash_result = hasher.finalize();
    let id = format!("{:x}", hash_result);

    Ok((id, file_len, modified))
}

pub fn ensure_document<'a>(
    pdfium: &'a Pdfium,
    current_document: &mut Option<(String, PdfDocument<'a>)>,
    path: &str,
) -> Result<(), String> {
    if current_document
        .as_ref()
        .map(|(p, _)| p != path)
        .unwrap_or(true)
    {
        let doc = pdfium
            .load_pdf_from_file(path, None)
            .map_err(|e| format!("Failed to load PDF: {:?}", e))?;
        *current_document = Some((path.to_string(), doc));
    }
    Ok(())
}

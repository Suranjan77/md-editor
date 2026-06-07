use std::fs;
use std::path::Path;

use crate::state::AppState;

use super::paths::list_all_md_files;

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub fn repair_rename_references(
    state: &AppState,
    vault_root: &Path,
    old_path: &str,
    new_path: &str,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    db.execute(
        "UPDATE pdf_documents SET vault_relative_path = ?1 WHERE vault_relative_path = ?2",
        rusqlite::params![new_path, old_path],
    )
    .ok();
    db.execute(
        "UPDATE pdf_text_search SET path = ?1 WHERE path = ?2",
        rusqlite::params![new_path, old_path],
    )
    .ok();
    db.execute(
        "UPDATE pdf_annotations SET linked_note_path = ?1 WHERE linked_note_path = ?2",
        rusqlite::params![new_path, old_path],
    )
    .ok();

    let md_files = list_all_md_files(vault_root)?;
    let old_encoded = percent_encode(old_path);
    let new_encoded = percent_encode(new_path);

    let old_path_stem = old_path
        .strip_suffix(".md")
        .or_else(|| old_path.strip_suffix(".markdown"))
        .unwrap_or(old_path);
    let new_path_stem = new_path
        .strip_suffix(".md")
        .or_else(|| new_path.strip_suffix(".markdown"))
        .unwrap_or(new_path);

    for md_path in md_files {
        let content = match fs::read_to_string(&md_path) {
            Ok(content) => content,
            Err(_) => continue,
        };

        let mut new_content = content.clone();
        new_content = new_content.replace(
            &format!("pdf://{old_encoded}"),
            &format!("pdf://{new_encoded}"),
        );
        new_content =
            new_content.replace(&format!("pdf://{old_path}"), &format!("pdf://{new_path}"));
        new_content = new_content.replace(&format!("({old_path})"), &format!("({new_path})"));
        new_content = new_content.replace(&format!("(./{old_path})"), &format!("(./{new_path})"));

        if old_path_stem != old_path {
            new_content = new_content.replace(
                &format!("[[{old_path_stem}]]"),
                &format!("[[{new_path_stem}]]"),
            );
            new_content = new_content.replace(
                &format!("[[{old_path_stem}#"),
                &format!("[[{new_path_stem}#"),
            );
            new_content = new_content.replace(
                &format!("[[{old_path_stem}|"),
                &format!("[[{new_path_stem}|"),
            );
            new_content =
                new_content.replace(&format!("[[{old_path}]]"), &format!("[[{new_path}]]"));
            new_content = new_content.replace(&format!("[[{old_path}#"), &format!("[[{new_path}#"));
            new_content = new_content.replace(&format!("[[{old_path}|"), &format!("[[{new_path}|"));
            new_content =
                new_content.replace(&format!("({old_path_stem})"), &format!("({new_path_stem})"));
            new_content = new_content.replace(
                &format!("(./{old_path_stem})"),
                &format!("(./{new_path_stem})"),
            );
        }

        if new_content != content {
            fs::write(&md_path, &new_content).map_err(|e| {
                format!(
                    "Failed to write updated links to {}: {}",
                    md_path.display(),
                    e
                )
            })?;
        }
    }

    Ok(())
}

use crate::editor::parser;
use crate::pdf_links::parse_pdf_link;
use md_editor_core::state::AppState;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BrokenReferenceKind {
    MissingPdf,
    DeletedAnnotation,
    MissingNote,
    MovedVaultPath { suggested_path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrokenReference {
    pub kind: BrokenReferenceKind,
    pub source_file: String, // vault-relative path
    pub target: String,      // vault-relative path or annotation ID
    pub detail: String,      // e.g. "Line 12"
}

/// Recursively scan the vault to find all .md and .pdf files.
pub fn list_all_files(root: &Path) -> Result<(HashSet<String>, HashSet<String>), String> {
    let mut mds = HashSet::new();
    let mut pdfs = HashSet::new();
    list_all_files_recursive(root, root, &mut mds, &mut pdfs)?;
    Ok((mds, pdfs))
}

fn list_all_files_recursive(
    dir: &Path,
    root: &Path,
    mds: &mut HashSet<String>,
    pdfs: &mut HashSet<String>,
) -> Result<(), String> {
    let read_dir = std::fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            list_all_files_recursive(&path, root, mds, pdfs)?;
        } else {
            let rel_path = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if path
                .extension()
                .is_some_and(|e| e == "md" || e == "markdown")
            {
                mds.insert(rel_path);
            } else if path.extension().is_some_and(|e| e == "pdf") {
                pdfs.insert(rel_path);
            }
        }
    }
    Ok(())
}

/// Find a suggested path for a missing filename in the vault.
pub fn find_suggested_path(
    target: &str,
    existing_mds: &HashSet<String>,
    existing_pdfs: &HashSet<String>,
) -> Option<String> {
    let target_path = Path::new(target);
    let file_name = target_path.file_name()?.to_string_lossy().to_string();

    if target.to_lowercase().ends_with(".pdf") {
        for pdf in existing_pdfs {
            if Path::new(pdf)
                .file_name()
                .is_some_and(|name| name.to_string_lossy() == file_name)
            {
                return Some(pdf.clone());
            }
        }
    } else {
        for md in existing_mds {
            if Path::new(md)
                .file_name()
                .is_some_and(|name| name.to_string_lossy() == file_name)
            {
                return Some(md.clone());
            }
        }
    }
    None
}

/// Check the vault for missing PDFs, deleted annotations, missing notes, and moved vault paths.
pub fn check_vault_integrity(
    state: &AppState,
    vault_root: &Path,
) -> Result<Vec<BrokenReference>, String> {
    let mut broken = Vec::new();

    // 1. Gather all existing vault files
    let (existing_mds, existing_pdfs) = list_all_files(vault_root)?;

    // 2. Build doc ID map
    let doc_id_to_path: std::collections::HashMap<String, String> = state
        .pdf_document_references()?
        .into_iter()
        .map(|doc| (doc.document_id, doc.vault_path))
        .collect();

    // 3. Check for missing PDFs in database
    for (doc_id, pdf_path) in &doc_id_to_path {
        if !existing_pdfs.contains(pdf_path) {
            if let Some(suggested) = find_suggested_path(pdf_path, &existing_mds, &existing_pdfs) {
                broken.push(BrokenReference {
                    kind: BrokenReferenceKind::MovedVaultPath {
                        suggested_path: suggested,
                    },
                    source_file: "database".to_string(),
                    target: pdf_path.clone(),
                    detail: format!("PDF Document ID: {}", doc_id),
                });
            } else {
                broken.push(BrokenReference {
                    kind: BrokenReferenceKind::MissingPdf,
                    source_file: "database".to_string(),
                    target: pdf_path.clone(),
                    detail: format!("PDF Document ID: {}", doc_id),
                });
            }
        }
    }

    // 4. Check for missing notes linked from annotations
    for ann in state.linked_pdf_annotation_references()? {
        let pdf_path = doc_id_to_path
            .get(&ann.document_id)
            .cloned()
            .unwrap_or_else(|| "unknown.pdf".to_string());
        if !existing_mds.contains(&ann.linked_note_path) {
            if let Some(suggested) =
                find_suggested_path(&ann.linked_note_path, &existing_mds, &existing_pdfs)
            {
                broken.push(BrokenReference {
                    kind: BrokenReferenceKind::MovedVaultPath {
                        suggested_path: suggested,
                    },
                    source_file: pdf_path,
                    target: ann.linked_note_path,
                    detail: format!(
                        "Annotation {} on page {}",
                        ann.annotation_id, ann.page_index
                    ),
                });
            } else {
                broken.push(BrokenReference {
                    kind: BrokenReferenceKind::MissingNote,
                    source_file: pdf_path,
                    target: ann.linked_note_path,
                    detail: format!(
                        "Annotation {} on page {}",
                        ann.annotation_id, ann.page_index
                    ),
                });
            }
        }
    }

    // 5. Check links in markdown files
    for md_file in &existing_mds {
        let abs_md_path = md_editor_core::vault::resolve_vault_path(vault_root, md_file);
        let content = match std::fs::read_to_string(&abs_md_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let highlighted = parser::highlight_markdown(&content);
        let metadata = parser::extract_document_metadata(&highlighted);

        for link in metadata.links {
            let target = link.target.trim();
            if target.is_empty() || target.starts_with('#') {
                continue;
            }

            // If it's a PDF link
            if let Some(pdf_target) = parse_pdf_link(target) {
                if !existing_pdfs.contains(&pdf_target.path) {
                    if let Some(suggested) =
                        find_suggested_path(&pdf_target.path, &existing_mds, &existing_pdfs)
                    {
                        broken.push(BrokenReference {
                            kind: BrokenReferenceKind::MovedVaultPath {
                                suggested_path: suggested,
                            },
                            source_file: md_file.clone(),
                            target: target.to_string(),
                            detail: format!("Line {}", link.line + 1),
                        });
                    } else {
                        broken.push(BrokenReference {
                            kind: BrokenReferenceKind::MissingPdf,
                            source_file: md_file.clone(),
                            target: target.to_string(),
                            detail: format!("Line {}", link.line + 1),
                        });
                    }
                } else if let Some(ann_id) = &pdf_target.annotation_id {
                    if !state.pdf_annotation_exists(ann_id)? {
                        broken.push(BrokenReference {
                            kind: BrokenReferenceKind::DeletedAnnotation,
                            source_file: md_file.clone(),
                            target: target.to_string(),
                            detail: format!("Line {}, Annotation ID: {}", link.line + 1, ann_id),
                        });
                    }
                }
            } else {
                // Regular local markdown link: resolve relative path
                // We avoid checking web/uri links
                if !crate::app::has_uri_scheme(target) {
                    let resolved_rel = crate::app::resolve_relative_link_path(
                        Some(vault_root.to_str().unwrap_or("")),
                        Some(md_file),
                        target,
                    );
                    if !existing_mds.contains(&resolved_rel)
                        && !existing_pdfs.contains(&resolved_rel)
                    {
                        if let Some(suggested) =
                            find_suggested_path(&resolved_rel, &existing_mds, &existing_pdfs)
                        {
                            broken.push(BrokenReference {
                                kind: BrokenReferenceKind::MovedVaultPath {
                                    suggested_path: suggested,
                                },
                                source_file: md_file.clone(),
                                target: target.to_string(),
                                detail: format!("Line {}", link.line + 1),
                            });
                        } else {
                            broken.push(BrokenReference {
                                kind: BrokenReferenceKind::MissingNote,
                                source_file: md_file.clone(),
                                target: target.to_string(),
                                detail: format!("Line {}", link.line + 1),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(broken)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("{}_{}", name, uuid::Uuid::new_v4()));
        path
    }

    #[test]
    fn test_vault_integrity_checks() {
        let root = unique_temp_dir("integrity_test");
        std::fs::create_dir_all(&root).unwrap();

        let state = AppState::new_in_memory();
        md_editor_core::vault::set_vault_root(&state, root.to_str().unwrap()).unwrap();

        // 1. Create a markdown note with:
        // - Wiki link pointing to missing note
        // - PDF link pointing to missing PDF
        // - PDF link pointing to existing PDF but deleted annotation
        let md_content = "See [[missing-note]]\nAlso [broken pdf](pdf://missing.pdf?page=1)\nValid [pdf annotation](pdf://valid.pdf?page=1&annotation=deleted-ann)";
        std::fs::write(root.join("source.md"), md_content).unwrap();

        // 2. Create valid.pdf file
        std::fs::write(root.join("valid.pdf"), "%PDF-1.4 ...").unwrap();

        state
            .save_pdf_document("doc-1", "valid.pdf", 0, None)
            .unwrap();
        state
            .save_pdf_annotation(&md_editor_core::pdf::PdfAnnotation {
                id: "ann-1".to_string(),
                document_id: "doc-1".to_string(),
                page_index: 1,
                kind: md_editor_core::pdf::PdfAnnotationKind::Note,
                color: md_editor_core::pdf::PdfAnnotationColor::Yellow,
                selected_text: "Text".to_string(),
                ranges: vec![],
                rects: vec![],
                note: Some("Some note".to_string()),
                linked_note_path: Some("missing-note-2.md".to_string()),
                markdown_anchor: None,
                tags: vec![],
                status: md_editor_core::pdf::PdfAnnotationStatus::Unresolved,
                created_at: 0,
                updated_at: 0,
            })
            .unwrap();

        // Run integrity check
        let broken = check_vault_integrity(&state, &root).unwrap();

        // Expect:
        // - missing-note is missing note (Line 1)
        // - missing.pdf is missing pdf (Line 2)
        // - deleted-ann is deleted annotation (Line 3)
        // - missing-note-2.md is missing note (linked from annotation in valid.pdf)
        assert_eq!(broken.len(), 4);

        assert!(
            broken
                .iter()
                .any(|b| matches!(b.kind, BrokenReferenceKind::MissingNote)
                    && b.source_file == "source.md"
                    && b.target == "missing-note")
        );
        assert!(
            broken
                .iter()
                .any(|b| matches!(b.kind, BrokenReferenceKind::MissingPdf)
                    && b.source_file == "source.md"
                    && b.target == "pdf://missing.pdf?page=1")
        );
        assert!(
            broken
                .iter()
                .any(|b| matches!(b.kind, BrokenReferenceKind::DeletedAnnotation)
                    && b.source_file == "source.md"
                    && b.target == "pdf://valid.pdf?page=1&annotation=deleted-ann")
        );
        assert!(
            broken
                .iter()
                .any(|b| matches!(b.kind, BrokenReferenceKind::MissingNote)
                    && b.source_file == "valid.pdf"
                    && b.target == "missing-note-2.md")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_vault_integrity_moved_paths() {
        let root = unique_temp_dir("integrity_moved_test");
        std::fs::create_dir_all(&root).unwrap();

        let state = AppState::new_in_memory();
        md_editor_core::vault::set_vault_root(&state, root.to_str().unwrap()).unwrap();

        // Create source.md linking to "missing.pdf" and "missing-note.md"
        let md_content = "[PDF](pdf://missing.pdf?page=1)\n[Note](missing-note.md)";
        std::fs::write(root.join("source.md"), md_content).unwrap();

        // Put the files in another directory (representing moved files)
        let archive_dir = root.join("archive");
        std::fs::create_dir_all(&archive_dir).unwrap();
        std::fs::write(archive_dir.join("missing.pdf"), "%PDF-1.4 ...").unwrap();
        std::fs::write(archive_dir.join("missing-note.md"), "text").unwrap();

        // Run check
        let broken = check_vault_integrity(&state, &root).unwrap();

        assert_eq!(broken.len(), 2);
        assert!(broken.iter().any(|b| {
            if let BrokenReferenceKind::MovedVaultPath { suggested_path } = &b.kind {
                suggested_path == "archive/missing.pdf"
            } else {
                false
            }
        }));
        assert!(broken.iter().any(|b| {
            if let BrokenReferenceKind::MovedVaultPath { suggested_path } = &b.kind {
                suggested_path == "archive/missing-note.md"
            } else {
                false
            }
        }));

        let _ = std::fs::remove_dir_all(root);
    }
}

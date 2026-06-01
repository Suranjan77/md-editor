use crate::pdf_links::build_pdf_link;

pub fn slug_fragment(s: &str) -> String {
    let slug = slugify(s);
    if slug.is_empty() {
        "document".to_string()
    } else {
        slug
    }
}

pub fn normalize_note_path(path: &str) -> String {
    let mut note_path = path.trim().replace('\\', "/");
    if note_path.is_empty() {
        note_path = "pdf-notes/note.md".to_string();
    }
    if std::path::Path::new(&note_path).extension().is_none() {
        note_path.push_str(".md");
    }
    note_path
}

pub fn note_filename_from_path(path: &str) -> String {
    let normalized = normalize_note_path(path);
    std::path::Path::new(&normalized)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("note.md")
        .to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkedPdfNoteAction {
    Created,
    Appended,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedPdfNoteUpdate {
    pub content: String,
    pub action: LinkedPdfNoteAction,
}

pub fn build_linked_pdf_note_content(
    existing: Option<&str>,
    note_path: &str,
    pdf_path: &str,
    ann: &md_editor_core::pdf::PdfAnnotation,
) -> LinkedPdfNoteUpdate {
    match existing {
        Some(existing) => {
            let link = pdf_annotation_link(pdf_path, ann);
            if existing.contains(&link) {
                LinkedPdfNoteUpdate {
                    content: existing.to_string(),
                    action: LinkedPdfNoteAction::Unchanged,
                }
            } else {
                LinkedPdfNoteUpdate {
                    content: append_linked_pdf_note_section(existing, pdf_path, ann),
                    action: LinkedPdfNoteAction::Appended,
                }
            }
        }
        None => LinkedPdfNoteUpdate {
            content: new_linked_pdf_note_content(note_path, pdf_path, ann),
            action: LinkedPdfNoteAction::Created,
        },
    }
}

pub fn new_linked_pdf_note_content(
    note_path: &str,
    pdf_path: &str,
    ann: &md_editor_core::pdf::PdfAnnotation,
) -> String {
    format!(
        "---\ntype: pdf-note\nsource_pdf: {}\n---\n\n# {}\n\n{}",
        pdf_path,
        note_title_from_path(note_path),
        linked_pdf_note_section(pdf_path, ann)
    )
}

pub fn append_linked_pdf_note_section(
    existing: &str,
    pdf_path: &str,
    ann: &md_editor_core::pdf::PdfAnnotation,
) -> String {
    let link = pdf_annotation_link(pdf_path, ann);
    if existing.contains(&link) {
        return existing.to_string();
    }

    let mut content = existing.trim_end().to_string();
    if !content.is_empty() {
        content.push_str("\n\n---\n\n");
    }
    content.push_str(&linked_pdf_note_section(pdf_path, ann));
    content
}

fn slugify(s: &str) -> String {
    let mut result = String::new();
    let mut last_was_hyphen = false;
    for c in s.to_lowercase().chars() {
        if c.is_alphanumeric() || c == '_' {
            result.push(c);
            last_was_hyphen = false;
        } else if c.is_whitespace() || c == '-' {
            if !last_was_hyphen {
                result.push('-');
                last_was_hyphen = true;
            }
        }
    }
    result.trim_matches('-').to_string()
}

fn note_title_from_path(path: &str) -> String {
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("PDF note");
    let title = stem.replace(['-', '_'], " ");
    let title = title.trim();
    if title.is_empty() {
        "PDF note".to_string()
    } else {
        let mut chars = title.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => "PDF note".to_string(),
        }
    }
}

fn pdf_annotation_link(pdf_path: &str, ann: &md_editor_core::pdf::PdfAnnotation) -> String {
    build_pdf_link(pdf_path, Some(ann.page_index + 1), Some(&ann.id))
}

fn markdown_quote(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "> ".to_string();
    }

    trimmed
        .lines()
        .map(|line| {
            if line.trim().is_empty() {
                ">".to_string()
            } else {
                format!("> {}", line.trim_end())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn linked_pdf_note_section(pdf_path: &str, ann: &md_editor_core::pdf::PdfAnnotation) -> String {
    format!(
        "## Page {}\n\n{}\n\n[Open highlight in PDF]({})\n\n### Notes\n\n",
        ann.page_index + 1,
        markdown_quote(&ann.selected_text),
        pdf_annotation_link(pdf_path, ann)
    )
}

pub fn export_annotations_to_markdown(
    pdf_filename: &str,
    pdf_path: &str,
    annotations: &[md_editor_core::pdf::PdfAnnotation],
) -> String {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut doc = format!(
        "# Annotations: {}\n\n**Document:** {}\n**Annotations:** {}\n**Exported:** {}\n\n---\n",
        pdf_filename,
        pdf_path,
        annotations.len(),
        now
    );

    let mut sorted_anns = annotations.to_vec();
    sorted_anns.sort_by_key(|ann| (ann.page_index, ann.created_at));

    for ann in sorted_anns {
        let col_text = match ann.color {
            md_editor_core::pdf::PdfAnnotationColor::Yellow => "Yellow",
            md_editor_core::pdf::PdfAnnotationColor::Green => "Green",
            md_editor_core::pdf::PdfAnnotationColor::Blue => "Blue",
            md_editor_core::pdf::PdfAnnotationColor::Pink => "Pink",
            md_editor_core::pdf::PdfAnnotationColor::Orange => "Orange",
        };

        doc.push_str(&format!(
            "\n## Page {}\n\n{}\n\n**Colour:** {}\n",
            ann.page_index + 1,
            markdown_quote(&ann.selected_text),
            col_text
        ));

        if let Some(ref note_str) = ann.note {
            if !note_str.trim().is_empty() {
                doc.push_str(&format!("\n**Note:** {}\n", note_str.trim()));
            }
        }

        doc.push_str(&format!(
            "\n[Open in PDF]({})\n",
            pdf_annotation_link(pdf_path, &ann)
        ));
    }

    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn annotation(
        id: &str,
        page_index: u16,
        selected_text: &str,
    ) -> md_editor_core::pdf::PdfAnnotation {
        md_editor_core::pdf::PdfAnnotation {
            id: id.to_string(),
            document_id: "doc".to_string(),
            page_index,
            kind: md_editor_core::pdf::PdfAnnotationKind::Highlight,
            color: md_editor_core::pdf::PdfAnnotationColor::Yellow,
            selected_text: selected_text.to_string(),
            ranges: vec![],
            rects: vec![],
            note: None,
            linked_note_path: None,
            markdown_anchor: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn formats_sections_for_shared_notes() {
        assert_eq!(slug_fragment("My PDF File"), "my-pdf-file");
        assert_eq!(normalize_note_path("notes/pdf note"), "notes/pdf note.md");

        let ann = annotation("abcdef123456", 4, "Important field result");

        let content =
            new_linked_pdf_note_content("notes/shared pdf note.md", "papers/My PDF File.pdf", &ann);
        assert!(content.contains("---\ntype: pdf-note\nsource_pdf: papers/My PDF File.pdf\n---"));
        assert!(content.contains("# Shared pdf note"));
        assert!(content.contains("## Page 5"));
        assert!(content.contains("> Important field result"));
        assert!(content.contains(
            "[Open highlight in PDF](pdf://papers/My%20PDF%20File.pdf?page=5&annotation=abcdef123456)"
        ));
        assert!(content.contains("### Notes"));
        assert!(!content.contains("pdf_annotation:"));

        let ann2 = md_editor_core::pdf::PdfAnnotation {
            id: "fedcba654321".to_string(),
            page_index: 7,
            selected_text: "Second highlight".to_string(),
            ..ann.clone()
        };
        let appended = append_linked_pdf_note_section(&content, "papers/My PDF File.pdf", &ann2);
        assert!(appended.contains("## Page 8"));
        assert!(appended.contains("> Second highlight"));
        assert!(appended.contains("---\n\n## Page 8"));

        let deduped = append_linked_pdf_note_section(&appended, "papers/My PDF File.pdf", &ann);
        assert_eq!(deduped, appended);
    }

    #[test]
    fn linked_pdf_note_builder_reports_create_append_and_unchanged() {
        let ann = annotation("ann#1", 6, "Important field result");

        let created =
            build_linked_pdf_note_content(None, "notes/result.md", "papers/My PDF.pdf", &ann);
        assert_eq!(created.action, LinkedPdfNoteAction::Created);
        assert!(created.content.contains("# Result"));
        assert!(created.content.contains("## Page 7"));
        assert!(created.content.contains(
            "[Open highlight in PDF](pdf://papers/My%20PDF.pdf?page=7&annotation=ann%231)"
        ));

        let ann2 = annotation("ann#2", 8, "Second field result");
        let appended = build_linked_pdf_note_content(
            Some(&created.content),
            "notes/result.md",
            "papers/My PDF.pdf",
            &ann2,
        );
        assert_eq!(appended.action, LinkedPdfNoteAction::Appended);
        assert!(appended.content.contains("## Page 9"));
        assert!(appended.content.contains("> Second field result"));

        let unchanged = build_linked_pdf_note_content(
            Some(&appended.content),
            "notes/result.md",
            "papers/My PDF.pdf",
            &ann,
        );
        assert_eq!(unchanged.action, LinkedPdfNoteAction::Unchanged);
        assert_eq!(unchanged.content, appended.content);
    }

    #[test]
    fn linked_pdf_note_builder_handles_empty_selected_text_deliberately() {
        let ann = annotation("ann-empty", 0, "   ");

        let created =
            build_linked_pdf_note_content(None, "notes/empty.md", "papers/paper.pdf", &ann);

        assert_eq!(created.action, LinkedPdfNoteAction::Created);
        assert!(created.content.contains("## Page 1"));
        assert!(created.content.contains("> \n\n[Open highlight in PDF]"));
        assert!(
            created
                .content
                .contains("pdf://papers/paper.pdf?page=1&annotation=ann-empty")
        );
    }

    #[test]
    fn test_export_annotations() {
        let ann1 = md_editor_core::pdf::PdfAnnotation {
            id: "1".to_string(),
            document_id: "doc".to_string(),
            page_index: 2,
            kind: md_editor_core::pdf::PdfAnnotationKind::Highlight,
            color: md_editor_core::pdf::PdfAnnotationColor::Yellow,
            selected_text: "First".to_string(),
            ranges: vec![],
            rects: vec![],
            note: Some("some note".to_string()),
            linked_note_path: None,
            markdown_anchor: None,
            created_at: 10,
            updated_at: 10,
        };
        let ann2 = md_editor_core::pdf::PdfAnnotation {
            id: "2".to_string(),
            page_index: 0,
            color: md_editor_core::pdf::PdfAnnotationColor::Green,
            selected_text: "Second".to_string(),
            note: None,
            created_at: 5,
            ..ann1.clone()
        };

        let result = export_annotations_to_markdown("doc.pdf", "/path/to/doc.pdf", &[ann1, ann2]);
        assert!(result.contains("# Annotations: doc.pdf"));
        assert!(result.contains("**Document:** /path/to/doc.pdf"));
        // Green color (page 1) should come before Yellow color (page 3) due to sorting by page
        let p1_idx = result.find("Page 1").unwrap();
        let p3_idx = result.find("Page 3").unwrap();
        assert!(p1_idx < p3_idx);
        assert!(result.contains("> First"));
        assert!(result.contains("**Note:** some note"));
        assert!(result.contains("> Second"));
        assert!(result.contains("[Open in PDF](pdf:///path/to/doc.pdf?page=3&annotation=1)"));
    }
}

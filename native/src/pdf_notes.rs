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
    format!(
        "pdf://{}?page={}&annotation={}",
        pdf_path,
        ann.page_index + 1,
        ann.id
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sections_for_shared_notes() {
        assert_eq!(slug_fragment("My PDF File"), "my-pdf-file");
        assert_eq!(normalize_note_path("notes/pdf note"), "notes/pdf note.md");

        let ann = md_editor_core::pdf::PdfAnnotation {
            id: "abcdef123456".to_string(),
            document_id: "doc".to_string(),
            page_index: 4,
            kind: md_editor_core::pdf::PdfAnnotationKind::Highlight,
            color: md_editor_core::pdf::PdfAnnotationColor::Yellow,
            selected_text: "Important field result".to_string(),
            ranges: vec![],
            rects: vec![],
            note: None,
            linked_note_path: None,
            markdown_anchor: None,
            created_at: 0,
            updated_at: 0,
        };

        let content =
            new_linked_pdf_note_content("notes/shared pdf note.md", "papers/My PDF File.pdf", &ann);
        assert!(content.contains("---\ntype: pdf-note\nsource_pdf: papers/My PDF File.pdf\n---"));
        assert!(content.contains("# Shared pdf note"));
        assert!(content.contains("## Page 5"));
        assert!(content.contains("> Important field result"));
        assert!(content.contains(
            "[Open highlight in PDF](pdf://papers/My PDF File.pdf?page=5&annotation=abcdef123456)"
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
}

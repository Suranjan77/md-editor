//! Linked notes for PDF highlights: paths, `pdf://` links and note content.

use md_editor_core::pdf::PageRegion;

/// A parsed `pdf://<path>?page=N&annotation=ID&crop=x,y,w,h` link (the
/// `pdf://` prefix already removed). `page` is 1-based as written.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfLink {
    pub path: String,
    pub page: Option<u16>,
    pub annotation: Option<String>,
    /// A region of `page`, in points from the page's top-left corner.
    pub crop: Option<PageRegion>,
}

pub fn parse_pdf_link(url: &str) -> PdfLink {
    let url = url.strip_prefix("pdf://").unwrap_or(url);
    let (encoded_path, query) = url.split_once('?').unwrap_or((url, ""));
    let mut link = PdfLink {
        path: decode_pdf_link_path(encoded_path),
        page: None,
        annotation: None,
        crop: None,
    };
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key {
            "page" => link.page = value.parse().ok(),
            "annotation" => link.annotation = Some(value.to_string()),
            "crop" => link.crop = parse_crop(value),
            _ => {}
        }
    }
    link
}

fn parse_crop(value: &str) -> Option<PageRegion> {
    let parts: Vec<f32> = value
        .split(',')
        .map(|v| v.trim().parse::<f32>())
        .collect::<Result<_, _>>()
        .ok()?;
    let [x, y, width, height] = parts[..] else {
        return None;
    };
    (width > 0.0 && height > 0.0 && [x, y, width, height].iter().all(|v| v.is_finite())).then_some(
        PageRegion {
            x,
            y,
            width,
            height,
        },
    )
}

/// The Markdown that shows `region` of page `page_index` (0-based) of
/// `pdf_path` inline in a note, rendered live from the PDF.
pub fn pdf_crop_markdown(pdf_path: &str, page_index: u16, region: PageRegion) -> String {
    let name = std::path::Path::new(pdf_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("PDF");
    let alt: String = format!("{name}, p. {}", page_index + 1)
        .chars()
        .filter(|c| !matches!(c, '[' | ']'))
        .collect();
    format!(
        "![{alt}](pdf://{}?page={}&crop={:.0},{:.0},{:.0},{:.0})",
        encode_pdf_link_target(pdf_path),
        page_index + 1,
        region.x,
        region.y,
        region.width,
        region.height
    )
}

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
        encode_pdf_link_path(pdf_path),
        note_title_from_path(note_path),
        linked_pdf_note_section(pdf_path, ann)
    )
}

pub fn encode_pdf_link_path(path: &str) -> String {
    path.replace('%', "%25")
        .replace('?', "%3F")
        .replace('&', "%26")
        .replace('#', "%23")
}

/// [`encode_pdf_link_path`], plus what would end or split a Markdown link
/// destination: spaces, parentheses and angle brackets. `decode_pdf_link_path`
/// reverses both.
pub fn encode_pdf_link_target(path: &str) -> String {
    encode_pdf_link_path(path)
        .replace(' ', "%20")
        .replace('(', "%28")
        .replace(')', "%29")
        .replace('<', "%3C")
        .replace('>', "%3E")
}

pub fn decode_pdf_link_path(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(value) = u8::from_str_radix(&path[index + 1..index + 3], 16)
        {
            decoded.push(value);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

pub fn append_linked_pdf_note_section(
    existing: &str,
    pdf_path: &str,
    ann: &md_editor_core::pdf::PdfAnnotation,
) -> String {
    let link = pdf_annotation_link(pdf_path, ann);
    // Notes written before link targets were encoded carry the raw path.
    let legacy = format!(
        "pdf://{pdf_path}?page={}&annotation={}",
        ann.page_index + 1,
        ann.id
    );
    if existing.contains(&link) || existing.contains(&legacy) {
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
        } else if (c.is_whitespace() || c == '-') && !last_was_hyphen {
            result.push('-');
            last_was_hyphen = true;
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
        encode_pdf_link_target(pdf_path),
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
    fn pdf_link_paths_escape_query_delimiters() {
        let path = "papers/why?this&that#1%.pdf";
        let encoded = encode_pdf_link_path(path);
        assert!(!encoded.contains('?'));
        assert!(!encoded.contains('&'));
        assert_eq!(decode_pdf_link_path(&encoded), path);
    }

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
    fn a_crop_link_round_trips_through_markdown_with_awkward_names() {
        let region = PageRegion {
            x: 112.4,
            y: 240.0,
            width: 380.0,
            height: 210.6,
        };
        let md = pdf_crop_markdown("papers/Vaswani (2017) [v2].pdf", 3, region);
        assert!(md.starts_with("![Vaswani (2017) v2, p. 4](pdf://"), "{md}");
        let dest = md
            .split_once("](")
            .and_then(|(_, rest)| rest.strip_suffix(')'))
            .unwrap();
        assert!(!dest.contains(' ') && !dest.contains('('), "{dest}");
        let link = parse_pdf_link(dest);
        assert_eq!(link.path, "papers/Vaswani (2017) [v2].pdf");
        assert_eq!(link.page, Some(4));
        assert_eq!(
            link.crop,
            Some(PageRegion {
                x: 112.0,
                y: 240.0,
                width: 380.0,
                height: 211.0
            })
        );
        assert_eq!(link.annotation, None);
    }

    #[test]
    fn a_malformed_crop_is_no_crop() {
        for query in [
            "crop=1,2,3",
            "crop=1,2,0,4",
            "crop=a,b,c,d",
            "crop=1,2,3,-4",
        ] {
            assert_eq!(
                parse_pdf_link(&format!("p.pdf?page=1&{query}")).crop,
                None,
                "{query}"
            );
        }
        let highlight = parse_pdf_link("pdf://papers/p.pdf?page=2&annotation=abc");
        assert_eq!(
            (highlight.page, highlight.annotation.as_deref()),
            (Some(2), Some("abc"))
        );
    }
}

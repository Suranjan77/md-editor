use super::model::StyledLine;
use super::reference::{collect_reference_definitions, get_ref_id_from_span_text};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutlineEntry {
    pub level: u8,
    pub text: String,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkdownLinkKind {
    Wiki,
    Inline,
    Reference,
    Footnote,
    ResolvedReference,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkdownLinkEntry {
    pub line: usize,
    pub target: String,
    pub display_text: String,
    pub source_text: String,
    pub kind: MarkdownLinkKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkdownAnchorKind {
    Heading,
    SpanId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkdownAnchorEntry {
    pub line: usize,
    pub slug: String,
    pub source_text: String,
    pub kind: MarkdownAnchorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkdownDocumentMetadata {
    pub outline: Vec<OutlineEntry>,
    pub links: Vec<MarkdownLinkEntry>,
    pub anchors: Vec<MarkdownAnchorEntry>,
    pub frontmatter: FrontmatterMetadata,
    pub reference_definitions: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FrontmatterMetadata {
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
}

pub(crate) fn extract_outline(lines: &[StyledLine]) -> Vec<OutlineEntry> {
    let mut outline = Vec::new();
    for (line_idx, line) in lines.iter().enumerate() {
        let mut heading_level = None;
        for span in &line.spans {
            if span.is_heading {
                heading_level = Some(span.heading_level);
                break;
            }
        }

        if let Some(level) = heading_level {
            let mut text = String::new();
            let mut spans_iter = line.spans.iter();
            if let Some(first_span) = line.spans.first() {
                if first_span.is_syntax && first_span.text.trim_start().starts_with('#') {
                    spans_iter.next();
                }
            }

            for span in spans_iter {
                text.push_str(span.display_text.as_deref().unwrap_or(&span.text));
            }

            outline.push(OutlineEntry {
                level,
                text: text.trim().to_string(),
                line: line_idx,
            });
        }
    }
    outline
}

pub(crate) fn extract_document_metadata(lines: &[StyledLine]) -> MarkdownDocumentMetadata {
    MarkdownDocumentMetadata {
        outline: extract_outline(lines),
        links: extract_markdown_links(lines),
        anchors: extract_markdown_anchors(lines),
        frontmatter: extract_frontmatter_metadata(lines),
        reference_definitions: collect_reference_definitions(lines),
    }
}

pub(crate) fn extract_frontmatter_metadata(lines: &[StyledLine]) -> FrontmatterMetadata {
    let Some(first_line) = lines.first() else {
        return FrontmatterMetadata::default();
    };
    if styled_line_source(first_line).trim() != "---" {
        return FrontmatterMetadata::default();
    }

    let mut metadata = FrontmatterMetadata::default();
    let mut active_key: Option<&str> = None;

    for line in lines.iter().skip(1) {
        let source = styled_line_source(line);
        let trimmed = source.trim();
        if trimmed == "---" {
            break;
        }

        if let Some(item) = trimmed.strip_prefix("- ") {
            match active_key {
                Some("aliases") => push_metadata_value(&mut metadata.aliases, item),
                Some("tags") => push_metadata_value(&mut metadata.tags, item),
                _ => {}
            }
            continue;
        }

        let Some((key, value)) = trimmed.split_once(':') else {
            active_key = None;
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        active_key = match key {
            "alias" | "aliases" => Some("aliases"),
            "tag" | "tags" => Some("tags"),
            _ => None,
        };

        match active_key {
            Some("aliases") => push_metadata_values(&mut metadata.aliases, value),
            Some("tags") => push_metadata_values(&mut metadata.tags, value),
            _ => {}
        }
    }

    metadata
}

pub(crate) fn extract_markdown_anchors(lines: &[StyledLine]) -> Vec<MarkdownAnchorEntry> {
    let mut anchors = Vec::new();
    for entry in extract_outline(lines) {
        anchors.push(MarkdownAnchorEntry {
            line: entry.line,
            slug: markdown_anchor_slug(&entry.text),
            source_text: entry.text,
            kind: MarkdownAnchorKind::Heading,
        });
    }

    for (line_idx, line) in lines.iter().enumerate() {
        for span in &line.spans {
            if let Some(id) = span.id.as_ref() {
                anchors.push(MarkdownAnchorEntry {
                    line: line_idx,
                    slug: id.to_string(),
                    source_text: span.text.clone(),
                    kind: MarkdownAnchorKind::SpanId,
                });
            }
        }
    }

    anchors
}

pub(crate) fn markdown_anchor_slug(s: &str) -> String {
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

pub(crate) fn styled_line_source(line: &StyledLine) -> String {
    line.spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>()
}

fn push_metadata_values(values: &mut Vec<String>, raw: &str) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }

    if let Some(list) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        for item in list.split(',') {
            push_metadata_value(values, item);
        }
    } else {
        push_metadata_value(values, trimmed);
    }
}

fn push_metadata_value(values: &mut Vec<String>, raw: &str) {
    let value = raw
        .trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .trim_start_matches('#')
        .trim();
    if !value.is_empty() {
        values.push(value.to_string());
    }
}
pub(crate) fn extract_markdown_links(lines: &[StyledLine]) -> Vec<MarkdownLinkEntry> {
    let defs = collect_reference_definitions(lines);
    let mut links = Vec::new();

    for (line_idx, line) in lines.iter().enumerate() {
        for (span_idx, span) in line.spans.iter().enumerate() {
            if !span.is_link {
                continue;
            }

            let Some(target) = span.link_target.clone() else {
                continue;
            };
            let display_text = span
                .display_text
                .clone()
                .unwrap_or_else(|| span.text.clone());

            let mut kind = markdown_link_kind(line, span_idx);

            if kind == MarkdownLinkKind::Reference {
                if let Some(ref_id) = get_ref_id_from_span_text(&span.text) {
                    if defs.contains_key(&ref_id.to_lowercase()) {
                        kind = MarkdownLinkKind::ResolvedReference;
                    }
                } else {
                    kind = MarkdownLinkKind::ResolvedReference;
                }
            }

            links.push(MarkdownLinkEntry {
                line: line_idx,
                target,
                display_text,
                source_text: span.text.clone(),
                kind,
            });
        }
    }

    links
}

fn markdown_link_kind(line: &StyledLine, span_idx: usize) -> MarkdownLinkKind {
    let span = &line.spans[span_idx];
    if span.text.starts_with("[^") {
        return MarkdownLinkKind::Footnote;
    }
    if line
        .spans
        .get(span_idx.saturating_sub(1))
        .is_some_and(|prev| prev.is_syntax && prev.text == "[[")
        && line
            .spans
            .get(span_idx + 1)
            .is_some_and(|next| next.is_syntax && next.text == "]]")
    {
        return MarkdownLinkKind::Wiki;
    }
    if span.text.starts_with('[') && span.text.contains("](") {
        return MarkdownLinkKind::Inline;
    }
    MarkdownLinkKind::Reference
}

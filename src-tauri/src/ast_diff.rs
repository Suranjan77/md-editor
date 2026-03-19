use regex::Regex;
use tree_sitter::{Node, Tree};

use crate::ipc_types::{AstDiff, AstNode};

/// Convert tree-sitter nodes to our AstNode types and produce an AstDiff.
///
/// tree-sitter-md v0.3 uses a two-grammar system. We only use the block
/// grammar, so inline content (bold, italic, links) is parsed with regex
/// after extracting raw text from block-level nodes.

/// Convert a tree-sitter node into our serialisable AstNode.
pub fn node_to_ast(node: Node, source: &str) -> AstNode {
    let kind = node.kind();
    match kind {
        "document" | "section" => {
            let children = collect_block_children(node, source);
            AstNode::Document { children }
        }
        "atx_heading" => {
            let level = determine_heading_level(node, source);
            let text = get_heading_text(node, source);
            let children = parse_inline_markdown(&text);
            AstNode::Heading { level, children }
        }
        "setext_heading" => {
            let text = get_setext_heading_text(node, source);
            let level = get_setext_level(node, source);
            let children = parse_inline_markdown(&text);
            AstNode::Heading { level, children }
        }
        "paragraph" => {
            let text = get_paragraph_text(node, source);
            let children = parse_inline_markdown(&text);
            AstNode::Paragraph { children }
        }
        "fenced_code_block" | "indented_code_block" => {
            let (lang, text) = get_code_block_info(node, source);
            AstNode::CodeBlock { lang, text }
        }
        "block_quote" => {
            let children = collect_block_children(node, source);
            AstNode::BlockQuote { children }
        }
        "list" => {
            let first_child = node.child(0);
            let is_ordered = if let Some(child) = first_child {
                if child.kind() == "list_item" {
                    if let Some(marker) = child.child(0) {
                        marker.kind() == "list_marker_dot"
                            || marker.kind() == "list_marker_parenthesis"
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };
            let children = collect_block_children(node, source);
            if is_ordered {
                AstNode::OrderedList { children }
            } else {
                AstNode::UnorderedList { children }
            }
        }
        "list_item" => {
            let children = collect_list_item_children(node, source);
            AstNode::ListItem { children }
        }
        "pipe_table" | "table" => parse_table(node, source),
        "thematic_break" => AstNode::ThematicBreak,
        "soft_line_break" => AstNode::SoftBreak,
        "hard_line_break" | "line_break" => AstNode::HardBreak,
        _ => {
            let text = node_text(node, source);
            if text.is_empty() {
                let children = collect_block_children(node, source);
                if children.is_empty() {
                    AstNode::Text {
                        text: String::new(),
                    }
                } else if children.len() == 1 {
                    children.into_iter().next().unwrap()
                } else {
                    AstNode::Document { children }
                }
            } else {
                AstNode::Text { text }
            }
        }
    }
}

/// Build a full-document AstDiff (used on file open).
pub fn full_ast_diff(tree: &Tree, source: &str) -> AstDiff {
    let root = tree.root_node();
    let subtree = node_to_ast(root, source);
    AstDiff {
        changed_range: (0, source.len()),
        subtree,
    }
}

/// Build an AstDiff (always full rebuild for correctness).
pub fn incremental_ast_diff(tree: &Tree, source: &str) -> AstDiff {
    full_ast_diff(tree, source)
}

/// Collect byte ranges of each top-level block node (flattening sections).
/// Returns a Vec of (start_byte, end_byte) tuples matching the Document's children.
pub fn collect_block_ranges(tree: &Tree) -> Vec<(usize, usize)> {
    let root = tree.root_node();
    let mut ranges = Vec::new();
    collect_ranges_recursive(root, &mut ranges);
    ranges
}

fn collect_ranges_recursive(node: Node, ranges: &mut Vec<(usize, usize)>) {
    let count = node.child_count();
    for i in 0..count {
        if let Some(child) = node.child(i) {
            let kind = child.kind();
            if kind == "section" || kind == "document" {
                collect_ranges_recursive(child, ranges);
            } else if is_block_node(kind) {
                ranges.push((child.start_byte(), child.end_byte()));
            }
        }
    }
}

// ── Inline Markdown Parsing (regex-based) ───────────────────────────

/// Parse inline markdown formatting from raw text.
/// Handles: **bold**, *italic*, `code`, [links](url), ![images](url), [[wikilinks]]
fn parse_inline_markdown(text: &str) -> Vec<AstNode> {
    let mut nodes: Vec<AstNode> = Vec::new();
    let mut remaining = text.to_string();

    // Combined regex that captures inline patterns in order of priority
    // Order: images, wikilinks, links, bold, italic, inline code
    let pattern = Regex::new(
        r"(?s)(`[^`]+`)|(\*\*[^*]+\*\*)|(__[^_]+__)|(\*[^*]+\*)|(_[^_]+_)|(!\[([^\]]*)\]\(([^)]+)\))|(\[\[([^\]|]+)(?:\|([^\]]*))?\]\])|(\[([^\]]+)\]\(([^)]+)\))"
    ).unwrap();

    while !remaining.is_empty() {
        if let Some(m) = pattern.find(&remaining) {
            let start = m.start();
            let end = m.end();
            let matched = &remaining[start..end];

            // Push text before the match
            if start > 0 {
                nodes.push(AstNode::Text {
                    text: remaining[..start].to_string(),
                });
            }

            // Determine which pattern matched
            if matched.starts_with('`') && matched.ends_with('`') {
                // Inline code
                let code = matched.trim_matches('`').to_string();
                nodes.push(AstNode::InlineCode { text: code });
            } else if matched.starts_with("**") && matched.ends_with("**") {
                // Bold
                let inner = &matched[2..matched.len() - 2];
                nodes.push(AstNode::Bold {
                    children: vec![AstNode::Text {
                        text: inner.to_string(),
                    }],
                });
            } else if matched.starts_with("__") && matched.ends_with("__") {
                // Bold (underscore)
                let inner = &matched[2..matched.len() - 2];
                nodes.push(AstNode::Bold {
                    children: vec![AstNode::Text {
                        text: inner.to_string(),
                    }],
                });
            } else if matched.starts_with("![") {
                // Image: ![alt](src)
                let img_re = Regex::new(r"!\[([^\]]*)\]\(([^)]+)\)").unwrap();
                if let Some(caps) = img_re.captures(matched) {
                    nodes.push(AstNode::Image {
                        alt: caps.get(1).map_or("", |m| m.as_str()).to_string(),
                        src: caps.get(2).map_or("", |m| m.as_str()).to_string(),
                    });
                }
            } else if matched.starts_with("[[") && matched.ends_with("]]") {
                // Wikilink: [[target]] or [[target|alias]]
                let inner = &matched[2..matched.len() - 2];
                let (target, alias) = if let Some(pos) = inner.find('|') {
                    (inner[..pos].to_string(), Some(inner[pos + 1..].to_string()))
                } else {
                    (inner.to_string(), None)
                };
                nodes.push(AstNode::WikiLink { target, alias });
            } else if matched.starts_with('[') && matched.contains("](") {
                // Link: [text](url)
                let link_re = Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").unwrap();
                if let Some(caps) = link_re.captures(matched) {
                    let link_text = caps.get(1).map_or("", |m| m.as_str()).to_string();
                    let href = caps.get(2).map_or("", |m| m.as_str()).to_string();
                    nodes.push(AstNode::Link {
                        href,
                        children: vec![AstNode::Text { text: link_text }],
                    });
                }
            } else if (matched.starts_with('*') && matched.ends_with('*'))
                || (matched.starts_with('_') && matched.ends_with('_'))
            {
                // Italic
                let inner = &matched[1..matched.len() - 1];
                nodes.push(AstNode::Italic {
                    children: vec![AstNode::Text {
                        text: inner.to_string(),
                    }],
                });
            } else {
                // Fallback: just text
                nodes.push(AstNode::Text {
                    text: matched.to_string(),
                });
            }

            remaining = remaining[end..].to_string();
        } else {
            // No more matches — push remaining text
            nodes.push(AstNode::Text { text: remaining });
            break;
        }
    }

    if nodes.is_empty() {
        nodes.push(AstNode::Text {
            text: text.to_string(),
        });
    }

    nodes
}

// ── Table Parsing ───────────────────────────────────────────────────

/// Parse a pipe table from node text.
fn parse_table(node: Node, source: &str) -> AstNode {
    let text = node_text(node, source);
    let lines: Vec<&str> = text.lines().collect();

    if lines.len() < 2 {
        return AstNode::Paragraph {
            children: vec![AstNode::Text { text }],
        };
    }

    let headers = parse_table_row(lines[0]);

    // Skip the delimiter row (line with ---, :--:, etc.)
    let mut rows = Vec::new();
    for line in lines.iter().skip(2) {
        let line = line.trim();
        if !line.is_empty() {
            rows.push(parse_table_row(line));
        }
    }

    AstNode::Table { headers, rows }
}

fn parse_table_row(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.trim_start_matches('|').trim_end_matches('|');
    line.split('|').map(|cell| cell.trim().to_string()).collect()
}

// ── Block-level child collection ────────────────────────────────────

/// Collect block-level children, flattening section/document wrappers
/// so the output is a flat list matching collect_block_ranges.
fn collect_block_children(node: Node, source: &str) -> Vec<AstNode> {
    let mut children = Vec::new();
    let count = node.child_count();
    for i in 0..count {
        if let Some(child) = node.child(i) {
            let kind = child.kind();
            if kind == "section" || kind == "document" {
                // Flatten: recurse into sections/documents
                children.extend(collect_block_children(child, source));
            } else if is_block_node(kind) {
                children.push(node_to_ast(child, source));
            }
        }
    }
    children
}

fn collect_list_item_children(node: Node, source: &str) -> Vec<AstNode> {
    let mut children = Vec::new();
    let count = node.child_count();
    for i in 0..count {
        if let Some(child) = node.child(i) {
            let kind = child.kind();
            if kind.starts_with("list_marker") {
                continue;
            }
            if is_block_node(kind) {
                children.push(node_to_ast(child, source));
            }
        }
    }

    if children.is_empty() {
        let text = node_text(node, source).trim().to_string();
        let text = text
            .trim_start_matches(|c: char| {
                c == '-' || c == '*' || c == '+' || c.is_ascii_digit() || c == '.' || c == ')'
            })
            .trim_start()
            .to_string();
        if !text.is_empty() {
            let inline_children = parse_inline_markdown(&text);
            children.push(AstNode::Paragraph {
                children: inline_children,
            });
        }
    }

    children
}

// ── Text extraction helpers ─────────────────────────────────────────

fn node_text(node: Node, source: &str) -> String {
    let start = node.start_byte();
    let end = node.end_byte();
    if start < source.len() && end <= source.len() {
        source[start..end].to_string()
    } else {
        String::new()
    }
}

fn get_heading_text(node: Node, source: &str) -> String {
    let count = node.child_count();
    for i in 0..count {
        if let Some(child) = node.child(i) {
            let kind = child.kind();
            if kind == "heading_content" || kind == "inline" {
                return node_text(child, source).trim().to_string();
            }
        }
    }
    let text = node_text(node, source);
    text.trim_start_matches('#').trim_start().trim().to_string()
}

fn get_setext_heading_text(node: Node, source: &str) -> String {
    let count = node.child_count();
    for i in 0..count {
        if let Some(child) = node.child(i) {
            let kind = child.kind();
            if kind == "heading_content" || kind == "inline" || kind == "paragraph" {
                return node_text(child, source).trim().to_string();
            }
        }
    }
    let text = node_text(node, source);
    text.lines().next().unwrap_or("").trim().to_string()
}

fn get_setext_level(node: Node, source: &str) -> u8 {
    let text = node_text(node, source);
    if text.contains("===") {
        1
    } else {
        2
    }
}

fn get_paragraph_text(node: Node, source: &str) -> String {
    let count = node.child_count();
    for i in 0..count {
        if let Some(child) = node.child(i) {
            if child.kind() == "inline" {
                return node_text(child, source);
            }
        }
    }
    node_text(node, source)
}

fn determine_heading_level(node: Node, source: &str) -> u8 {
    let count = node.child_count();
    for i in 0..count {
        if let Some(child) = node.child(i) {
            let kind = child.kind();
            if kind.starts_with("atx_h") && kind.ends_with("_marker") {
                let marker_text = node_text(child, source);
                return marker_text.chars().filter(|&c| c == '#').count() as u8;
            }
        }
    }
    let text = node_text(node, source);
    text.chars().take_while(|&c| c == '#').count() as u8
}

fn get_code_block_info(node: Node, source: &str) -> (Option<String>, String) {
    let mut lang = None;
    let mut text = String::new();
    let count = node.child_count();
    for i in 0..count {
        if let Some(child) = node.child(i) {
            match child.kind() {
                "info_string" => {
                    let info = node_text(child, source).trim().to_string();
                    if !info.is_empty() {
                        lang = Some(info);
                    }
                }
                "code_fence_content" => {
                    text = node_text(child, source);
                }
                _ => {}
            }
        }
    }

    if text.is_empty() {
        text = node_text(node, source);
    }

    (lang, text)
}

fn is_block_node(kind: &str) -> bool {
    // NOTE: document and section are NOT listed here — they are
    // handled as transparent containers by collect_block_children
    // and collect_block_ranges (flattened, not treated as blocks).
    matches!(
        kind,
        "atx_heading"
            | "setext_heading"
            | "paragraph"
            | "fenced_code_block"
            | "indented_code_block"
            | "block_quote"
            | "list"
            | "list_item"
            | "thematic_break"
            | "soft_line_break"
            | "hard_line_break"
            | "line_break"
            | "pipe_table"
            | "table"
    )
}

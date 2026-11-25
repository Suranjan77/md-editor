//! Single-line Markdown: headings, lists, tasks, rules and inline spans (emphasis, code, links, wikilinks, math).

use super::*;

pub(super) fn highlight_line(line: &str) -> StyledLine {
    let mut sl = StyledLine::new();
    let trimmed = line.trim_start();

    if let Some(level) = detect_heading(trimmed) {
        return heading_line(line, trimmed, level);
    }

    // Horizontal rule
    if is_horizontal_rule(trimmed) {
        sl.spans.push(StyledSpan {
            text: line.to_string(),
            display_text: Some(String::new()),
            color: crate::design::tokens().doc_quote_bar,
            is_rule: true,
            is_syntax: true,
            ..StyledSpan::plain("")
        });
        return sl;
    }

    if trimmed.starts_with('>') {
        return blockquote_line(line, trimmed);
    }

    // Task list items (must check before regular list items)
    if is_task_prefix(trimmed) {
        return task_line(line, trimmed);
    }

    // List items
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
        let bullet_end = line.len() - trimmed.len() + 2;
        // Show bullet character in preview
        sl.spans.push(StyledSpan {
            text: line[..bullet_end].to_string(),
            display_text: Some("  • ".to_string()),
            color: crate::design::tokens().doc_link,
            is_list_marker: true,
            ..StyledSpan::plain("")
        });
        parse_inline_spans(&line[bullet_end..], &mut sl.spans);
        return sl;
    }

    // Numbered list items
    if let Some(num_end) = detect_numbered_list(trimmed) {
        let prefix_end = line.len() - trimmed.len() + num_end;
        sl.spans.push(StyledSpan {
            text: line[..prefix_end].to_string(),
            display_text: None,
            color: crate::design::tokens().doc_link,
            is_list_marker: true,
            ..StyledSpan::plain("")
        });
        parse_inline_spans(&line[prefix_end..], &mut sl.spans);
        return sl;
    }

    parse_inline_spans(line, &mut sl.spans);

    if sl.spans.is_empty() {
        sl.spans.push(StyledSpan::plain(line));
    }

    sl
}

/// `# Heading`: a hidden hash prefix and bold heading text.
fn heading_line(line: &str, trimmed: &str, level: u8) -> StyledLine {
    let mut sl = StyledLine::new();
    let prefix_len = if trimmed.chars().nth(level as usize) == Some(' ') {
        level as usize + 1
    } else {
        level as usize
    };
    let display = if trimmed.len() > prefix_len {
        &trimmed[prefix_len..]
    } else {
        ""
    };

    let hash_part = &line[..(line.len() - trimmed.len() + prefix_len)];
    // The hash prefix is a syntax marker — hidden in preview
    sl.spans.push(StyledSpan {
        text: hash_part.to_string(),
        display_text: Some(String::new()),
        color: crate::design::tokens().doc_syntax,
        is_syntax: true,
        font_size: heading_size(level),
        is_heading: true,
        heading_level: level,
        ..StyledSpan::plain("")
    });

    // The heading text content
    sl.spans.push(StyledSpan {
        text: display.to_string(),
        display_text: None,
        color: crate::design::tokens().doc_text,
        bold: true,
        font_size: heading_size(level),
        is_heading: true,
        heading_level: level,
        ..StyledSpan::plain("")
    });

    sl
}

/// `> quote`: a hidden marker, then inline Markdown in quote styling.
fn blockquote_line(line: &str, trimmed: &str) -> StyledLine {
    let mut sl = StyledLine::new();
    sl.is_blockquote = true;
    // The > marker
    let marker_end = line.len() - trimmed.len() + 1;
    let rest_start = if trimmed.len() > 1 && trimmed.as_bytes()[1] == b' ' {
        line.len() - trimmed.len() + 2
    } else {
        marker_end
    };
    sl.spans.push(StyledSpan {
        text: line[..rest_start].to_string(),
        display_text: Some(String::new()),
        color: crate::design::tokens().doc_syntax,
        is_syntax: true,
        ..StyledSpan::plain("")
    });
    // Blockquote content can contain the same inline Markdown as a
    // paragraph. Keep the quote styling while retaining actionable links.
    let content_start = sl.spans.len();
    parse_inline_spans(&line[rest_start..], &mut sl.spans);
    if sl.spans.len() == content_start {
        sl.spans.push(StyledSpan::plain(""));
    }
    let t = crate::design::tokens();
    for span in &mut sl.spans[content_start..] {
        if span.color == t.doc_text {
            span.color = t.fg_secondary;
        }
        span.italic = true;
    }
    sl
}

/// `- [ ] task` / `- [x] task`: a checkbox marker, then the item text
/// (muted once checked).
fn task_line(line: &str, trimmed: &str) -> StyledLine {
    let mut sl = StyledLine::new();
    let prefix_len = if trimmed.len() == 5 { 5 } else { 6 };
    let checkbox_end = line.len() - trimmed.len() + prefix_len;
    let is_checked = trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]");
    sl.spans.push(StyledSpan {
        text: line[..checkbox_end].to_string(),
        display_text: Some(if is_checked {
            "☑ ".to_string()
        } else {
            "☐ ".to_string()
        }),
        color: if is_checked {
            crate::design::tokens().doc_link
        } else {
            crate::design::tokens().doc_syntax
        },
        is_checkbox: true,
        is_checked,
        is_list_marker: true,
        ..StyledSpan::plain("")
    });
    let start_idx = sl.spans.len();
    parse_inline_spans(&line[checkbox_end..], &mut sl.spans);
    if is_checked {
        for span in &mut sl.spans[start_idx..] {
            span.color = crate::design::tokens().doc_syntax;
        }
    }
    sl
}

pub(super) fn is_horizontal_rule(trimmed: &str) -> bool {
    let mut chars = trimmed.chars();
    let Some(marker @ ('-' | '*' | '_')) = chars.next() else {
        return false;
    };
    chars.clone().count() >= 2 && chars.all(|ch| ch == marker)
}

pub(super) fn is_task_prefix(trimmed: &str) -> bool {
    ["- [ ]", "- [x]", "- [X]"]
        .iter()
        .any(|prefix| trimmed == *prefix || trimmed.starts_with(&format!("{prefix} ")))
}

pub(super) fn parse_inline_spans(text: &str, spans: &mut Vec<StyledSpan>) {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut current = String::new();

    // Constructs are tried in order. Each flushes pending plain text as soon
    // as its opener is seen, even if it then fails to close; span boundaries
    // (and so the oracle tests) depend on that.
    while i < len {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let candidates: [(bool, InlineParser); 7] = [
            (c == '`', inline_code),
            (c == '*' && next == Some('*'), bold),
            (c == '*' && next != Some('*'), italic),
            (c == '[' && next == Some('['), wikilink),
            (c == '[' && (i == 0 || chars[i - 1] != '!'), markdown_link),
            (c == '$' && next.is_some_and(|n| n != '$'), inline_math),
            (c == '!' && next == Some('['), image),
        ];
        let mut parsed = None;
        for (applies, parse) in candidates {
            if applies {
                flush_plain(&mut current, spans);
                parsed = parse(&chars, i, spans);
                if parsed.is_some() {
                    break;
                }
            }
        }
        match parsed {
            Some(after) => i = after,
            None => {
                current.push(c);
                i += 1;
            }
        }
    }
    flush_plain(&mut current, spans);
}

/// Parses one inline construct starting at `i`, returning the index after it.
type InlineParser = fn(&[char], usize, &mut Vec<StyledSpan>) -> Option<usize>;

fn flush_plain(current: &mut String, spans: &mut Vec<StyledSpan>) {
    if !current.is_empty() {
        spans.push(StyledSpan::plain(current));
        current.clear();
    }
}

/// `open`, the styled body, then `close`, with both markers as syntax spans.
fn push_wrapped(
    spans: &mut Vec<StyledSpan>,
    open: &str,
    body: StyledSpan,
    close: &str,
    marker_size: f32,
) {
    let syntax = crate::design::tokens().doc_syntax;
    spans.push(StyledSpan::syntax(open, syntax, marker_size));
    spans.push(body);
    spans.push(StyledSpan::syntax(close, syntax, marker_size));
}

/// `` `code` `` at `i`; returns the index after the closing backtick.
fn inline_code(chars: &[char], i: usize, spans: &mut Vec<StyledSpan>) -> Option<usize> {
    let end = find_char(chars, i + 1, '`')?;
    let body = StyledSpan {
        text: chars[i + 1..end].iter().collect(),
        display_text: None,
        color: crate::design::tokens().doc_code_fg,
        font_size: 14.0,
        is_code: true,
        ..StyledSpan::plain("")
    };
    push_wrapped(spans, "`", body, "`", 14.0);
    Some(end + 1)
}

/// `**bold**` at `i`.
fn bold(chars: &[char], i: usize, spans: &mut Vec<StyledSpan>) -> Option<usize> {
    let end = find_double(chars, i + 2, '*')?;
    let body = StyledSpan {
        text: chars[i + 2..end].iter().collect(),
        color: crate::design::tokens().doc_text,
        bold: true,
        font_size: 16.0,
        ..StyledSpan::plain("")
    };
    push_wrapped(spans, "**", body, "**", 16.0);
    Some(end + 2)
}

/// `*italic*` at `i` (a single `*`, not `**`).
fn italic(chars: &[char], i: usize, spans: &mut Vec<StyledSpan>) -> Option<usize> {
    let end = find_char(chars, i + 1, '*')?;
    let body = StyledSpan {
        text: chars[i + 1..end].iter().collect(),
        color: crate::design::tokens().doc_text,
        italic: true,
        font_size: 16.0,
        ..StyledSpan::plain("")
    };
    push_wrapped(spans, "*", body, "*", 16.0);
    Some(end + 1)
}

/// `[[target|display]]` at `i`.
fn wikilink(chars: &[char], i: usize, spans: &mut Vec<StyledSpan>) -> Option<usize> {
    let end = find_double(chars, i + 2, ']')?;
    let link_text: String = chars[i + 2..end].iter().collect();
    let parts: Vec<&str> = link_text.split('|').collect();
    let target = parts[0].trim();
    let display = parts
        .get(1)
        .map(|d| d.trim())
        .filter(|d| !d.is_empty())
        .map(|d| d.to_string())
        .unwrap_or_else(|| extract_display_name(target));
    let body = StyledSpan {
        text: link_text.clone(),
        display_text: Some(display),
        color: crate::design::tokens().doc_link,
        is_link: true,
        link_target: Some(target.to_string()),
        ..StyledSpan::plain("")
    };
    push_wrapped(spans, "[[", body, "]]", 16.0);
    Some(end + 2)
}

/// `[text](url)` at `i`: one span holding the raw source, displaying the text.
fn markdown_link(chars: &[char], i: usize, spans: &mut Vec<StyledSpan>) -> Option<usize> {
    let (end_text, end_url) = bracket_then_paren(chars, i + 1)?;
    spans.push(StyledSpan {
        text: chars[i..=end_url].iter().collect(),
        display_text: Some(chars[i + 1..end_text].iter().collect()),
        color: crate::design::tokens().doc_link,
        is_link: true,
        link_target: Some(chars[end_text + 2..end_url].iter().collect()),
        ..StyledSpan::plain("")
    });
    Some(end_url + 1)
}

/// `$math$` at `i`.
fn inline_math(chars: &[char], i: usize, spans: &mut Vec<StyledSpan>) -> Option<usize> {
    let end = find_char(chars, i + 1, '$')?;
    spans.push(StyledSpan {
        text: chars[i..=end].iter().collect(),
        color: crate::design::tokens().doc_math,
        italic: true,
        font_size: 16.0,
        is_math: true,
        ..StyledSpan::plain("")
    });
    Some(end + 1)
}

/// `![alt](url)` at `i`; hidden in preview, since the image is drawn separately.
fn image(chars: &[char], i: usize, spans: &mut Vec<StyledSpan>) -> Option<usize> {
    let (end_alt, end_url) = bracket_then_paren(chars, i + 2)?;
    spans.push(StyledSpan {
        text: chars[i..=end_url].iter().collect(),
        display_text: Some(String::new()),
        color: crate::design::tokens().doc_math,
        italic: true,
        font_size: 14.0,
        is_image: true,
        image_path: Some(chars[end_alt + 2..end_url].iter().collect()),
        image_alt: Some(chars[i + 2..end_alt].iter().collect()),
        ..StyledSpan::plain("")
    });
    Some(end_url + 1)
}

/// For `…](…)` starting at `from`: the indices of `]` and of the closing `)`.
fn bracket_then_paren(chars: &[char], from: usize) -> Option<(usize, usize)> {
    let close = find_char(chars, from, ']')?;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = find_char(chars, close + 2, ')')?;
    Some((close, end))
}

pub(super) fn detect_heading(trimmed: &str) -> Option<u8> {
    if trimmed.starts_with("###### ") {
        return Some(6);
    }
    if trimmed.starts_with("##### ") {
        return Some(5);
    }
    if trimmed.starts_with("#### ") {
        return Some(4);
    }
    if trimmed.starts_with("### ") {
        return Some(3);
    }
    if trimmed.starts_with("## ") {
        return Some(2);
    }
    if trimmed.starts_with("# ") {
        return Some(1);
    }
    None
}

pub(super) fn math_block_end_for(trimmed: &str) -> Option<MathBlockEnd> {
    if trimmed.starts_with("$$") {
        Some(MathBlockEnd::Dollars)
    } else if trimmed == "\\[" {
        Some(MathBlockEnd::Bracket)
    } else {
        math_env(trimmed, "\\begin")
            .map(|environment| MathBlockEnd::Environment(environment.to_string()))
    }
}

pub(super) fn is_matching_math_end(trimmed: &str, expected: &MathBlockEnd) -> bool {
    match expected {
        MathBlockEnd::Dollars => trimmed.starts_with("$$"),
        MathBlockEnd::Bracket => trimmed == "\\]",
        MathBlockEnd::Environment(environment) => {
            math_env(trimmed, "\\end") == Some(environment.as_str())
        }
    }
}

pub(super) fn has_matching_end_on_line(trimmed: &str, environment: &str) -> bool {
    let Some(open_end) = trimmed.find('}') else {
        return false;
    };
    trimmed[open_end + 1..]
        .match_indices("\\end")
        .any(|(idx, _)| math_env(&trimmed[open_end + 1 + idx..], "\\end") == Some(environment))
}

pub(super) fn math_env<'a>(trimmed: &'a str, command: &str) -> Option<&'a str> {
    let rest = trimmed.strip_prefix(command)?.trim_start();
    let rest = rest.strip_prefix('{')?;
    let end = rest.find('}')?;
    let env = &rest[..end];
    match env {
        "equation" | "equation*" | "align" | "align*" | "aligned" | "gather" | "gather*"
        | "multline" | "multline*" | "split" | "cases" | "matrix" | "pmatrix" | "bmatrix"
        | "vmatrix" | "Vmatrix" => Some(env),
        _ => None,
    }
}

pub(super) fn single_line_display_math(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix("$$")?;
    let end = rest.rfind("$$")?;
    if end == 0 && rest.trim() == "$$" {
        return None;
    }
    let math = rest[..end].trim();
    if math.is_empty() { None } else { Some(math) }
}

pub(super) fn is_obvious_markdown_boundary(trimmed: &str) -> bool {
    if trimmed.is_empty() {
        return false;
    }
    detect_heading(trimmed).is_some()
        || trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("+ ")
        || detect_numbered_list(trimmed).is_some()
        || trimmed.starts_with("![")
}

pub(super) fn detect_numbered_list(trimmed: &str) -> Option<usize> {
    let mut i = 0;
    let bytes = trimmed.as_bytes();
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && i < bytes.len() && bytes[i] == b'.' && i + 1 < bytes.len() && bytes[i + 1] == b' ' {
        return Some(i + 2);
    }
    None
}

/// A ×1.25 modular scale from the 17px body (`pages/editor.md`): H1 33,
/// H2 27, H3 21; H4–H6 stay at body size and differ by weight and spacing.
pub(super) fn heading_size(level: u8) -> f32 {
    match level {
        1 => 33.0,
        2 => 27.0,
        3 => 21.0,
        _ => 17.0,
    }
}

pub(super) fn find_char(chars: &[char], start: usize, target: char) -> Option<usize> {
    (start..chars.len()).find(|&i| chars[i] == target)
}

pub(super) fn find_double(chars: &[char], start: usize, target: char) -> Option<usize> {
    (start..chars.len().saturating_sub(1)).find(|&i| chars[i] == target && chars[i + 1] == target)
}

pub(super) fn extract_display_name(target: &str) -> String {
    if target.starts_with('#') {
        return target.to_string();
    }
    let (path_part, anchor_part) = if let Some(idx) = target.find('#') {
        let anchor = &target[idx + 1..];
        if anchor
            .chars()
            .any(|c| matches!(c, '%' | '^' | '&' | '*' | '!' | '@' | '(' | ')'))
        {
            (target, None)
        } else {
            (&target[..idx], Some(anchor))
        }
    } else {
        (target, None)
    };
    let path_part = path_part.trim();
    let anchor_part = anchor_part.map(|s| s.trim());

    let file_name = path_part
        .split('/')
        .next_back()
        .and_then(|s| s.split('\\').next_back())
        .unwrap_or(path_part)
        .trim();

    let clean_name = if let Some(stripped) = file_name.strip_suffix(".md") {
        stripped
    } else if let Some(stripped) = file_name.strip_suffix(".markdown") {
        stripped
    } else {
        file_name
    };

    if let Some(anchor) = anchor_part {
        if path_part.is_empty() {
            format!("#{}", anchor)
        } else {
            format!("{}#{}", clean_name, anchor)
        }
    } else {
        clean_name.to_string()
    }
}

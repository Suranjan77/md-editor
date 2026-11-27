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
    if let Some(header) = callout_header(&line[rest_start..]) {
        return callout_line(sl, line, rest_start, header);
    }
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

/// `[!type]` (optionally `+`/`-` for folding) at the start of quote content:
/// the marker's byte length and the type.
fn callout_header(content: &str) -> Option<(usize, &str)> {
    let rest = content.strip_prefix("[!")?;
    let close = rest.find(']')?;
    let kind = &rest[..close];
    if kind.is_empty()
        || !kind
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let mut len = 2 + close + 1;
    if matches!(content.as_bytes().get(len), Some(b'+' | b'-')) {
        len += 1;
    }
    Some((len, kind))
}

/// The first line of a callout: the `[!type]` marker shows as the type's
/// name in its colour, followed by the (bold) title.
fn callout_line(
    mut sl: StyledLine,
    line: &str,
    content_start: usize,
    (marker_len, kind): (usize, &str),
) -> StyledLine {
    let callout = Callout::from_type(kind);
    sl.callout = Some(callout);
    let mut label: String = kind.replace(['-', '_'], " ");
    if let Some(first) = label.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    let title_start = content_start + marker_len;
    let title = &line[title_start..];
    sl.spans.push(StyledSpan {
        text: line[content_start..title_start].to_string(),
        display_text: Some(if title.trim().is_empty() {
            label
        } else {
            format!("{label} · ")
        }),
        color: callout.color(),
        bold: true,
        ..StyledSpan::plain("")
    });
    let title_from = sl.spans.len();
    parse_inline_spans(title, &mut sl.spans);
    for span in &mut sl.spans[title_from..] {
        if !span.is_syntax {
            span.bold = true;
        }
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
        let candidates: [(bool, InlineParser); 8] = [
            (c == '`', inline_code),
            (c == '~' && next == Some('~'), strikethrough),
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

/// `~~struck~~` at `i`. As in GFM, the body is not empty and neither starts
/// nor ends with whitespace or `~`, so `~~~~` and `~~ x ~~` stay text.
fn strikethrough(chars: &[char], i: usize, spans: &mut Vec<StyledSpan>) -> Option<usize> {
    let flanking = |c: &char| !c.is_whitespace() && *c != '~';
    let end = find_double(chars, i + 2, '~')
        .filter(|&end| end > i + 2 && flanking(&chars[i + 2]) && flanking(&chars[end - 1]))?;
    let body = StyledSpan {
        text: chars[i + 2..end].iter().collect(),
        color: crate::design::tokens().doc_text,
        strikethrough: true,
        font_size: 16.0,
        ..StyledSpan::plain("")
    };
    push_wrapped(spans, "~~", body, "~~", 16.0);
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
    let (end_text, dest, end_url) = bracket_then_paren(chars, i + 1)?;
    spans.push(StyledSpan {
        text: chars[i..=end_url].iter().collect(),
        display_text: Some(chars[i + 1..end_text].iter().collect()),
        color: crate::design::tokens().doc_link,
        is_link: true,
        link_target: Some(link_destination(&chars[dest].iter().collect::<String>())),
        ..StyledSpan::plain("")
    });
    Some(end_url + 1)
}

/// A Markdown link destination as the link router wants it: a vault path
/// percent-decoded (`My%20Note.md`), a URL or `pdf://` link untouched (their
/// handlers decode their own parts).
fn link_destination(raw: &str) -> String {
    if raw.contains("://") || raw.starts_with("mailto:") {
        raw.to_string()
    } else {
        crate::platform::percent_decode(raw)
    }
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
    let (end_alt, dest, end_url) = bracket_then_paren(chars, i + 2)?;
    spans.push(StyledSpan {
        text: chars[i..=end_url].iter().collect(),
        display_text: Some(String::new()),
        color: crate::design::tokens().doc_math,
        italic: true,
        font_size: 14.0,
        is_image: true,
        image_path: Some(chars[dest].iter().collect()),
        image_alt: Some(chars[i + 2..end_alt].iter().collect()),
        ..StyledSpan::plain("")
    });
    Some(end_url + 1)
}

/// For `…](destination "title")` starting at `from`: the index of `]`, the
/// destination's range (without `<>` or the title), and the index of the
/// closing `)`. The destination is `<...>` (spaces and parentheses allowed)
/// or a run without spaces whose parentheses balance, so `Paper (2024).pdf`
/// is one destination.
fn bracket_then_paren(
    chars: &[char],
    from: usize,
) -> Option<(usize, std::ops::Range<usize>, usize)> {
    let close = find_char(chars, from, ']')?;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let mut i = close + 2;
    while chars.get(i) == Some(&' ') {
        i += 1;
    }
    let dest = if chars.get(i) == Some(&'<') {
        let end = (i + 1..chars.len()).find(|&j| matches!(chars[j], '>' | '<'))?;
        if chars[end] != '>' {
            return None;
        }
        let dest = i + 1..end;
        i = end + 1;
        dest
    } else {
        let start = i;
        let mut depth = 0usize;
        while let Some(&c) = chars.get(i) {
            match c {
                '\\' if chars.get(i + 1).is_some_and(|n| matches!(n, '(' | ')')) => i += 1,
                '(' => depth += 1,
                ')' if depth == 0 => break,
                ')' => depth -= 1,
                c if c.is_whitespace() => break,
                _ => {}
            }
            i += 1;
        }
        if depth > 0 {
            return None;
        }
        start..i
    };
    // An optional title: "…", '…' or (…), then the closing parenthesis.
    while chars.get(i).is_some_and(|c| c.is_whitespace()) {
        i += 1;
    }
    if let Some(&open @ ('"' | '\'' | '(')) = chars.get(i)
        && i > dest.end
    {
        let shut = if open == '(' { ')' } else { open };
        i = find_char(chars, i + 1, shut)? + 1;
        while chars.get(i).is_some_and(|c| c.is_whitespace()) {
            i += 1;
        }
    }
    (chars.get(i) == Some(&')')).then_some((close, dest, i))
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

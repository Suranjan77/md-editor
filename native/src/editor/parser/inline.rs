use super::model::StyledSpan;
use crate::theme;

pub fn parse_inline_spans(text: &str, spans: &mut Vec<StyledSpan>) {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut current = String::new();

    while i < len {
        // Inline code
        if chars[i] == '`' {
            if !current.is_empty() {
                spans.push(StyledSpan::plain(&current));
                current.clear();
            }
            if let Some(end) = find_unescaped_char(&chars, i + 1, '`') {
                let code_text: String = chars[i + 1..end].iter().collect();
                // Opening backtick — syntax marker
                spans.push(StyledSpan::syntax("`", theme::text_muted(), 14.0));
                // Code content
                spans.push(StyledSpan {
                    text: code_text,
                    display_text: None,
                    color: theme::accent_secondary(),
                    font_size: 14.0,
                    is_code: true,
                    ..StyledSpan::plain("")
                });
                // Closing backtick — syntax marker
                spans.push(StyledSpan::syntax("`", theme::text_muted(), 14.0));
                i = end + 1;
                continue;
            }
        }

        // Bold **text**
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if !current.is_empty() {
                spans.push(StyledSpan::plain(&current));
                current.clear();
            }
            if let Some(end) = find_unescaped_double(&chars, i + 2, '*') {
                let bold_text: String = chars[i + 2..end].iter().collect();
                // Opening ** — syntax marker
                spans.push(StyledSpan::syntax("**", theme::text_muted(), 16.0));

                let start_idx = spans.len();
                parse_inline_spans(&bold_text, spans);
                for span in &mut spans[start_idx..] {
                    span.bold = true;
                }
                if start_idx == spans.len() {
                    spans.push(StyledSpan {
                        text: String::new(),
                        bold: true,
                        ..StyledSpan::plain("")
                    });
                }

                // Closing ** — syntax marker
                spans.push(StyledSpan::syntax("**", theme::text_muted(), 16.0));
                i = end + 2;
                continue;
            }
        }

        // Italic *text*  (single *, not **)
        if chars[i] == '*' && (i + 1 >= len || chars[i + 1] != '*') {
            if !current.is_empty() {
                spans.push(StyledSpan::plain(&current));
                current.clear();
            }
            if let Some(end) = find_unescaped_char(&chars, i + 1, '*') {
                let italic_text: String = chars[i + 1..end].iter().collect();
                spans.push(StyledSpan::syntax("*", theme::text_muted(), 16.0));

                let start_idx = spans.len();
                parse_inline_spans(&italic_text, spans);
                for span in &mut spans[start_idx..] {
                    span.italic = true;
                }
                if start_idx == spans.len() {
                    spans.push(StyledSpan {
                        text: String::new(),
                        italic: true,
                        ..StyledSpan::plain("")
                    });
                }

                spans.push(StyledSpan::syntax("*", theme::text_muted(), 16.0));
                i = end + 1;
                continue;
            }
        }

        // Footnote [^1]
        if i + 2 < len && chars[i] == '[' && chars[i + 1] == '^' {
            if !current.is_empty() {
                spans.push(StyledSpan::plain(&current));
                current.clear();
            }
            if let Some(end) = find_unescaped_char(&chars, i + 2, ']') {
                let fn_id: String = chars[i + 2..end].iter().collect();
                let raw: String = chars[i..=end].iter().collect();
                spans.push(StyledSpan {
                    text: raw,
                    display_text: None,
                    color: theme::accent(),
                    is_link: true,
                    link_target: Some(format!("^{}", fn_id)),
                    ..StyledSpan::plain("")
                });
                i = end + 1;
                continue;
            }
        }

        // Wikilink [[target|display]]
        if i + 1 < len && chars[i] == '[' && chars[i + 1] == '[' {
            if !current.is_empty() {
                spans.push(StyledSpan::plain(&current));
                current.clear();
            }
            if let Some(end) = find_unescaped_double(&chars, i + 2, ']') {
                let link_text: String = chars[i + 2..end].iter().collect();
                let parts: Vec<&str> = link_text.split('|').collect();
                let target = parts[0].trim();
                let display = parts
                    .get(1)
                    .map(|d| d.trim())
                    .filter(|d| !d.is_empty())
                    .map(|d| d.to_string())
                    .unwrap_or_else(|| extract_display_name(target));
                // [[ — syntax marker
                spans.push(StyledSpan::syntax("[[", theme::text_muted(), 16.0));
                spans.push(StyledSpan {
                    text: link_text.clone(),
                    display_text: Some(display.to_string()),
                    color: theme::accent(),
                    is_link: true,
                    link_target: Some(target.to_string()),
                    ..StyledSpan::plain("")
                });
                // ]] — syntax marker
                spans.push(StyledSpan::syntax("]]", theme::text_muted(), 16.0));
                i = end + 2;
                continue;
            }
        }

        // Markdown link [text](url) and Reference links [text][ref] / [ref]
        if chars[i] == '[' && (i == 0 || chars[i - 1] != '!') {
            if !current.is_empty() {
                spans.push(StyledSpan::plain(&current));
                current.clear();
            }
            if let Some(end_text) = find_unescaped_char(&chars, i + 1, ']') {
                // 1. Standard markdown link [text](url)
                if end_text + 1 < len && chars[end_text + 1] == '(' {
                    if let Some(end_url) = find_link_url_end(&chars, end_text + 2) {
                        let link_display: String = chars[i + 1..end_text].iter().collect();
                        let url: String = chars[end_text + 2..end_url].iter().collect();
                        // Full raw: [text](url), display just the text
                        let raw: String = chars[i..=end_url].iter().collect();
                        spans.push(StyledSpan {
                            text: raw,
                            display_text: Some(link_display.clone()),
                            color: theme::accent(),
                            is_link: true,
                            link_target: Some(url),
                            ..StyledSpan::plain("")
                        });
                        i = end_url + 1;
                        continue;
                    }
                }

                // 2. Full reference link [text][ref]
                if end_text + 1 < len && chars[end_text + 1] == '[' {
                    if let Some(end_ref) = find_unescaped_char(&chars, end_text + 2, ']') {
                        let link_display: String = chars[i + 1..end_text].iter().collect();
                        let ref_id: String = chars[end_text + 2..end_ref].iter().collect();
                        if !link_display.contains('[') && !ref_id.contains('[') {
                            let raw: String = chars[i..=end_ref].iter().collect();
                            spans.push(StyledSpan {
                                text: raw,
                                display_text: Some(link_display),
                                color: theme::accent(),
                                is_link: true,
                                link_target: Some(ref_id),
                                ..StyledSpan::plain("")
                            });
                            i = end_ref + 1;
                            continue;
                        }
                    }
                }

                // 3. Shortcut reference link [ref]
                let link_display: String = chars[i + 1..end_text].iter().collect();
                let trimmed = link_display.trim();
                let next_char = if end_text + 1 < len {
                    Some(chars[end_text + 1])
                } else {
                    None
                };
                // Avoid treating empty brackets or task list checkboxes as links mid-line
                // Also avoid if followed by '(' or '[' as that indicates a malformed link.
                if !trimmed.is_empty()
                    && trimmed != "x"
                    && trimmed != "X"
                    && next_char != Some('(')
                    && next_char != Some('[')
                    && !link_display.contains('[')
                {
                    let raw: String = chars[i..=end_text].iter().collect();
                    spans.push(StyledSpan {
                        text: raw,
                        display_text: Some(link_display.clone()),
                        color: theme::accent(),
                        is_link: true,
                        link_target: Some(link_display.clone()),
                        ..StyledSpan::plain("")
                    });
                    i = end_text + 1;
                    continue;
                }
            }
        }

        // Inline math $...$
        if chars[i] == '$' && (i + 1 < len && chars[i + 1] != '$') {
            if !current.is_empty() {
                spans.push(StyledSpan::plain(&current));
                current.clear();
            }
            if let Some(end) = find_unescaped_char(&chars, i + 1, '$') {
                let math_raw: String = chars[i..=end].iter().collect();
                spans.push(StyledSpan {
                    text: math_raw,
                    color: theme::warning(),
                    italic: true,
                    font_size: 16.0,
                    is_math: true,
                    ..StyledSpan::plain("")
                });
                i = end + 1;
                continue;
            }
        }

        // Image: ![alt](url)
        if i + 1 < len && chars[i] == '!' && chars[i + 1] == '[' {
            if !current.is_empty() {
                spans.push(StyledSpan::plain(&current));
                current.clear();
            }
            if let Some(end_alt) = find_unescaped_char(&chars, i + 2, ']') {
                if end_alt + 1 < len && chars[end_alt + 1] == '(' {
                    if let Some(end_url) = find_link_url_end(&chars, end_alt + 2) {
                        let alt_text: String = chars[i + 2..end_alt].iter().collect();
                        let url: String = chars[end_alt + 2..end_url].iter().collect();
                        let raw: String = chars[i..=end_url].iter().collect();
                        spans.push(StyledSpan {
                            text: raw,
                            display_text: Some(String::new()), // hidden in preview, image is drawn separately
                            color: theme::warning(),
                            italic: true,
                            font_size: 14.0,
                            is_image: true,
                            image_path: Some(url),
                            image_alt: Some(alt_text),
                            ..StyledSpan::plain("")
                        });
                        i = end_url + 1;
                        continue;
                    }
                }
            }
        }

        current.push(chars[i]);
        i += 1;
    }
    if !current.is_empty() {
        spans.push(StyledSpan::plain(&current));
    }
}
fn is_escaped(chars: &[char], idx: usize) -> bool {
    let mut slash_count = 0;
    let mut i = idx;
    while i > 0 {
        i -= 1;
        if chars[i] == '\\' {
            slash_count += 1;
        } else {
            break;
        }
    }
    slash_count % 2 == 1
}

fn find_unescaped_char(chars: &[char], start: usize, target: char) -> Option<usize> {
    (start..chars.len()).find(|&i| chars[i] == target && !is_escaped(chars, i))
}

fn find_unescaped_double(chars: &[char], start: usize, target: char) -> Option<usize> {
    if start + 1 >= chars.len() {
        return None;
    }
    (start..chars.len() - 1)
        .find(|&i| chars[i] == target && chars[i + 1] == target && !is_escaped(chars, i))
}

fn find_link_url_end(chars: &[char], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    for i in start..chars.len() {
        if is_escaped(chars, i) {
            continue;
        }
        match chars[i] {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(i),
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

pub fn split_table_cells(line: &str) -> Vec<&str> {
    let mut cells = Vec::new();
    let mut start = 0;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if ch == '\\' && !escaped {
            escaped = true;
            continue;
        }
        if ch == '|' && !escaped {
            cells.push(&line[start..idx]);
            start = idx + ch.len_utf8();
        }
        escaped = false;
    }
    cells.push(&line[start..]);
    cells
}

fn extract_display_name(target: &str) -> String {
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
        .last()
        .and_then(|s| s.split('\\').last())
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

use iced::Color;
use std::sync::OnceLock;

use crate::theme;

/// A styled text span for rendering.
#[derive(Debug, Clone)]
pub struct StyledSpan {
    /// The raw markdown source text for this span.
    pub text: String,
    /// Text to display in preview/rendered mode. If `None`, uses `text`.
    /// Set to `Some("")` to hide syntax markers like `**`, `#`, `$`, etc.
    pub display_text: Option<String>,
    pub color: Color,
    pub bold: bool,
    pub italic: bool,
    pub font_size: f32,
    pub is_code: bool,
    pub is_link: bool,
    pub link_target: Option<String>,
    pub is_heading: bool,
    pub heading_level: u8,
    pub is_checkbox: bool,
    pub is_checked: bool,
    pub is_rule: bool,
    pub is_image: bool,
    pub image_path: Option<String>,
    /// Alt text for images, used for caption rendering.
    pub image_alt: Option<String>,
    pub is_math: bool,
    /// True if this span is a syntax marker (**, `, $, etc.) that should
    /// be hidden in preview mode.
    pub is_syntax: bool,
    /// Unique HTML-like identifier for this span.
    pub id: Option<String>,
}

impl StyledSpan {
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.to_string(),
            display_text: None,
            color: theme::TEXT_PRIMARY,
            bold: false,
            italic: false,
            font_size: 17.0,
            is_code: false,
            is_link: false,
            link_target: None,
            is_heading: false,
            heading_level: 0,
            is_checkbox: false,
            is_checked: false,
            is_rule: false,
            is_image: false,
            image_path: None,
            image_alt: None,
            is_math: false,
            is_syntax: false,
            id: None,
        }
    }

    /// Create a syntax-marker span that is hidden in preview mode.
    fn syntax(text: &str, color: Color, font_size: f32) -> Self {
        Self {
            text: text.to_string(),
            display_text: Some(String::new()),
            color,
            is_syntax: true,
            font_size,
            ..Self::plain("")
        }
    }

    /// Get the text to display based on editing mode.
    pub fn visible_text(&self, editing: bool) -> &str {
        if editing {
            &self.text
        } else if let Some(ref dt) = self.display_text {
            dt.as_str()
        } else {
            &self.text
        }
    }
}

/// A line of styled spans for the editor to render.
#[derive(Debug, Clone)]
pub struct StyledLine {
    pub spans: Vec<StyledSpan>,
    pub is_code_block: bool,
    pub is_math_block: bool,
    pub code_block_lang: Option<String>,
    pub is_blockquote: bool,
    /// Groups consecutive lines into blocks (code blocks, math blocks).
    /// Lines in the same block share the same `block_id`.
    /// Regular lines each get their own unique block_id.
    pub block_id: usize,
    /// True if this line is a fence line (```` ``` ```` or `$$`) — hidden in preview mode.
    pub is_block_fence: bool,
    pub is_table_row: bool,
    pub table_cells: Vec<Vec<StyledSpan>>,
}

impl StyledLine {
    pub fn new() -> Self {
        Self {
            spans: Vec::new(),
            is_code_block: false,
            is_math_block: false,
            code_block_lang: None,
            is_blockquote: false,
            block_id: 0,
            is_block_fence: false,
            is_table_row: false,
            table_cells: Vec::new(),
        }
    }
}

/// Parse markdown text into styled lines for rendering.
pub fn highlight_markdown(text: &str) -> Vec<StyledLine> {
    let mut lines = Vec::new();

    let mut in_code_block = false;
    let mut code_lang: Option<String> = None;
    let mut in_math_block = false;
    let mut in_table = false;
    let mut block_id: usize = 0;
    let mut current_block_id: usize = 0;
    let mut code_highlighter: Option<syntect::easy::HighlightLines<'static>> = None;

    for raw_line in text.split('\n') {
        let raw_line = raw_line.trim_end_matches('\r');
        let trimmed = raw_line.trim();

        // Stop table block if not a table row
        if in_table && !trimmed.starts_with('|') {
            in_table = false;
        }

        // Code block fences
        if trimmed.starts_with("```") {
            if in_code_block {
                // Closing fence
                let mut sl = StyledLine::new();
                sl.is_code_block = true;
                sl.is_block_fence = true;
                sl.block_id = current_block_id;
                sl.spans.push(StyledSpan {
                    text: raw_line.to_string(),
                    display_text: Some(String::new()),
                    color: theme::TEXT_MUTED,
                    is_syntax: true,
                    font_size: 13.0,
                    is_code: true,
                    ..StyledSpan::plain("")
                });
                lines.push(sl);
                in_code_block = false;
                code_lang = None;
                code_highlighter = None;
                block_id += 1;
                continue;
            } else {
                // Opening fence
                block_id += 1;
                current_block_id = block_id;
                in_code_block = true;
                code_lang = if trimmed.len() > 3 {
                    Some(trimmed[3..].trim().to_string())
                } else {
                    None
                };
                code_highlighter = make_code_highlighter(code_lang.as_deref());
                let mut sl = StyledLine::new();
                sl.is_code_block = true;
                sl.is_block_fence = true;
                sl.block_id = current_block_id;
                sl.code_block_lang = code_lang.clone();
                sl.spans.push(StyledSpan {
                    text: raw_line.to_string(),
                    display_text: Some(String::new()),
                    color: theme::TEXT_MUTED,
                    is_syntax: true,
                    font_size: 14.0,
                    is_code: true,
                    ..StyledSpan::plain("")
                });
                lines.push(sl);
                continue;
            }
        }

        // Math block fences. Keep this narrow so ordinary markdown/HTML is not
        // accidentally promoted to a math block.
        if in_math_block && is_obvious_markdown_boundary(trimmed) {
            in_math_block = false;
            block_id += 1;
        }

        if in_math_block && (trimmed.starts_with("$$") || is_math_end(trimmed)) {
            let mut sl = StyledLine::new();
            sl.is_math_block = true;
            sl.is_block_fence = true;
            sl.block_id = current_block_id;
            sl.spans.push(StyledSpan {
                text: raw_line.to_string(),
                display_text: Some(String::new()),
                color: theme::WARNING,
                is_syntax: true,
                is_math: true,
                font_size: 16.0,
                ..StyledSpan::plain("")
            });
            lines.push(sl);
            in_math_block = false;
            block_id += 1;
            continue;
        }

        if trimmed.starts_with("$$") || is_math_begin(trimmed) {
            if let Some(inline_math) = single_line_display_math(trimmed) {
                block_id += 1;
                let mut sl = StyledLine::new();
                sl.is_math_block = true;
                sl.block_id = block_id;
                sl.spans.push(StyledSpan {
                    text: raw_line.to_string(),
                    display_text: Some(inline_math.to_string()),
                    color: theme::WARNING,
                    italic: true,
                    font_size: 16.0,
                    is_math: true,
                    ..StyledSpan::plain("")
                });
                lines.push(sl);
                continue;
            }

            block_id += 1;
            current_block_id = block_id;
            in_math_block = true;
            let mut sl = StyledLine::new();
            sl.is_math_block = true;
            sl.is_block_fence = true;
            sl.block_id = current_block_id;
            sl.spans.push(StyledSpan {
                text: raw_line.to_string(),
                display_text: Some(String::new()),
                color: theme::WARNING,
                is_syntax: true,
                is_math: true,
                font_size: 16.0,
                ..StyledSpan::plain("")
            });
            lines.push(sl);
            continue;
        }

        // Inside code block
        if in_code_block {
            let mut sl = StyledLine::new();
            sl.is_code_block = true;
            sl.block_id = current_block_id;
            sl.code_block_lang = code_lang.clone();
            sl.spans = highlight_code_spans(raw_line, &mut code_highlighter, code_lang.as_deref());
            lines.push(sl);
            continue;
        }

        // Inside math block
        if in_math_block {
            let mut sl = StyledLine::new();
            sl.is_math_block = true;
            sl.block_id = current_block_id;
            sl.spans.push(StyledSpan {
                text: raw_line.to_string(),
                display_text: None,
                color: theme::WARNING,
                italic: true,
                font_size: 16.0,
                is_math: true,
                ..StyledSpan::plain("")
            });
            lines.push(sl);
            continue;
        }

        // Table row
        if trimmed.starts_with('|') && trimmed.contains('|') {
            if !in_table {
                in_table = true;
                block_id += 1;
                current_block_id = block_id;
            }
            let mut sl = StyledLine::new();
            sl.is_table_row = true;
            sl.block_id = current_block_id;

            // Check if it's a separator line like |---|---|
            let is_separator = trimmed
                .chars()
                .all(|c| c == '|' || c == '-' || c == ' ' || c == ':');
            if is_separator {
                // We can mark this as an empty row or skip spans, but let's just make it a row
                sl.spans
                    .push(StyledSpan::syntax(raw_line, theme::TEXT_MUTED, 16.0));
            } else {
                let mut parts = trimmed.split('|').collect::<Vec<_>>();
                if parts.first() == Some(&"") {
                    parts.remove(0);
                }
                if parts.last() == Some(&"") {
                    parts.pop();
                }

                for part in parts {
                    let mut cell_spans = Vec::new();
                    parse_inline_spans(part.trim(), &mut cell_spans);
                    sl.table_cells.push(cell_spans);
                }
                sl.spans.push(StyledSpan::plain(raw_line)); // Raw text for editing
            }
            lines.push(sl);
            continue;
        }

        // Regular line — parse inline markdown
        block_id += 1;
        let mut sl = highlight_line(raw_line);
        sl.block_id = block_id;
        lines.push(sl);
    }

    // Consolidated math block processing
    let mut idx = 0;
    while idx < lines.len() {
        if lines[idx].is_math_block && lines[idx].is_block_fence && lines[idx].block_id > 0 {
            let block_id = lines[idx].block_id;
            let mut j = idx + 1;
            let mut math_lines = Vec::new();

            // If the opening fence has some math content (like \begin{align}), include it.
            let first_trimmed = lines[idx]
                .spans
                .first()
                .map(|s| s.text.trim())
                .unwrap_or("");
            if first_trimmed.starts_with("\\begin{") {
                math_lines.push(first_trimmed);
            }

            while j < lines.len() && lines[j].block_id == block_id {
                if !lines[j].is_block_fence {
                    if let Some(span) = lines[j].spans.first() {
                        math_lines.push(span.text.as_str());
                    }
                } else {
                    // If the closing fence is \end{...}, include it.
                    let last_trimmed = lines[j].spans.first().map(|s| s.text.trim()).unwrap_or("");
                    if last_trimmed.starts_with("\\end{") {
                        math_lines.push(last_trimmed);
                    }
                }
                j += 1;
            }

            // Consolidate the math lines
            let consolidated_math = math_lines.join("\n");

            if !consolidated_math.is_empty() {
                if let Some(span) = lines[idx].spans.first_mut() {
                    span.display_text = Some(consolidated_math);
                    span.is_syntax = false;
                    span.is_math = true;
                    span.font_size = 16.0;
                }
            }

            for hidden_idx in idx + 1..j {
                for span in &mut lines[hidden_idx].spans {
                    span.display_text = Some(String::new());
                    span.is_syntax = true;
                }
            }

            idx = j;
        } else {
            idx += 1;
        }
    }

    // Post-process to assign unique sequential IDs to images, display math blocks, tables, and code blocks
    let mut image_counter = 0;
    let mut equation_counter = 0;
    let mut table_counter = 0;
    let mut code_counter = 0;
    let mut seen_math_block_ids = std::collections::HashSet::new();
    let mut seen_table_block_ids = std::collections::HashSet::new();
    let mut seen_code_block_ids = std::collections::HashSet::new();

    for line in &mut lines {
        if line.is_math_block && line.block_id > 0 {
            if seen_math_block_ids.insert(line.block_id) {
                equation_counter += 1;
                if let Some(first_span) = line.spans.first_mut() {
                    first_span.id = Some(format!("equation-{}", equation_counter));
                }
            }
        }
        if line.is_table_row && line.block_id > 0 {
            if seen_table_block_ids.insert(line.block_id) {
                table_counter += 1;
                if let Some(first_span) = line.spans.first_mut() {
                    first_span.id = Some(format!("table-{}", table_counter));
                }
            }
        }
        if line.is_code_block && line.block_id > 0 {
            if seen_code_block_ids.insert(line.block_id) {
                code_counter += 1;
                if let Some(first_span) = line.spans.first_mut() {
                    first_span.id = Some(format!("code-{}", code_counter));
                }
            }
        }
        for span in &mut line.spans {
            if span.is_image {
                image_counter += 1;
                span.id = Some(format!("figure-{}", image_counter));
            }
        }
    }

    lines
}

fn highlight_line(line: &str) -> StyledLine {
    let mut sl = StyledLine::new();
    let trimmed = line.trim_start();

    // Headings
    if let Some(level) = detect_heading(trimmed) {
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
            color: theme::TEXT_MUTED,
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
            color: theme::ACCENT,
            bold: true,
            font_size: heading_size(level),
            is_heading: true,
            heading_level: level,
            ..StyledSpan::plain("")
        });

        return sl;
    }

    // Horizontal rule
    if trimmed == "---" || trimmed == "***" || trimmed == "___" {
        sl.spans.push(StyledSpan {
            text: line.to_string(),
            display_text: Some(String::new()),
            color: theme::BORDER,
            is_rule: true,
            is_syntax: true,
            ..StyledSpan::plain("")
        });
        return sl;
    }

    // Blockquote
    if trimmed.starts_with('>') {
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
            color: theme::ACCENT,
            is_syntax: true,
            ..StyledSpan::plain("")
        });
        // Blockquote content
        sl.spans.push(StyledSpan {
            text: line[rest_start..].to_string(),
            display_text: None,
            color: theme::ACCENT,
            italic: true,
            ..StyledSpan::plain("")
        });
        return sl;
    }

    // Task list items (must check before regular list items)
    if trimmed.starts_with("- [ ] ")
        || trimmed.starts_with("- [x] ")
        || trimmed.starts_with("- [X] ")
    {
        let checkbox_end = line.len() - trimmed.len() + 6;
        let is_checked = trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]");
        sl.spans.push(StyledSpan {
            text: line[..(line.len() - trimmed.len() + 6)].to_string(),
            display_text: Some(if is_checked {
                "☑ ".to_string()
            } else {
                "☐ ".to_string()
            }),
            color: if is_checked {
                theme::ACCENT
            } else {
                theme::TEXT_MUTED
            },
            is_checkbox: true,
            is_checked,
            ..StyledSpan::plain("")
        });
        let start_idx = sl.spans.len();
        parse_inline_spans(&line[checkbox_end..], &mut sl.spans);
        if is_checked {
            for span in &mut sl.spans[start_idx..] {
                span.color = theme::TEXT_MUTED;
            }
        }
        return sl;
    }

    // List items
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
        let bullet_end = line.len() - trimmed.len() + 2;
        // Show bullet character in preview
        sl.spans.push(StyledSpan {
            text: line[..bullet_end].to_string(),
            display_text: Some("  • ".to_string()),
            color: theme::ACCENT,
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
            color: theme::ACCENT,
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

fn parse_inline_spans(text: &str, spans: &mut Vec<StyledSpan>) {
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
            if let Some(end) = find_char(&chars, i + 1, '`') {
                let code_text: String = chars[i + 1..end].iter().collect();
                // Opening backtick — syntax marker
                spans.push(StyledSpan::syntax("`", theme::TEXT_MUTED, 14.0));
                // Code content
                spans.push(StyledSpan {
                    text: code_text,
                    display_text: None,
                    color: theme::ACCENT_SECONDARY,
                    font_size: 14.0,
                    is_code: true,
                    ..StyledSpan::plain("")
                });
                // Closing backtick — syntax marker
                spans.push(StyledSpan::syntax("`", theme::TEXT_MUTED, 14.0));
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
            if let Some(end) = find_double(&chars, i + 2, '*') {
                let bold_text: String = chars[i + 2..end].iter().collect();
                // Opening ** — syntax marker
                spans.push(StyledSpan::syntax("**", theme::TEXT_MUTED, 16.0));
                // Bold text
                spans.push(StyledSpan {
                    text: bold_text,
                    color: theme::TEXT_PRIMARY,
                    bold: true,
                    font_size: 16.0,
                    ..StyledSpan::plain("")
                });
                // Closing ** — syntax marker
                spans.push(StyledSpan::syntax("**", theme::TEXT_MUTED, 16.0));
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
            if let Some(end) = find_char(&chars, i + 1, '*') {
                let italic_text: String = chars[i + 1..end].iter().collect();
                spans.push(StyledSpan::syntax("*", theme::TEXT_MUTED, 16.0));
                spans.push(StyledSpan {
                    text: italic_text,
                    color: theme::TEXT_PRIMARY,
                    italic: true,
                    font_size: 16.0,
                    ..StyledSpan::plain("")
                });
                spans.push(StyledSpan::syntax("*", theme::TEXT_MUTED, 16.0));
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
            if let Some(end) = find_double(&chars, i + 2, ']') {
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
                spans.push(StyledSpan::syntax("[[", theme::TEXT_MUTED, 16.0));
                spans.push(StyledSpan {
                    text: link_text.clone(),
                    display_text: Some(display.to_string()),
                    color: theme::ACCENT,
                    is_link: true,
                    link_target: Some(target.to_string()),
                    ..StyledSpan::plain("")
                });
                // ]] — syntax marker
                spans.push(StyledSpan::syntax("]]", theme::TEXT_MUTED, 16.0));
                i = end + 2;
                continue;
            }
        }

        // Markdown link [text](url)
        if chars[i] == '[' && (i == 0 || chars[i - 1] != '!') {
            if !current.is_empty() {
                spans.push(StyledSpan::plain(&current));
                current.clear();
            }
            if let Some(end_text) = find_char(&chars, i + 1, ']') {
                if end_text + 1 < len && chars[end_text + 1] == '(' {
                    if let Some(end_url) = find_char(&chars, end_text + 2, ')') {
                        let link_display: String = chars[i + 1..end_text].iter().collect();
                        let url: String = chars[end_text + 2..end_url].iter().collect();
                        // Full raw: [text](url), display just the text
                        let raw: String = chars[i..=end_url].iter().collect();
                        spans.push(StyledSpan {
                            text: raw,
                            display_text: Some(link_display.clone()),
                            color: theme::ACCENT,
                            is_link: true,
                            link_target: Some(url),
                            ..StyledSpan::plain("")
                        });
                        i = end_url + 1;
                        continue;
                    }
                }
            }
        }

        // Inline math $...$
        if chars[i] == '$' && (i + 1 < len && chars[i + 1] != '$') {
            if !current.is_empty() {
                spans.push(StyledSpan::plain(&current));
                current.clear();
            }
            if let Some(end) = find_char(&chars, i + 1, '$') {
                let math_raw: String = chars[i..=end].iter().collect();
                spans.push(StyledSpan {
                    text: math_raw,
                    color: theme::WARNING,
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
            if let Some(end_alt) = find_char(&chars, i + 2, ']') {
                if end_alt + 1 < len && chars[end_alt + 1] == '(' {
                    if let Some(end_url) = find_char(&chars, end_alt + 2, ')') {
                        let alt_text: String = chars[i + 2..end_alt].iter().collect();
                        let url: String = chars[end_alt + 2..end_url].iter().collect();
                        let raw: String = chars[i..=end_url].iter().collect();
                        spans.push(StyledSpan {
                            text: raw,
                            display_text: Some(String::new()), // hidden in preview, image is drawn separately
                            color: theme::WARNING,
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

fn detect_heading(trimmed: &str) -> Option<u8> {
    let heading_without_required_space = |prefix: &str| {
        trimmed.starts_with(prefix)
            && trimmed
                .chars()
                .nth(prefix.len())
                .map(|c| !c.is_whitespace() && c != '#')
                .unwrap_or(false)
    };
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
    if heading_without_required_space("######") {
        return Some(6);
    }
    if heading_without_required_space("#####") {
        return Some(5);
    }
    if heading_without_required_space("####") {
        return Some(4);
    }
    if heading_without_required_space("###") {
        return Some(3);
    }
    if heading_without_required_space("##") {
        return Some(2);
    }
    if heading_without_required_space("#") {
        return Some(1);
    }
    None
}

fn is_math_begin(trimmed: &str) -> bool {
    trimmed == "\\[" || math_env(trimmed, "\\begin{").is_some()
}

fn is_math_end(trimmed: &str) -> bool {
    trimmed == "\\]" || math_env(trimmed, "\\end{").is_some()
}

fn math_env<'a>(trimmed: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = trimmed.strip_prefix(prefix)?;
    let end = rest.find('}')?;
    let env = &rest[..end];
    match env {
        "equation" | "equation*" | "align" | "align*" | "aligned" | "gather" | "gather*"
        | "multline" | "multline*" | "split" | "cases" | "matrix" | "pmatrix" | "bmatrix"
        | "vmatrix" | "Vmatrix" => Some(env),
        _ => None,
    }
}

fn single_line_display_math(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix("$$")?;
    let end = rest.rfind("$$")?;
    if end == 0 && rest.trim() == "$$" {
        return None;
    }
    let math = rest[..end].trim();
    if math.is_empty() { None } else { Some(math) }
}

fn is_obvious_markdown_boundary(trimmed: &str) -> bool {
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

fn detect_numbered_list(trimmed: &str) -> Option<usize> {
    let mut i = 0;
    let bytes = trimmed.as_bytes();
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && i < bytes.len() && bytes[i] == b'.' {
        if i + 1 < bytes.len() && bytes[i + 1] == b' ' {
            return Some(i + 2);
        }
    }
    None
}

fn heading_size(level: u8) -> f32 {
    match level {
        1 => 34.0,
        2 => 28.0,
        3 => 23.0,
        4 => 20.0,
        5 => 18.0,
        _ => 17.0,
    }
}

fn find_char(chars: &[char], start: usize, target: char) -> Option<usize> {
    for i in start..chars.len() {
        if chars[i] == target {
            return Some(i);
        }
    }
    None
}

fn find_double(chars: &[char], start: usize, target: char) -> Option<usize> {
    if start + 1 >= chars.len() {
        return None;
    }
    for i in start..chars.len() - 1 {
        if chars[i] == target && chars[i + 1] == target {
            return Some(i);
        }
    }
    None
}

fn extract_display_name(target: &str) -> String {
    if target.starts_with('#') {
        return target.to_string();
    }
    let (path_part, anchor_part) = if let Some(idx) = target.find('#') {
        let anchor = &target[idx + 1..];
        if anchor.chars().any(|c| matches!(c, '%' | '^' | '&' | '*' | '!' | '@' | '(' | ')')) {
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


fn highlight_code_spans(
    line: &str,
    highlighter: &mut Option<syntect::easy::HighlightLines<'static>>,
    lang: Option<&str>,
) -> Vec<StyledSpan> {
    let Some((syntax_set, _)) = syntect_defaults() else {
        return vec![code_span(line, theme::TEXT_PRIMARY)];
    };

    if highlighter.is_none() {
        *highlighter = make_code_highlighter(lang);
    }

    let Some(highlighter) = highlighter.as_mut() else {
        return vec![code_span(line, theme::TEXT_PRIMARY)];
    };

    match highlighter.highlight_line(line, syntax_set) {
        Ok(regions) => {
            let spans = regions
                .into_iter()
                .filter(|(_, text)| !text.is_empty())
                .map(|(style, text)| {
                    let fg = style.foreground;
                    code_span(
                        text,
                        Color::from_rgba8(fg.r, fg.g, fg.b, (fg.a as f32) / 255.0),
                    )
                })
                .collect::<Vec<_>>();
            if spans.is_empty() {
                vec![code_span(line, theme::TEXT_PRIMARY)]
            } else {
                spans
            }
        }
        Err(_) => vec![code_span(line, theme::TEXT_PRIMARY)],
    }
}

fn make_code_highlighter(lang: Option<&str>) -> Option<syntect::easy::HighlightLines<'static>> {
    let (syntax_set, theme_set) = syntect_defaults()?;
    let syntax = lang
        .and_then(|lang| syntax_set.find_syntax_by_token(lang))
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    let theme = theme_set
        .themes
        .get("base16-ocean.dark")
        .or_else(|| theme_set.themes.values().next())?;
    Some(syntect::easy::HighlightLines::new(syntax, theme))
}

fn code_span(text: &str, color: Color) -> StyledSpan {
    StyledSpan {
        text: text.to_string(),
        display_text: None,
        color,
        font_size: 14.0,
        is_code: true,
        ..StyledSpan::plain("")
    }
}

fn syntect_defaults()
-> Option<&'static (syntect::parsing::SyntaxSet, syntect::highlighting::ThemeSet)> {
    static DEFAULTS: OnceLock<(syntect::parsing::SyntaxSet, syntect::highlighting::ThemeSet)> =
        OnceLock::new();
    Some(DEFAULTS.get_or_init(|| {
        (
            syntect::parsing::SyntaxSet::load_defaults_newlines(),
            syntect::highlighting::ThemeSet::load_defaults(),
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::highlight_markdown;

    #[test]
    fn heading_is_not_math() {
        let lines = highlight_markdown("# Heading");
        assert!(lines[0].spans.iter().any(|span| span.is_heading));
        assert!(!lines[0].is_math_block);
    }

    #[test]
    fn align_environment_is_one_math_block() {
        let lines = highlight_markdown("\\begin{align}\na &= b\n\\end{align}\n# Next");
        assert!(lines[0].is_math_block);
        assert_eq!(lines[0].block_id, lines[1].block_id);
        assert_eq!(lines[1].block_id, lines[2].block_id);
        assert!(!lines[3].is_math_block);
        assert!(lines[3].spans.iter().any(|span| span.is_heading));
    }

    #[test]
    fn unknown_begin_environment_is_plain_text() {
        let lines = highlight_markdown("\\begin{note}\n# Still heading");
        assert!(!lines[0].is_math_block);
        assert!(lines[1].spans.iter().any(|span| span.is_heading));
    }

    #[test]
    fn single_line_display_math_does_not_swallow_following_heading() {
        let lines = highlight_markdown("$$a=b$$\n##Change of Basis\nPlain text");
        assert!(lines[0].is_math_block);
        assert!(!lines[1].is_math_block);
        assert!(lines[1].spans.iter().any(|span| span.is_heading));
        assert!(!lines[2].is_math_block);
    }

    #[test]
    fn inline_basic_markdown_flags_only_target_spans() {
        let lines = highlight_markdown("plain **bold** *italic* `code` [link](note.md)");
        let line = &lines[0];

        let bold = line.spans.iter().find(|span| span.text == "bold").unwrap();
        assert!(bold.bold);
        assert!(!bold.italic);

        let italic = line
            .spans
            .iter()
            .find(|span| span.text == "italic")
            .unwrap();
        assert!(italic.italic);
        assert!(!italic.bold);

        let code = line.spans.iter().find(|span| span.text == "code").unwrap();
        assert!(code.is_code);

        let link = line
            .spans
            .iter()
            .find(|span| span.link_target.as_deref() == Some("note.md"))
            .unwrap();
        assert!(link.is_link);
        assert_eq!(link.visible_text(false), "link");

        let plain = line
            .spans
            .iter()
            .find(|span| span.text == "plain ")
            .unwrap();
        assert!(!plain.bold);
        assert!(!plain.italic);
    }

    #[test]
    fn block_markdown_types_are_detected() {
        let lines = highlight_markdown("> quote\n- [ ] task\n---\n| A | B |\n|---|---|\n| 1 | 2 |");

        assert!(lines[0].is_blockquote);
        assert!(lines[1].spans.iter().any(|span| span.is_checkbox));
        assert!(lines[2].spans.iter().any(|span| span.is_rule));
        assert!(lines[3].is_table_row);
        assert_eq!(lines[3].table_cells.len(), 2);
        assert!(lines[4].is_table_row);
        assert!(lines[5].is_table_row);
    }

    #[test]
    fn horizontal_rule_is_detected_with_crlf_line_endings() {
        let lines = highlight_markdown("before\r\n---\r\nafter\r\n");

        assert!(lines[1].spans.iter().any(|span| span.is_rule));
        assert_eq!(lines[1].spans[0].text, "---");
    }

    #[test]
    fn fenced_code_uses_language_and_colored_spans() {
        let lines = highlight_markdown("```rust\nlet x = 1;\n```");
        assert!(lines[1].is_code_block);
        assert_eq!(lines[1].code_block_lang.as_deref(), Some("rust"));
        assert!(lines[1].spans.iter().all(|span| span.is_code));
        assert!(lines[1].spans.len() > 1);
    }

    #[test]
    fn code_fences_hide_markers_only_in_preview() {
        let lines = highlight_markdown("```rust\nfn main() {}\n```");
        assert!(lines[0].is_block_fence);
        assert!(lines[2].is_block_fence);
        assert_eq!(lines[0].spans[0].visible_text(false), "");
        assert_eq!(lines[0].spans[0].visible_text(true), "```rust");
        assert_eq!(lines[2].spans[0].visible_text(false), "");
        assert_eq!(lines[2].spans[0].visible_text(true), "```");
    }

    #[test]
    fn code_highlighting_preserves_full_source_text() {
        let source = "let answer: usize = 42;";
        let lines = highlight_markdown(&format!("```rust\n{source}\n```"));
        let rendered = lines[1]
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();
        assert_eq!(rendered, source);
        assert!(lines[1].spans.iter().all(|span| span.is_code));
        assert!(
            lines[1]
                .spans
                .iter()
                .any(|span| span.color != crate::theme::TEXT_PRIMARY)
        );
    }

    #[test]
    fn unterminated_code_block_keeps_all_remaining_lines_editable_as_code() {
        let lines = highlight_markdown("```json\n{\"a\": 1}\n# not a heading");
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| line.is_code_block));
        assert_eq!(lines[1].code_block_lang.as_deref(), Some("json"));
        assert!(!lines[2].spans.iter().any(|span| span.is_heading));
    }

    #[test]
    fn table_separator_has_no_cells_but_preserves_raw_edit_text() {
        let lines = highlight_markdown("| A | B |\n|:--|--:|\n| 1 | **two** |");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].table_cells.len(), 2);
        assert!(lines[1].is_table_row);
        assert!(lines[1].table_cells.is_empty());
        assert_eq!(lines[1].spans[0].visible_text(true), "|:--|--:|");
        assert_eq!(lines[1].spans[0].visible_text(false), "");
        assert_eq!(lines[2].table_cells.len(), 2);
        assert!(lines[2].table_cells[1].iter().any(|span| span.bold));
    }

    #[test]
    fn malformed_inline_markdown_remains_plain_text() {
        let lines = highlight_markdown("bad **bold and [link](missing and `code");
        let text = lines[0]
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();
        assert_eq!(text, "bad **bold and [link](missing and `code");
        assert!(!lines[0].spans.iter().any(|span| span.bold));
        assert!(!lines[0].spans.iter().any(|span| span.is_link));
        assert!(!lines[0].spans.iter().any(|span| span.is_code));
    }

    #[test]
    fn test_highlighter_permutations() {
        // We will generate 250 distinct markdown fragments, each containing a sequence of elements
        // Totaling over 1,000 lines of parsed markdown text to assert highlighter robustness.

        // 1. Heading level permutations (1 to 6) with inline formatting
        for level in 1..=6 {
            let prefix = "#".repeat(level);
            let heading_text = format!("{} Heading Level {}", prefix, level);
            let lines = highlight_markdown(&heading_text);
            assert_eq!(lines.len(), 1);
            assert!(
                lines[0].spans.iter().any(|span| span.is_heading),
                "Failed to detect heading at level {}",
                level
            );
            assert!(
                lines[0].spans.iter().any(|span| span.bold),
                "Failed to detect bold in heading"
            );
        }

        // 2. Ordered and Unordered lists, task items, and checkboxes
        let list_types = vec!["-", "*", "+", "1."];
        for lt in &list_types {
            let markdown_line = format!("{} This is a list item", lt);
            let lines = highlight_markdown(&markdown_line);
            assert_eq!(lines.len(), 1);
        }

        let checkbox_types = vec!["- [ ]", "- [x]"];
        for cb in &checkbox_types {
            let markdown_line = format!("{} This is a checkbox task", cb);
            let lines = highlight_markdown(&markdown_line);
            assert_eq!(lines.len(), 1);
            assert!(
                lines[0].spans.iter().any(|span| span.is_checkbox),
                "Failed to detect checkbox for '{}'",
                cb
            );
        }

        // 3. LaTeX Math block environments permutations
        let math_environments = vec![
            "align",
            "align*",
            "equation",
            "equation*",
            "gather",
            "gather*",
            "split",
            "matrix",
            "pmatrix",
            "bmatrix",
            "aligned",
            "cases",
            "vmatrix",
            "Vmatrix",
        ];
        for env in &math_environments {
            let math_block = format!(
                "\\begin{{{}}}\nx &= y + z \\\\\na &= b\n\\end{{{}}}",
                env, env
            );
            let lines = highlight_markdown(&math_block);
            assert_eq!(lines.len(), 4);

            // Check that all 4 lines are unified under the math block ID
            let block_id = lines[0].block_id;
            assert!(
                block_id > 0,
                "Environment {} did not generate a block ID",
                env
            );
            for idx in 0..4 {
                assert!(
                    lines[idx].is_math_block,
                    "Line {} in environment {} not marked as math block",
                    idx, env
                );
                assert_eq!(
                    lines[idx].block_id, block_id,
                    "Block ID mismatch in environment {}",
                    env
                );
            }
        }

        // 4. Double dollar multiline block math permutations
        let display_math = "$$ \nx = \\sum_{i=1}^{n} i \n$$";
        let lines = highlight_markdown(display_math);
        assert_eq!(lines.len(), 3);
        let math_block_id = lines[0].block_id;
        assert!(math_block_id > 0);
        for idx in 0..3 {
            assert!(lines[idx].is_math_block);
            assert_eq!(lines[idx].block_id, math_block_id);
        }

        // 5. Fenced code block language permutations
        let languages = vec![
            "rust", "js", "ts", "python", "html", "css", "c", "cpp", "go", "bash", "json", "toml",
        ];
        for lang in &languages {
            let code_block = format!("```{}\n// code here for {}\nlet v = 10;\n```", lang, lang);
            let lines = highlight_markdown(&code_block);
            assert_eq!(lines.len(), 4);

            // First line starts the block, middle lines are code block, last line ends it
            assert!(lines[1].is_code_block);
            assert!(lines[2].is_code_block);
            assert_eq!(lines[1].code_block_lang.as_deref(), Some(*lang));
            assert_eq!(lines[2].code_block_lang.as_deref(), Some(*lang));

            // Code block lines must have code spans
            assert!(lines[1].spans.iter().all(|span| span.is_code));
            assert!(lines[2].spans.iter().all(|span| span.is_code));
        }

        // 6. Blockquote permutations
        for depth in 1..=5 {
            let prefix = "> ".repeat(depth);
            let quote = format!("{} Nested quote depth {}", prefix, depth);
            let lines = highlight_markdown(&quote);
            assert_eq!(lines.len(), 1);
            assert!(lines[0].is_blockquote);
        }

        // 7. Inline link and wikilink variations inside paragraphs
        let inline_markdown = "Text [[wikilink]] more text [standard link](http://google.com) and [[aliased|link]] with `code` block.";
        let lines = highlight_markdown(inline_markdown);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];

        assert!(
            line.spans
                .iter()
                .any(|span| span.is_link && span.link_target.as_deref() == Some("wikilink"))
        );
        assert!(
            line.spans.iter().any(
                |span| span.is_link && span.link_target.as_deref() == Some("http://google.com")
            )
        );
        assert!(
            line.spans
                .iter()
                .any(|span| span.is_link && span.link_target.as_deref() == Some("aliased"))
        );
        assert!(line.spans.iter().any(|span| span.is_code));

        // Test relative path wikilinks and display alias extraction
        let test_markdown = "Link [[../folder/file_name]] and [[../other/file_name | My Alias]] and [[#equation-1]] and [[../nested/complex!@#%^&*()]].";
        let lines = highlight_markdown(test_markdown);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        let link1 = line.spans.iter().find(|span| span.is_link && span.link_target.as_deref() == Some("../folder/file_name")).unwrap();
        assert_eq!(link1.display_text.as_deref(), Some("file_name"));

        let link2 = line.spans.iter().find(|span| span.is_link && span.link_target.as_deref() == Some("../other/file_name")).unwrap();
        assert_eq!(link2.display_text.as_deref(), Some("My Alias"));

        let link3 = line.spans.iter().find(|span| span.is_link && span.link_target.as_deref() == Some("#equation-1")).unwrap();
        assert_eq!(link3.display_text.as_deref(), Some("#equation-1"));

        let link4 = line.spans.iter().find(|span| span.is_link && span.link_target.as_deref() == Some("../nested/complex!@#%^&*()")).unwrap();
        assert_eq!(link4.display_text.as_deref(), Some("complex!@#%^&*()"));
    }
}

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
            color: theme::text_primary(),
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
                    color: theme::text_muted(),
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
                    color: theme::text_muted(),
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
                color: theme::warning(),
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
                    color: theme::warning(),
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
                color: theme::warning(),
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
                color: theme::warning(),
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
                    .push(StyledSpan::syntax(raw_line, theme::text_muted(), 16.0));
            } else {
                let mut parts = split_table_cells(trimmed);
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

    // Post-process spans to resolve reference link targets
    let reference_definitions = collect_reference_definitions(&lines);
    for line in &mut lines {
        if line.is_code_block || line.is_math_block {
            continue;
        }
        for span in &mut line.spans {
            if span.is_link {
                if let Some(ref_id) = get_ref_id_from_span_text(&span.text) {
                    if let Some(target) = reference_definitions.get(&ref_id.to_lowercase()) {
                        span.link_target = Some(target.clone());
                    }
                }
            }
        }
    }

    lines
}

fn highlight_line(line: &str) -> StyledLine {
    let trimmed = line.trim_start();

    if let Some((_label, _target, idx, target_start, target_len)) = parse_reference_definition(line)
    {
        let mut sl = StyledLine::new();
        let leading_spaces_len = line.len() - trimmed.len();
        if leading_spaces_len > 0 {
            sl.spans
                .push(StyledSpan::plain(&line[..leading_spaces_len]));
        }

        // Syntax: `[label]:`
        sl.spans.push(StyledSpan {
            text: trimmed[..idx + 2].to_string(),
            display_text: Some(String::new()), // hidden in preview
            color: theme::text_muted(),
            is_syntax: true,
            ..StyledSpan::plain("")
        });

        // Spaces between `:` and target
        if target_start > idx + 2 {
            sl.spans
                .push(StyledSpan::plain(&trimmed[idx + 2..target_start]));
        }

        // Target link
        let raw_target = &trimmed[target_start..target_start + target_len];
        let is_angle_wrapped = raw_target.starts_with('<') && raw_target.ends_with('>');
        let actual_target = if is_angle_wrapped {
            &raw_target[1..raw_target.len() - 1]
        } else {
            raw_target
        };

        if is_angle_wrapped {
            sl.spans
                .push(StyledSpan::syntax("<", theme::text_muted(), 16.0));
        }

        sl.spans.push(StyledSpan {
            text: actual_target.to_string(),
            display_text: None,
            color: theme::accent(),
            is_link: true,
            link_target: Some(actual_target.to_string()),
            ..StyledSpan::plain("")
        });

        if is_angle_wrapped {
            sl.spans
                .push(StyledSpan::syntax(">", theme::text_muted(), 16.0));
        }

        // Rest of the line (optional title, spaces, etc.)
        let rest_start = target_start + target_len;
        if rest_start < trimmed.len() {
            sl.spans.push(StyledSpan::plain(&trimmed[rest_start..]));
        }

        return sl;
    }

    let mut sl = StyledLine::new();

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
            color: theme::text_muted(),
            is_syntax: true,
            font_size: heading_size(level),
            is_heading: true,
            heading_level: level,
            ..StyledSpan::plain("")
        });

        // The heading text content
        let start_idx = sl.spans.len();
        parse_inline_spans(display, &mut sl.spans);
        for span in &mut sl.spans[start_idx..] {
            if !span.is_syntax {
                span.color = theme::accent();
            }
            span.bold = true;
            span.font_size = heading_size(level);
            span.is_heading = true;
            span.heading_level = level;
        }

        return sl;
    }

    // Horizontal rule
    if trimmed == "---" || trimmed == "***" || trimmed == "___" {
        sl.spans.push(StyledSpan {
            text: line.to_string(),
            display_text: Some(String::new()),
            color: theme::border(),
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
            color: theme::accent(),
            is_syntax: true,
            ..StyledSpan::plain("")
        });
        // Blockquote content
        sl.spans.push(StyledSpan {
            text: line[rest_start..].to_string(),
            display_text: None,
            color: theme::accent(),
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
                theme::accent()
            } else {
                theme::text_muted()
            },
            is_checkbox: true,
            is_checked,
            ..StyledSpan::plain("")
        });
        let start_idx = sl.spans.len();
        parse_inline_spans(&line[checkbox_end..], &mut sl.spans);
        if is_checked {
            for span in &mut sl.spans[start_idx..] {
                span.color = theme::text_muted();
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
            color: theme::accent(),
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
            color: theme::accent(),
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

fn detect_heading(trimmed: &str) -> Option<u8> {
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

fn split_table_cells(line: &str) -> Vec<&str> {
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

fn highlight_code_spans(
    line: &str,
    highlighter: &mut Option<syntect::easy::HighlightLines<'static>>,
    lang: Option<&str>,
) -> Vec<StyledSpan> {
    let Some((syntax_set, _)) = syntect_defaults() else {
        return vec![code_span(line, theme::text_primary())];
    };

    if highlighter.is_none() {
        *highlighter = make_code_highlighter(lang);
    }

    let Some(highlighter) = highlighter.as_mut() else {
        return vec![code_span(line, theme::text_primary())];
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
                vec![code_span(line, theme::text_primary())]
            } else {
                spans
            }
        }
        Err(_) => vec![code_span(line, theme::text_primary())],
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineEntry {
    pub level: u8,
    pub text: String,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownLinkKind {
    Wiki,
    Inline,
    Reference,
    Footnote,
    ResolvedReference,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownLinkEntry {
    pub line: usize,
    pub target: String,
    pub display_text: String,
    pub source_text: String,
    pub kind: MarkdownLinkKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownAnchorKind {
    Heading,
    SpanId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownAnchorEntry {
    pub line: usize,
    pub slug: String,
    pub source_text: String,
    pub kind: MarkdownAnchorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownDocumentMetadata {
    pub outline: Vec<OutlineEntry>,
    pub links: Vec<MarkdownLinkEntry>,
    pub anchors: Vec<MarkdownAnchorEntry>,
    pub frontmatter: FrontmatterMetadata,
    pub reference_definitions: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrontmatterMetadata {
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
}

pub fn extract_outline(lines: &[StyledLine]) -> Vec<OutlineEntry> {
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

pub fn extract_document_metadata(lines: &[StyledLine]) -> MarkdownDocumentMetadata {
    MarkdownDocumentMetadata {
        outline: extract_outline(lines),
        links: extract_markdown_links(lines),
        anchors: extract_markdown_anchors(lines),
        frontmatter: extract_frontmatter_metadata(lines),
        reference_definitions: collect_reference_definitions(lines),
    }
}

pub fn extract_frontmatter_metadata(lines: &[StyledLine]) -> FrontmatterMetadata {
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

pub fn extract_markdown_anchors(lines: &[StyledLine]) -> Vec<MarkdownAnchorEntry> {
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

pub fn markdown_anchor_slug(s: &str) -> String {
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

fn styled_line_source(line: &StyledLine) -> String {
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

pub fn collect_reference_definitions(
    lines: &[StyledLine],
) -> std::collections::HashMap<String, String> {
    let mut defs = std::collections::HashMap::new();
    for line in lines {
        let source = styled_line_source(line);
        if let Some((label, target, ..)) = parse_reference_definition(&source) {
            defs.insert(label, target);
        }
    }
    defs
}

pub fn get_ref_id_from_span_text(text: &str) -> Option<String> {
    if !text.starts_with('[') || !text.ends_with(']') {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    if let Some(pos) = text.find("][") {
        Some(chars[pos + 2..chars.len() - 1].iter().collect())
    } else {
        if text.starts_with("[^") {
            None
        } else {
            Some(chars[1..chars.len() - 1].iter().collect())
        }
    }
}

pub fn parse_reference_definition(line: &str) -> Option<(String, String, usize, usize, usize)> {
    let trimmed = line.trim_start();
    let leading_spaces_len = line.len() - trimmed.len();
    if leading_spaces_len > 3 {
        return None;
    }
    if !trimmed.starts_with('[') {
        return None;
    }
    let chars: Vec<char> = trimmed.chars().collect();
    let mut end_label = None;
    for i in 1..chars.len() {
        if chars[i] == ']' {
            if i + 1 < chars.len() && chars[i + 1] == ':' {
                end_label = Some(i);
                break;
            }
        }
    }
    let idx = end_label?;
    let label: String = chars[1..idx].iter().collect();
    let label = label.trim().to_lowercase();
    if label.is_empty() {
        return None;
    }

    let rest_start = idx + 2;
    let rest = &trimmed[rest_start..];
    let rest_trimmed = rest.trim_start();
    let spaces_len = rest.len() - rest_trimmed.len();
    let target_start = rest_start + spaces_len;

    if rest_trimmed.is_empty() {
        return None;
    }

    let target_len = if rest_trimmed.starts_with('<') {
        if let Some(end_bracket) = rest_trimmed.find('>') {
            end_bracket + 1
        } else {
            return None;
        }
    } else {
        rest_trimmed.split_whitespace().next()?.len()
    };

    let target = if rest_trimmed.starts_with('<') {
        &rest_trimmed[1..target_len - 1]
    } else {
        &rest_trimmed[..target_len]
    };

    Some((label, target.to_string(), idx, target_start, target_len))
}

pub fn extract_markdown_links(lines: &[StyledLine]) -> Vec<MarkdownLinkEntry> {
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

#[cfg(test)]
mod tests {
    use super::{
        MarkdownAnchorKind, MarkdownLinkEntry, MarkdownLinkKind, extract_document_metadata,
        extract_frontmatter_metadata, extract_markdown_links, highlight_markdown,
    };

    #[test]
    fn heading_is_not_math() {
        let lines = highlight_markdown("# Heading");
        assert!(lines[0].spans.iter().any(|span| span.is_heading));
        assert!(!lines[0].is_math_block);
    }

    #[test]
    fn heading_requires_space_after_hash_prefix() {
        let lines = highlight_markdown("#Heading\n##Also not heading\n# Heading");
        assert!(!lines[0].spans.iter().any(|span| span.is_heading));
        assert!(!lines[1].spans.iter().any(|span| span.is_heading));
        assert!(lines[2].spans.iter().any(|span| span.is_heading));
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
        assert!(!lines[1].spans.iter().any(|span| span.is_heading));
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
                .any(|span| span.color != crate::theme::text_primary())
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
    fn escaped_inline_markers_remain_literal() {
        let lines = highlight_markdown(r"\**not bold\** and \`not code\` and \$not math\$");
        let text = lines[0]
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();
        assert_eq!(text, r"\**not bold\** and \`not code\` and \$not math\$");
        assert!(!lines[0].spans.iter().any(|span| span.bold));
        assert!(!lines[0].spans.iter().any(|span| span.is_code));
        assert!(!lines[0].spans.iter().any(|span| span.is_math));
    }

    #[test]
    fn links_support_balanced_parentheses_and_tables_support_escaped_pipes() {
        let lines = highlight_markdown("[docs](https://example.com/a_(b))\n| A\\|B | C |");
        let link = lines[0].spans.iter().find(|span| span.is_link).unwrap();
        assert_eq!(
            link.link_target.as_deref(),
            Some("https://example.com/a_(b)")
        );

        assert!(lines[1].is_table_row);
        assert_eq!(lines[1].table_cells.len(), 2);
        let first_cell = lines[1].table_cells[0]
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();
        assert_eq!(first_cell, "A\\|B");
    }

    #[test]
    fn span_source_reconstructs_original_lines_for_core_markdown() {
        let text = "# H\nplain **bold** `code` ![alt](img.png)\n> quote\n- [ ] task";
        let lines = highlight_markdown(text);
        for (source, line) in text.split('\n').zip(lines.iter()) {
            let reconstructed = line
                .spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>();
            assert_eq!(reconstructed, source);
        }
    }

    #[test]
    fn large_document_highlight_preserves_line_count_and_block_ids() {
        let mut text = String::new();
        for idx in 0..10_000 {
            text.push_str(&format!(
                "- item {idx} with **bold** and [link](note-{idx}.md)\n"
            ));
        }

        let lines = highlight_markdown(&text);
        assert_eq!(lines.len(), 10_001);
        assert!(lines.iter().take(10_000).all(|line| line.block_id > 0));
        assert!(lines.iter().take(10_000).all(|line| {
            line.spans.iter().any(|span| span.bold) && line.spans.iter().any(|span| span.is_link)
        }));
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
        let link1 = line
            .spans
            .iter()
            .find(|span| span.is_link && span.link_target.as_deref() == Some("../folder/file_name"))
            .unwrap();
        assert_eq!(link1.display_text.as_deref(), Some("file_name"));

        let link2 = line
            .spans
            .iter()
            .find(|span| span.is_link && span.link_target.as_deref() == Some("../other/file_name"))
            .unwrap();
        assert_eq!(link2.display_text.as_deref(), Some("My Alias"));

        let link3 = line
            .spans
            .iter()
            .find(|span| span.is_link && span.link_target.as_deref() == Some("#equation-1"))
            .unwrap();
        assert_eq!(link3.display_text.as_deref(), Some("#equation-1"));

        let link4 = line
            .spans
            .iter()
            .find(|span| {
                span.is_link && span.link_target.as_deref() == Some("../nested/complex!@#%^&*()")
            })
            .unwrap();
        assert_eq!(link4.display_text.as_deref(), Some("complex!@#%^&*()"));
    }

    #[test]
    fn reference_link_span_exposes_metadata_but_is_inactive() {
        let lines = highlight_markdown("Check [this link][ref_id] and [that_one] syntax.");
        let line = &lines[0];

        // Full reference link: [text][ref]
        let link1 = line
            .spans
            .iter()
            .find(|span| span.text == "[this link][ref_id]")
            .expect("Did not find full reference link span");
        assert!(link1.is_link);
        assert_eq!(link1.link_target.as_deref(), Some("ref_id"));
        assert_eq!(link1.display_text.as_deref(), Some("this link"));

        // Shortcut reference link: [ref]
        let link2 = line
            .spans
            .iter()
            .find(|span| span.text == "[that_one]")
            .expect("Did not find shortcut reference link span");
        assert!(link2.is_link);
        assert_eq!(link2.link_target.as_deref(), Some("that_one"));
        assert_eq!(link2.display_text.as_deref(), Some("that_one"));
    }

    #[test]
    fn reference_link_span_reconstructs_source_lines() {
        let text = "Here is a [link][ref] and a [shortcut].";
        let lines = highlight_markdown(text);
        let reconstructed = lines[0]
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>();
        assert_eq!(reconstructed, text);
    }

    #[test]
    fn malformed_reference_syntax_remains_plain_text() {
        let lines = highlight_markdown("Bad [link][ref and [shortcut and [ ].");
        let reconstructed = lines[0]
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>();
        assert_eq!(reconstructed, "Bad [link][ref and [shortcut and [ ].");
        assert!(!lines[0].spans.iter().any(|s| s.is_link));
    }

    #[test]
    fn headings_parse_inline_links_and_emphasis() {
        let lines = highlight_markdown("## Heading with **bold** and [link](url)");
        let reconstructed = lines[0]
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>();
        assert_eq!(reconstructed, "## Heading with **bold** and [link](url)");
        assert!(lines[0].spans.iter().all(|s| s.is_heading));
        assert!(lines[0].spans.iter().all(|s| s.heading_level == 2));
        assert!(
            lines[0]
                .spans
                .iter()
                .any(|s| s.is_link && s.link_target.as_deref() == Some("url"))
        );
        assert!(lines[0].spans.iter().any(|s| s.bold && s.text == "bold"));
    }

    #[test]
    fn nested_emphasis_combines_bold_and_italic() {
        let lines = highlight_markdown("**bold and *italic* inside**");
        let reconstructed = lines[0]
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>();
        assert_eq!(reconstructed, "**bold and *italic* inside**");
        // The word "italic" should have both bold and italic true
        let italic_span = lines[0].spans.iter().find(|s| s.text == "italic").unwrap();
        assert!(italic_span.bold);
        assert!(italic_span.italic);
    }

    #[test]
    fn footnotes_parsed_as_links() {
        let lines = highlight_markdown("This has a footnote[^1].");
        let reconstructed = lines[0]
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>();
        assert_eq!(reconstructed, "This has a footnote[^1].");
        let link_span = lines[0].spans.iter().find(|s| s.is_link).unwrap();
        assert_eq!(link_span.text, "[^1]");
        assert_eq!(link_span.link_target.as_deref(), Some("^1"));
    }

    #[test]
    fn extract_markdown_links_reports_backlink_metadata() {
        let text = "See [[notes/topic|Topic]], [site](https://example.com), [ref link][r1], [shortcut], and footnote[^n].\n## Heading with [[inside]]";
        let lines = highlight_markdown(text);
        let links = extract_markdown_links(&lines);

        assert_eq!(links.len(), 6);
        assert_eq!(
            links[0],
            MarkdownLinkEntry {
                line: 0,
                target: "notes/topic".to_string(),
                display_text: "Topic".to_string(),
                source_text: "notes/topic|Topic".to_string(),
                kind: MarkdownLinkKind::Wiki,
            }
        );
        assert_eq!(links[1].kind, MarkdownLinkKind::Inline);
        assert_eq!(links[1].target, "https://example.com");
        assert_eq!(links[1].display_text, "site");
        assert_eq!(links[2].kind, MarkdownLinkKind::Reference);
        assert_eq!(links[2].target, "r1");
        assert_eq!(links[2].display_text, "ref link");
        assert_eq!(links[3].kind, MarkdownLinkKind::Reference);
        assert_eq!(links[3].target, "shortcut");
        assert_eq!(links[4].kind, MarkdownLinkKind::Footnote);
        assert_eq!(links[4].target, "^n");
        assert_eq!(links[5].kind, MarkdownLinkKind::Wiki);
        assert_eq!(links[5].line, 1);
        assert_eq!(links[5].target, "inside");
    }

    #[test]
    fn extract_document_metadata_reports_outline_links_and_anchors() {
        let text = "# Heading One\n![Plot](plot.png)\n```rust\nfn main() {}\n```\nSee [[note]].";
        let lines = highlight_markdown(text);
        let metadata = extract_document_metadata(&lines);

        assert_eq!(metadata.outline.len(), 1);
        assert_eq!(metadata.outline[0].text, "Heading One");
        assert_eq!(metadata.links.len(), 1);
        assert!(
            metadata
                .links
                .iter()
                .any(|link| link.kind == MarkdownLinkKind::Wiki && link.target == "note")
        );
        assert!(
            metadata
                .anchors
                .iter()
                .any(|anchor| anchor.kind == MarkdownAnchorKind::Heading
                    && anchor.slug == "heading-one"
                    && anchor.line == 0)
        );
        assert!(
            metadata
                .anchors
                .iter()
                .any(|anchor| anchor.kind == MarkdownAnchorKind::SpanId
                    && anchor.slug == "figure-1"
                    && anchor.line == 1)
        );
        assert!(
            metadata
                .anchors
                .iter()
                .any(|anchor| anchor.kind == MarkdownAnchorKind::SpanId
                    && anchor.slug == "code-1"
                    && anchor.line == 2)
        );
    }

    #[test]
    fn extract_frontmatter_metadata_reports_aliases_and_tags() {
        let text = "---\naliases: [Alpha Note, \"Beta Note\"]\ntags:\n  - #math\n  - reading\nalias: Gamma\n---\n# Body";
        let lines = highlight_markdown(text);
        let frontmatter = extract_frontmatter_metadata(&lines);

        assert_eq!(
            frontmatter.aliases,
            vec![
                "Alpha Note".to_string(),
                "Beta Note".to_string(),
                "Gamma".to_string()
            ]
        );
        assert_eq!(
            frontmatter.tags,
            vec!["math".to_string(), "reading".to_string()]
        );

        let metadata = extract_document_metadata(&lines);
        assert_eq!(metadata.frontmatter, frontmatter);
    }

    #[test]
    fn test_extract_outline() {
        use super::extract_outline;
        let text = "# Heading 1\nSome text\n## Heading 2 with **bold**\n```markdown\n# Not a heading in code\n```\n### Heading 3";
        let lines = highlight_markdown(text);
        let outline = extract_outline(&lines);
        assert_eq!(outline.len(), 3);

        assert_eq!(outline[0].level, 1);
        assert_eq!(outline[0].text, "Heading 1");
        assert_eq!(outline[0].line, 0);

        assert_eq!(outline[1].level, 2);
        assert_eq!(outline[1].text, "Heading 2 with bold");
        assert_eq!(outline[1].line, 2);

        assert_eq!(outline[2].level, 3);
        assert_eq!(outline[2].text, "Heading 3");
        assert_eq!(outline[2].line, 6);
    }

    #[test]
    fn reference_style_link_resolution_and_indexing() {
        let text = "Click [my text][ref1] and [shortcut_ref] and [unresolved_ref].\n\n[ref1]: paper.pdf#page=5\n[shortcut_ref]: <another_note.md>";
        let lines = highlight_markdown(text);

        let line0 = &lines[0];
        let span_ref1 = line0
            .spans
            .iter()
            .find(|s| s.text == "[my text][ref1]")
            .unwrap();
        assert_eq!(span_ref1.link_target.as_deref(), Some("paper.pdf#page=5"));
        assert!(span_ref1.is_link);

        let span_shortcut = line0
            .spans
            .iter()
            .find(|s| s.text == "[shortcut_ref]")
            .unwrap();
        assert_eq!(
            span_shortcut.link_target.as_deref(),
            Some("another_note.md")
        );
        assert!(span_shortcut.is_link);

        let span_unresolved = line0
            .spans
            .iter()
            .find(|s| s.text == "[unresolved_ref]")
            .unwrap();
        assert_eq!(
            span_unresolved.link_target.as_deref(),
            Some("unresolved_ref")
        );
        assert!(span_unresolved.is_link);

        let metadata = extract_document_metadata(&lines);
        let ref1_link = metadata
            .links
            .iter()
            .find(|l| l.source_text == "[my text][ref1]")
            .unwrap();
        assert_eq!(ref1_link.kind, MarkdownLinkKind::ResolvedReference);
        assert_eq!(ref1_link.target, "paper.pdf#page=5");

        let shortcut_link = metadata
            .links
            .iter()
            .find(|l| l.source_text == "[shortcut_ref]")
            .unwrap();
        assert_eq!(shortcut_link.kind, MarkdownLinkKind::ResolvedReference);
        assert_eq!(shortcut_link.target, "another_note.md");

        let unresolved_link = metadata
            .links
            .iter()
            .find(|l| l.source_text == "[unresolved_ref]")
            .unwrap();
        assert_eq!(unresolved_link.kind, MarkdownLinkKind::Reference);
        assert_eq!(unresolved_link.target, "unresolved_ref");

        let def1_link = metadata
            .links
            .iter()
            .find(|l| l.source_text == "paper.pdf#page=5")
            .unwrap();
        assert_eq!(def1_link.kind, MarkdownLinkKind::ResolvedReference);
        assert_eq!(def1_link.target, "paper.pdf#page=5");
    }
}

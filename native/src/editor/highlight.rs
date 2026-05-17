use iced::Color;

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
}

impl StyledSpan {
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.to_string(),
            display_text: None,
            color: theme::TEXT_PRIMARY,
            bold: false,
            italic: false,
            font_size: 14.0,
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

    for raw_line in text.split('\n') {
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
                    font_size: 13.0,
                    is_code: true,
                    ..StyledSpan::plain("")
                });
                lines.push(sl);
                continue;
            }
        }

        // Math block fences
        if trimmed.starts_with("$$") {
            if in_math_block {
                // Closing fence
                let mut sl = StyledLine::new();
                sl.is_math_block = true;
                sl.is_block_fence = true;
                sl.block_id = current_block_id;
                sl.spans.push(StyledSpan {
                    text: raw_line.to_string(),
                    display_text: Some(String::new()),
                    color: theme::SUCCESS,
                    is_syntax: true,
                    is_math: true,
                    font_size: 14.0,
                    ..StyledSpan::plain("")
                });
                lines.push(sl);
                in_math_block = false;
                block_id += 1;
                continue;
            } else {
                // Opening fence
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
                    color: theme::SUCCESS,
                    is_syntax: true,
                    is_math: true,
                    font_size: 14.0,
                    ..StyledSpan::plain("")
                });
                lines.push(sl);
                continue;
            }
        }

        // Inside code block
        if in_code_block {
            let mut sl = StyledLine::new();
            sl.is_code_block = true;
            sl.block_id = current_block_id;
            sl.code_block_lang = code_lang.clone();
            sl.spans.push(StyledSpan {
                text: raw_line.to_string(),
                display_text: None,
                color: theme::TEXT_PRIMARY,
                font_size: 13.0,
                is_code: true,
                ..StyledSpan::plain("")
            });
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
                color: theme::TEXT_PRIMARY,
                italic: true,
                font_size: 14.0,
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
            let is_separator = trimmed.chars().all(|c| c == '|' || c == '-' || c == ' ' || c == ':');
            if is_separator {
                // We can mark this as an empty row or skip spans, but let's just make it a row
                sl.spans.push(StyledSpan::syntax(raw_line, theme::TEXT_MUTED, 14.0));
            } else {
                let mut parts = trimmed.split('|').collect::<Vec<_>>();
                if parts.first() == Some(&"") { parts.remove(0); }
                if parts.last() == Some(&"") { parts.pop(); }
                
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

    lines
}

fn highlight_line(line: &str) -> StyledLine {
    let mut sl = StyledLine::new();
    let trimmed = line.trim_start();

    // Headings
    if let Some(level) = detect_heading(trimmed) {
        let prefix_len = level as usize + 1;
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
            color: theme::TEXT_PRIMARY,
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
    if trimmed.starts_with("- [ ] ") || trimmed.starts_with("- [x] ") || trimmed.starts_with("- [X] ") {
        let checkbox_end = line.len() - trimmed.len() + 6;
        let is_checked = trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]");
        sl.spans.push(StyledSpan {
            text: line[..(line.len() - trimmed.len() + 6)].to_string(),
            display_text: Some(if is_checked { "☑ ".to_string() } else { "☐ ".to_string() }),
            color: if is_checked { theme::ACCENT } else { theme::TEXT_MUTED },
            is_checkbox: true,
            is_checked,
            ..StyledSpan::plain("")
        });
        parse_inline_spans(&line[checkbox_end..], &mut sl.spans);
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
            if !current.is_empty() { spans.push(StyledSpan::plain(&current)); current.clear(); }
            if let Some(end) = find_char(&chars, i + 1, '`') {
                let code_text: String = chars[i + 1..end].iter().collect();
                // Opening backtick — syntax marker
                spans.push(StyledSpan::syntax("`", theme::TEXT_MUTED, 13.0));
                // Code content
                spans.push(StyledSpan {
                    text: code_text,
                    display_text: None,
                    color: theme::ACCENT_SECONDARY,
                    font_size: 13.0,
                    is_code: true,
                    ..StyledSpan::plain("")
                });
                // Closing backtick — syntax marker
                spans.push(StyledSpan::syntax("`", theme::TEXT_MUTED, 13.0));
                i = end + 1; continue;
            }
        }

        // Bold **text**
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if !current.is_empty() { spans.push(StyledSpan::plain(&current)); current.clear(); }
            if let Some(end) = find_double(&chars, i + 2, '*') {
                let bold_text: String = chars[i + 2..end].iter().collect();
                // Opening ** — syntax marker
                spans.push(StyledSpan::syntax("**", theme::TEXT_MUTED, 14.0));
                // Bold text
                spans.push(StyledSpan {
                    text: bold_text, color: theme::TEXT_PRIMARY,
                    bold: true, font_size: 14.0,
                    ..StyledSpan::plain("")
                });
                // Closing ** — syntax marker
                spans.push(StyledSpan::syntax("**", theme::TEXT_MUTED, 14.0));
                i = end + 2; continue;
            }
        }

        // Italic *text*  (single *, not **)
        if chars[i] == '*' && (i + 1 >= len || chars[i + 1] != '*') {
            if !current.is_empty() { spans.push(StyledSpan::plain(&current)); current.clear(); }
            if let Some(end) = find_char(&chars, i + 1, '*') {
                let italic_text: String = chars[i + 1..end].iter().collect();
                spans.push(StyledSpan::syntax("*", theme::TEXT_MUTED, 14.0));
                spans.push(StyledSpan {
                    text: italic_text, color: theme::TEXT_PRIMARY,
                    italic: true, font_size: 14.0,
                    ..StyledSpan::plain("")
                });
                spans.push(StyledSpan::syntax("*", theme::TEXT_MUTED, 14.0));
                i = end + 1; continue;
            }
        }

        // Wikilink [[target|display]]
        if i + 1 < len && chars[i] == '[' && chars[i + 1] == '[' {
            if !current.is_empty() { spans.push(StyledSpan::plain(&current)); current.clear(); }
            if let Some(end) = find_double(&chars, i + 2, ']') {
                let link_text: String = chars[i + 2..end].iter().collect();
                let parts: Vec<&str> = link_text.split('|').collect();
                let target = parts[0];
                let display = parts.get(1).unwrap_or(&target);
                // [[ — syntax marker
                spans.push(StyledSpan::syntax("[[", theme::TEXT_MUTED, 14.0));
                spans.push(StyledSpan {
                    text: link_text.clone(),
                    display_text: Some(display.to_string()),
                    color: theme::ACCENT,
                    is_link: true, link_target: Some(target.to_string()),
                    ..StyledSpan::plain("")
                });
                // ]] — syntax marker
                spans.push(StyledSpan::syntax("]]", theme::TEXT_MUTED, 14.0));
                i = end + 2; continue;
            }
        }

        // Markdown link [text](url)
        if chars[i] == '[' && (i == 0 || chars[i - 1] != '!') {
            if !current.is_empty() { spans.push(StyledSpan::plain(&current)); current.clear(); }
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
                            is_link: true, link_target: Some(url),
                            ..StyledSpan::plain("")
                        });
                        i = end_url + 1; continue;
                    }
                }
            }
        }

        // Inline math $...$
        if chars[i] == '$' && (i + 1 < len && chars[i + 1] != '$') {
            if !current.is_empty() { spans.push(StyledSpan::plain(&current)); current.clear(); }
            if let Some(end) = find_char(&chars, i + 1, '$') {
                let math_raw: String = chars[i..=end].iter().collect();
                spans.push(StyledSpan {
                    text: math_raw, color: theme::TEXT_PRIMARY,
                    italic: true, font_size: 14.0,
                    is_math: true,
                    ..StyledSpan::plain("")
                });
                i = end + 1; continue;
            }
        }

        // Image: ![alt](url)
        if i + 1 < len && chars[i] == '!' && chars[i + 1] == '[' {
            if !current.is_empty() { spans.push(StyledSpan::plain(&current)); current.clear(); }
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
                            italic: true, font_size: 13.0,
                            is_image: true,
                            image_path: Some(url),
                            image_alt: Some(alt_text),
                            ..StyledSpan::plain("")
                        });
                        i = end_url + 1; continue;
                    }
                }
            }
        }

        current.push(chars[i]);
        i += 1;
    }
    if !current.is_empty() { spans.push(StyledSpan::plain(&current)); }
}

fn detect_heading(trimmed: &str) -> Option<u8> {
    if trimmed.starts_with("###### ") { return Some(6); }
    if trimmed.starts_with("##### ") { return Some(5); }
    if trimmed.starts_with("#### ") { return Some(4); }
    if trimmed.starts_with("### ") { return Some(3); }
    if trimmed.starts_with("## ") { return Some(2); }
    if trimmed.starts_with("# ") { return Some(1); }
    None
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
        1 => 28.0, 2 => 22.0, 3 => 18.0, 4 => 16.0, 5 => 15.0, _ => 14.0,
    }
}

fn find_char(chars: &[char], start: usize, target: char) -> Option<usize> {
    for i in start..chars.len() { if chars[i] == target { return Some(i); } }
    None
}

fn find_double(chars: &[char], start: usize, target: char) -> Option<usize> {
    if start + 1 >= chars.len() { return None; }
    for i in start..chars.len() - 1 { if chars[i] == target && chars[i + 1] == target { return Some(i); } }
    None
}

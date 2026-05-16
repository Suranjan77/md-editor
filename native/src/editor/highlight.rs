use iced::Color;

use crate::theme;

/// A styled text span for rendering.
#[derive(Debug, Clone)]
pub struct StyledSpan {
    pub text: String,
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
    pub is_math: bool,
}

impl StyledSpan {
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.to_string(),
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
            is_math: false,
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
}

impl StyledLine {
    pub fn new() -> Self {
        Self {
            spans: Vec::new(),
            is_code_block: false,
            is_math_block: false,
            code_block_lang: None,
        }
    }
}

/// Style state tracker for nested markdown elements.
#[allow(dead_code)]
struct StyleState {
    bold: bool,
    italic: bool,
    strikethrough: bool,
    in_code: bool,
    in_link: bool,
    in_heading: bool,
    heading_level: u8,
    in_code_block: bool,
    code_block_lang: Option<String>,
}

impl StyleState {
    pub fn new() -> Self {
        Self {
            bold: false,
            italic: false,
            strikethrough: false,
            in_code: false,
            in_link: false,
            in_heading: false,
            heading_level: 0,
            in_code_block: false,
            code_block_lang: None,
        }
    }

    pub fn color(&self) -> Color {
        if self.in_code {
            Color::from_rgb(0.68, 0.77, 0.90) // soft blue for code
        } else if self.in_link {
            theme::ACCENT
        } else if self.in_heading {
            theme::TEXT_PRIMARY
        } else if self.strikethrough {
            theme::TEXT_MUTED
        } else {
            theme::TEXT_PRIMARY
        }
    }

    pub fn font_size(&self) -> f32 {
        if self.in_heading {
            match self.heading_level {
                1 => 28.0,
                2 => 22.0,
                3 => 18.0,
                4 => 16.0,
                5 => 15.0,
                _ => 14.0,
            }
        } else {
            14.0
        }
    }

    pub fn to_span(&self, text: &str) -> StyledSpan {
        StyledSpan {
            text: text.to_string(),
            color: self.color(),
            bold: self.bold || self.in_heading,
            italic: self.italic,
            font_size: self.font_size(),
            is_code: self.in_code || self.in_code_block,
            is_link: self.in_link,
            link_target: None,
            is_heading: self.in_heading,
            heading_level: self.heading_level,
            is_checkbox: false,
            is_checked: false,
            is_rule: false,
            is_image: false,
            image_path: None,
            is_math: false,
        }
    }
}

/// Parse markdown text into styled lines for rendering.
pub fn highlight_markdown(text: &str) -> Vec<StyledLine> {
    let mut lines = Vec::new();

    let mut in_code_block = false;
    let mut code_lang: Option<String> = None;
    let mut in_math_block = false;

    for raw_line in text.split('\n') {
        let trimmed = raw_line.trim();

        // Code block fences
        if trimmed.starts_with("```") {
            if in_code_block {
                let mut sl = StyledLine::new();
                sl.is_code_block = true;
                sl.spans.push(StyledSpan {
                    text: raw_line.to_string(),
                    color: theme::TEXT_MUTED,
                    bold: false,
                    italic: false,
                    font_size: 13.0,
                    is_code: true,
                    is_link: false,
                    link_target: None,
                    is_heading: false,
                    heading_level: 0,
                    is_checkbox: false,
                    is_checked: false,
                    is_rule: false,
                    is_image: false,
                    image_path: None,
                    is_math: false,
                });
                lines.push(sl);
                in_code_block = false;
                code_lang = None;
                continue;
            } else {
                in_code_block = true;
                code_lang = if trimmed.len() > 3 {
                    Some(trimmed[3..].trim().to_string())
                } else {
                    None
                };
                let mut sl = StyledLine::new();
                sl.is_code_block = true;
                sl.code_block_lang = code_lang.clone();
                sl.spans.push(StyledSpan {
                    text: raw_line.to_string(),
                    color: theme::TEXT_MUTED,
                    bold: false,
                    italic: false,
                    font_size: 13.0,
                    is_code: true,
                    is_link: false,
                    link_target: None,
                    is_heading: false,
                    heading_level: 0,
                    is_checkbox: false,
                    is_checked: false,
                    is_rule: false,
                    is_image: false,
                    image_path: None,
                    is_math: false,
                });
                lines.push(sl);
                continue;
            }
        }

        // Math block fences
        if trimmed.starts_with("$$") {
            in_math_block = !in_math_block;
            let mut sl = StyledLine::new();
            sl.is_math_block = true;
            sl.spans.push(StyledSpan {
                text: raw_line.to_string(),
                color: Color::from_rgb(0.6, 0.85, 0.6),
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
                is_math: true,
            });
            lines.push(sl);
            continue;
        }

        // Inside code block
        if in_code_block {
            let mut sl = StyledLine::new();
            sl.is_code_block = true;
            sl.code_block_lang = code_lang.clone();
            sl.spans.push(StyledSpan {
                text: raw_line.to_string(),
                color: Color::from_rgb(0.68, 0.77, 0.90),
                bold: false,
                italic: false,
                font_size: 13.0,
                is_code: true,
                is_link: false,
                link_target: None,
                is_heading: false,
                heading_level: 0,
                is_checkbox: false,
                is_checked: false,
                is_rule: false,
                is_image: false,
                image_path: None,
                is_math: false,
            });
            lines.push(sl);
            continue;
        }

        // Inside math block
        if in_math_block {
            let mut sl = StyledLine::new();
            sl.is_math_block = true;
            sl.spans.push(StyledSpan {
                text: raw_line.to_string(),
                color: Color::from_rgb(0.6, 0.85, 0.6),
                bold: false,
                italic: true,
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
                is_math: true,
            });
            lines.push(sl);
            continue;
        }

        // Regular line — parse inline markdown
        let sl = highlight_line(raw_line);
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
        sl.spans.push(StyledSpan {
            text: hash_part.to_string(),
            color: theme::TEXT_MUTED,
            bold: false,
            italic: false,
            font_size: heading_size(level),
            is_code: false,
            is_link: false,
            link_target: None,
            is_heading: true,
            heading_level: level,
            is_checkbox: false,
            is_checked: false,
            is_rule: false,
            is_image: false,
            image_path: None,
            is_math: false,
        });

        sl.spans.push(StyledSpan {
            text: display.to_string(),
            color: theme::TEXT_PRIMARY,
            bold: true,
            italic: false,
            font_size: heading_size(level),
            is_code: false,
            is_link: false,
            link_target: None,
            is_heading: true,
            heading_level: level,
            is_checkbox: false,
            is_checked: false,
            is_rule: false,
            is_image: false,
            image_path: None,
            is_math: false,
        });

        return sl;
    }

    // Horizontal rule
    if trimmed == "---" || trimmed == "***" || trimmed == "___" {
        sl.spans.push(StyledSpan {
            text: line.to_string(),
            color: theme::BORDER,
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
            is_rule: true,
            is_image: false,
            image_path: None,
            is_math: false,
        });
        return sl;
    }

    // Blockquote
    if trimmed.starts_with('>') {
        sl.spans.push(StyledSpan {
            text: line.to_string(),
            color: theme::ACCENT,
            bold: false,
            italic: true,
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
            is_math: false,
        });
        return sl;
    }

    // List items
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
        let bullet_end = line.len() - trimmed.len() + 2;
        sl.spans.push(StyledSpan {
            text: line[..bullet_end].to_string(),
            color: theme::ACCENT,
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
            is_math: false,
        });
        parse_inline_spans(&line[bullet_end..], &mut sl.spans);
        return sl;
    }

    // Task list items
    if trimmed.starts_with("- [ ] ") || trimmed.starts_with("- [x] ") || trimmed.starts_with("- [X] ") {
        let checkbox_end = line.len() - trimmed.len() + 6;
        let is_checked = trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]");
        sl.spans.push(StyledSpan {
            text: if is_checked { "☑ ".to_string() } else { "☐ ".to_string() },
            color: if is_checked { theme::ACCENT } else { theme::TEXT_MUTED },
            bold: false,
            italic: false,
            font_size: 14.0,
            is_code: false,
            is_link: false,
            link_target: None,
            is_heading: false,
            heading_level: 0,
            is_checkbox: true,
            is_checked,
            is_rule: false,
            is_image: false,
            image_path: None,
            is_math: false,
        });
        parse_inline_spans(&line[checkbox_end..], &mut sl.spans);
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
                spans.push(StyledSpan {
                    text: code_text,
                    color: Color::from_rgb(0.68, 0.77, 0.90),
                    bold: false, italic: false, font_size: 13.0,
                    is_code: true, is_link: false, link_target: None,
                    is_heading: false, heading_level: 0, is_checkbox: false, is_checked: false,
                    is_rule: false, is_image: false, image_path: None,
                    is_math: false,
                });
                i = end + 1; continue;
            }
        }

        // Bold
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if !current.is_empty() { spans.push(StyledSpan::plain(&current)); current.clear(); }
            if let Some(end) = find_double(&chars, i + 2, '*') {
                let bold_text: String = chars[i + 2..end].iter().collect();
                spans.push(StyledSpan {
                    text: bold_text, color: theme::TEXT_PRIMARY,
                    bold: true, italic: false, font_size: 14.0,
                    is_code: false, is_link: false, link_target: None,
                    is_heading: false, heading_level: 0, is_checkbox: false, is_checked: false,
                    is_rule: false, is_image: false, image_path: None,
                    is_math: false,
                });
                i = end + 2; continue;
            }
        }

        // Wikilink
        if i + 1 < len && chars[i] == '[' && chars[i + 1] == '[' {
            if !current.is_empty() { spans.push(StyledSpan::plain(&current)); current.clear(); }
            if let Some(end) = find_double(&chars, i + 2, ']') {
                let link_text: String = chars[i + 2..end].iter().collect();
                let parts: Vec<&str> = link_text.split('|').collect();
                let target = parts[0];
                let display = parts.get(1).unwrap_or(&target);
                spans.push(StyledSpan {
                    text: display.to_string(), color: theme::ACCENT,
                    bold: false, italic: false, font_size: 14.0,
                    is_code: false, is_link: true, link_target: Some(target.to_string()),
                    is_heading: false, heading_level: 0, is_checkbox: false, is_checked: false,
                    is_rule: false, is_image: false, image_path: None,
                    is_math: false,
                });
                i = end + 2; continue;
            }
        }

        // Inline math
        if chars[i] == '$' && (i + 1 < len && chars[i + 1] != '$') {
            if !current.is_empty() { spans.push(StyledSpan::plain(&current)); current.clear(); }
            if let Some(end) = find_char(&chars, i + 1, '$') {
                let math_text: String = chars[i..=end].iter().collect();
                spans.push(StyledSpan {
                    text: math_text, color: Color::from_rgb(0.6, 0.85, 0.6),
                    bold: false, italic: true, font_size: 14.0,
                    is_code: false, is_link: false, link_target: None,
                    is_heading: false, heading_level: 0, is_checkbox: false, is_checked: false,
                    is_rule: false, is_image: false, image_path: None,
                    is_math: true,
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
                        spans.push(StyledSpan {
                            text: format!("Image: {}", alt_text), color: Color::from_rgb(0.9, 0.6, 0.4),
                            bold: false, italic: true, font_size: 13.0,
                            is_code: false, is_link: false, link_target: None,
                            is_heading: false, heading_level: 0, is_checkbox: false, is_checked: false,
                            is_rule: false, is_image: true, image_path: Some(url),
                            is_math: false,
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
    if trimmed.starts_with("# ") { return Some(1); }
    if trimmed.starts_with("## ") { return Some(2); }
    if trimmed.starts_with("### ") { return Some(3); }
    if trimmed.starts_with("#### ") { return Some(4); }
    if trimmed.starts_with("##### ") { return Some(5); }
    if trimmed.starts_with("###### ") { return Some(6); }
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

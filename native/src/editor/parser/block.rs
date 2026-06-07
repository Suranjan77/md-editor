use super::inline::parse_inline_spans;
use super::inline::split_table_cells;
use super::model::{StyledLine, StyledSpan};
use super::reference::{
    collect_reference_definitions, get_ref_id_from_span_text, parse_reference_definition,
};
use super::syntax::highlight_code_spans;
use super::syntax::make_code_highlighter;
use crate::theme;

pub(crate) fn highlight_markdown(text: &str) -> Vec<StyledLine> {
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

pub(crate) fn heading_size(level: u8) -> f32 {
    match level {
        1 => 34.0,
        2 => 28.0,
        3 => 23.0,
        4 => 20.0,
        5 => 18.0,
        _ => 17.0,
    }
}

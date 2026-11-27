//! The pre-incremental highlighter, kept as a test oracle: incremental results must equal it.

#![allow(clippy::all)]
use super::*;

/// Parse markdown text into styled lines for rendering.
pub fn highlight_markdown(text: &str) -> Vec<StyledLine> {
    let mut lines = Vec::new();

    let mut in_code_block = false;
    let mut code_lang: Option<String> = None;
    let mut math_block_end: Option<MathBlockEnd> = None;
    let mut in_table = false;
    let mut block_id: usize = 0;
    let mut current_block_id: usize = 0;
    let mut code_highlighter: Option<syntect::easy::HighlightLines<'static>> = None;
    let all: Vec<&str> = text.split('\n').collect();
    let mut opens_frontmatter = super::frontmatter_closes(&all);
    let mut in_frontmatter = false;

    for raw_line in all {
        let raw_line = raw_line.trim_end_matches('\r');
        let mut trimmed = raw_line.trim();

        // YAML frontmatter is a `yaml` code block fenced by `---` / `...`.
        if std::mem::take(&mut opens_frontmatter) {
            in_frontmatter = true;
            trimmed = "```yaml";
        } else if in_frontmatter && matches!(raw_line.trim_end(), "---" | "...") {
            in_frontmatter = false;
            trimmed = "```";
        } else if in_frontmatter && trimmed.starts_with("```") {
            trimmed = "";
        }

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
                    color: crate::design::tokens().doc_syntax,
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
                    color: crate::design::tokens().doc_syntax,
                    is_syntax: true,
                    font_size: 14.0,
                    is_code: true,
                    ..StyledSpan::plain("")
                });
                lines.push(sl);
                continue;
            }
        }

        // Fenced code is literal Markdown. In particular, math delimiters in
        // a `markdown` example must not start a display-math block.
        if in_code_block {
            let mut sl = StyledLine::new();
            sl.is_code_block = true;
            sl.block_id = current_block_id;
            sl.code_block_lang = code_lang.clone();
            sl.spans = highlight_code_spans(raw_line, &mut code_highlighter, code_lang.as_deref());
            lines.push(sl);
            continue;
        }

        // Math block fences. Keep this narrow so ordinary markdown/HTML is not
        // accidentally promoted to a math block.
        if math_block_end.is_some() && is_obvious_markdown_boundary(trimmed) {
            math_block_end = None;
            block_id += 1;
        }

        if math_block_end
            .as_ref()
            .is_some_and(|end| is_matching_math_end(trimmed, end))
        {
            let mut sl = StyledLine::new();
            sl.is_math_block = true;
            sl.is_block_fence = true;
            sl.block_id = current_block_id;
            sl.spans.push(StyledSpan {
                text: raw_line.to_string(),
                display_text: Some(String::new()),
                color: crate::design::tokens().doc_math,
                is_syntax: true,
                is_math: true,
                font_size: 16.0,
                ..StyledSpan::plain("")
            });
            lines.push(sl);
            math_block_end = None;
            block_id += 1;
            continue;
        }

        if math_block_end.is_none()
            && let Some(end) = math_block_end_for(trimmed)
        {
            if let Some(inline_math) = single_line_display_math(trimmed) {
                block_id += 1;
                let mut sl = StyledLine::new();
                sl.is_math_block = true;
                sl.block_id = block_id;
                sl.spans.push(StyledSpan {
                    text: raw_line.to_string(),
                    display_text: Some(inline_math.to_string()),
                    color: crate::design::tokens().doc_math,
                    italic: true,
                    font_size: 16.0,
                    is_math: true,
                    ..StyledSpan::plain("")
                });
                lines.push(sl);
                continue;
            }

            if matches!(&end, MathBlockEnd::Environment(env) if has_matching_end_on_line(trimmed, env))
            {
                block_id += 1;
                let mut sl = StyledLine::new();
                sl.is_math_block = true;
                sl.block_id = block_id;
                sl.spans.push(StyledSpan {
                    text: raw_line.to_string(),
                    display_text: Some(trimmed.to_string()),
                    color: crate::design::tokens().doc_math,
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
            math_block_end = Some(end);
            let mut sl = StyledLine::new();
            sl.is_math_block = true;
            sl.is_block_fence = true;
            sl.block_id = current_block_id;
            sl.spans.push(StyledSpan {
                text: raw_line.to_string(),
                display_text: Some(String::new()),
                color: crate::design::tokens().doc_math,
                is_syntax: true,
                is_math: true,
                font_size: 16.0,
                ..StyledSpan::plain("")
            });
            lines.push(sl);
            continue;
        }

        // Inside math block
        if math_block_end.is_some() {
            let mut sl = StyledLine::new();
            sl.is_math_block = true;
            sl.block_id = current_block_id;
            sl.spans.push(StyledSpan {
                text: raw_line.to_string(),
                display_text: None,
                color: crate::design::tokens().doc_math,
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
                sl.spans.push(StyledSpan::syntax(
                    raw_line,
                    crate::design::tokens().doc_syntax,
                    16.0,
                ));
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
            if math_env(first_trimmed, "\\begin").is_some() {
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
                    if math_env(last_trimmed, "\\end").is_some() {
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

pub fn highlight_code_spans(
    line: &str,
    highlighter: &mut Option<syntect::easy::HighlightLines<'static>>,
    lang: Option<&str>,
) -> Vec<StyledSpan> {
    let Some((syntax_set, _)) = syntect_defaults() else {
        return vec![code_span(line, crate::design::tokens().doc_text)];
    };

    if highlighter.is_none() {
        *highlighter = make_code_highlighter(lang);
    }

    let Some(highlighter) = highlighter.as_mut() else {
        return vec![code_span(line, crate::design::tokens().doc_text)];
    };

    let terminated = format!("{line}\n");
    match highlighter.highlight_line(&terminated, syntax_set) {
        Ok(regions) => {
            let spans = regions
                .into_iter()
                .map(|(style, text)| (style, text.strip_suffix('\n').unwrap_or(text)))
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
                vec![code_span(line, crate::design::tokens().doc_text)]
            } else {
                spans
            }
        }
        Err(_) => vec![code_span(line, crate::design::tokens().doc_text)],
    }
}

pub fn make_code_highlighter(lang: Option<&str>) -> Option<syntect::easy::HighlightLines<'static>> {
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

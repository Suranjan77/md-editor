use iced::Color;
use std::sync::OnceLock;
use syntect::highlighting::{HighlightIterator, HighlightState, Highlighter};
use syntect::parsing::{ParseState, ScopeStack};

use crate::theme;

/// A styled text span for rendering.
#[derive(Debug, Clone, PartialEq)]
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
    /// The marker that starts a list item — bullet, number or checkbox.
    /// Wrapped rows of the item hang indented past it.
    pub is_list_marker: bool,
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
            is_list_marker: false,
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
#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum MathBlockEnd {
    Dollars,
    Bracket,
    Environment(String),
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

/// The multi-line context a line is highlighted in: which fenced code block,
/// math block or table, if any, it continues.
///
/// A line's styling is a pure function of its text and this state. That is
/// what lets an edit re-highlight only the lines it can affect: once the state
/// entering an unchanged line matches the state it was highlighted with
/// before, it and everything after it are known to be unchanged.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LineState {
    code: Option<CodeState>,
    /// The delimiter that closes the open math block, if one is open.
    math_end: Option<MathBlockEnd>,
    in_table: bool,
}

/// Inside a fenced code block.
#[derive(Debug, Clone, PartialEq)]
struct CodeState {
    lang: Option<String>,
    /// Syntax highlighting state carried from line to line.
    syntax: Option<(ParseState, HighlightState)>,
}

/// What the document pass needs to know about a line's local result.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct BlockStep {
    /// How the line's spans present on their own, before any math block
    /// conceals them.
    presentation: Presentation,
    /// Blocks begun before the line takes its id.
    before: u8,
    /// The line opens a block that following lines continue.
    opens: bool,
    /// The line belongs to the open block rather than a block of its own.
    continues: bool,
    /// Blocks begun after the line.
    after: u8,
}

/// How a line that continues a block presents on its own. Only such lines
/// can share a math block's id and be concealed by it, and each is either
/// shown or concealed as a whole, which is what lets the document pass
/// restore it exactly.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
enum Presentation {
    /// A line with its own block; never concealed by another.
    #[default]
    Own,
    /// All spans shown as their source.
    Shown,
    /// All spans concealed: fences and table separators.
    Concealed,
}

#[derive(Debug, Clone)]
struct LineSource {
    text: String,
    /// The state the line was highlighted in.
    entry: LineState,
    step: BlockStep,
}

/// A document's highlighted lines, kept exactly in step with its text.
///
/// [`Highlighted::update`] re-highlights from the first changed line only
/// until the block state converges with what was there before, then splices
/// the result in; the outcome is identical to highlighting from scratch.
#[derive(Debug, Clone, Default)]
pub struct Highlighted {
    pub lines: Vec<StyledLine>,
    sources: Vec<LineSource>,
    /// The state after the last line.
    end_state: LineState,
    /// Changes whenever `lines` does, and never repeats, even across
    /// instances: a revision names one set of lines.
    revision: u64,
}

/// A revision no `Highlighted` has had before.
fn next_revision() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Highlighted {
    /// Identifies the current `lines`; see the field.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    #[cfg(test)]
    pub fn new(text: &str) -> Self {
        let mut highlighted = Self::default();
        highlighted.update(text);
        highlighted
    }

    /// Bring the lines in step with `text`. Returns the range of lines that
    /// were re-highlighted.
    pub fn update(&mut self, text: &str) -> std::ops::Range<usize> {
        let texts: Vec<&str> = text.split('\n').collect();
        let (old_n, new_n) = (self.sources.len(), texts.len());

        let prefix = self
            .sources
            .iter()
            .zip(&texts)
            .take_while(|(source, text)| source.text == **text)
            .count();
        let suffix = self
            .sources
            .iter()
            .rev()
            .zip(texts.iter().rev())
            .take(old_n.min(new_n) - prefix)
            .take_while(|(source, text)| source.text == **text)
            .count();

        let mut state = match self.sources.get(prefix) {
            Some(source) => source.entry.clone(),
            None => self.end_state.clone(),
        };
        let mut lines = Vec::new();
        let mut sources = Vec::new();
        let mut i = prefix;
        // Index of the first old line that is kept.
        let mut old_resume = old_n;
        let mut converged = false;
        while i < new_n {
            if i >= new_n - suffix {
                let old_i = i + old_n - new_n;
                if self.sources[old_i].entry == state {
                    old_resume = old_i;
                    converged = true;
                    break;
                }
            }
            let entry = state;
            let (line, exit, step) = highlight_with_state(&entry, texts[i]);
            lines.push(line);
            sources.push(LineSource {
                text: texts[i].to_string(),
                entry,
                step,
            });
            state = exit;
            i += 1;
        }

        self.lines.splice(prefix..old_resume, lines);
        self.sources.splice(prefix..old_resume, sources);
        if !converged {
            self.end_state = state;
        }
        decorate(&mut self.lines, &self.sources);
        self.revision = next_revision();
        prefix..i
    }
}

/// Parse markdown text into styled lines for rendering.
#[cfg(test)]
pub fn highlight_markdown(text: &str) -> Vec<StyledLine> {
    Highlighted::new(text).lines
}

/// Highlight one raw line in `state`, returning the styled line, the state
/// after it, and how it steps block numbering.
fn highlight_with_state(state: &LineState, raw_line: &str) -> (StyledLine, LineState, BlockStep) {
    let raw_line = raw_line.trim_end_matches('\r');
    let trimmed = raw_line.trim();
    let mut state = state.clone();
    let mut step = BlockStep::default();

    // A table ends at the first line that isn't a row.
    if state.in_table && !trimmed.starts_with('|') {
        state.in_table = false;
    }

    // Code block fences.
    if trimmed.starts_with("```") {
        let mut sl = StyledLine::new();
        sl.is_code_block = true;
        sl.is_block_fence = true;
        step.continues = true;
        step.presentation = Presentation::Concealed;
        let font_size = if state.code.is_some() {
            state.code = None;
            step.after = 1;
            13.0
        } else {
            let lang = trimmed
                .strip_prefix("```")
                .filter(|rest| !rest.is_empty())
                .map(|rest| rest.trim().to_string());
            step.before = 1;
            step.opens = true;
            sl.code_block_lang = lang.clone();
            state.code = Some(CodeState {
                syntax: start_code_syntax(lang.as_deref()),
                lang,
            });
            14.0
        };
        sl.spans.push(StyledSpan {
            text: raw_line.to_string(),
            display_text: Some(String::new()),
            color: theme::TEXT_MUTED,
            is_syntax: true,
            font_size,
            is_code: true,
            ..StyledSpan::plain("")
        });
        return (sl, state, step);
    }

    // Inside a code block. Fenced code is literal Markdown: in particular,
    // math delimiters in a `markdown` example must not start a math block.
    if let Some(code) = &mut state.code {
        let mut sl = StyledLine::new();
        sl.is_code_block = true;
        sl.code_block_lang = code.lang.clone();
        sl.spans = highlight_code_spans(raw_line, code);
        step.continues = true;
        step.presentation = Presentation::Shown;
        return (sl, state, step);
    }

    // Math block fences. Keep this narrow so ordinary markdown/HTML is not
    // accidentally promoted to a math block.
    if state.math_end.is_some() && is_obvious_markdown_boundary(trimmed) {
        state.math_end = None;
        step.before += 1;
    }

    let math_fence = |sl: &mut StyledLine| {
        sl.is_math_block = true;
        sl.is_block_fence = true;
        sl.spans.push(StyledSpan {
            text: raw_line.to_string(),
            display_text: Some(String::new()),
            color: theme::WARNING,
            is_syntax: true,
            is_math: true,
            font_size: 16.0,
            ..StyledSpan::plain("")
        });
    };

    if state
        .math_end
        .as_ref()
        .is_some_and(|end| is_matching_math_end(trimmed, end))
    {
        let mut sl = StyledLine::new();
        math_fence(&mut sl);
        state.math_end = None;
        step.presentation = Presentation::Concealed;
        step.continues = true;
        step.after = 1;
        return (sl, state, step);
    }

    if state.math_end.is_none()
        && let Some(end) = math_block_end_for(trimmed)
    {
        step.before += 1;
        let mut sl = StyledLine::new();
        let single_line = single_line_display_math(trimmed).or_else(|| {
            matches!(&end, MathBlockEnd::Environment(env) if has_matching_end_on_line(trimmed, env))
                .then_some(trimmed)
        });
        if let Some(inline_math) = single_line {
            sl.is_math_block = true;
            sl.spans.push(StyledSpan {
                text: raw_line.to_string(),
                display_text: Some(inline_math.to_string()),
                color: theme::WARNING,
                italic: true,
                font_size: 16.0,
                is_math: true,
                ..StyledSpan::plain("")
            });
            return (sl, state, step);
        }
        math_fence(&mut sl);
        state.math_end = Some(end);
        step.presentation = Presentation::Concealed;
        step.opens = true;
        step.continues = true;
        return (sl, state, step);
    }

    // Inside a math block.
    if state.math_end.is_some() {
        let mut sl = StyledLine::new();
        sl.is_math_block = true;
        sl.spans.push(StyledSpan {
            text: raw_line.to_string(),
            display_text: None,
            color: theme::WARNING,
            italic: true,
            font_size: 16.0,
            is_math: true,
            ..StyledSpan::plain("")
        });
        step.presentation = Presentation::Shown;
        step.continues = true;
        return (sl, state, step);
    }

    // Table row.
    if trimmed.starts_with('|') && trimmed.contains('|') {
        if !state.in_table {
            state.in_table = true;
            step.before += 1;
            step.opens = true;
        }
        step.continues = true;
        let mut sl = StyledLine::new();
        sl.is_table_row = true;

        let is_separator = trimmed
            .chars()
            .all(|c| c == '|' || c == '-' || c == ' ' || c == ':');
        if is_separator {
            step.presentation = Presentation::Concealed;
            sl.spans
                .push(StyledSpan::syntax(raw_line, theme::TEXT_MUTED, 16.0));
        } else {
            step.presentation = Presentation::Shown;
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
        return (sl, state, step);
    }

    // A regular line: its own block, styled inline.
    step.before += 1;
    (highlight_line(raw_line), state, step)
}

/// Document-wide decoration of the line-local results: block ids, merged
/// display math, and caption numbers. Idempotent, so it can run after every
/// update over lines it has already decorated.
fn decorate(lines: &mut [StyledLine], sources: &[LineSource]) {
    number_blocks(lines, sources);
    merge_math_blocks(lines, sources);
    number_captions(lines);
}

/// Assign block ids: consecutive lines of one code block, math block or
/// table share an id, and every other line has its own.
///
/// Every block is homogeneous — all its lines are of one kind — so consumers
/// can take a block's kind from any of its lines. Malformed nesting (a `$$`
/// inside a code fence, say) can make a line continue a block of another
/// kind; such a line starts a block of its own instead.
fn number_blocks(lines: &mut [StyledLine], sources: &[LineSource]) {
    let (mut next, mut open) = (0usize, 0usize);
    let mut open_kind = None;
    for (line, source) in lines.iter_mut().zip(sources) {
        let step = source.step;
        let kind = block_kind(line);
        next += step.before as usize;
        if step.opens {
            open = next;
            open_kind = kind;
        } else if step.continues && kind != open_kind {
            next += 1;
            open = next;
            open_kind = kind;
        }
        line.block_id = if step.continues { open } else { next };
        next += step.after as usize;
    }
}

#[derive(Clone, Copy, PartialEq)]
enum BlockKind {
    Code,
    Math,
    Table,
}

fn block_kind(line: &StyledLine) -> Option<BlockKind> {
    if line.is_code_block {
        Some(BlockKind::Code)
    } else if line.is_math_block {
        Some(BlockKind::Math)
    } else if line.is_table_row {
        Some(BlockKind::Table)
    } else {
        None
    }
}

/// Collect each fenced math block's source onto its opening fence, which
/// renders the equation, and conceal the lines after it.
fn merge_math_blocks(lines: &mut [StyledLine], sources: &[LineSource]) {
    // Start from each line's own presentation, so a line that has left a
    // math block since the last pass doesn't keep that block's concealment.
    for (line, source) in lines.iter_mut().zip(sources) {
        let (display, is_syntax) = match source.step.presentation {
            Presentation::Own => continue,
            Presentation::Shown => (None, false),
            Presentation::Concealed => (Some(String::new()), true),
        };
        for span in &mut line.spans {
            span.display_text = display.clone();
            span.is_syntax = is_syntax;
        }
    }

    let mut idx = 0;
    while idx < lines.len() {
        if !(lines[idx].is_math_block && lines[idx].is_block_fence && lines[idx].block_id > 0) {
            idx += 1;
            continue;
        }
        let block_id = lines[idx].block_id;
        let mut math_lines: Vec<String> = Vec::new();

        // If the opening fence has some math content (like \begin{align}), include it.
        let first_trimmed = lines[idx]
            .spans
            .first()
            .map(|s| s.text.trim())
            .unwrap_or("");
        if math_env(first_trimmed, "\\begin").is_some() {
            math_lines.push(first_trimmed.to_string());
        }

        let mut j = idx + 1;
        while j < lines.len() && lines[j].block_id == block_id {
            let first = lines[j].spans.first().map(|s| s.text.as_str());
            if !lines[j].is_block_fence {
                if let Some(text) = first {
                    math_lines.push(text.to_string());
                }
            } else if let Some(text) = first.map(str::trim)
                && math_env(text, "\\end").is_some()
            {
                // If the closing fence is \end{...}, include it.
                math_lines.push(text.to_string());
            }
            j += 1;
        }

        let consolidated = math_lines.join("\n");
        if let Some(span) = lines[idx].spans.first_mut()
            && !consolidated.is_empty()
        {
            span.display_text = Some(consolidated);
            span.is_syntax = false;
            span.is_math = true;
            span.font_size = 16.0;
        }

        for hidden in &mut lines[idx + 1..j] {
            for span in &mut hidden.spans {
                span.display_text = Some(String::new());
                span.is_syntax = true;
            }
        }

        idx = j;
    }
}

/// Assign sequential ids to images, display math blocks, tables and code
/// blocks, for numbered captions and cross-reference links.
fn number_captions(lines: &mut [StyledLine]) {
    let (mut images, mut equations, mut tables, mut code) = (0, 0, 0, 0);
    let mut seen_math = std::collections::HashSet::new();
    let mut seen_tables = std::collections::HashSet::new();
    let mut seen_code = std::collections::HashSet::new();

    for line in lines.iter_mut() {
        if let Some(first) = line.spans.first_mut()
            && !first.is_image
        {
            first.id = None;
        }
        if line.is_math_block && line.block_id > 0 && seen_math.insert(line.block_id) {
            equations += 1;
            if let Some(first) = line.spans.first_mut() {
                first.id = Some(format!("equation-{}", equations));
            }
        }
        if line.is_table_row && line.block_id > 0 && seen_tables.insert(line.block_id) {
            tables += 1;
            if let Some(first) = line.spans.first_mut() {
                first.id = Some(format!("table-{}", tables));
            }
        }
        if line.is_code_block && line.block_id > 0 && seen_code.insert(line.block_id) {
            code += 1;
            if let Some(first) = line.spans.first_mut() {
                first.id = Some(format!("code-{}", code));
            }
        }
        for span in &mut line.spans {
            if span.is_image {
                images += 1;
                span.id = Some(format!("figure-{}", images));
            }
        }
    }
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
    if is_horizontal_rule(trimmed) {
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
        // Blockquote content can contain the same inline Markdown as a
        // paragraph. Keep the quote styling while retaining actionable links.
        let content_start = sl.spans.len();
        parse_inline_spans(&line[rest_start..], &mut sl.spans);
        if sl.spans.len() == content_start {
            sl.spans.push(StyledSpan::plain(""));
        }
        for span in &mut sl.spans[content_start..] {
            span.color = theme::ACCENT;
            span.italic = true;
        }
        return sl;
    }

    // Task list items (must check before regular list items)
    if is_task_prefix(trimmed) {
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
                theme::ACCENT
            } else {
                theme::TEXT_MUTED
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
            color: theme::ACCENT,
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

fn is_horizontal_rule(trimmed: &str) -> bool {
    let mut chars = trimmed.chars();
    let Some(marker @ ('-' | '*' | '_')) = chars.next() else {
        return false;
    };
    chars.clone().count() >= 2 && chars.all(|ch| ch == marker)
}

fn is_task_prefix(trimmed: &str) -> bool {
    ["- [ ]", "- [x]", "- [X]"]
        .iter()
        .any(|prefix| trimmed == *prefix || trimmed.starts_with(&format!("{prefix} ")))
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
            if let Some(end_text) = find_char(&chars, i + 1, ']')
                && end_text + 1 < len
                && chars[end_text + 1] == '('
                && let Some(end_url) = find_char(&chars, end_text + 2, ')')
            {
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
            if let Some(end_alt) = find_char(&chars, i + 2, ']')
                && end_alt + 1 < len
                && chars[end_alt + 1] == '('
                && let Some(end_url) = find_char(&chars, end_alt + 2, ')')
            {
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

fn math_block_end_for(trimmed: &str) -> Option<MathBlockEnd> {
    if trimmed.starts_with("$$") {
        Some(MathBlockEnd::Dollars)
    } else if trimmed == "\\[" {
        Some(MathBlockEnd::Bracket)
    } else {
        math_env(trimmed, "\\begin")
            .map(|environment| MathBlockEnd::Environment(environment.to_string()))
    }
}

fn is_matching_math_end(trimmed: &str, expected: &MathBlockEnd) -> bool {
    match expected {
        MathBlockEnd::Dollars => trimmed.starts_with("$$"),
        MathBlockEnd::Bracket => trimmed == "\\]",
        MathBlockEnd::Environment(environment) => {
            math_env(trimmed, "\\end") == Some(environment.as_str())
        }
    }
}

fn has_matching_end_on_line(trimmed: &str, environment: &str) -> bool {
    let Some(open_end) = trimmed.find('}') else {
        return false;
    };
    trimmed[open_end + 1..]
        .match_indices("\\end")
        .any(|(idx, _)| math_env(&trimmed[open_end + 1 + idx..], "\\end") == Some(environment))
}

fn math_env<'a>(trimmed: &'a str, command: &str) -> Option<&'a str> {
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
    if i > 0 && i < bytes.len() && bytes[i] == b'.' && i + 1 < bytes.len() && bytes[i + 1] == b' ' {
        return Some(i + 2);
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
    (start..chars.len()).find(|&i| chars[i] == target)
}

fn find_double(chars: &[char], start: usize, target: char) -> Option<usize> {
    (start..chars.len().saturating_sub(1)).find(|&i| chars[i] == target && chars[i + 1] == target)
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

/// Syntax-highlight one line of a code block, advancing its state.
fn highlight_code_spans(line: &str, code: &mut CodeState) -> Vec<StyledSpan> {
    let plain = || vec![code_span(line, theme::TEXT_PRIMARY)];
    let Some((syntax_set, _)) = syntect_defaults() else {
        return plain();
    };
    if code.syntax.is_none() {
        code.syntax = start_code_syntax(code.lang.as_deref());
    }
    let (Some((parse, highlight)), Some(highlighter)) = (code.syntax.as_mut(), code_highlighter())
    else {
        return plain();
    };

    // The bundled grammars expect each line to end in a newline; without one,
    // constructs that end at the line end (a `#` comment, say) never close and
    // bleed into the following lines.
    let terminated = format!("{line}\n");
    match parse.parse_line(&terminated, syntax_set) {
        Ok(ops) => {
            let spans = HighlightIterator::new(highlight, &ops, &terminated, highlighter)
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
            if spans.is_empty() { plain() } else { spans }
        }
        Err(_) => plain(),
    }
}

/// Fresh syntax state for a code block in `lang`.
fn start_code_syntax(lang: Option<&str>) -> Option<(ParseState, HighlightState)> {
    let (syntax_set, _) = syntect_defaults()?;
    let syntax = lang
        .and_then(|lang| syntax_set.find_syntax_by_token(lang))
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    let highlighter = code_highlighter()?;
    Some((
        ParseState::new(syntax),
        HighlightState::new(highlighter, ScopeStack::new()),
    ))
}

/// The code highlighter for the editor's syntax theme.
fn code_highlighter() -> Option<&'static Highlighter<'static>> {
    static HIGHLIGHTER: OnceLock<Option<Highlighter<'static>>> = OnceLock::new();
    HIGHLIGHTER
        .get_or_init(|| {
            let (_, theme_set) = syntect_defaults()?;
            let theme = theme_set
                .themes
                .get("base16-ocean.dark")
                .or_else(|| theme_set.themes.values().next())?;
            Some(Highlighter::new(theme))
        })
        .as_ref()
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

/// The highlighter as it was before it became incremental, kept as a
/// reference: the stateful implementation must produce identical lines.
#[cfg(test)]
mod reference;

#[cfg(test)]
mod tests;

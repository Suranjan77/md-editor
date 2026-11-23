use iced::Color;
use std::sync::OnceLock;
use syntect::highlighting::{HighlightIterator, HighlightState, Highlighter};
use syntect::parsing::{ParseState, ScopeStack};

use crate::theme;

mod code;
mod inline;

use code::*;
use inline::*;

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

/// The highlighter as it was before it became incremental, kept as a
/// reference: the stateful implementation must produce identical lines.
#[cfg(test)]
mod reference;

#[cfg(test)]
mod tests;

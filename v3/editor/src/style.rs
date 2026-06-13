//! Phase-1 styling: inline span parsing and the production [`MarkdownStyler`].
//!
//! The conceal strategy is **reserved width** (plan §3.2): the display
//! string is always the full source text — markers included — and conceal
//! only flips how marker spans are *painted* (hidden vs muted). Measured
//! geometry is therefore identical in both modes, which is what makes
//! `Styler::layout_stable()` true by construction instead of by hope.
//!
//! Span ranges are **char offsets** into the display string, matching the
//! buffer's offset space.

use std::ops::Range;

use crate::layout::{ConcealMode, StyledLine, Styler};
use crate::parse::{BlockState, LineKind, classify};
use crate::syntax::{Lang, LexState, SyntaxRole, lex_line};

/// Paint semantics of a span. The shell maps these to theme attributes;
/// nothing here knows about colors or fonts (ADR-0100).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanKind {
    Text,
    /// Syntax characters (`**`, `` ` ``, `[[`, heading `#`s, …). Painted
    /// muted when the line is revealed, invisible when concealed — but
    /// always measured (reserved width).
    Marker,
    Bold,
    Italic,
    Code,
    /// A syntax-highlighted run inside fenced code (ADR-0106). The shell maps
    /// the [`SyntaxRole`] to a theme color; the measurer shapes it with the
    /// *identical* monospace attrs it uses for [`SpanKind::CodeContent`], so
    /// highlighting changes paint only and never moves a glyph.
    CodeToken(SyntaxRole),
    Math,
    /// Link label; `url` is what activation opens.
    LinkText {
        url: String,
    },
    /// Markdown image label; `url` is resolved by shell-side asset loading.
    Image {
        url: String,
    },
    WikiLink,
    /// Whole-line kinds reuse spans too:
    CodeContent,
    MathContent,
    FrontMatter,
    QuoteText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub kind: SpanKind,
}

impl Span {
    fn new(range: Range<usize>, kind: SpanKind) -> Span {
        Span { range, kind }
    }
}

/// The production styler: classification via [`classify`] (same grammar as
/// the incremental parser) + inline span extraction.
#[derive(Debug, Clone, Copy, Default)]
pub struct MarkdownStyler;

impl Styler for MarkdownStyler {
    fn style(&self, text: &str, block: &BlockState, conceal: ConcealMode) -> StyledLine {
        let (kind, _) = classify(text, block);
        let mut spans = line_spans(text, &kind);

        // Syntax-highlight fenced code: replace the single `CodeContent` span
        // with role-tagged sub-spans (ADR-0106). The entry lexer state lives
        // in the line's `Fence` block state, so multi-line constructs resolve
        // correctly. This is paint-only — the sub-spans tile the same range
        // and shape with identical monospace attrs (geometry-invariant).
        if let (LineKind::CodeContent, BlockState::Fence { lang, lex, .. }) = (&kind, block)
            && *lang != Lang::None
        {
            spans = highlight_code(text, *lang, *lex);
        }

        let mut display = text.to_string();

        if conceal == ConcealMode::Concealed {
            // Drop marker spans and adjust remaining span offsets
            let mut new_spans = Vec::new();
            let mut new_display = String::new();
            let chars: Vec<char> = text.chars().collect();

            // Build a list of ranges that we KEEP.
            // A character is kept if it is not inside any SpanKind::Marker.
            let mut keep = vec![true; chars.len()];

            // Some markers shouldn't be concealed in block contexts like tables, wait, marker_is_concealed handled this!
            // Let's see what marker_is_concealed did.
            // "Pipes are markers (never concealed by the shell — tables keep their structure visible)"
            let hide_markers = !matches!(kind, LineKind::TableRow | LineKind::TableSep);

            if hide_markers {
                for span in &spans {
                    if span.kind == SpanKind::Marker {
                        for i in span.range.clone() {
                            if i < keep.len() {
                                keep[i] = false;
                            }
                        }
                    }
                }
            }

            let mut old_to_new = vec![0; chars.len() + 1];
            let mut current = 0;
            for (i, &k) in keep.iter().enumerate() {
                old_to_new[i] = current;
                if k {
                    new_display.push(chars[i]);
                    current += 1;
                }
            }
            old_to_new[chars.len()] = current;

            for span in spans {
                if span.kind != SpanKind::Marker || !hide_markers {
                    let start = old_to_new[span.range.start.min(chars.len())];
                    let end = old_to_new[span.range.end.min(chars.len())];
                    if start < end {
                        new_spans.push(Span::new(start..end, span.kind));
                    }
                }
            }
            display = new_display;
            spans = new_spans;
        }

        StyledLine {
            display,
            conceal,
            kind,
            spans,
        }
    }
}

/// Tokenize a fenced-code line into role-tagged spans that **tile** `0..n`
/// exactly (gaps between highlighted runs become base [`SpanKind::CodeContent`]).
/// Char-offset based, matching the buffer/display offset space.
fn highlight_code(text: &str, lang: Lang, lex: LexState) -> Vec<Span> {
    let n = text.chars().count();
    let mut spans = Vec::new();
    let mut cursor = 0;
    lex_line(lang, lex, text, |start, end, role| {
        if start > cursor {
            spans.push(Span::new(cursor..start, SpanKind::CodeContent));
        }
        spans.push(Span::new(start..end, SpanKind::CodeToken(role)));
        cursor = end;
    });
    if cursor < n {
        spans.push(Span::new(cursor..n, SpanKind::CodeContent));
    }
    if spans.is_empty() {
        spans.push(Span::new(0..n, SpanKind::CodeContent));
    }
    spans
}

/// Spans for one line given its block kind. Char-offset based.
pub fn line_spans(text: &str, kind: &LineKind) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut spans = Vec::new();
    match kind {
        LineKind::Blank => {}
        LineKind::FenceOpen { .. } | LineKind::FenceClose => {
            spans.push(Span::new(0..n, SpanKind::Marker));
        }
        LineKind::CodeContent => spans.push(Span::new(0..n, SpanKind::CodeContent)),
        LineKind::MathOpen | LineKind::MathClose => {
            spans.push(Span::new(0..n, SpanKind::Marker));
        }
        LineKind::MathContent => spans.push(Span::new(0..n, SpanKind::MathContent)),
        LineKind::MathLine => {
            // $$ … $$ on one line: markers at both ends.
            let lead = leading_ws(&chars);
            spans.push(Span::new(lead..lead + 2, SpanKind::Marker));
            let close = n.saturating_sub(2).max(lead + 2);
            if close > lead + 2 {
                spans.push(Span::new(lead + 2..close, SpanKind::MathContent));
            }
            spans.push(Span::new(close..n, SpanKind::Marker));
        }
        LineKind::FrontMatterDelim => spans.push(Span::new(0..n, SpanKind::Marker)),
        LineKind::FrontMatterContent => spans.push(Span::new(0..n, SpanKind::FrontMatter)),
        LineKind::Rule => spans.push(Span::new(0..n, SpanKind::Marker)),
        LineKind::Heading { level } => {
            let lead = leading_ws(&chars);
            // `#…# ` prefix (marker includes the space — reserved width).
            let prefix = (lead + *level as usize + 1).min(n);
            spans.push(Span::new(lead..prefix, SpanKind::Marker));
            parse_inline(&chars, prefix, n, &mut spans);
        }
        LineKind::Quote => {
            let lead = leading_ws(&chars);
            let mut after = lead;
            while after < n && (chars[after] == '>' || chars[after] == ' ') {
                after += 1;
            }
            spans.push(Span::new(lead..after, SpanKind::Marker));
            parse_inline(&chars, after, n, &mut spans);
        }
        LineKind::Bullet { checkbox } => {
            let lead = leading_ws(&chars);
            let mut after = (lead + 2).min(n); // "- "
            if checkbox.is_some() {
                after = (after + 4).min(n); // "[x] "
            }
            spans.push(Span::new(lead..after, SpanKind::Marker));
            parse_inline(&chars, after, n, &mut spans);
        }
        LineKind::Ordered => {
            let lead = leading_ws(&chars);
            let mut after = lead;
            while after < n && chars[after].is_ascii_digit() {
                after += 1;
            }
            after = (after + 2).min(n); // ". "
            spans.push(Span::new(lead..after, SpanKind::Marker));
            parse_inline(&chars, after, n, &mut spans);
        }
        LineKind::TableRow | LineKind::TableSep => {
            // Pipes are markers (never concealed by the shell — tables keep
            // their structure visible); cells get inline styling.
            let mut cell_start = 0;
            for (i, &c) in chars.iter().enumerate() {
                if c == '|' && (i == 0 || chars[i - 1] != '\\') {
                    if i > cell_start {
                        parse_inline(&chars, cell_start, i, &mut spans);
                    }
                    spans.push(Span::new(i..i + 1, SpanKind::Marker));
                    cell_start = i + 1;
                }
            }
            if cell_start < n {
                parse_inline(&chars, cell_start, n, &mut spans);
            }
        }
        LineKind::Paragraph => parse_inline(&chars, 0, n, &mut spans),
    }
    spans
}

fn leading_ws(chars: &[char]) -> usize {
    chars.iter().take_while(|c| c.is_whitespace()).count()
}

/// Inline scanner over `chars[start..end]`: emphasis, code, math, links,
/// wikilinks, escapes. Unmatched markers stay literal text. Pragmatic
/// CommonMark subset — the conformance corpus tightens this over time.
fn parse_inline(chars: &[char], start: usize, end: usize, out: &mut Vec<Span>) {
    let mut text_start = start;
    let mut i = start;
    let flush = |from: usize, to: usize, out: &mut Vec<Span>| {
        if to > from {
            out.push(Span::new(from..to, SpanKind::Text));
        }
    };
    while i < end {
        let c = chars[i];
        match c {
            '\\' if i + 1 < end => {
                i += 2; // escaped char stays literal text
            }
            '`' => {
                if let Some(close) = find(chars, i + 1, end, '`') {
                    flush(text_start, i, out);
                    out.push(Span::new(i..i + 1, SpanKind::Marker));
                    out.push(Span::new(i + 1..close, SpanKind::Code));
                    out.push(Span::new(close..close + 1, SpanKind::Marker));
                    i = close + 1;
                    text_start = i;
                } else {
                    i += 1;
                }
            }
            '$' => {
                let ok_open = i + 1 < end && !chars[i + 1].is_whitespace() && chars[i + 1] != '$';
                if ok_open
                    && let Some(close) = find(chars, i + 1, end, '$')
                    && !chars[close - 1].is_whitespace()
                {
                    flush(text_start, i, out);
                    out.push(Span::new(i..i + 1, SpanKind::Marker));
                    out.push(Span::new(i + 1..close, SpanKind::Math));
                    out.push(Span::new(close..close + 1, SpanKind::Marker));
                    i = close + 1;
                    text_start = i;
                } else {
                    i += 1;
                }
            }
            '!' if i + 1 < end && chars[i + 1] == '[' => {
                if let Some((label_end, url_end)) = find_link(chars, i + 1, end) {
                    flush(text_start, i, out);
                    let url: String = chars[label_end + 2..url_end].iter().collect();
                    out.push(Span::new(i..url_end + 1, SpanKind::Image { url }));
                    i = url_end + 1;
                    text_start = i;
                } else {
                    i += 1;
                }
            }
            '*' => {
                let run = run_len(chars, i, end, '*').min(2);
                if let Some((close, close_run)) = find_emphasis_close(chars, i + run, end, run) {
                    flush(text_start, i, out);
                    out.push(Span::new(i..i + run, SpanKind::Marker));
                    let kind = if run == 2 {
                        SpanKind::Bold
                    } else {
                        SpanKind::Italic
                    };
                    // Recurse for nested emphasis/code inside.
                    let mut inner = Vec::new();
                    parse_inline(chars, i + run, close, &mut inner);
                    if inner.iter().all(|s| s.kind == SpanKind::Text) {
                        out.push(Span::new(i + run..close, kind));
                    } else {
                        out.extend(inner);
                    }
                    out.push(Span::new(close..close + close_run, SpanKind::Marker));
                    i = close + close_run;
                    text_start = i;
                } else {
                    i += 1;
                }
            }
            '[' => {
                if i + 1 < end && chars[i + 1] == '[' {
                    if let Some(close) = find_seq(chars, i + 2, end, &[']', ']']) {
                        flush(text_start, i, out);
                        out.push(Span::new(i..i + 2, SpanKind::Marker));
                        out.push(Span::new(i + 2..close, SpanKind::WikiLink));
                        out.push(Span::new(close..close + 2, SpanKind::Marker));
                        i = close + 2;
                        text_start = i;
                        continue;
                    }
                } else if let Some((label_end, url_end)) = find_link(chars, i, end) {
                    flush(text_start, i, out);
                    let url: String = chars[label_end + 2..url_end].iter().collect();
                    out.push(Span::new(i..i + 1, SpanKind::Marker));
                    out.push(Span::new(i + 1..label_end, SpanKind::LinkText { url }));
                    out.push(Span::new(label_end..url_end + 1, SpanKind::Marker));
                    i = url_end + 1;
                    text_start = i;
                    continue;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    flush(text_start, end, out);
}

fn find(chars: &[char], from: usize, end: usize, target: char) -> Option<usize> {
    (from..end).find(|&i| chars[i] == target)
}

fn find_seq(chars: &[char], from: usize, end: usize, seq: &[char]) -> Option<usize> {
    (from..end.saturating_sub(seq.len() - 1))
        .find(|&i| (0..seq.len()).all(|k| chars[i + k] == seq[k]))
}

fn run_len(chars: &[char], from: usize, end: usize, c: char) -> usize {
    (from..end).take_while(|&i| chars[i] == c).count()
}

/// Closing `*`-run of exactly `run` length with a non-space before it.
fn find_emphasis_close(
    chars: &[char],
    from: usize,
    end: usize,
    run: usize,
) -> Option<(usize, usize)> {
    if from >= end || chars[from].is_whitespace() {
        return None; // opener must be followed by non-space
    }
    let mut i = from;
    while i < end {
        if chars[i] == '*' && !chars[i - 1].is_whitespace() && i > from {
            let close_run = run_len(chars, i, end, '*');
            if close_run >= run {
                return Some((i, run));
            }
            i += close_run;
        } else {
            i += 1;
        }
    }
    None
}

/// `[label](url)` with one level of balanced parens in the url.
/// Returns (index of `]`, index of closing `)`).
fn find_link(chars: &[char], open: usize, end: usize) -> Option<(usize, usize)> {
    let label_end = find(chars, open + 1, end, ']')?;
    if label_end + 1 >= end || chars[label_end + 1] != '(' {
        return None;
    }
    let mut depth = 1;
    let mut i = label_end + 2;
    while i < end {
        match chars[i] {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((label_end, i));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str) -> Vec<(String, SpanKind)> {
        let (kind, _) = classify(text, &BlockState::Normal);
        let chars: Vec<char> = text.chars().collect();
        line_spans(text, &kind)
            .into_iter()
            .map(|s| (chars[s.range.clone()].iter().collect(), s.kind))
            .collect()
    }

    /// Spans must tile the line: reconstruction == source (reserved width).
    fn assert_tiles(text: &str) {
        let joined: String = spans(text).iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(joined, text, "spans must cover the full source text");
    }

    #[test]
    fn bold_italic_code_math() {
        assert_eq!(
            spans("a **b** c"),
            vec![
                ("a ".into(), SpanKind::Text),
                ("**".into(), SpanKind::Marker),
                ("b".into(), SpanKind::Bold),
                ("**".into(), SpanKind::Marker),
                (" c".into(), SpanKind::Text),
            ]
        );
        assert_eq!(
            spans("*i* `c` $m$"),
            vec![
                ("*".into(), SpanKind::Marker),
                ("i".into(), SpanKind::Italic),
                ("*".into(), SpanKind::Marker),
                (" ".into(), SpanKind::Text),
                ("`".into(), SpanKind::Marker),
                ("c".into(), SpanKind::Code),
                ("`".into(), SpanKind::Marker),
                (" ".into(), SpanKind::Text),
                ("$".into(), SpanKind::Marker),
                ("m".into(), SpanKind::Math),
                ("$".into(), SpanKind::Marker),
            ]
        );
    }

    #[test]
    fn unmatched_markers_stay_literal() {
        assert_eq!(spans("a ** b"), vec![("a ** b".into(), SpanKind::Text)]);
        assert_eq!(
            spans("2 * 3 = 6"),
            vec![("2 * 3 = 6".into(), SpanKind::Text)]
        );
        assert_tiles("`unclosed");
        assert_tiles("[label only");
        assert_tiles("$5 and $6");
    }

    #[test]
    fn escapes_disable_markup() {
        assert_eq!(
            spans(r"\*\*not bold\*\*"),
            vec![(r"\*\*not bold\*\*".into(), SpanKind::Text)]
        );
    }

    #[test]
    fn links_and_wikilinks() {
        assert_eq!(
            spans("see [docs](https://x.y/(v2)) now"),
            vec![
                ("see ".into(), SpanKind::Text),
                ("[".into(), SpanKind::Marker),
                (
                    "docs".into(),
                    SpanKind::LinkText {
                        url: "https://x.y/(v2)".into()
                    }
                ),
                ("](https://x.y/(v2))".into(), SpanKind::Marker),
                (" now".into(), SpanKind::Text),
            ]
        );
        assert_eq!(
            spans("a [[note]] b"),
            vec![
                ("a ".into(), SpanKind::Text),
                ("[[".into(), SpanKind::Marker),
                ("note".into(), SpanKind::WikiLink),
                ("]]".into(), SpanKind::Marker),
                (" b".into(), SpanKind::Text),
            ]
        );
    }

    #[test]
    fn image_is_one_semantic_span() {
        assert_eq!(
            spans("before ![plot](images/plot.png) after"),
            vec![
                ("before ".into(), SpanKind::Text),
                (
                    "![plot](images/plot.png)".into(),
                    SpanKind::Image {
                        url: "images/plot.png".into()
                    }
                ),
                (" after".into(), SpanKind::Text),
            ]
        );
    }

    #[test]
    fn heading_and_list_prefixes_are_markers() {
        assert_eq!(spans("## Title **b**")[0], ("## ".into(), SpanKind::Marker));
        assert_eq!(spans("- [x] done")[0], ("- [x] ".into(), SpanKind::Marker));
        assert_eq!(spans("12. step")[0], ("12. ".into(), SpanKind::Marker));
        assert_eq!(spans("> quoted")[0], ("> ".into(), SpanKind::Marker));
    }

    #[test]
    fn table_pipes_are_markers_cells_styled() {
        let s = spans("| **a** | b |");
        assert_eq!(s[0], ("|".into(), SpanKind::Marker));
        assert!(s.contains(&("a".into(), SpanKind::Bold)));
        assert_tiles("| **a** | b |");
    }

    #[test]
    fn every_line_tiles_exactly() {
        for text in [
            "",
            "plain",
            "# h1 with [link](u) and **bold**",
            "```rust",
            "- [ ] todo with `code`",
            "> q **b** *i*",
            "$$x$$",
            "***wat***",
            "**a *b* c**",
            "🇳🇵 **한글** 👨‍👩‍👧‍👦",
            "| a | **b** | `c` |",
            r"\* literal",
        ] {
            assert_tiles(text);
        }
    }

    #[test]
    fn block_state_overrides_inline_rules() {
        let styler = MarkdownStyler;
        // A plain (no-language) fence keeps the single code span.
        let inside_fence = styler.style(
            "**not bold**",
            &BlockState::Fence {
                marker: '`',
                len: 3,
                lang: Lang::None,
                lex: LexState::Normal,
            },
            ConcealMode::Concealed,
        );
        assert_eq!(
            inside_fence.spans,
            vec![Span::new(0..12, SpanKind::CodeContent)]
        );
    }

    fn fence(lang: Lang, lex: LexState) -> BlockState {
        BlockState::Fence {
            marker: '`',
            len: 3,
            lang,
            lex,
        }
    }

    #[test]
    fn rust_code_line_splits_into_role_spans() {
        let styled = MarkdownStyler.style(
            "let x = 1;",
            &fence(Lang::Rust, LexState::Normal),
            ConcealMode::Concealed,
        );
        // Spans must still tile the full line (reserved-width invariant).
        let joined: String = styled
            .spans
            .iter()
            .flat_map(|s| "let x = 1;".chars().collect::<Vec<_>>()[s.range.clone()].to_vec())
            .collect();
        assert_eq!(joined, "let x = 1;");
        assert!(
            styled
                .spans
                .iter()
                .any(|s| s.kind == SpanKind::CodeToken(SyntaxRole::Keyword))
        );
        assert!(
            styled
                .spans
                .iter()
                .any(|s| s.kind == SpanKind::CodeToken(SyntaxRole::Number))
        );
    }

    #[test]
    fn unknown_language_keeps_single_code_span() {
        let styled = MarkdownStyler.style(
            "let x = 1;",
            &fence(Lang::None, LexState::Normal),
            ConcealMode::Concealed,
        );
        assert_eq!(styled.spans, vec![Span::new(0..10, SpanKind::CodeContent)]);
    }

    #[test]
    fn highlight_span_set_does_not_depend_on_conceal_mode() {
        // Code has no markers, so conceal must not change the spans — this is
        // what keeps highlighting geometry-invariant across reveal toggles.
        let line = "fn main() {}";
        let block = fence(Lang::Rust, LexState::Normal);
        let concealed = MarkdownStyler.style(line, &block, ConcealMode::Concealed);
        let revealed = MarkdownStyler.style(line, &block, ConcealMode::Revealed);
        assert_eq!(concealed.spans, revealed.spans);
        assert_eq!(concealed.display, revealed.display);
    }
}

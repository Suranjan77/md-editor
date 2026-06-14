//! Incremental block-level parser (plan §3.2, ADR-0101): every line is
//! classified given an explicit **entry state** (what multi-line construct
//! we are inside) and produces an **exit state**. An edit re-parses the
//! edited lines, then continues *forward only* until a line's computed
//! entry state matches what was already stored — convergence. Typing a
//! ``` fence therefore invalidates exactly the lines whose meaning changed,
//! and the parser reports that range so styling can follow.
//!
//! Classification is range-scoped and free of implicit global state, so styling
//! follows only the spans the parser reports as changed.

use std::ops::Range;

use crate::syntax::{Lang, LexState, lex_line};

/// State carried *across* a line boundary. `PartialEq` is what convergence
/// is defined over.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum BlockState {
    /// Entry state of line 0 only. Identical to `Normal` except that `---`
    /// opens YAML front matter here. Encoding the position in the *state*
    /// (rather than special-casing index 0 in the parser) keeps
    /// classification pure on `(text, entry)`, which the convergence rule
    /// depends on: a line moving to or away from line 0 sees a different
    /// entry state and therefore reparses.
    DocStart,
    #[default]
    Normal,
    /// Inside a fenced code block. Closing requires the same marker char
    /// and at least the same run length (CommonMark rule).
    ///
    /// `lang` is the resolved highlight language (constant within a fence,
    /// so it never perturbs convergence) and `lex` is the cross-line syntax
    /// lexer state (ADR-0106) — opening a multi-line construct changes the
    /// exit state, so following code lines reparse until it converges, reusing
    /// the same forward-convergence rule the fence itself relies on.
    Fence {
        marker: char,
        len: usize,
        lang: Lang,
        lex: LexState,
    },
    /// Inside a `$$ … $$` display-math block.
    ///
    /// The first content line owns the consolidated rendered asset. Tracking
    /// that transition lets layout collapse later concealed source lines
    /// without relying on their text being unique.
    Math { first_content: bool },
    /// Inside the YAML front-matter block (only enterable from line 0).
    FrontMatter,
}

/// What a line *is*, given its text and entry state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineKind {
    Blank,
    Heading {
        level: u8,
    },
    /// `-`/`*`/`+` list item; `checkbox` = Some(checked) for `- [ ]`/`- [x]`.
    Bullet {
        checkbox: Option<bool>,
    },
    Ordered,
    Quote,
    Rule,
    FenceOpen {
        lang: String,
    },
    FenceClose,
    CodeContent,
    MathOpen,
    MathClose,
    /// Single-line `$$ … $$`.
    MathLine,
    MathContentStart,
    MathContent,
    FrontMatterDelim,
    FrontMatterContent,
    TableRow,
    TableSep,
    Paragraph,
}

/// Classify one line: `(kind, exit state)` from `(text, entry state)`.
/// Pure — this is the whole grammar, and the styler reuses it.
pub fn classify(text: &str, entry: &BlockState) -> (LineKind, BlockState) {
    let trimmed = text.trim();
    match entry {
        BlockState::Fence {
            marker,
            len,
            lang,
            lex,
        } => {
            let close_run = trimmed.chars().take_while(|c| c == marker).count();
            if close_run >= *len && trimmed.chars().all(|c| c == *marker) {
                (LineKind::FenceClose, BlockState::Normal)
            } else {
                // Advance the syntax lexer across this code line so multi-line
                // constructs carry forward (the styler recomputes the tokens
                // themselves for paint). Only the exit state matters for
                // convergence, so the tokens are discarded here.
                let exit_lex = lex_line(*lang, *lex, text, |_, _, _| {});
                (
                    LineKind::CodeContent,
                    BlockState::Fence {
                        marker: *marker,
                        len: *len,
                        lang: *lang,
                        lex: exit_lex,
                    },
                )
            }
        }
        BlockState::Math { first_content } => {
            if trimmed.starts_with("$$") {
                (LineKind::MathClose, BlockState::Normal)
            } else if *first_content {
                (
                    LineKind::MathContentStart,
                    BlockState::Math {
                        first_content: false,
                    },
                )
            } else {
                (
                    LineKind::MathContent,
                    BlockState::Math {
                        first_content: false,
                    },
                )
            }
        }
        BlockState::FrontMatter => {
            if trimmed == "---" {
                (LineKind::FrontMatterDelim, BlockState::Normal)
            } else {
                (LineKind::FrontMatterContent, BlockState::FrontMatter)
            }
        }
        BlockState::DocStart => {
            if trimmed == "---" {
                (LineKind::FrontMatterDelim, BlockState::FrontMatter)
            } else {
                classify_normal(trimmed)
            }
        }
        BlockState::Normal => classify_normal(trimmed),
    }
}

fn classify_normal(trimmed: &str) -> (LineKind, BlockState) {
    if trimmed.is_empty() {
        return (LineKind::Blank, BlockState::Normal);
    }
    // Fence open: ``` or ~~~, length >= 3.
    for marker in ['`', '~'] {
        if trimmed.starts_with(&String::from(marker).repeat(3)) {
            let len = trimmed.chars().take_while(|c| *c == marker).count();
            let lang = trimmed[len..].trim().to_string();
            // ```foo``` on one line is inline code, not a fence.
            if marker == '`' && trimmed[len..].contains('`') {
                break;
            }
            let resolved = Lang::from_tag(&lang);
            return (
                LineKind::FenceOpen { lang },
                BlockState::Fence {
                    marker,
                    len,
                    lang: resolved,
                    lex: LexState::Normal,
                },
            );
        }
    }
    if let Some(rest) = trimmed.strip_prefix("$$") {
        if !rest.is_empty() && rest.ends_with("$$") {
            return (LineKind::MathLine, BlockState::Normal);
        }
        return (
            LineKind::MathOpen,
            BlockState::Math {
                first_content: true,
            },
        );
    }
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&level) && trimmed.chars().nth(level).is_none_or(|c| c == ' ') {
        return (LineKind::Heading { level: level as u8 }, BlockState::Normal);
    }
    if trimmed.starts_with('>') {
        return (LineKind::Quote, BlockState::Normal);
    }
    if is_rule(trimmed) {
        return (LineKind::Rule, BlockState::Normal);
    }
    if let Some(rest) = strip_bullet(trimmed) {
        let checkbox = match rest.get(..4) {
            Some("[ ] ") => Some(false),
            Some("[x] ") | Some("[X] ") => Some(true),
            _ if rest == "[ ]" => Some(false),
            _ if rest == "[x]" || rest == "[X]" => Some(true),
            _ => None,
        };
        return (LineKind::Bullet { checkbox }, BlockState::Normal);
    }
    if is_ordered(trimmed) {
        return (LineKind::Ordered, BlockState::Normal);
    }
    if trimmed.starts_with('|') {
        if is_table_separator(trimmed) {
            return (LineKind::TableSep, BlockState::Normal);
        }
        return (LineKind::TableRow, BlockState::Normal);
    }
    (LineKind::Paragraph, BlockState::Normal)
}

fn is_rule(trimmed: &str) -> bool {
    let mut chars = trimmed.chars().filter(|c| !c.is_whitespace());
    let Some(first) = chars.next() else {
        return false;
    };
    if !matches!(first, '-' | '*' | '_') {
        return false;
    }
    let mut count = 1;
    for c in chars {
        if c != first {
            return false;
        }
        count += 1;
    }
    count >= 3
}

fn strip_bullet(trimmed: &str) -> Option<&str> {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            return Some(rest);
        }
    }
    if matches!(trimmed, "-" | "*" | "+") {
        return Some("");
    }
    None
}

fn is_ordered(trimmed: &str) -> bool {
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 9 {
        return false;
    }
    let rest = &trimmed[digits..];
    rest.starts_with(". ") || rest.starts_with(") ") || rest == "." || rest == ")"
}

fn is_table_separator(trimmed: &str) -> bool {
    let inner = trimmed.trim_matches('|');
    !inner.is_empty()
        && inner.split('|').all(|cell| {
            let cell = cell.trim().trim_matches(':');
            !cell.is_empty() && cell.chars().all(|c| c == '-')
        })
}

/// One parsed line: its kind plus the states on both sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineParse {
    pub kind: LineKind,
    pub entry: BlockState,
    pub exit: BlockState,
}

/// The incremental parser: a per-line parse vector kept converged with the
/// buffer through [`IncrementalParser::splice`].
#[derive(Debug, Default)]
pub struct IncrementalParser {
    lines: Vec<LineParse>,
}

impl IncrementalParser {
    pub fn new() -> IncrementalParser {
        IncrementalParser { lines: Vec::new() }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, index: usize) -> Option<&LineParse> {
        self.lines.get(index)
    }

    /// Full (re)parse — initial load.
    pub fn parse_full<I, T>(&mut self, lines: I)
    where
        I: IntoIterator<Item = T>,
        T: AsRef<str>,
    {
        self.lines.clear();
        let mut state = BlockState::DocStart;
        for line in lines {
            let entry = state.clone();
            let (kind, exit) = classify(line.as_ref(), &entry);
            state = exit.clone();
            self.lines.push(LineParse { kind, entry, exit });
        }
    }

    /// Apply a buffer edit: lines `first..first+old_lines` were replaced by
    /// `first..first+new_lines`, whose current text `fetch` returns. After
    /// the splice, re-parsing continues forward until the stored entry
    /// state of the next line matches — the convergence rule that makes
    /// fence edits invalidate exactly the lines whose meaning changed.
    ///
    /// Returns the range of lines whose parse actually changed (callers
    /// restyle these; it can extend well past the edit).
    pub fn splice<F>(
        &mut self,
        first: usize,
        old_lines: usize,
        new_lines: usize,
        fetch: F,
    ) -> Range<usize>
    where
        F: Fn(usize) -> String,
    {
        let first = first.min(self.lines.len());
        let old_end = (first + old_lines).min(self.lines.len());
        let mut state = match first.checked_sub(1).and_then(|i| self.lines.get(i)) {
            Some(prev) => prev.exit.clone(),
            None => BlockState::DocStart,
        };
        let replacement: Vec<LineParse> = (first..first + new_lines)
            .map(|i| {
                let entry = state.clone();
                let (kind, exit) = classify(&fetch(i), &entry);
                state = exit.clone();
                LineParse { kind, entry, exit }
            })
            .collect();
        self.lines.splice(first..old_end, replacement);
        // Forward to convergence.
        let mut index = first + new_lines;
        while index < self.lines.len() {
            if self.lines[index].entry == state {
                break;
            }
            let entry = state.clone();
            let (kind, exit) = classify(&fetch(index), &entry);
            state = exit.clone();
            self.lines[index] = LineParse { kind, entry, exit };
            index += 1;
        }
        first..index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> IncrementalParser {
        let mut p = IncrementalParser::new();
        p.parse_full(text.split('\n'));
        p
    }

    fn kind(p: &IncrementalParser, i: usize) -> LineKind {
        match p.line(i) {
            Some(l) => l.kind.clone(),
            None => panic!("no line {i}"),
        }
    }

    #[test]
    fn basic_block_kinds() {
        let p = parse("# Title\n\n- [x] done\n1. step\n> quote\n---\n| a | b |\n|---|---|\npara");
        assert_eq!(kind(&p, 0), LineKind::Heading { level: 1 });
        assert_eq!(kind(&p, 1), LineKind::Blank);
        assert_eq!(
            kind(&p, 2),
            LineKind::Bullet {
                checkbox: Some(true)
            }
        );
        assert_eq!(kind(&p, 3), LineKind::Ordered);
        assert_eq!(kind(&p, 4), LineKind::Quote);
        assert_eq!(kind(&p, 5), LineKind::Rule);
        assert_eq!(kind(&p, 6), LineKind::TableRow);
        assert_eq!(kind(&p, 7), LineKind::TableSep);
        assert_eq!(kind(&p, 8), LineKind::Paragraph);
    }

    #[test]
    fn fence_state_carries_and_heading_inside_is_code() {
        let p = parse("```rust\n# not a heading\n```\n# heading");
        assert_eq!(
            kind(&p, 0),
            LineKind::FenceOpen {
                lang: "rust".into()
            }
        );
        assert_eq!(kind(&p, 1), LineKind::CodeContent);
        assert_eq!(kind(&p, 2), LineKind::FenceClose);
        assert_eq!(kind(&p, 3), LineKind::Heading { level: 1 });
    }

    #[test]
    fn unterminated_fence_runs_to_eof() {
        let p = parse("```\ncode\nmore");
        assert_eq!(kind(&p, 1), LineKind::CodeContent);
        assert_eq!(kind(&p, 2), LineKind::CodeContent);
    }

    #[test]
    fn shorter_close_run_does_not_close() {
        let p = parse("````\n```\n````\nafter");
        assert_eq!(kind(&p, 1), LineKind::CodeContent, "``` can't close ````");
        assert_eq!(kind(&p, 2), LineKind::FenceClose);
        assert_eq!(kind(&p, 3), LineKind::Paragraph);
    }

    #[test]
    fn math_block_and_single_line_math() {
        let p = parse("$$\nx = y\n$$\n$$e = mc^2$$\nafter");
        assert_eq!(kind(&p, 0), LineKind::MathOpen);
        assert_eq!(kind(&p, 1), LineKind::MathContentStart);
        assert_eq!(kind(&p, 2), LineKind::MathClose);
        assert_eq!(kind(&p, 3), LineKind::MathLine);
        assert_eq!(kind(&p, 4), LineKind::Paragraph);
    }

    #[test]
    fn front_matter_only_from_line_zero() {
        let p = parse("---\ntitle: x\n---\nbody\n---");
        assert_eq!(kind(&p, 0), LineKind::FrontMatterDelim);
        assert_eq!(kind(&p, 1), LineKind::FrontMatterContent);
        assert_eq!(kind(&p, 2), LineKind::FrontMatterDelim);
        assert_eq!(kind(&p, 3), LineKind::Paragraph);
        assert_eq!(kind(&p, 4), LineKind::Rule, "mid-document --- is a rule");
    }

    #[test]
    fn editing_above_front_matter_demotes_it() {
        let mut p = parse("---\ntitle: x\n---");
        // Insert a paragraph before the front matter: the old delimiters
        // are now mid-document and must reparse as rules.
        let lines = ["intro", "---", "title: x", "---"];
        let changed = p.splice(0, 0, 1, |i| lines[i].to_string());
        assert_eq!(changed, 0..4);
        assert_eq!(kind(&p, 1), LineKind::Rule);
        assert_eq!(kind(&p, 2), LineKind::Paragraph);
        assert_eq!(kind(&p, 3), LineKind::Rule);
    }

    #[test]
    fn typing_a_fence_cascades_and_converges() {
        let text = "a\nb\nc\nd";
        let mut p = parse(text);
        // Replace line 1 with an opening fence: lines 2.. become code.
        let lines = ["a", "```", "c", "d"];
        let changed = p.splice(1, 1, 1, |i| lines[i].to_string());
        assert_eq!(changed, 1..4, "cascade to EOF");
        assert_eq!(kind(&p, 2), LineKind::CodeContent);
        assert_eq!(kind(&p, 3), LineKind::CodeContent);
        // Now close it at line 2: line 3 must flip back, convergence at 4.
        let lines2 = ["a", "```", "```", "d"];
        let changed = p.splice(2, 1, 1, |i| lines2[i].to_string());
        assert_eq!(changed, 2..4);
        assert_eq!(kind(&p, 2), LineKind::FenceClose);
        assert_eq!(kind(&p, 3), LineKind::Paragraph);
    }

    fn entry_lex(p: &IncrementalParser, i: usize) -> LexState {
        let entry = match p.line(i) {
            Some(l) => &l.entry,
            None => panic!("no line {i}"),
        };
        match entry {
            BlockState::Fence { lex, .. } => *lex,
            other => panic!("line {i} entry is {other:?}, not a fence"),
        }
    }

    fn fence_lang(p: &IncrementalParser, i: usize) -> Lang {
        let entry = match p.line(i) {
            Some(l) => &l.entry,
            None => panic!("no line {i}"),
        };
        match entry {
            BlockState::Fence { lang, .. } => *lang,
            other => panic!("line {i} entry is {other:?}, not a fence"),
        }
    }

    #[test]
    fn fence_carries_resolved_language() {
        let p = parse("```rust\nlet x = 1;\n```\n```\nplain\n```");
        assert_eq!(fence_lang(&p, 1), Lang::Rust);
        assert_eq!(fence_lang(&p, 4), Lang::None);
    }

    /// ADR-0106 contract: editing inside a multi-line construct must keep
    /// invalidating following lines until the *lexer* state converges, reusing
    /// the same forward-convergence rule fences already use.
    #[test]
    fn block_comment_lexer_state_cascades_and_converges() {
        let base = ["```rust", "let a = 1;", "let b = 2;", "let c = 3;", "```"];
        let mut p = parse(&base.join("\n"));
        assert_eq!(entry_lex(&p, 2), LexState::Normal);

        // Open a block comment at the end of line 1 (no close on that line).
        // The cascade must run forward through the code lines until the fence
        // close, flipping each to "inside comment" lexer state.
        let opened = [
            "```rust",
            "let a = 1; /*",
            "let b = 2;",
            "let c = 3;",
            "```",
        ];
        let changed = p.splice(1, 1, 1, |i| opened[i].to_string());
        assert_eq!(
            entry_lex(&p, 2),
            LexState::BlockComment { depth: 1 },
            "open comment carries into following code lines"
        );
        assert_eq!(entry_lex(&p, 3), LexState::BlockComment { depth: 1 });
        assert!(
            changed.contains(&2) && changed.contains(&3),
            "cascade ({changed:?}) reaches past the edited line until close"
        );

        // Close the comment on line 2: line 3's lexer state must converge back
        // to Normal. The incrementally-maintained parse must match a full
        // reparse exactly — including every line's entry/exit lexer state.
        let closed = [
            "```rust",
            "let a = 1; /*",
            "*/ let b = 2;",
            "let c = 3;",
            "```",
        ];
        p.splice(2, 1, 1, |i| closed[i].to_string());
        assert_eq!(
            entry_lex(&p, 3),
            LexState::Normal,
            "closing the comment converges the following line"
        );
        let mut fresh = IncrementalParser::new();
        fresh.parse_full(closed.iter());
        assert_eq!(
            p.lines, fresh.lines,
            "incremental cascade matches full reparse"
        );
    }

    #[test]
    fn local_edit_converges_immediately() {
        let mut p = parse("a\nb\nc");
        let lines = ["a", "bX", "c"];
        let changed = p.splice(1, 1, 1, |i| lines[i].to_string());
        assert_eq!(changed, 1..2, "paragraph edit invalidates only itself");
    }

    #[test]
    fn inserting_and_removing_lines_keeps_states_converged() {
        let mut p = parse("```\ncode\n```");
        // Insert a line inside the fence.
        let lines = ["```", "new", "code", "```"];
        let changed = p.splice(1, 0, 1, |i| lines[i].to_string());
        assert_eq!(changed, 1..2);
        assert_eq!(kind(&p, 1), LineKind::CodeContent);
        // Delete the opening fence: everything below re-interprets.
        let lines2 = ["new", "code", "```"];
        let changed = p.splice(0, 1, 0, |i| lines2[i].to_string());
        assert_eq!(changed, 0..3);
        assert_eq!(kind(&p, 0), LineKind::Paragraph);
        assert_eq!(
            kind(&p, 2),
            LineKind::FenceOpen {
                lang: String::new()
            },
            "the old close fence now opens a new block"
        );
    }

    /// Differential: splice-maintained state must equal a from-scratch
    /// parse after every random edit (seeded, height-tree style).
    #[test]
    fn splice_agrees_with_full_reparse_under_random_edits() {
        let mut rng: u64 = 1234;
        let mut next = move || {
            rng ^= rng >> 12;
            rng ^= rng << 25;
            rng ^= rng >> 27;
            rng.wrapping_mul(0x2545_F491_4F6C_DD1D)
        };
        let atoms = [
            "para",
            "# h",
            "```",
            "```rust",
            "````",
            "$$",
            "x=y$$",
            "- item",
            "> q",
            "---",
            "",
            "| a |",
            "code",
            "let x = 1; /*",
            "*/ done",
            "fn f() {}",
        ];
        let mut doc: Vec<String> = vec!["start".into()];
        let mut p = IncrementalParser::new();
        p.parse_full(doc.iter());
        for _ in 0..2000 {
            let op = next() % 3;
            let at = (next() as usize) % doc.len();
            match op {
                0 => {
                    let text = atoms[(next() as usize) % atoms.len()].to_string();
                    doc.insert(at, text);
                    p.splice(at, 0, 1, |i| doc[i].clone());
                }
                1 if doc.len() > 1 => {
                    doc.remove(at);
                    p.splice(at, 1, 0, |i| doc[i].clone());
                }
                _ => {
                    doc[at] = atoms[(next() as usize) % atoms.len()].to_string();
                    p.splice(at, 1, 1, |i| doc[i].clone());
                }
            }
            let mut fresh = IncrementalParser::new();
            fresh.parse_full(doc.iter());
            assert_eq!(p.lines, fresh.lines, "diverged on doc {doc:?}");
        }
    }
}

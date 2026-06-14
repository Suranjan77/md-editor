//! Language-aware tokenization for fenced code (ADR-0106).
//!
//! Tokens carry **semantic roles**, never theme colors — the shell maps
//! roles to colors while building the draw plan. Lexing is line-by-line and
//! **stateful**: [`LexState`] is carried across line boundaries by the block
//! parser ([`crate::parse`]) so multi-line constructs (block comments)
//! converge through the same forward-convergence rule that fences use.
//!
//! Highlighting is paint-only by contract: token boundaries never change
//! font, shaping, wrapping, line height, or caret/selection geometry. The
//! measurer shapes every code token with the identical monospace attrs it
//! uses for plain code content, so geometry is invariant by construction.

/// Languages we syntax-highlight. Anything else (including an empty fence
/// tag) is [`Lang::None`] and falls back to the single plain-code color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Lang {
    #[default]
    None,
    Rust,
}

impl Lang {
    /// Resolve a fence info-string tag: the `rust` in an opening ` ```rust `.
    pub fn from_tag(tag: &str) -> Lang {
        match tag.trim().to_ascii_lowercase().as_str() {
            "rust" | "rs" => Lang::Rust,
            _ => Lang::None,
        }
    }
}

/// Cross-line lexer state. Derives `Eq`/`Hash` so it participates in the
/// parser's convergence rule, which is defined over [`crate::parse::BlockState`]
/// equality: opening a block comment changes a code line's *exit* state, so
/// following lines reparse until the matching `*/` restores [`LexState::Normal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LexState {
    #[default]
    Normal,
    /// Inside a `/* … */` block comment. Rust block comments nest, so we
    /// carry the open depth rather than a bool.
    BlockComment { depth: usize },
}

/// Semantic role of one run of code characters (ADR-0106 initial set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxRole {
    Comment,
    Keyword,
    String,
    Number,
    Type,
    Function,
    Operator,
    Punctuation,
}

const OPERATOR_CHARS: &str = "+-*/%=<>!&|^~";
const PUNCTUATION_CHARS: &str = "(){}[],;:.?@#";

/// Tokenize one `lang` code line starting from the `entry` lexer state.
///
/// `emit(start, end, role)` is called for each *highlighted* run, in order,
/// with **char** offsets into `text` (the styler's span offset space). Gaps
/// between emitted runs are plain code — the caller tiles them with the base
/// code role. Returns the **exit** lexer state, which is the only thing the
/// parser needs for convergence (it discards the tokens via a no-op `emit`).
pub fn lex_line(
    lang: Lang,
    entry: LexState,
    text: &str,
    mut emit: impl FnMut(usize, usize, SyntaxRole),
) -> LexState {
    if lang == Lang::None {
        // Unhighlighted fence: no tokens, and no cross-line state to carry.
        return LexState::Normal;
    }
    // Currently Rust is the only highlighted grammar; others resolve to None.
    lex_rust(entry, text, &mut emit)
}

fn lex_rust(
    entry: LexState,
    text: &str,
    emit: &mut impl FnMut(usize, usize, SyntaxRole),
) -> LexState {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = 0;

    // Resume an open block comment from a previous line.
    if let LexState::BlockComment { mut depth } = entry {
        let start = i;
        i = scan_block_comment(&chars, n, i, &mut depth);
        emit(start, i, SyntaxRole::Comment);
        if depth > 0 {
            return LexState::BlockComment { depth };
        }
    }

    while i < n {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // Line comment: `//` to end of line.
        if c == '/' && i + 1 < n && chars[i + 1] == '/' {
            emit(i, n, SyntaxRole::Comment);
            return LexState::Normal;
        }
        // Block comment open (nesting).
        if c == '/' && i + 1 < n && chars[i + 1] == '*' {
            let start = i;
            let mut depth = 1;
            i += 2;
            i = scan_block_comment(&chars, n, i, &mut depth);
            emit(start, i, SyntaxRole::Comment);
            if depth > 0 {
                return LexState::BlockComment { depth };
            }
            continue;
        }
        // String literal (line-local; an unterminated string ends at EOL).
        if c == '"' {
            let start = i;
            i += 1;
            while i < n {
                if chars[i] == '\\' && i + 1 < n {
                    i += 2;
                    continue;
                }
                let q = chars[i] == '"';
                i += 1;
                if q {
                    break;
                }
            }
            emit(start, i, SyntaxRole::String);
            continue;
        }
        // Number literal.
        if c.is_ascii_digit() {
            let start = i;
            while i < n && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_')
            {
                i += 1;
            }
            emit(start, i, SyntaxRole::Number);
            continue;
        }
        // Identifier → keyword / type / function call / plain.
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if is_keyword(&word) {
                emit(start, i, SyntaxRole::Keyword);
            } else if word.starts_with(|c: char| c.is_uppercase()) {
                emit(start, i, SyntaxRole::Type);
            } else if next_nonspace(&chars, n, i) == Some('(') {
                emit(start, i, SyntaxRole::Function);
            }
            // else: plain identifier — left as a gap (base code color).
            continue;
        }
        // Operator run.
        if OPERATOR_CHARS.contains(c) {
            let start = i;
            while i < n && OPERATOR_CHARS.contains(chars[i]) {
                i += 1;
            }
            emit(start, i, SyntaxRole::Operator);
            continue;
        }
        // Punctuation run.
        if PUNCTUATION_CHARS.contains(c) {
            let start = i;
            while i < n && PUNCTUATION_CHARS.contains(chars[i]) {
                i += 1;
            }
            emit(start, i, SyntaxRole::Punctuation);
            continue;
        }
        // Anything else (e.g. a lone `'` lifetime tick): plain.
        i += 1;
    }
    LexState::Normal
}

/// Advance `i` through block-comment body from index `i`, tracking nesting in
/// `depth`. Stops past the `*/` that returns depth to 0, or at EOL.
fn scan_block_comment(chars: &[char], n: usize, mut i: usize, depth: &mut usize) -> usize {
    while i < n {
        if i + 1 < n && chars[i] == '/' && chars[i + 1] == '*' {
            *depth += 1;
            i += 2;
        } else if i + 1 < n && chars[i] == '*' && chars[i + 1] == '/' {
            *depth -= 1;
            i += 2;
            if *depth == 0 {
                break;
            }
        } else {
            i += 1;
        }
    }
    i
}

fn next_nonspace(chars: &[char], n: usize, mut i: usize) -> Option<char> {
    while i < n && chars[i].is_whitespace() {
        i += 1;
    }
    chars.get(i).copied()
}

fn is_keyword(word: &str) -> bool {
    matches!(
        word,
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "union"
            | "unsafe"
            | "use"
            | "where"
            | "while"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(lang: Lang, entry: LexState, text: &str) -> (Vec<(String, SyntaxRole)>, LexState) {
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::new();
        let exit = lex_line(lang, entry, text, |s, e, r| {
            out.push((chars[s..e].iter().collect(), r))
        });
        (out, exit)
    }

    #[test]
    fn tag_resolution() {
        assert_eq!(Lang::from_tag("rust"), Lang::Rust);
        assert_eq!(Lang::from_tag("  RS "), Lang::Rust);
        assert_eq!(Lang::from_tag(""), Lang::None);
        assert_eq!(Lang::from_tag("python"), Lang::None);
    }

    #[test]
    fn unknown_language_emits_nothing() {
        let (out, exit) = toks(Lang::None, LexState::Normal, "fn main() {}");
        assert!(out.is_empty());
        assert_eq!(exit, LexState::Normal);
    }

    #[test]
    fn rust_basic_roles() {
        let (out, exit) = toks(Lang::Rust, LexState::Normal, "let x = 42; // note");
        assert_eq!(exit, LexState::Normal);
        assert!(out.contains(&("let".into(), SyntaxRole::Keyword)));
        assert!(out.contains(&("42".into(), SyntaxRole::Number)));
        assert!(out.contains(&("=".into(), SyntaxRole::Operator)));
        assert!(out.contains(&(";".into(), SyntaxRole::Punctuation)));
        assert!(out.contains(&("// note".into(), SyntaxRole::Comment)));
    }

    #[test]
    fn string_and_function_and_type() {
        let (out, _) = toks(Lang::Rust, LexState::Normal, r#"Foo::bar("hi")"#);
        assert!(out.contains(&("Foo".into(), SyntaxRole::Type)));
        assert!(out.contains(&("bar".into(), SyntaxRole::Function)));
        assert!(out.contains(&(r#""hi""#.into(), SyntaxRole::String)));
    }

    #[test]
    fn block_comment_carries_across_lines_and_converges() {
        // Open on line 1, body on line 2, close on line 3.
        let (o1, s1) = toks(Lang::Rust, LexState::Normal, "a /* open");
        assert_eq!(s1, LexState::BlockComment { depth: 1 });
        assert!(o1.contains(&("/* open".into(), SyntaxRole::Comment)));

        let (o2, s2) = toks(Lang::Rust, s1, "still comment");
        assert_eq!(s2, LexState::BlockComment { depth: 1 });
        assert_eq!(o2, vec![("still comment".into(), SyntaxRole::Comment)]);

        let (o3, s3) = toks(Lang::Rust, s2, "end */ let y");
        assert_eq!(s3, LexState::Normal, "lexer state converges after close");
        assert!(o3.contains(&("end */".into(), SyntaxRole::Comment)));
        assert!(o3.contains(&("let".into(), SyntaxRole::Keyword)));
    }

    #[test]
    fn nested_block_comments() {
        let (_, s1) = toks(Lang::Rust, LexState::Normal, "/* outer /* inner");
        assert_eq!(s1, LexState::BlockComment { depth: 2 });
        let (_, s2) = toks(Lang::Rust, s1, "*/ still open");
        assert_eq!(s2, LexState::BlockComment { depth: 1 });
        let (_, s3) = toks(Lang::Rust, s2, "*/ done");
        assert_eq!(s3, LexState::Normal);
    }

    /// Emitted runs must be ordered and non-overlapping so the styler can
    /// tile gaps deterministically.
    #[test]
    fn emitted_runs_are_ordered_and_disjoint() {
        let chars: Vec<char> = "fn f(x: u32) -> u32 { x + 1 }".chars().collect();
        let mut last_end = 0;
        lex_line(
            Lang::Rust,
            LexState::Normal,
            &"fn f(x: u32) -> u32 { x + 1 }".chars().collect::<String>(),
            |s, e, _| {
                assert!(
                    s >= last_end,
                    "overlap or out-of-order at {s} (last {last_end})"
                );
                assert!(e <= chars.len());
                assert!(e > s);
                last_end = e;
            },
        );
    }
}

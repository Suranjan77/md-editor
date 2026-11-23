//! Fenced-code syntax highlighting with syntect.

use super::*;

/// Syntax-highlight one line of a code block, advancing its state.
pub(super) fn highlight_code_spans(line: &str, code: &mut CodeState) -> Vec<StyledSpan> {
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
pub(super) fn start_code_syntax(lang: Option<&str>) -> Option<(ParseState, HighlightState)> {
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
pub(super) fn code_highlighter() -> Option<&'static Highlighter<'static>> {
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

pub(super) fn code_span(text: &str, color: Color) -> StyledSpan {
    StyledSpan {
        text: text.to_string(),
        display_text: None,
        color,
        font_size: 14.0,
        is_code: true,
        ..StyledSpan::plain("")
    }
}

pub(super) fn syntect_defaults()
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

use super::model::StyledSpan;
use crate::theme;
use iced::Color;
use std::sync::OnceLock;

pub fn highlight_code_spans(
    line: &str,
    highlighter: &mut Option<syntect::easy::HighlightLines<'static>>,
    lang: Option<&str>,
) -> Vec<StyledSpan> {
    let Some((syntax_set, _)) = syntect_defaults() else {
        return vec![code_span(line, theme::text_primary())];
    };

    if highlighter.is_none() {
        *highlighter = make_code_highlighter(lang);
    }

    let Some(highlighter) = highlighter.as_mut() else {
        return vec![code_span(line, theme::text_primary())];
    };

    match highlighter.highlight_line(line, syntax_set) {
        Ok(regions) => {
            let spans = regions
                .into_iter()
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
                vec![code_span(line, theme::text_primary())]
            } else {
                spans
            }
        }
        Err(_) => vec![code_span(line, theme::text_primary())],
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

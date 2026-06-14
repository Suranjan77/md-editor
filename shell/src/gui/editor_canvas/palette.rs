//! Editor theme colors. This is the one place that maps semantic paint and
//! syntax roles (which cross the toolkit-agnostic editor→shell boundary as
//! roles, never colors — ADR-0100/0106) to concrete theme [`Color`]s, so a
//! theme change never re-tokenizes or re-measures anything.

use crate::gui::tokens::Tokens;
use iced::Color;

pub fn bg(tokens: &Tokens) -> Color {
    tokens.bg_secondary
}
pub fn text(tokens: &Tokens) -> Color {
    tokens.text_primary
}
pub fn marker(tokens: &Tokens) -> Color {
    tokens.text_muted
}
pub fn heading(tokens: &Tokens) -> Color {
    tokens.text_heading
}
pub fn code(tokens: &Tokens) -> Color {
    tokens.success
}
pub fn math(tokens: &Tokens) -> Color {
    tokens.warning
}
pub fn link(tokens: &Tokens) -> Color {
    tokens.accent
}
pub fn wikilink(tokens: &Tokens) -> Color {
    tokens.accent_secondary
}
pub fn quote(tokens: &Tokens) -> Color {
    tokens.accent
}
pub fn caret(tokens: &Tokens) -> Color {
    tokens.accent
}
pub fn selection(tokens: &Tokens) -> Color {
    tokens.sel_tint
}
pub fn code_bg(tokens: &Tokens) -> Color {
    let mut color = tokens.bg_tertiary;
    color.a = 0.72;
    color
}
/// Map a semantic syntax role (ADR-0106) to a theme color. Roles, not
/// colors, cross the editor→shell boundary; this is the only place that
/// decides how a token looks.
pub fn syntax(tokens: &Tokens, role: md_editor::syntax::SyntaxRole) -> Color {
    use md_editor::syntax::SyntaxRole::*;
    match role {
        Comment => tokens.text_muted,
        Keyword => tokens.danger,
        String => tokens.success,
        Number => tokens.warning,
        Type => tokens.accent_secondary,
        Function => tokens.accent,
        Operator | Punctuation => tokens.text_secondary,
    }
}

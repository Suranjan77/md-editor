//! Editor theme colors. This is the one place that maps semantic paint and
//! syntax roles (which cross the toolkit-agnostic editor→shell boundary as
//! roles, never colors — ADR-0100/0106) to concrete theme [`Color`]s, so a
//! theme change never re-tokenizes or re-measures anything.

use crate::gui::tokens;
use iced::Color;

pub fn bg() -> Color {
    tokens::dark().bg_secondary
}
pub fn text() -> Color {
    tokens::dark().text_primary
}
pub fn marker() -> Color {
    tokens::dark().text_muted
}
pub fn heading() -> Color {
    tokens::dark().danger
}
pub fn code() -> Color {
    tokens::dark().success
}
pub fn math() -> Color {
    tokens::dark().warning
}
pub fn link() -> Color {
    tokens::dark().accent
}
pub fn wikilink() -> Color {
    tokens::dark().accent_secondary
}
pub fn quote() -> Color {
    tokens::dark().accent
}
pub fn caret() -> Color {
    tokens::dark().accent
}
pub fn selection() -> Color {
    tokens::dark().sel_tint
}
pub fn code_bg() -> Color {
    let mut color = tokens::dark().bg_tertiary;
    color.a = 0.72;
    color
}
/// Map a semantic syntax role (ADR-0106) to a theme color. Roles, not
/// colors, cross the editor→shell boundary; this is the only place that
/// decides how a token looks.
pub fn syntax(role: md3_editor::syntax::SyntaxRole) -> Color {
    use md3_editor::syntax::SyntaxRole::*;
    let t = tokens::dark();
    match role {
        Comment => t.text_muted,
        Keyword => t.danger,
        String => t.success,
        Number => t.warning,
        Type => t.accent_secondary,
        Function => t.accent,
        Operator | Punctuation => t.text_secondary,
    }
}

//! Semantic color palette. Views name *roles* (`surface`, `text_muted`,
//! `syntax_marker`), never hex values. Backed by the theme resolvers in
//! `theme.rs` (which stays the single owner of raw color literals alongside
//! this module) and theme-aware at call time.

#![allow(dead_code)] // consumed incrementally as views migrate (UXA.T2a–g)

use crate::theme as legacy;
use iced::Color;

// Direct semantic re-exports of the existing theme-aware resolvers.
#[allow(unused_imports)] // consumed as views migrate (UXA.T2a-g)
pub(crate) use legacy::{
    accent, accent_dim, accent_glow, accent_secondary, border, border_subtle, danger, success,
    text_muted, text_secondary, warning,
};

/// Base app background.
pub(crate) fn surface() -> Color {
    legacy::bg_primary()
}

/// Raised panels: sidebars, cards, popovers.
pub(crate) fn surface_raised() -> Color {
    legacy::bg_secondary()
}

/// Inset/recessed regions: inputs, code blocks, wells.
pub(crate) fn surface_sunken() -> Color {
    legacy::bg_tertiary()
}

/// Highlighted surface (selected rows, active states).
pub(crate) fn surface_selected() -> Color {
    legacy::bg_surface()
}

/// Primary body text.
pub(crate) fn text() -> Color {
    legacy::text_primary()
}

/// Editor text selection background.
pub(crate) fn selection() -> Color {
    legacy::accent_dim()
}

/// Editor caret.
pub(crate) fn caret() -> Color {
    legacy::accent()
}

/// Concealed/revealed Markdown syntax markers on the active line (UXC.T1).
pub(crate) fn syntax_marker() -> Color {
    legacy::text_muted()
}

/// Code block / inline code background.
pub(crate) fn code_bg() -> Color {
    legacy::bg_tertiary()
}

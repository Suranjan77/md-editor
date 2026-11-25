//! Primitive colour ramps, DARK/LIGHT semantic role tables, document roles and the content palette (MASTER §2).
#![allow(dead_code)] // Full token tables (MASTER §2); content hues and ink paper colours are adopted view by view.

use iced::Color;

const fn hex(rgb: u32) -> Color {
    Color::from_rgb8(
        ((rgb >> 16) & 0xFF) as u8,
        ((rgb >> 8) & 0xFF) as u8,
        (rgb & 0xFF) as u8,
    )
}

const fn hexa(rgb: u32, alpha: f32) -> Color {
    Color {
        a: alpha,
        ..hex(rgb)
    }
}

/// Semantic colour roles for a single theme (MASTER §2.1–2.2 and `pages/editor.md`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    pub is_dark: bool,
    // Surfaces
    pub bg_canvas: Color,
    pub bg_surface: Color,
    pub bg_raised: Color,
    pub bg_overlay: Color,
    pub bg_scrim: Color,
    // Borders
    pub border_subtle: Color,
    pub border_default: Color,
    pub border_control: Color,
    // Foregrounds
    pub fg_primary: Color,
    pub fg_secondary: Color,
    pub fg_tertiary: Color,
    // Accent & focus
    pub accent: Color,
    pub on_accent: Color,
    pub accent_muted: Color,
    pub focus_ring: Color,
    pub highlight: Color,
    // Status
    pub status_danger: Color,
    pub status_success: Color,
    pub status_warning: Color,
    pub status_info: Color,
    // Document content roles (`pages/editor.md`)
    pub doc_text: Color,
    pub doc_syntax: Color,
    pub doc_link: Color,
    pub doc_code_fg: Color,
    pub doc_code_bg: Color,
    pub doc_math: Color,
    pub doc_math_source: Color,
    pub doc_quote_bar: Color,
    pub doc_selection: Color,
    pub doc_find: Color,
    pub doc_find_active: Color,
    pub doc_caret: Color,
    pub doc_block_bg: Color,
    // Viewer canvas (`pages/viewers.md`)
    pub viewer_ground: Color,
}

pub const DARK: Tokens = Tokens {
    is_dark: true,
    bg_canvas: hex(0x0B1120),
    bg_surface: hex(0x111827),
    bg_raised: hex(0x1B2336),
    bg_overlay: hex(0x272F42),
    bg_scrim: hexa(0x020617, 0.60),
    border_subtle: hex(0x1E293B),
    border_default: hex(0x334155),
    border_control: hex(0x64748B),
    fg_primary: hex(0xF8FAFC),
    fg_secondary: hex(0xCBD5E1),
    fg_tertiary: hex(0x94A3B8),
    accent: hex(0x2DD4BF),
    on_accent: hex(0x042F2E),
    accent_muted: hexa(0x2DD4BF, 0.16),
    focus_ring: hex(0x5EEAD4),
    highlight: hex(0xF59E0B),
    status_danger: hex(0xF87171),
    status_success: hex(0x4ADE80),
    status_warning: hex(0xFBBF24),
    status_info: hex(0x60A5FA),
    doc_text: hex(0xF8FAFC),
    doc_syntax: hex(0x94A3B8),
    doc_link: hex(0x2DD4BF),
    doc_code_fg: hex(0xCBD5E1),
    doc_code_bg: hex(0x1B2336),
    doc_math: hex(0xF8FAFC),
    doc_math_source: hex(0xCBD5E1),
    doc_quote_bar: hex(0x64748B),
    doc_selection: hexa(0x2DD4BF, 0.28),
    doc_find: hexa(0xF59E0B, 0.30),
    doc_find_active: hexa(0xF59E0B, 0.55),
    doc_caret: hex(0x2DD4BF),
    doc_block_bg: hex(0x111827),
    viewer_ground: hex(0x0B1120),
};

pub const LIGHT: Tokens = Tokens {
    is_dark: false,
    bg_canvas: hex(0xF8FAFC),
    bg_surface: hex(0xFFFFFF),
    bg_raised: hex(0xF1F5F9),
    bg_overlay: hex(0xE2E8F0),
    bg_scrim: hexa(0x0F172A, 0.45),
    border_subtle: hex(0xE2E8F0),
    border_default: hex(0xCBD5E1),
    border_control: hex(0x64748B),
    fg_primary: hex(0x0F172A),
    fg_secondary: hex(0x334155),
    fg_tertiary: hex(0x475569),
    accent: hex(0x0F766E),
    on_accent: hex(0xFFFFFF),
    accent_muted: hexa(0x0F766E, 0.12),
    focus_ring: hex(0x0D9488),
    highlight: hex(0xB45309),
    status_danger: hex(0xB91C1C),
    status_success: hex(0x15803D),
    status_warning: hex(0xA16207),
    status_info: hex(0x1D4ED8),
    doc_text: hex(0x0F172A),
    doc_syntax: hex(0x475569),
    doc_link: hex(0x0F766E),
    doc_code_fg: hex(0x334155),
    doc_code_bg: hex(0xF1F5F9),
    doc_math: hex(0x0F172A),
    doc_math_source: hex(0x334155),
    doc_quote_bar: hex(0x64748B),
    doc_selection: hexa(0x0F766E, 0.20),
    doc_find: hexa(0xF59E0B, 0.30),
    doc_find_active: hexa(0xF59E0B, 0.55),
    doc_caret: hex(0x0F766E),
    doc_block_bg: hex(0xF1F5F9),
    viewer_ground: hex(0xE2E8F0),
};

/// User-chosen content hues (MASTER §2.4) — stable across UI themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentHue {
    Yellow,
    Green,
    Blue,
    Pink,
    Orange,
}

impl ContentHue {
    pub const ALL: [Self; 5] = [
        Self::Yellow,
        Self::Green,
        Self::Blue,
        Self::Pink,
        Self::Orange,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Pink => "Pink",
            Self::Orange => "Orange",
        }
    }

    pub const fn color(self, on_dark: bool) -> Color {
        match (self, on_dark) {
            (Self::Yellow, true) => hex(0xFACC15),
            (Self::Green, true) => hex(0x4ADE80),
            (Self::Blue, true) => hex(0x60A5FA),
            (Self::Pink, true) => hex(0xF472B6),
            (Self::Orange, true) => hex(0xFB923C),
            (Self::Yellow, false) => hex(0xCA8A04),
            (Self::Green, false) => hex(0x16A34A),
            (Self::Blue, false) => hex(0x2563EB),
            (Self::Pink, false) => hex(0xDB2777),
            (Self::Orange, false) => hex(0xEA580C),
        }
    }

    /// 35% alpha highlight tint over a white PDF page (MASTER §2.4).
    pub const fn pdf_highlight(self) -> Color {
        let base = self.color(true);
        Color { a: 0.35, ..base }
    }
}

// ── Handwriting paper palette (`pages/ink.md`) ───────────────────────

pub const INK_LIGHT_PAPER: Color = hex(0xFAFAF7);
pub const INK_LIGHT_RULING: Color = hex(0xD6D3CB);
pub const INK_DARK_PAPER: Color = hex(0x16181D);
pub const INK_DARK_RULING: Color = hex(0x2B2F38);
pub const INK_ON_LIGHT: Color = hex(0x18181B);
pub const INK_ON_DARK: Color = hex(0xF8FAFC);

/// Scale a colour's alpha by `opacity` (clamped to 0..=1).
pub fn fade(color: Color, opacity: f32) -> Color {
    Color {
        a: color.a * opacity.clamp(0.0, 1.0),
        ..color
    }
}

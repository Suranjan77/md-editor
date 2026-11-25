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
    /// Wells and cards nested inside the document sheet (chart areas, list cards, graph canvas).
    pub bg_inset: Color,
    /// Floating surfaces: menus, popovers, tooltips, dialogs, the palette and toasts.
    pub bg_popover: Color,
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

/// "Folio" dark: warm graphite chrome around a slightly lifted document sheet, periwinkle ink accent.
pub const DARK: Tokens = Tokens {
    is_dark: true,
    bg_canvas: hex(0x1E1D1B),
    bg_surface: hex(0x111010),
    bg_raised: hex(0x262421),
    bg_overlay: hex(0x322F2B),
    bg_inset: hex(0x23221F),
    bg_popover: hex(0x282623),
    bg_scrim: hexa(0x0A0908, 0.62),
    border_subtle: hex(0x2A2825),
    border_default: hex(0x3B3834),
    border_control: hex(0x7C766D),
    fg_primary: hex(0xEDEAE4),
    fg_secondary: hex(0xCAC5BC),
    fg_tertiary: hex(0xA09A90),
    accent: hex(0x8FA1FF),
    on_accent: hex(0x0E1436),
    accent_muted: hexa(0x8FA1FF, 0.16),
    focus_ring: hex(0xADBAFF),
    highlight: hex(0xF2A93B),
    status_danger: hex(0xF27A6E),
    status_success: hex(0x74C98E),
    status_warning: hex(0xE9B44F),
    status_info: hex(0x82B4F2),
    doc_text: hex(0xEDEAE4),
    doc_syntax: hex(0xA09A90),
    doc_link: hex(0x8FA1FF),
    doc_code_fg: hex(0xD6D1C8),
    doc_code_bg: hex(0x23211F),
    doc_math: hex(0xEDEAE4),
    doc_math_source: hex(0xCAC5BC),
    doc_quote_bar: hex(0x7C766D),
    doc_selection: hexa(0x8FA1FF, 0.28),
    doc_find: hexa(0xF2A93B, 0.30),
    doc_find_active: hexa(0xF2A93B, 0.55),
    doc_caret: hex(0x8FA1FF),
    doc_block_bg: hex(0x23211F),
    viewer_ground: hex(0x1E1D1B),
};

/// "Folio" light: warm stone chrome around a paper-white sheet, deep ink-blue accent.
pub const LIGHT: Tokens = Tokens {
    is_dark: false,
    bg_canvas: hex(0xFFFEFB),
    bg_surface: hex(0xF3F1EC),
    bg_raised: hex(0xEAE7E0),
    bg_overlay: hex(0xDFDBD2),
    bg_inset: hex(0xF7F5F0),
    bg_popover: hex(0xFFFFFF),
    bg_scrim: hexa(0x1C1A17, 0.40),
    border_subtle: hex(0xE4E0D8),
    border_default: hex(0xD3CEC4),
    border_control: hex(0x7A7369),
    fg_primary: hex(0x1C1A17),
    fg_secondary: hex(0x433E38),
    fg_tertiary: hex(0x625B52),
    accent: hex(0x3346C4),
    on_accent: hex(0xFFFFFF),
    accent_muted: hexa(0x3346C4, 0.11),
    focus_ring: hex(0x3346C4),
    highlight: hex(0x924A06),
    status_danger: hex(0xA8231B),
    status_success: hex(0x256B30),
    status_warning: hex(0x845006),
    status_info: hex(0x1F4DB0),
    doc_text: hex(0x1C1A17),
    doc_syntax: hex(0x625B52),
    doc_link: hex(0x3346C4),
    doc_code_fg: hex(0x3A352F),
    doc_code_bg: hex(0xF4F1EB),
    doc_math: hex(0x1C1A17),
    doc_math_source: hex(0x433E38),
    doc_quote_bar: hex(0x7A7369),
    doc_selection: hexa(0x3346C4, 0.18),
    doc_find: hexa(0xF59E0B, 0.30),
    doc_find_active: hexa(0xF59E0B, 0.55),
    doc_caret: hex(0x3346C4),
    doc_block_bg: hex(0xF4F1EB),
    viewer_ground: hex(0xFFFEFB),
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

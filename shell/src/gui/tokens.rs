use iced::Color;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub bg_primary: Color,
    pub bg_secondary: Color,
    pub bg_tertiary: Color,
    pub bg_surface: Color,
    pub bg_chrome: Color,
    pub bg_rail: Color,
    pub surface_palette: Color,
    pub border: Color,
    pub border_subtle: Color,
    pub border_overlay: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub text_heading: Color,
    pub accent: Color,
    pub accent_secondary: Color,
    pub wikilink: Color,
    pub danger: Color,
    pub success: Color,
    pub warning: Color,
    pub sel_tint: Color,
    pub highlight_default: Color,
    pub bg_hover: Color,
    pub bg_pressed: Color,
    pub focus_ring: Color,
    pub code_inline_text: Color,
    pub syn_base: Color,
    pub syn_keyword: Color,
    pub syn_function: Color,
    pub syn_string: Color,
    pub syn_type: Color,
    pub syn_param: Color,
    pub syn_comment: Color,
}

const fn hex_color(hex: u32) -> Color {
    Color::from_rgb(
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
    )
}

const fn hex_color_alpha(hex: u32, alpha: f32) -> Color {
    Color::from_rgba(
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
        alpha,
    )
}

// Quiet Vault — dark-only. Neutral near-black grays; one purple accent
// (`#bd93f9`) used sparingly; a single teal secondary (`#67c6b0`) reserved for
// wikilinks. Headings read by weight/size, never color. See docs/DESIGN-SYSTEM.md
// §1 for the full token table; this is the source of truth in code.
pub static DARK_TOKENS: LazyLock<Tokens> = LazyLock::new(|| Tokens {
    bg_primary: hex_color(0x0e0e12),   // bg/canvas
    bg_secondary: hex_color(0x16161c), // surface/2
    bg_tertiary: hex_color(0x1a1a21),  // surface/3 (active tab)
    bg_surface: hex_color(0x101014),   // surface/1 (cards)
    bg_chrome: hex_color(0x0c0c10),    // top bar, status bar
    bg_rail: hex_color(0x0a0a0d),      // left & right rails, code-block bg
    surface_palette: hex_color(0x141419), // command palette / overlay panels
    border: hex_color_alpha(0xffffff, 0.07),
    border_subtle: hex_color_alpha(0xffffff, 0.05),
    border_overlay: hex_color_alpha(0xffffff, 0.10),
    text_primary: hex_color(0xe8e8ec),
    text_secondary: hex_color(0xa8a8b0),
    text_muted: hex_color(0x6b6b73),
    text_heading: hex_color(0xf2f2f5),
    accent: hex_color(0xbd93f9),           // purple — links, focus, caret, dirty dot
    accent_secondary: hex_color(0xd6bbfb), // muted lilac (minor labels)
    wikilink: hex_color(0x67c6b0),         // teal — [[wikilinks]] only
    danger: hex_color(0xe0735f),
    success: hex_color(0x6c8c7c),
    warning: hex_color(0xe0b04a),
    sel_tint: hex_color_alpha(0xbd93f9, 0.28),
    highlight_default: hex_color(0xffd866),
    bg_hover: hex_color(0x1a1a21),
    bg_pressed: hex_color(0x1f1f27),
    focus_ring: hex_color(0xbd93f9),
    code_inline_text: hex_color(0xd9c7f5),
    syn_base: hex_color(0xc4c4cc),
    syn_keyword: hex_color(0xbd93f9),
    syn_function: hex_color(0xe0b04a),
    syn_string: hex_color(0x8fbf7f),
    syn_type: hex_color(0x67c6b0),
    syn_param: hex_color(0xcf8f6a),
    syn_comment: hex_color(0x5a5a62),
});

// Quiet Vault is dark only — there is no light token set or theme switcher.
pub fn dark() -> &'static Tokens {
    &DARK_TOKENS
}

impl super::Shell {
    pub(crate) fn tokens(&self) -> &'static Tokens {
        dark()
    }

    pub(crate) fn theme(&self) -> iced::Theme {
        let tokens = self.tokens();
        iced::Theme::custom_with_fn(
            "MD Editor".to_string(),
            iced::theme::Palette {
                background: tokens.bg_primary,
                text: tokens.text_primary,
                primary: tokens.accent,
                success: tokens.success,
                danger: tokens.danger,
                warning: tokens.warning,
            },
            iced::theme::palette::Extended::generate,
        )
    }
}

use iced::Color;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub bg_primary: Color,
    pub bg_secondary: Color,
    pub bg_tertiary: Color,
    pub bg_surface: Color,
    pub border: Color,
    pub border_subtle: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub accent_secondary: Color,
    pub danger: Color,
    pub success: Color,
    pub warning: Color,
    pub sel_tint: Color,
    pub highlight_default: Color,
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

pub static DARK_TOKENS: LazyLock<Tokens> = LazyLock::new(|| Tokens {
    bg_primary: hex_color(0x0d0e10),
    bg_secondary: hex_color(0x181a1d),
    bg_tertiary: hex_color(0x23262b),
    bg_surface: hex_color(0x334b47),
    border: hex_color(0x45484e),
    border_subtle: hex_color(0x1d2024),
    text_primary: hex_color(0xe3e5ed),
    text_secondary: hex_color(0xa9abb2),
    text_muted: hex_color(0x9d9ea3),
    accent: hex_color(0xb1ccc6),
    accent_secondary: hex_color(0xcde8e2),
    danger: hex_color(0xee7d77),
    success: hex_color(0xd9f2d2),
    warning: hex_color(0xbfdad4),
    sel_tint: hex_color_alpha(0xb1ccc6, 0.30),
    highlight_default: hex_color(0xffd866),
});

pub fn dark() -> &'static Tokens {
    &DARK_TOKENS
}

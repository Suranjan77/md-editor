use iced::Color;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

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
    pub bg_hover: Color,
    pub bg_pressed: Color,
    pub focus_ring: Color,
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
    bg_hover: hex_color(0x2d3139),
    bg_pressed: hex_color(0x3e444f),
    focus_ring: hex_color(0xb1ccc6),
});

pub static LIGHT_TOKENS: LazyLock<Tokens> = LazyLock::new(|| Tokens {
    bg_primary: hex_color(0xf5f6f8),
    bg_secondary: hex_color(0xeaecef),
    bg_tertiary: hex_color(0xdbe0e5),
    bg_surface: hex_color(0xdde8e5),
    border: hex_color(0xb0b5be),
    border_subtle: hex_color(0xd2d6dc),
    text_primary: hex_color(0x1a1c1e),
    text_secondary: hex_color(0x4a4d53),
    text_muted: hex_color(0x70757f),
    accent: hex_color(0x3f6a62),
    accent_secondary: hex_color(0x56827a),
    danger: hex_color(0xd32f2f),
    success: hex_color(0x2e7d32),
    warning: hex_color(0xed6c02),
    sel_tint: hex_color_alpha(0x3f6a62, 0.20),
    highlight_default: hex_color(0xffeb3b),
    bg_hover: hex_color(0xe0e4e8),
    bg_pressed: hex_color(0xd0d6de),
    focus_ring: hex_color(0x3f6a62),
});

static USE_LIGHT_THEME: AtomicBool = AtomicBool::new(false);

pub fn set_light_theme(light: bool) {
    USE_LIGHT_THEME.store(light, Ordering::Relaxed);
}

pub fn active() -> &'static Tokens {
    if USE_LIGHT_THEME.load(Ordering::Relaxed) {
        &LIGHT_TOKENS
    } else {
        &DARK_TOKENS
    }
}

pub fn dark() -> &'static Tokens {
    active()
}

pub fn light() -> &'static Tokens {
    &LIGHT_TOKENS
}

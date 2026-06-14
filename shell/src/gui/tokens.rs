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
    pub text_heading: Color,
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

// Neutral dark-gray system (GitHub-dark-like) with teal reserved purely as the
// accent. Surfaces are neutral gray — no green tint — so the app reads
// professional; teal appears only on links, active tab, focus, selection.
// Headings are near-white weight, not a color (the coral borrow is gone).
pub static DARK_TOKENS: LazyLock<Tokens> = LazyLock::new(|| Tokens {
    bg_primary: hex_color(0x0d1117),
    bg_secondary: hex_color(0x161b22),
    bg_tertiary: hex_color(0x1f242c),
    bg_surface: hex_color(0x1a1f27),
    border: hex_color(0x2d333b),
    border_subtle: hex_color(0x21262d),
    text_primary: hex_color(0xe6edf3),
    text_secondary: hex_color(0x9aa4af),
    text_muted: hex_color(0x6e7681),
    text_heading: hex_color(0xf0f4f8),
    accent: hex_color(0x4fd1b5),
    accent_secondary: hex_color(0x7ee3cd),
    danger: hex_color(0xef7a72),
    success: hex_color(0x5cd6a0),
    warning: hex_color(0xe0b95e),
    sel_tint: hex_color_alpha(0x4fd1b5, 0.18),
    highlight_default: hex_color(0xffd866),
    bg_hover: hex_color(0x21262d),
    bg_pressed: hex_color(0x2d333b),
    focus_ring: hex_color(0x4fd1b5),
});

pub static LIGHT_TOKENS: LazyLock<Tokens> = LazyLock::new(|| Tokens {
    bg_primary: hex_color(0xfbfcfd),
    bg_secondary: hex_color(0xf0f2f5),
    bg_tertiary: hex_color(0xe4e8ec),
    bg_surface: hex_color(0xeceef1),
    border: hex_color(0xc8cdd4),
    border_subtle: hex_color(0xdde1e6),
    text_primary: hex_color(0x1c2027),
    text_secondary: hex_color(0x4a515b),
    text_muted: hex_color(0x717a85),
    text_heading: hex_color(0x0d1117),
    accent: hex_color(0x12937c),
    accent_secondary: hex_color(0x0f7a67),
    danger: hex_color(0xcf4b43),
    success: hex_color(0x1f9d63),
    warning: hex_color(0xb07d22),
    sel_tint: hex_color_alpha(0x12937c, 0.14),
    highlight_default: hex_color(0xffe08a),
    bg_hover: hex_color(0xe7eaee),
    bg_pressed: hex_color(0xd8dde3),
    focus_ring: hex_color(0x12937c),
});

pub fn dark() -> &'static Tokens {
    &DARK_TOKENS
}

pub fn light() -> &'static Tokens {
    &LIGHT_TOKENS
}

pub fn for_name(name: &str) -> &'static Tokens {
    if name == "light" { light() } else { dark() }
}

impl super::Shell {
    pub(crate) fn tokens(&self) -> &'static Tokens {
        for_name(&self.theme_name)
    }

    pub(crate) fn theme(&self) -> iced::Theme {
        let tokens = self.tokens();
        iced::Theme::custom_with_fn(
            format!("MD Editor {}", self.theme_name),
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

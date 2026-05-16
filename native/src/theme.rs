use iced::theme::Palette;
use iced::{Color, Theme};

// ── Premium Dark Theme Tokens ────────────────────────────────────────

pub const BG_PRIMARY: Color = Color::from_rgb(0.06, 0.07, 0.09);
pub const BG_SECONDARY: Color = Color::from_rgb(0.09, 0.10, 0.12);
pub const BG_TERTIARY: Color = Color::from_rgb(0.12, 0.13, 0.16);
pub const BG_SURFACE: Color = Color::from_rgb(0.16, 0.17, 0.20);
pub const BORDER: Color = Color::from_rgb(0.25, 0.27, 0.31);

pub const TEXT_PRIMARY: Color = Color::from_rgb(0.92, 0.94, 0.98);
pub const TEXT_SECONDARY: Color = Color::from_rgb(0.75, 0.77, 0.82);
pub const TEXT_MUTED: Color = Color::from_rgb(0.55, 0.57, 0.62);

pub const ACCENT: Color = Color::from_rgb(0.694, 0.80, 0.775); // #b1ccc6 — sage green
pub const ACCENT_GLOW: Color = Color::from_rgb(0.78, 0.88, 0.85);
pub const ACCENT_DIM: Color = Color::from_rgba(0.694, 0.80, 0.775, 0.08);

pub const DANGER: Color = Color::from_rgb(0.95, 0.45, 0.45);
pub const SUCCESS: Color = Color::from_rgb(0.45, 0.85, 0.55);
pub const WARNING: Color = Color::from_rgb(0.95, 0.75, 0.45);

/// Build the custom dark theme.
pub fn md_editor_theme() -> Theme {
    Theme::custom_with_fn(
        "MD Editor Premium Dark".to_string(),
        Palette {
            background: BG_PRIMARY,
            text: TEXT_PRIMARY,
            primary: ACCENT,
            success: SUCCESS,
            danger: DANGER,
            warning: WARNING,
        },
        |palette| iced::theme::palette::Extended::generate(palette),
    )
}

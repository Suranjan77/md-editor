#![allow(dead_code)]

use iced::theme::Palette;
use iced::{Color, Theme};
use std::sync::atomic::{AtomicU8, Ordering};

// ── Layout & Spacing Tokens ──────────────────────────────────────────
pub const SPACE_NONE: f32 = 0.0;
pub const SPACE_XXS: f32 = 2.0;
pub const SPACE_XS: f32 = 4.0;
pub const SPACE_S: f32 = 6.0;
pub const SPACE_M: f32 = 8.0;
pub const SPACE_L: f32 = 10.0;
pub const SPACE_XL: f32 = 12.0;
pub const SPACE_XXL: f32 = 16.0;
pub const SPACE_XXXL: f32 = 20.0;
pub const SPACE_HUGE: f32 = 24.0;

// ── Typography Scale ─────────────────────────────────────────────────
pub const FONT_SIZE_TINY: u16 = 10;
pub const FONT_SIZE_SMALL: u16 = 11;
pub const FONT_SIZE_REGULAR: u16 = 12;
pub const FONT_SIZE_MEDIUM: u16 = 14;
pub const FONT_SIZE_LARGE: u16 = 16;
pub const FONT_SIZE_HEADING: u16 = 18;
pub const FONT_SIZE_TITLE: u16 = 20;

// ── Shape Tokens (Border Radii) ──────────────────────────────────────
pub const RADIUS_NONE: f32 = 0.0;
pub const RADIUS_SMALL: f32 = 2.0;
pub const RADIUS_REGULAR: f32 = 4.0;
pub const RADIUS_LARGE: f32 = 8.0;
pub const RADIUS_ROUND: f32 = 9999.0;

// ── Focus & Dividers ─────────────────────────────────────────────────
pub const FOCUS_RING_WIDTH: f32 = 1.5;
pub const DIVIDER_WIDTH: f32 = 1.0;

// ── Premium Dark Theme Colors (Default) ──────────────────────────────
pub const DARK_BG_PRIMARY: Color = Color::from_rgb8(13, 14, 16); // #0d0e10
pub const DARK_BG_SECONDARY: Color = Color::from_rgb8(24, 26, 29); // #181a1d
pub const DARK_BG_TERTIARY: Color = Color::from_rgb8(35, 38, 43); // #23262b
pub const DARK_BG_SURFACE: Color = Color::from_rgb8(51, 75, 71); // #334b47
pub const DARK_BORDER: Color = Color::from_rgb8(69, 72, 78); // #45484e
pub const DARK_BORDER_SUBTLE: Color = Color::from_rgb8(29, 32, 36); // #1d2024
pub const DARK_TEXT_PRIMARY: Color = Color::from_rgb8(227, 229, 237); // #e3e5ed
pub const DARK_TEXT_SECONDARY: Color = Color::from_rgb8(169, 171, 178); // #a9abb2
pub const DARK_TEXT_MUTED: Color = Color::from_rgb8(157, 158, 163); // #9d9ea3
pub const DARK_ACCENT: Color = Color::from_rgb8(177, 204, 198); // #b1ccc6
pub const DARK_ACCENT_SECONDARY: Color = Color::from_rgb8(205, 232, 226); // #cde8e2
pub const DARK_ACCENT_GLOW: Color = Color::from_rgba8(177, 204, 198, 0.5);
pub const DARK_ACCENT_DIM: Color = Color::from_rgba8(177, 204, 198, 0.25);
pub const DARK_DANGER: Color = Color::from_rgb8(238, 125, 119); // #ee7d77
pub const DARK_SUCCESS: Color = Color::from_rgb8(217, 242, 210); // #d9f2d2
pub const DARK_WARNING: Color = Color::from_rgb8(191, 218, 212); // #bfdad4

// ── Premium Light Theme Colors ───────────────────────────────────────
pub const LIGHT_BG_PRIMARY: Color = Color::from_rgb8(247, 248, 250); // #f7f8fa
pub const LIGHT_BG_SECONDARY: Color = Color::from_rgb8(237, 240, 242); // #edf0f2
pub const LIGHT_BG_TERTIARY: Color = Color::from_rgb8(225, 228, 230); // #e1e4e6
pub const LIGHT_BG_SURFACE: Color = Color::from_rgb8(208, 225, 219); // #d0e1db
pub const LIGHT_BORDER: Color = Color::from_rgb8(184, 192, 197); // #b8c0c5
pub const LIGHT_BORDER_SUBTLE: Color = Color::from_rgb8(216, 222, 225); // #d8dee1
pub const LIGHT_TEXT_PRIMARY: Color = Color::from_rgb8(27, 30, 34); // #1b1e22
pub const LIGHT_TEXT_SECONDARY: Color = Color::from_rgb8(72, 79, 86); // #484f56
pub const LIGHT_TEXT_MUTED: Color = Color::from_rgb8(92, 101, 108); // #5c656c
pub const LIGHT_ACCENT: Color = Color::from_rgb8(46, 92, 84); // #2e5c54
pub const LIGHT_ACCENT_SECONDARY: Color = Color::from_rgb8(65, 125, 114); // #417d72
pub const LIGHT_ACCENT_GLOW: Color = Color::from_rgba8(46, 92, 84, 0.5);
pub const LIGHT_ACCENT_DIM: Color = Color::from_rgba8(46, 92, 84, 0.25);
pub const LIGHT_DANGER: Color = Color::from_rgb8(192, 57, 43); // #c0392b
pub const LIGHT_SUCCESS: Color = Color::from_rgb8(31, 122, 70); // #1f7a46
pub const LIGHT_WARNING: Color = Color::from_rgb8(154, 79, 0); // #9a4f00

// ── High Contrast Theme Colors ───────────────────────────────────────
pub const HC_BG_PRIMARY: Color = Color::from_rgb8(0, 0, 0); // #000000
pub const HC_BG_SECONDARY: Color = Color::from_rgb8(18, 18, 18); // #121212
pub const HC_BG_TERTIARY: Color = Color::from_rgb8(36, 36, 36); // #242424
pub const HC_BG_SURFACE: Color = Color::from_rgb8(0, 51, 0); // #003300
pub const HC_BORDER: Color = Color::from_rgb8(255, 255, 255); // #ffffff
pub const HC_BORDER_SUBTLE: Color = Color::from_rgb8(204, 204, 204); // #cccccc
pub const HC_TEXT_PRIMARY: Color = Color::from_rgb8(255, 255, 255); // #ffffff
pub const HC_TEXT_SECONDARY: Color = Color::from_rgb8(224, 224, 224); // #e0e0e0
pub const HC_TEXT_MUTED: Color = Color::from_rgb8(170, 170, 170); // #aaaaaa
pub const HC_ACCENT: Color = Color::from_rgb8(0, 255, 255); // #00ffff
pub const HC_ACCENT_SECONDARY: Color = Color::from_rgb8(0, 204, 204); // #00cccc
pub const HC_ACCENT_GLOW: Color = Color::from_rgba8(0, 255, 255, 0.5);
pub const HC_ACCENT_DIM: Color = Color::from_rgba8(0, 255, 255, 0.25);
pub const HC_DANGER: Color = Color::from_rgb8(255, 0, 0); // #ff0000
pub const HC_SUCCESS: Color = Color::from_rgb8(0, 255, 0); // #00ff00
pub const HC_WARNING: Color = Color::from_rgb8(255, 255, 0); // #ffff00

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum AppTheme {
    #[default]
    Dark,
    Light,
    HighContrast,
}

impl AppTheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::HighContrast => "high_contrast",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            "high_contrast" => Some(Self::HighContrast),
            _ => None,
        }
    }
}

// ── Theme State ──────────────────────────────────────────────────────
static ACTIVE_THEME: AtomicU8 = AtomicU8::new(0); // 0 = Dark, 1 = Light, 2 = HighContrast

pub fn set_active_theme(theme: AppTheme) {
    let val = match theme {
        AppTheme::Dark => 0,
        AppTheme::Light => 1,
        AppTheme::HighContrast => 2,
    };
    ACTIVE_THEME.store(val, Ordering::SeqCst);
}

pub fn get_active_theme() -> AppTheme {
    match ACTIVE_THEME.load(Ordering::SeqCst) {
        1 => AppTheme::Light,
        2 => AppTheme::HighContrast,
        _ => AppTheme::Dark,
    }
}

// ── Dynamic Color Resolvers ──────────────────────────────────────────
pub fn bg_primary() -> Color {
    match get_active_theme() {
        AppTheme::Dark => DARK_BG_PRIMARY,
        AppTheme::Light => LIGHT_BG_PRIMARY,
        AppTheme::HighContrast => HC_BG_PRIMARY,
    }
}

pub fn bg_secondary() -> Color {
    match get_active_theme() {
        AppTheme::Dark => DARK_BG_SECONDARY,
        AppTheme::Light => LIGHT_BG_SECONDARY,
        AppTheme::HighContrast => HC_BG_SECONDARY,
    }
}

pub fn bg_tertiary() -> Color {
    match get_active_theme() {
        AppTheme::Dark => DARK_BG_TERTIARY,
        AppTheme::Light => LIGHT_BG_TERTIARY,
        AppTheme::HighContrast => HC_BG_TERTIARY,
    }
}

pub fn bg_surface() -> Color {
    match get_active_theme() {
        AppTheme::Dark => DARK_BG_SURFACE,
        AppTheme::Light => LIGHT_BG_SURFACE,
        AppTheme::HighContrast => HC_BG_SURFACE,
    }
}

pub fn border() -> Color {
    match get_active_theme() {
        AppTheme::Dark => DARK_BORDER,
        AppTheme::Light => LIGHT_BORDER,
        AppTheme::HighContrast => HC_BORDER,
    }
}

pub fn border_subtle() -> Color {
    match get_active_theme() {
        AppTheme::Dark => DARK_BORDER_SUBTLE,
        AppTheme::Light => LIGHT_BORDER_SUBTLE,
        AppTheme::HighContrast => HC_BORDER_SUBTLE,
    }
}

pub fn text_primary() -> Color {
    match get_active_theme() {
        AppTheme::Dark => DARK_TEXT_PRIMARY,
        AppTheme::Light => LIGHT_TEXT_PRIMARY,
        AppTheme::HighContrast => HC_TEXT_PRIMARY,
    }
}

pub fn text_secondary() -> Color {
    match get_active_theme() {
        AppTheme::Dark => DARK_TEXT_SECONDARY,
        AppTheme::Light => LIGHT_TEXT_SECONDARY,
        AppTheme::HighContrast => HC_TEXT_SECONDARY,
    }
}

pub fn text_muted() -> Color {
    match get_active_theme() {
        AppTheme::Dark => DARK_TEXT_MUTED,
        AppTheme::Light => LIGHT_TEXT_MUTED,
        AppTheme::HighContrast => HC_TEXT_MUTED,
    }
}

pub fn accent() -> Color {
    match get_active_theme() {
        AppTheme::Dark => DARK_ACCENT,
        AppTheme::Light => LIGHT_ACCENT,
        AppTheme::HighContrast => HC_ACCENT,
    }
}

pub fn accent_secondary() -> Color {
    match get_active_theme() {
        AppTheme::Dark => DARK_ACCENT_SECONDARY,
        AppTheme::Light => LIGHT_ACCENT_SECONDARY,
        AppTheme::HighContrast => HC_ACCENT_SECONDARY,
    }
}

pub fn accent_glow() -> Color {
    match get_active_theme() {
        AppTheme::Dark => DARK_ACCENT_GLOW,
        AppTheme::Light => LIGHT_ACCENT_GLOW,
        AppTheme::HighContrast => HC_ACCENT_GLOW,
    }
}

pub fn accent_dim() -> Color {
    match get_active_theme() {
        AppTheme::Dark => DARK_ACCENT_DIM,
        AppTheme::Light => LIGHT_ACCENT_DIM,
        AppTheme::HighContrast => HC_ACCENT_DIM,
    }
}

pub fn danger() -> Color {
    match get_active_theme() {
        AppTheme::Dark => DARK_DANGER,
        AppTheme::Light => LIGHT_DANGER,
        AppTheme::HighContrast => HC_DANGER,
    }
}

pub fn success() -> Color {
    match get_active_theme() {
        AppTheme::Dark => DARK_SUCCESS,
        AppTheme::Light => LIGHT_SUCCESS,
        AppTheme::HighContrast => HC_SUCCESS,
    }
}

pub fn warning() -> Color {
    match get_active_theme() {
        AppTheme::Dark => DARK_WARNING,
        AppTheme::Light => LIGHT_WARNING,
        AppTheme::HighContrast => HC_WARNING,
    }
}

/// Build the custom theme dynamically based on current configuration.
pub fn md_editor_theme() -> Theme {
    match get_active_theme() {
        AppTheme::Dark => Theme::custom_with_fn(
            "MD Editor Premium Dark".to_string(),
            Palette {
                background: DARK_BG_PRIMARY,
                text: DARK_TEXT_PRIMARY,
                primary: DARK_ACCENT,
                success: DARK_SUCCESS,
                danger: DARK_DANGER,
                warning: DARK_WARNING,
            },
            |palette| iced::theme::palette::Extended::generate(palette),
        ),
        AppTheme::Light => Theme::custom_with_fn(
            "MD Editor Premium Light".to_string(),
            Palette {
                background: LIGHT_BG_PRIMARY,
                text: LIGHT_TEXT_PRIMARY,
                primary: LIGHT_ACCENT,
                success: LIGHT_SUCCESS,
                danger: LIGHT_DANGER,
                warning: LIGHT_WARNING,
            },
            |palette| iced::theme::palette::Extended::generate(palette),
        ),
        AppTheme::HighContrast => Theme::custom_with_fn(
            "MD Editor High Contrast".to_string(),
            Palette {
                background: HC_BG_PRIMARY,
                text: HC_TEXT_PRIMARY,
                primary: HC_ACCENT,
                success: HC_SUCCESS,
                danger: HC_DANGER,
                warning: HC_WARNING,
            },
            |palette| iced::theme::palette::Extended::generate(palette),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linear_channel(channel: f32) -> f32 {
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(color: Color) -> f32 {
        0.2126 * linear_channel(color.r)
            + 0.7152 * linear_channel(color.g)
            + 0.0722 * linear_channel(color.b)
    }

    fn contrast_ratio(a: Color, b: Color) -> f32 {
        let (lighter, darker) = if luminance(a) >= luminance(b) {
            (luminance(a), luminance(b))
        } else {
            (luminance(b), luminance(a))
        };
        (lighter + 0.05) / (darker + 0.05)
    }

    #[test]
    fn theme_text_and_status_colors_meet_normal_text_contrast() {
        let palettes = [
            (
                DARK_BG_PRIMARY,
                [
                    DARK_TEXT_PRIMARY,
                    DARK_TEXT_SECONDARY,
                    DARK_TEXT_MUTED,
                    DARK_DANGER,
                    DARK_SUCCESS,
                    DARK_WARNING,
                ],
            ),
            (
                LIGHT_BG_PRIMARY,
                [
                    LIGHT_TEXT_PRIMARY,
                    LIGHT_TEXT_SECONDARY,
                    LIGHT_TEXT_MUTED,
                    LIGHT_DANGER,
                    LIGHT_SUCCESS,
                    LIGHT_WARNING,
                ],
            ),
            (
                HC_BG_PRIMARY,
                [
                    HC_TEXT_PRIMARY,
                    HC_TEXT_SECONDARY,
                    HC_TEXT_MUTED,
                    HC_DANGER,
                    HC_SUCCESS,
                    HC_WARNING,
                ],
            ),
        ];

        for (background, colors) in palettes {
            for color in colors {
                assert!(
                    contrast_ratio(color, background) >= 4.5,
                    "{color:?} must meet 4.5:1 against {background:?}"
                );
            }
        }
    }
}

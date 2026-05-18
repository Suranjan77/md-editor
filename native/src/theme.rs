use iced::theme::Palette;
use iced::{Color, Theme};

// ── Premium Dark Theme Tokens ────────────────────────────────────────

pub const BG_PRIMARY: Color = Color::from_rgb8(13, 14, 16);       // #0d0e10
pub const BG_SECONDARY: Color = Color::from_rgb8(24, 26, 29);     // #181a1d
pub const BG_TERTIARY: Color = Color::from_rgb8(35, 38, 43);      // #23262b
pub const BG_SURFACE: Color = Color::from_rgb8(51, 75, 71);       // #334b47
pub const BORDER: Color = Color::from_rgb8(69, 72, 78);           // #45484e
pub const BORDER_SUBTLE: Color = Color::from_rgb8(29, 32, 36);    // #1d2024

pub const TEXT_PRIMARY: Color = Color::from_rgb8(227, 229, 237);  // #e3e5ed
pub const TEXT_SECONDARY: Color = Color::from_rgb8(169, 171, 178);// #a9abb2
pub const TEXT_MUTED: Color = Color::from_rgb8(157, 158, 163);    // #9d9ea3

pub const ACCENT: Color = Color::from_rgb8(177, 204, 198);        // #b1ccc6
pub const ACCENT_SECONDARY: Color = Color::from_rgb8(205, 232, 226); // #cde8e2
pub const ACCENT_GLOW: Color = Color::from_rgba8(177, 204, 198, 0.5);
pub const ACCENT_DIM: Color = Color::from_rgba8(177, 204, 198, 0.2);

pub const DANGER: Color = Color::from_rgb8(238, 125, 119);        // #ee7d77
pub const SUCCESS: Color = Color::from_rgb8(217, 242, 210);       // #d9f2d2
pub const WARNING: Color = Color::from_rgb8(191, 218, 212);       // #bfdad4


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

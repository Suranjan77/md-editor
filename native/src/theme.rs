//! Design tokens for the application chrome.
//!
//! Everything the UI draws — sidebar, toolbar, panels, modals, tracker — takes
//! its colour, text size, spacing, and corner radius from this module. Values
//! are not chosen per call site: the app had eleven distinct font sizes and
//! seventeen distinct spacing values before these scales existed, which is the
//! visual noise that reads as "unpolished" without ever being nameable.
//!
//! Document typography (heading sizes, code and math scale inside a note) is a
//! separate concern and lives with the markdown renderer.

use iced::theme::Palette;
use iced::{Color, Theme};

// ── Type scale ───────────────────────────────────────────────────────
//
// Six steps. Anything the chrome renders picks one of these; there is no
// in-between size.

/// Fine print: badges, counters, gutter numbers.
pub const TEXT_XS: f32 = 10.0;
/// Secondary UI text: captions, metadata, tree affordances.
pub const TEXT_SM: f32 = 12.0;
/// Default UI text.
pub const TEXT_BASE: f32 = 14.0;
/// Emphasised UI text and section headings.
pub const TEXT_MD: f32 = 16.0;
/// Panel and dialog titles.
pub const TEXT_LG: f32 = 18.0;
/// The welcome screen wordmark; deliberately the only display-scale text.
pub const TEXT_DISPLAY: f32 = 42.0;

// ── Spacing scale ────────────────────────────────────────────────────
//
// Seven steps on a 2px grid, growing roughly geometrically so adjacent steps
// stay visibly distinct.

/// Flush; no gap.
pub const SPACE_0: f32 = 0.0;
/// Hairline separation between tightly related items.
pub const SPACE_1: f32 = 2.0;
/// Within a control: icon to label.
pub const SPACE_2: f32 = 4.0;
/// Default gap between siblings, and default control padding.
pub const SPACE_3: f32 = 8.0;
/// Between grouped rows; comfortable control padding.
pub const SPACE_4: f32 = 12.0;
/// Between groups within a panel.
pub const SPACE_5: f32 = 16.0;
/// Between major regions.
pub const SPACE_6: f32 = 24.0;

// ── Radius ───────────────────────────────────────────────────────────

/// Chips, badges, and inline markers.
pub const RADIUS_SM: f32 = 3.0;
/// Buttons, inputs, list rows.
pub const RADIUS_MD: f32 = 6.0;
/// Panels, cards, and modals.
pub const RADIUS_LG: f32 = 10.0;

// ── Premium Dark Theme Tokens ────────────────────────────────────────

pub const BG_PRIMARY: Color = Color::from_rgb8(13, 14, 16); // #0d0e10
pub const BG_SECONDARY: Color = Color::from_rgb8(24, 26, 29); // #181a1d
pub const BG_TERTIARY: Color = Color::from_rgb8(35, 38, 43); // #23262b
pub const BG_SURFACE: Color = Color::from_rgb8(51, 75, 71); // #334b47
pub const BORDER: Color = Color::from_rgb8(69, 72, 78); // #45484e
pub const BORDER_SUBTLE: Color = Color::from_rgb8(29, 32, 36); // #1d2024

pub const TEXT_PRIMARY: Color = Color::from_rgb8(227, 229, 237); // #e3e5ed
pub const TEXT_SECONDARY: Color = Color::from_rgb8(169, 171, 178); // #a9abb2
pub const TEXT_MUTED: Color = Color::from_rgb8(157, 158, 163); // #9d9ea3

pub const ACCENT: Color = Color::from_rgb8(177, 204, 198); // #b1ccc6
pub const ACCENT_SECONDARY: Color = Color::from_rgb8(205, 232, 226); // #cde8e2
pub const ACCENT_GLOW: Color = Color::from_rgba8(177, 204, 198, 0.5);
pub const ACCENT_DIM: Color = Color::from_rgba8(177, 204, 198, 0.2);

pub const DANGER: Color = Color::from_rgb8(238, 125, 119); // #ee7d77
pub const SUCCESS: Color = Color::from_rgb8(217, 242, 210); // #d9f2d2
pub const WARNING: Color = Color::from_rgb8(191, 218, 212); // #bfdad4

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
        iced::theme::palette::Extended::generate,
    )
}

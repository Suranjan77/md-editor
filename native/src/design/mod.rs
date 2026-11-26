//! Design system root: runtime theme selection, interface scaling, document typography settings, and revision counter.

pub mod fonts;
pub mod metrics;
pub mod motion;
pub mod palette;
mod setting;
pub mod type_scale;

#[cfg(test)]
mod tests;

use setting::Setting;

use iced::Theme;
use iced::theme::Palette;

pub use palette::{ContentHue, DARK, LIGHT, Tokens, fade};
pub use type_scale::TextRole;

/// User-selected theme preference (Settings → Appearance → Theme).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    System,
    Dark,
    Light,
}

impl ThemeMode {
    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "dark" => Self::Dark,
            "light" => Self::Light,
            _ => Self::System,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

/// Resolved active theme (`Dark` or `Light`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTheme {
    Dark = 0,
    Light = 1,
}

/// Document reading typeface (`pages/editor.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadingFont {
    Sans = 0,
    Serif = 1,
}

impl ReadingFont {
    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "serif" => Self::Serif,
            _ => Self::Sans,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sans => "sans",
            Self::Serif => "serif",
        }
    }
}

/// Editor line-width preset (`pages/editor.md`: 640 / 720 / 880 px).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentMeasure {
    Narrow = 0,
    Default = 1,
    Wide = 2,
}

impl ContentMeasure {
    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "narrow" | "640" => Self::Narrow,
            "wide" | "880" => Self::Wide,
            _ => Self::Default,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Narrow => "narrow",
            Self::Default => "default",
            Self::Wide => "wide",
        }
    }

    pub const fn max_width_px(self) -> f32 {
        match self {
            Self::Narrow => 640.0,
            Self::Default => 720.0,
            Self::Wide => 880.0,
        }
    }
}

static THEME_MODE: Setting = Setting::new(0);
static ACTIVE_THEME: Setting = Setting::new(ActiveTheme::Dark as u64);
/// Interface scale in percent (90..=130), stored as an integer so atomic reads are trivial.
static INTERFACE_SCALE_PCT: Setting = Setting::new(100);
static READING_FONT: Setting = Setting::new(ReadingFont::Sans as u64);
static CONTENT_MEASURE: Setting = Setting::new(ContentMeasure::Default as u64);
static EDITOR_FONT_SIZE_X10: Setting = Setting::new(170);
/// Line numbers in the editor's left margin (Settings → Editor); off by default.
static LINE_NUMBERS: Setting = Setting::new(0);
static THEME_REVISION: Setting = Setting::new(1);

fn bump_revision() {
    let revision = THEME_REVISION.get();
    THEME_REVISION.replace(revision + 1);
}

/// Monotonic counter incremented whenever the active theme, interface scale,
/// reading font, editor font size or content measure changes. Caches (`EditorPane`,
/// `LayoutKey`, `GraphState`) fold this in so switching repaints cleanly.
pub fn theme_revision() -> u64 {
    THEME_REVISION.get()
}

pub fn active_theme() -> ActiveTheme {
    if ACTIVE_THEME.get() == ActiveTheme::Light as u64 {
        ActiveTheme::Light
    } else {
        ActiveTheme::Dark
    }
}

pub fn set_active_theme(theme: ActiveTheme) {
    let prev = ACTIVE_THEME.replace(theme as u64);
    if prev != theme as u64 {
        bump_revision();
    }
}

/// The user's theme preference; `System` follows the OS (resolved into [`active_theme`]).
pub fn theme_mode() -> ThemeMode {
    match THEME_MODE.get() {
        1 => ThemeMode::Dark,
        2 => ThemeMode::Light,
        _ => ThemeMode::System,
    }
}

pub fn set_theme_mode(mode: ThemeMode) {
    let raw = match mode {
        ThemeMode::System => 0,
        ThemeMode::Dark => 1,
        ThemeMode::Light => 2,
    };
    THEME_MODE.replace(raw);
}

/// Active semantic token table (`&DARK` or `&LIGHT`).
pub fn tokens() -> &'static Tokens {
    match active_theme() {
        ActiveTheme::Dark => &DARK,
        ActiveTheme::Light => &LIGHT,
    }
}

pub fn interface_scale_percent() -> u32 {
    // Clamped to 90..=130, so the narrowing cannot truncate.
    INTERFACE_SCALE_PCT.get().clamp(90, 130) as u32
}

pub fn interface_scale() -> f32 {
    interface_scale_percent() as f32 / 100.0
}

pub fn set_interface_scale_percent(percent: u32) {
    let clamped = u64::from(percent.clamp(90, 130));
    let prev = INTERFACE_SCALE_PCT.replace(clamped);
    if prev != clamped {
        bump_revision();
    }
}

pub fn reading_font() -> ReadingFont {
    if READING_FONT.get() == ReadingFont::Serif as u64 {
        ReadingFont::Serif
    } else {
        ReadingFont::Sans
    }
}

pub fn set_reading_font(font: ReadingFont) {
    let prev = READING_FONT.replace(font as u64);
    if prev != font as u64 {
        bump_revision();
    }
}

pub fn line_numbers() -> bool {
    LINE_NUMBERS.get() != 0
}

/// Line numbers paint in the margin and change no line's height, so this does
/// not bump the theme revision.
pub fn set_line_numbers(on: bool) {
    LINE_NUMBERS.replace(u64::from(on));
}

pub fn content_measure() -> ContentMeasure {
    match CONTENT_MEASURE.get() {
        0 => ContentMeasure::Narrow,
        2 => ContentMeasure::Wide,
        _ => ContentMeasure::Default,
    }
}

pub fn set_content_measure(measure: ContentMeasure) {
    let prev = CONTENT_MEASURE.replace(measure as u64);
    if prev != measure as u64 {
        bump_revision();
    }
}

pub fn editor_font_size() -> f32 {
    (EDITOR_FONT_SIZE_X10.get().clamp(140, 220) as f32) / 10.0
}

pub fn set_editor_font_size(px: f32) {
    let x10 = (px.clamp(14.0, 22.0) * 10.0).round() as u64;
    let prev = EDITOR_FONT_SIZE_X10.replace(x10);
    if prev != x10 {
        bump_revision();
    }
}

/// Build the Iced [`Theme`] matching the active semantic tokens so built-in
/// scrollbars, selections and widget defaults match the chrome.
pub fn iced_theme() -> Theme {
    let t = tokens();
    let name = if t.is_dark {
        "MD Editor Dark"
    } else {
        "MD Editor Light"
    };
    Theme::custom_with_fn(
        name.to_string(),
        Palette {
            background: t.bg_canvas,
            text: t.fg_primary,
            primary: t.accent,
            success: t.status_success,
            danger: t.status_danger,
            warning: t.status_warning,
        },
        iced::theme::palette::Extended::generate,
    )
}

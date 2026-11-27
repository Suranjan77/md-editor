//! Bundled OFL fonts: Inter (UI & sans reading), JetBrains Mono (code & tabular numerals), and Source Serif (serif reading).
#![allow(dead_code)] // Bundled faces and weights; the serif reading font is not applied by the editor yet.

use iced::Font;
use iced::font::{Family, Style, Weight};

pub const INTER_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.otf");
pub const INTER_MEDIUM: &[u8] = include_bytes!("../../assets/fonts/Inter-Medium.otf");
pub const INTER_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-SemiBold.otf");
pub const INTER_BOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-Bold.otf");
pub const INTER_ITALIC: &[u8] = include_bytes!("../../assets/fonts/Inter-Italic.otf");

pub const JETBRAINS_MONO_REGULAR: &[u8] =
    include_bytes!("../../assets/fonts/JetBrainsMono-Regular.otf");
pub const JETBRAINS_MONO_SEMIBOLD: &[u8] =
    include_bytes!("../../assets/fonts/JetBrainsMono-SemiBold.otf");
pub const JETBRAINS_MONO_BOLD: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Bold.otf");
pub const JETBRAINS_MONO_ITALIC: &[u8] =
    include_bytes!("../../assets/fonts/JetBrainsMono-Italic.otf");

pub const SOURCE_SERIF_REGULAR: &[u8] =
    include_bytes!("../../assets/fonts/SourceSerifPro-Regular.otf");
pub const SOURCE_SERIF_SEMIBOLD: &[u8] =
    include_bytes!("../../assets/fonts/SourceSerifPro-Semibold.otf");
pub const SOURCE_SERIF_BOLD: &[u8] = include_bytes!("../../assets/fonts/SourceSerifPro-Bold.otf");
pub const SOURCE_SERIF_ITALIC: &[u8] =
    include_bytes!("../../assets/fonts/SourceSerifPro-RegularIt.otf");

/// Every font blob registered on startup so metrics and glyph shaping match
/// across Linux, Windows and macOS.
pub const BUNDLED_FONTS: &[&[u8]] = &[
    INTER_REGULAR,
    INTER_MEDIUM,
    INTER_SEMIBOLD,
    INTER_BOLD,
    INTER_ITALIC,
    JETBRAINS_MONO_REGULAR,
    JETBRAINS_MONO_SEMIBOLD,
    JETBRAINS_MONO_BOLD,
    JETBRAINS_MONO_ITALIC,
    SOURCE_SERIF_REGULAR,
    SOURCE_SERIF_SEMIBOLD,
    SOURCE_SERIF_BOLD,
    SOURCE_SERIF_ITALIC,
];

pub const UI_FAMILY: &str = "Inter";
pub const MONO_FAMILY: &str = "JetBrains Mono";
pub const SERIF_FAMILY: &str = "Source Serif Pro";

pub const UI_FONT: Font = Font {
    family: Family::Name(UI_FAMILY),
    weight: Weight::Normal,
    stretch: iced::font::Stretch::Normal,
    style: Style::Normal,
};

pub const UI_FONT_MEDIUM: Font = Font {
    weight: Weight::Medium,
    ..UI_FONT
};

pub const UI_FONT_SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..UI_FONT
};

pub const UI_FONT_BOLD: Font = Font {
    weight: Weight::Bold,
    ..UI_FONT
};

pub const UI_FONT_ITALIC: Font = Font {
    style: Style::Italic,
    ..UI_FONT
};

pub const MONO_FONT: Font = Font {
    family: Family::Name(MONO_FAMILY),
    weight: Weight::Normal,
    stretch: iced::font::Stretch::Normal,
    style: Style::Normal,
};

pub const MONO_FONT_SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..MONO_FONT
};

pub const MONO_FONT_BOLD: Font = Font {
    weight: Weight::Bold,
    ..MONO_FONT
};

pub const SERIF_FONT: Font = Font {
    family: Family::Name(SERIF_FAMILY),
    weight: Weight::Normal,
    stretch: iced::font::Stretch::Normal,
    style: Style::Normal,
};

pub const SERIF_FONT_SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..SERIF_FONT
};

pub const SERIF_FONT_BOLD: Font = Font {
    weight: Weight::Bold,
    ..SERIF_FONT
};

pub const SERIF_FONT_ITALIC: Font = Font {
    style: Style::Italic,
    ..SERIF_FONT
};

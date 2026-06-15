//! Embedded typefaces for the Quiet Vault design (docs/DESIGN-SYSTEM.md §2):
//! **Hanken Grotesk** for all UI + editor body, **Geist Mono** for code and
//! numerals (hours, dates, keycaps, line/col). The font bytes are vendored under
//! `assets/fonts/` (OFL) and registered on the iced application builder in
//! `welcome.rs`; everything else refers to the faces through these consts.

use iced::Font;

/// Family names as they appear in the vendored fonts' name tables. `with_name`
/// matches on these, so they must stay in sync with the registered bytes.
pub const SANS_NAME: &str = "Hanken Grotesk";
pub const MONO_NAME: &str = "Geist Mono";

pub const SANS: Font = Font::with_name(SANS_NAME);
pub const MONO: Font = Font::with_name(MONO_NAME);

pub const SANS_BOLD: Font = Font {
    weight: iced::font::Weight::Bold,
    ..Font::with_name(SANS_NAME)
};

pub const SANS_ITALIC: Font = Font {
    style: iced::font::Style::Italic,
    ..Font::with_name(SANS_NAME)
};

/// Raw font bytes, registered once on the application builder.
pub const HANKEN_GROTESK_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/HankenGrotesk-VariableFont_wght.ttf");
pub const GEIST_MONO_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/GeistMono-VariableFont_wght.ttf");

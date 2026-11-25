//! UI typography scale (MASTER §3.1): every UI text element picks a `TextRole` (minimum 12px at 100%).
#![allow(dead_code)] // Full type scale (MASTER §3.1); not every role has a user yet.

use iced::widget::text::LineHeight;
use iced::{Font, Pixels};

use super::fonts::{
    MONO_FONT, MONO_FONT_SEMIBOLD, UI_FONT, UI_FONT_BOLD, UI_FONT_MEDIUM, UI_FONT_SEMIBOLD,
};
use super::interface_scale;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRole {
    /// 12 / 16 · 400 — Metadata, counters, helper text, status bar.
    Caption,
    /// 12 / 16 · 500 — Section labels (sentence case).
    Label,
    /// 13 / 20 · 400 — Default UI text, rows, menu items.
    Body,
    /// 13 / 20 · 600 — Emphasis within UI text.
    BodyStrong,
    /// 14 / 20 · 600 — Panel headers, dialog body headings.
    TitleSm,
    /// 16 / 24 · 600 — Dialog titles, view titles.
    Title,
    /// 20 / 28 · 600 — Settings sections, tracker headers, stat values.
    Heading,
    /// 32 / 40 · 700 — Welcome wordmark only.
    Display,
    /// 12 / 16 · Mono 400 — Shortcut keys, counters, compact tabular figures.
    MonoCaption,
    /// 13 / 20 · Mono 400 — Timers, page/zoom inputs, code config.
    MonoBody,
    /// 14 / 20 · Mono 600 — Prominent timer readout.
    MonoStrong,
}

impl TextRole {
    pub const fn raw_size(self) -> f32 {
        match self {
            Self::Caption | Self::Label | Self::MonoCaption => 12.0,
            Self::Body | Self::BodyStrong | Self::MonoBody => 13.0,
            Self::TitleSm | Self::MonoStrong => 14.0,
            Self::Title => 16.0,
            Self::Heading => 20.0,
            Self::Display => 32.0,
        }
    }

    pub const fn raw_line_height(self) -> f32 {
        match self {
            Self::Caption | Self::Label | Self::MonoCaption => 16.0,
            Self::Body | Self::BodyStrong | Self::TitleSm | Self::MonoBody | Self::MonoStrong => {
                20.0
            }
            Self::Title => 24.0,
            Self::Heading => 28.0,
            Self::Display => 40.0,
        }
    }

    pub fn size(self) -> f32 {
        (self.raw_size() * interface_scale()).round().max(11.0)
    }

    pub fn line_height(self) -> LineHeight {
        LineHeight::Absolute(Pixels((self.raw_line_height() * interface_scale()).round()))
    }

    pub const fn font(self) -> Font {
        match self {
            Self::Caption | Self::Body => UI_FONT,
            Self::Label => UI_FONT_MEDIUM,
            Self::BodyStrong | Self::TitleSm | Self::Title | Self::Heading => UI_FONT_SEMIBOLD,
            Self::Display => UI_FONT_BOLD,
            Self::MonoCaption | Self::MonoBody => MONO_FONT,
            Self::MonoStrong => MONO_FONT_SEMIBOLD,
        }
    }
}

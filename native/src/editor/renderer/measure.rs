//! Text measurement.
//!
//! Layout, drawing and hit-testing all measure the same spans repeatedly —
//! once per frame, per visible line — and shaping dominates that work. The
//! width of a string at a given size and font never changes, so widths are
//! computed once and memoized here.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use iced::Size;
use iced::advanced::text;

use super::Measure;
use crate::editor::highlight::{StyledLine, StyledSpan};

/// The default font at bold weight.
pub(super) fn bold_font() -> iced::Font {
    iced::Font {
        weight: iced::font::Weight::Bold,
        ..iced::Font::DEFAULT
    }
}

/// Pick the iced font for a span.
pub(super) fn span_font(span: &StyledSpan, line: &StyledLine) -> iced::Font {
    if span.is_code || line.is_code_block || line.is_math_block {
        iced::Font::MONOSPACE
    } else if span.bold {
        bold_font()
    } else if span.italic {
        iced::Font {
            style: iced::font::Style::Italic,
            ..iced::Font::DEFAULT
        }
    } else {
        iced::Font::DEFAULT
    }
}

/// Shape `content` and return its advance width. This is the expensive call —
/// it builds a fresh paragraph and runs the text shaper — so everything goes
/// through the memoized [`measure_width`] instead of calling this directly.
fn shape_width<R: Measure>(content: &str, size: f32, font: iced::Font) -> f32 {
    use iced::advanced::text::Paragraph;
    let paragraph = R::Paragraph::with_text(text::Text {
        content,
        bounds: Size::new(f32::INFINITY, f32::INFINITY),
        size: size.into(),
        line_height: text::LineHeight::default(),
        font,
        align_x: iced::alignment::Horizontal::Left.into(),
        align_y: iced::alignment::Vertical::Top,
        shaping: text::Shaping::Basic,
        wrapping: text::Wrapping::None,
    });
    paragraph.min_bounds().width
}

#[derive(Hash, PartialEq, Eq, Clone, Debug)]
struct SpanCacheKey {
    content: Box<str>,
    font: iced::Font,
    size_bits: u32,
}

#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
struct CharCacheKey {
    ch: char,
    font: iced::Font,
    size_bits: u32,
}

/// Above this many entries the span-width cache is dropped wholesale. Layout
/// and drawing touch a bounded working set (the visible lines), so a periodic
/// reset costs one re-shape per live span and keeps a long session from
/// accumulating every string the document ever contained.
const SPAN_WIDTH_CACHE_CAP: usize = 8192;

/// Width of `content` when shaped at `size` in `font`, memoized.
pub(super) fn measure_width<R: Measure>(content: &str, size: f32, font: iced::Font) -> f32 {
    if content.is_empty() {
        return 0.0;
    }

    static CACHE: OnceLock<Mutex<HashMap<SpanCacheKey, f32>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    let key = SpanCacheKey {
        content: content.into(),
        font,
        size_bits: size.to_bits(),
    };

    if let Ok(cache) = cache.lock()
        && let Some(width) = cache.get(&key)
    {
        return *width;
    }

    let width = shape_width::<R>(content, size, font);

    if let Ok(mut cache) = cache.lock() {
        if cache.len() >= SPAN_WIDTH_CACHE_CAP {
            cache.clear();
        }
        cache.insert(key, width);
    }

    width
}

/// Width of a single character, memoized separately from spans.
pub(super) fn measure_char_width<R: Measure>(ch: char, size: f32, font: iced::Font) -> f32 {
    static CACHE: OnceLock<Mutex<HashMap<CharCacheKey, f32>>> = OnceLock::new();
    let key = CharCacheKey {
        ch,
        font,
        size_bits: size.to_bits(),
    };
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(cache) = cache.lock()
        && let Some(width) = cache.get(&key)
    {
        return *width;
    }

    // Straight to the shaper: this cache is the char-level one, so going
    // through `measure_width` would only add a second lookup and a redundant
    // entry in the span cache.
    let width = shape_width::<R>(&ch.to_string(), size, font);
    if let Ok(mut cache) = cache.lock() {
        cache.insert(key, width);
    }
    width
}

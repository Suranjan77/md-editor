//! Text measurement.
//!
//! Layout, drawing and hit-testing all measure the same spans repeatedly —
//! once per frame, per visible line — and shaping dominates that work. The
//! width of a string at a given size and font never changes, so widths are
//! computed once and memoized here.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use iced::Size;
use iced::advanced::text;

use super::Measure;
use crate::editor::highlight::{StyledLine, StyledSpan};

/// A font's vertical metrics, per unit of font size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct FontMetrics {
    /// Baseline to the top of the tallest glyphs.
    pub ascent: f32,
    /// Baseline to the bottom of the deepest glyphs.
    pub descent: f32,
    /// Height of lowercase letters: inline equations and checkboxes centre on
    /// half of it, the line a minus sign sits on.
    pub x_height: f32,
}

impl FontMetrics {
    /// Used when the font system can't answer.
    const FALLBACK: Self = Self {
        ascent: 0.95,
        descent: 0.25,
        x_height: 0.52,
    };

    /// Distance from the top of an iced text box of `font_size` to its
    /// baseline. The shaper centres the glyph extents in a box of
    /// `LINE_BOX_FACTOR × font_size`.
    pub fn baseline_in_box(&self, font_size: f32) -> f32 {
        font_size
            * ((super::metrics::LINE_BOX_FACTOR - self.ascent - self.descent) / 2.0 + self.ascent)
    }
}

/// Top of the iced text box that centres the glyph extents of text of
/// `font_size` in `font` in a row `row_height` tall, relative to the row's top.
pub(super) fn centered_text_top(row_height: f32, font_size: f32, font: iced::Font) -> f32 {
    let metrics = font_metrics(font);
    let content = (metrics.ascent + metrics.descent) * font_size;
    let baseline = (row_height - content) / 2.0 + metrics.ascent * font_size;
    baseline - metrics.baseline_in_box(font_size)
}

/// Vertical metrics of the face `font` resolves to, read from the font file
/// itself — the same numbers the shaper positions glyphs with.
pub(super) fn font_metrics(font: iced::Font) -> FontMetrics {
    static CACHE: OnceLock<Mutex<HashMap<iced::Font, FontMetrics>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(cache) = cache.lock()
        && let Some(metrics) = cache.get(&font)
    {
        return *metrics;
    }
    let metrics = read_font_metrics(font).unwrap_or(FontMetrics::FALLBACK);
    if let Ok(mut cache) = cache.lock() {
        cache.insert(font, metrics);
    }
    metrics
}

fn read_font_metrics(font: iced::Font) -> Option<FontMetrics> {
    use iced::advanced::graphics::text::{cosmic_text::fontdb, font_system, to_attributes};

    let mut system = font_system().write().ok()?;
    let raw = system.raw();
    let attrs = to_attributes(font);
    let id = raw.db().query(&fontdb::Query {
        families: &[attrs.family],
        weight: attrs.weight,
        stretch: attrs.stretch,
        style: attrs.style,
    })?;
    let face = raw.get_font(id, attrs.weight)?;
    let metrics = face.as_swash().metrics(&[]);
    let em = f32::from(metrics.units_per_em);
    (em > 0.0).then(|| FontMetrics {
        ascent: metrics.ascent / em,
        descent: metrics.descent / em,
        x_height: if metrics.x_height > 0.0 {
            metrics.x_height / em
        } else {
            FontMetrics::FALLBACK.x_height
        },
    })
}

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

/// How text in `font` is shaped. Prose gets the full shaper — kerning and
/// ligatures. Code stays on basic shaping so every character keeps its own
/// advance and columns line up exactly as typed.
pub(super) fn shaping_for(font: iced::Font) -> text::Shaping {
    if font.family == iced::font::Family::Monospace {
        text::Shaping::Basic
    } else {
        text::Shaping::Advanced
    }
}

fn paragraph<R: Measure>(content: &str, size: f32, font: iced::Font) -> R::Paragraph {
    use iced::advanced::text::Paragraph;
    R::Paragraph::with_text(text::Text {
        content,
        bounds: Size::new(f32::INFINITY, f32::INFINITY),
        size: size.into(),
        line_height: text::LineHeight::default(),
        font,
        align_x: iced::alignment::Horizontal::Left.into(),
        align_y: iced::alignment::Vertical::Top,
        shaping: shaping_for(font),
        wrapping: text::Wrapping::None,
    })
}

/// Shape `content` and return its advance width. This is the expensive call —
/// it builds a fresh paragraph and runs the text shaper — so everything goes
/// through the memoized [`measure_width`] instead of calling this directly.
fn shape_width<R: Measure>(content: &str, size: f32, font: iced::Font) -> f32 {
    use iced::advanced::text::Paragraph;
    paragraph::<R>(content, size, font).min_bounds().width
}

/// Above this many entries the word-offset cache is dropped wholesale.
const WORD_OFFSET_CACHE_CAP: usize = 16_384;

/// The x offset of every character boundary of `word` as shaped at `size` in
/// `font`: `offsets[i]` is where character `i` starts, and the last entry is
/// the word's full advance. Positions come from the shaper, so they include
/// kerning; characters inside one grapheme cluster share its start, so the
/// caret can't split a cluster. Memoized.
pub(super) fn word_offsets<R: Measure>(word: &str, size: f32, font: iced::Font) -> Arc<[f32]> {
    use iced::advanced::text::Paragraph;
    use unicode_segmentation::UnicodeSegmentation;

    static CACHE: OnceLock<Mutex<HashMap<SpanCacheKey, Arc<[f32]>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = SpanCacheKey {
        content: word.into(),
        font,
        size_bits: size.to_bits(),
    };
    if let Ok(cache) = cache.lock()
        && let Some(offsets) = cache.get(&key)
    {
        return offsets.clone();
    }

    let mut offsets = Vec::with_capacity(word.len() + 1);
    if !word.is_empty() {
        let shaped = paragraph::<R>(word, size, font);
        let width = shaped.min_bounds().width;
        let position = |grapheme: usize| {
            shaped
                .grapheme_position(0, grapheme)
                .map_or(0.0, |p| p.x)
                .clamp(0.0, width)
        };
        let mut last = 0.0_f32;
        for (grapheme, cluster) in word.graphemes(true).enumerate() {
            // Never step backwards, whatever the shaper reports.
            let start = position(grapheme).max(last);
            last = start;
            offsets.extend(std::iter::repeat_n(start, cluster.chars().count()));
        }
        offsets.push(width.max(last));
    } else {
        offsets.push(0.0);
    }
    let offsets: Arc<[f32]> = offsets.into();

    if let Ok(mut cache) = cache.lock() {
        if cache.len() >= WORD_OFFSET_CACHE_CAP {
            cache.clear();
        }
        cache.insert(key, offsets.clone());
    }
    offsets
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

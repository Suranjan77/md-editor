//! Fonts for slide text: which installed face stands in for the typeface a
//! slide names, and how wide text set in it is.
//!
//! Slides name the fonts of the machine they were made on — Arial, Calibri,
//! Consolas. When one is installed it is used as is. Otherwise a
//! metric-compatible substitute is preferred (Liberation Sans has Arial's
//! widths, Carlito has Calibri's), so lines break where they did in
//! PowerPoint; failing that, the generic serif, sans-serif or monospace face.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use iced::advanced::graphics::text::{Paragraph, cosmic_text::fontdb, font_system, to_attributes};
use iced::advanced::text::{self, Paragraph as _};
use iced::font::{Family, Stretch, Style, Weight};
use iced::{Font, Pixels, Size};

use super::text::TextMetrics;

/// Text is measured at this size and scaled: the shaper does no hinting, so
/// widths are linear in size, and one cached width serves every size.
const REFERENCE_SIZE: f32 = 100.0;
/// Bound on the width cache. A deck has at most a few thousand distinct
/// words; this only guards against a pathological one.
const WIDTH_CACHE_LIMIT: usize = 200_000;
/// Ascent and descent when a face's metrics cannot be read.
const FALLBACK_VERTICAL: (f32, f32) = (0.9, 0.22);

/// Words in a typeface name that select a width or weight rather than the
/// family: "Arial Narrow" is Arial, condensed.
const STYLE_WORDS: [&str; 10] = [
    "narrow",
    "condensed",
    "cond",
    "light",
    "semibold",
    "demibold",
    "black",
    "heavy",
    "medium",
    "bold",
];

/// Measures with the same shaper the canvas draws with.
pub struct Shaper;

impl TextMetrics for Shaper {
    fn font(&self, typeface: &str, bold: bool, italic: bool) -> Font {
        resolve(typeface, bold, italic)
    }

    fn width(&self, text: &str, font: Font, size: f32) -> f32 {
        let body = text.trim_end_matches(' ');
        let trailing = (text.len() - body.len()) as f32;
        let body_width = if body.is_empty() {
            0.0
        } else {
            reference_width(body, font)
        };
        (body_width + trailing * space_width(font)) * size / REFERENCE_SIZE
    }

    fn vertical(&self, font: Font) -> (f32, f32) {
        vertical_metrics(font)
    }
}

/// Resolved fonts, keyed by typeface name, bold and italic.
type FontCache = Mutex<HashMap<(String, bool, bool), Font>>;

/// The iced font to set a run named `typeface` in.
pub fn resolve(typeface: &str, bold: bool, italic: bool) -> Font {
    static CACHE: OnceLock<FontCache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = (typeface.to_string(), bold, italic);
    if let Ok(cache) = cache.lock()
        && let Some(font) = cache.get(&key)
    {
        return *font;
    }

    let lower = typeface.trim().to_lowercase();
    let has_word = |words: &[&str]| lower.split_whitespace().any(|part| words.contains(&part));
    let font = Font {
        family: family_for(&lower),
        weight: if bold || has_word(&["bold"]) {
            Weight::Bold
        } else if has_word(&["black", "heavy"]) {
            Weight::Black
        } else if has_word(&["semibold", "demibold"]) {
            Weight::Semibold
        } else if has_word(&["light"]) {
            Weight::Light
        } else if has_word(&["medium"]) {
            Weight::Medium
        } else {
            Weight::Normal
        },
        stretch: if has_word(&["narrow", "condensed", "cond"]) {
            Stretch::Condensed
        } else {
            Stretch::Normal
        },
        style: if italic { Style::Italic } else { Style::Normal },
    };
    if let Ok(mut cache) = cache.lock() {
        cache.insert(key, font);
    }
    font
}

fn family_for(lower: &str) -> Family {
    let installed = installed_families();
    let find = |name: &str| {
        installed
            .get(name)
            .or_else(|| substitutes(name).iter().find_map(|s| installed.get(*s)))
            .map(|name| Family::Name(name))
    };
    if let Some(family) = find(lower) {
        return family;
    }
    let base = lower
        .split_whitespace()
        .filter(|word| !STYLE_WORDS.contains(word))
        .collect::<Vec<_>>()
        .join(" ");
    if base != lower
        && let Some(family) = find(&base)
    {
        return family;
    }
    generic_family(&base)
}

/// Installed faces with the same metrics as a common Office font, best first.
fn substitutes(name: &str) -> &'static [&'static str] {
    match name {
        "arial narrow" => &["liberation sans narrow"],
        "arial" | "helvetica" | "helvetica neue" | "arial unicode ms" => {
            &["liberation sans", "arimo", "nimbus sans", "nimbus sans l"]
        }
        "times new roman" | "times" => &["liberation serif", "tinos", "nimbus roman"],
        "courier new" | "courier" => &["liberation mono", "cousine", "nimbus mono ps"],
        "calibri" | "calibri light" => &["carlito"],
        "cambria" => &["caladea"],
        "georgia" => &["gelasio"],
        "consolas" | "lucida console" | "menlo" | "monaco" => {
            &["dejavu sans mono", "liberation mono"]
        }
        _ => &[],
    }
}

fn generic_family(name: &str) -> Family {
    const MONOSPACE: [&str; 7] = [
        "mono", "consolas", "courier", "code", "console", "menlo", "monaco",
    ];
    const SERIF: [&str; 10] = [
        "times",
        "georgia",
        "garamond",
        "cambria",
        "palatino",
        "antiqua",
        "baskerville",
        "constantia",
        "bodoni",
        "serif",
    ];
    if MONOSPACE.iter().any(|word| name.contains(word)) {
        Family::Monospace
    } else if SERIF.iter().any(|word| name.contains(word)) && !name.contains("sans") {
        Family::Serif
    } else {
        Family::SansSerif
    }
}

/// Every family the font system can load, keyed by lowercased name.
fn installed_families() -> &'static HashMap<String, &'static str> {
    static FAMILIES: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
    FAMILIES.get_or_init(|| {
        let mut families = HashMap::new();
        let Ok(mut system) = font_system().write() else {
            return families;
        };
        for face in system.raw().db().faces() {
            for (name, _) in &face.families {
                // Leaked once per installed family: iced names fonts with
                // `&'static str`, and the set of families is fixed.
                families
                    .entry(name.to_lowercase())
                    .or_insert_with(|| &*Box::leak(name.clone().into_boxed_str()));
            }
        }
        families
    })
}

fn reference_width(content: &str, font: Font) -> f32 {
    static CACHE: OnceLock<Mutex<HashMap<(String, Font), f32>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = (content.to_string(), font);
    if let Ok(cache) = cache.lock()
        && let Some(width) = cache.get(&key)
    {
        return *width;
    }
    let paragraph = Paragraph::with_text(text::Text {
        content,
        bounds: Size::new(f32::INFINITY, f32::INFINITY),
        size: Pixels(REFERENCE_SIZE),
        line_height: text::LineHeight::Relative(1.0),
        font,
        align_x: text::Alignment::Left,
        align_y: iced::alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::None,
    });
    let width = paragraph.min_width();
    if let Ok(mut cache) = cache.lock() {
        if cache.len() >= WIDTH_CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, width);
    }
    width
}

/// Width of one space at the reference size. Shapers drop trailing
/// whitespace from a line's width, so it is measured between two letters.
fn space_width(font: Font) -> f32 {
    reference_width("n n", font) - reference_width("nn", font)
}

fn vertical_metrics(font: Font) -> (f32, f32) {
    static CACHE: OnceLock<Mutex<HashMap<Font, (f32, f32)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Ok(cache) = cache.lock()
        && let Some(metrics) = cache.get(&font)
    {
        return *metrics;
    }
    let metrics = read_vertical_metrics(font).unwrap_or(FALLBACK_VERTICAL);
    if let Ok(mut cache) = cache.lock() {
        cache.insert(font, metrics);
    }
    metrics
}

fn read_vertical_metrics(font: Font) -> Option<(f32, f32)> {
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
    (em > 0.0).then(|| (metrics.ascent / em, metrics.descent / em))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typeface_names_pick_width_weight_and_generic_family() {
        let narrow = resolve("Some Uninstalled Narrow", false, false);
        assert_eq!(narrow.stretch, Stretch::Condensed);
        assert_eq!(resolve("Anything", true, true).weight, Weight::Bold);
        assert_eq!(resolve("Anything", true, true).style, Style::Italic);
        assert_eq!(generic_family("consolas"), Family::Monospace);
        assert_eq!(generic_family("georgia"), Family::Serif);
        assert_eq!(generic_family("pt sans"), Family::SansSerif);
    }
}

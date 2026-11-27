//! Dual-mode `.ink.svg` vector export and lossless stroke metadata round-trip import.

use std::fmt::Write as _;

use iced::Color;

use super::document::{InkDocument, StrokeKind};
use super::stroke::{self, StrokeOptions, Vec2};

#[cfg(test)]
mod tests;

/// ID of the `<metadata>` element carrying lossless `InkDocument` JSON inside `.ink.svg`.
pub const INK_METADATA_ID: &str = "md-editor-ink";

const MARGIN: f32 = 16.0;
const EMPTY_VIEWBOX: (f32, f32, f32, f32) = (0.0, 0.0, 800.0, 600.0);

/// Render `doc` as a standalone light/dark-adaptive SVG with embedded lossless
/// `InkDocument` JSON in `<metadata id="md-editor-ink">`.
pub fn to_svg(doc: &InkDocument) -> Result<String, String> {
    let json = doc.to_json()?;
    let outlines: Vec<(Vec<Vec2>, [f32; 4], StrokeKind)> = doc
        .strokes()
        .iter()
        .map(|s| {
            let options = StrokeOptions {
                size: s.size,
                ..super::options_for(s.kind, StrokeOptions::default())
            };
            (stroke::outline(&s.ink_points(), &options), s.color, s.kind)
        })
        .filter(|(outline, ..)| outline.len() >= 3)
        .collect();

    let (vx, vy, vw, vh) = outlines
        .iter()
        .filter_map(|(outline, ..)| stroke::bounds(outline))
        .reduce(|(a0, a1), (b0, b1)| {
            (
                Vec2::new(a0.x.min(b0.x), a0.y.min(b0.y)),
                Vec2::new(a1.x.max(b1.x), a1.y.max(b1.y)),
            )
        })
        .map(|(min, max)| {
            (
                min.x - MARGIN,
                min.y - MARGIN,
                (max.x - min.x + 2.0 * MARGIN).max(1.0),
                (max.y - min.y + 2.0 * MARGIN).max(1.0),
            )
        })
        .unwrap_or(EMPTY_VIEWBOX);

    let mut out = String::with_capacity(1024);
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{vx:.2} {vy:.2} {vw:.2} {vh:.2}\" width=\"{vw:.0}\" height=\"{vh:.0}\">"
    );
    out.push_str("  <style>\n");
    out.push_str("    .ink-neutral { fill: #18181b; }\n");
    out.push_str("    @media (prefers-color-scheme: dark) { .ink-neutral { fill: #e3e6ed; } }\n");
    out.push_str("  </style>\n");
    out.push_str("  <metadata id=\"md-editor-ink\">");
    push_xml_escaped(&mut out, &json);
    out.push_str("</metadata>\n");
    out.push_str("  <g id=\"ink-strokes\">\n");

    for pass in [StrokeKind::Highlighter, StrokeKind::Pen] {
        for (outline, color, _) in outlines.iter().filter(|(.., kind)| *kind == pass) {
            out.push_str("    <path ");
            if is_neutral_color(*color) {
                out.push_str("class=\"ink-neutral\" ");
            } else {
                let _ = write!(
                    out,
                    "fill=\"#{:02x}{:02x}{:02x}\" ",
                    channel(color[0]),
                    channel(color[1]),
                    channel(color[2])
                );
            }
            if color[3] < 0.999 {
                let _ = write!(out, "fill-opacity=\"{:.3}\" ", color[3].clamp(0.0, 1.0));
            }
            out.push_str("d=\"");
            write_path_data(&mut out, outline);
            out.push_str("\"/>\n");
        }
    }

    out.push_str("  </g>\n");
    out.push_str("</svg>\n");
    Ok(out)
}

/// Recover an editable [`InkDocument`] from an `.ink.svg` file's `<metadata id="md-editor-ink">`.
pub fn from_svg(svg: &str) -> Result<InkDocument, String> {
    let raw_meta = extract_metadata_inner(svg, INK_METADATA_ID)
        .ok_or_else(|| "Not an md-editor .ink.svg file (missing ink metadata)".to_string())?;
    let trimmed = raw_meta.trim();
    let json = if let Some(inner) = trimmed
        .strip_prefix("<![CDATA[")
        .and_then(|s| s.strip_suffix("]]>"))
    {
        inner.to_string()
    } else {
        xml_unescape(trimmed)
    };
    InkDocument::from_json(json.trim())
}

fn is_neutral_color(c: [f32; 4]) -> bool {
    let color = Color::from_rgba(c[0], c[1], c[2], c[3]);
    super::same_color(color, super::COLOR_PRESETS[0].1)
        || super::same_color(color, crate::design::palette::INK_ON_LIGHT)
        || super::same_color(color, crate::design::palette::INK_ON_DARK)
}

fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn write_path_data(out: &mut String, outline: &[Vec2]) {
    if let Some((first, rest)) = outline.split_first() {
        let _ = write!(out, "M {:.2} {:.2}", first.x, first.y);
        for v in rest {
            let _ = write!(out, " L {:.2} {:.2}", v.x, v.y);
        }
        out.push_str(" Z");
    }
}

fn push_xml_escaped(out: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
}

fn xml_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp..];
        if let Some(tail) = after.strip_prefix("&amp;") {
            out.push('&');
            rest = tail;
        } else if let Some(tail) = after.strip_prefix("&lt;") {
            out.push('<');
            rest = tail;
        } else if let Some(tail) = after.strip_prefix("&gt;") {
            out.push('>');
            rest = tail;
        } else if let Some(tail) = after.strip_prefix("&quot;") {
            out.push('"');
            rest = tail;
        } else if let Some(tail) = after.strip_prefix("&apos;") {
            out.push('\'');
            rest = tail;
        } else {
            out.push('&');
            rest = &after[1..];
        }
    }
    out.push_str(rest);
    out
}

fn extract_metadata_inner<'a>(svg: &'a str, target_id: &str) -> Option<&'a str> {
    let mut search_from = 0usize;
    let dq = format!("id=\"{target_id}\"");
    let sq = format!("id='{target_id}'");
    while let Some(rel) = svg[search_from..].find("<metadata") {
        let tag_start = search_from + rel;
        let after_kw = tag_start + "<metadata".len();
        let next_byte = svg.as_bytes().get(after_kw).copied();
        if !matches!(next_byte, Some(b'>' | b' ' | b'\t' | b'\n' | b'\r')) {
            search_from = after_kw;
            continue;
        }
        let open_end = svg[tag_start..].find('>')? + tag_start;
        let attrs = &svg[tag_start..=open_end];
        if attrs.contains(&dq) || attrs.contains(&sq) {
            let body_start = open_end + 1;
            let body_end = svg[body_start..].find("</metadata>")? + body_start;
            return Some(&svg[body_start..body_end]);
        }
        search_from = open_end + 1;
    }
    None
}

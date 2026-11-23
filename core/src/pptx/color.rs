//! DrawingML colours: scheme/preset/sRGB bases and their modifiers.

use super::*;

// ── Colours ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ColorBase {
    Rgb(Rgba),
    Scheme(String),
    /// `phClr`: the colour a theme style is instantiated with.
    Placeholder,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum ColorMod {
    Alpha(f32),
    LumMod(f32),
    LumOff(f32),
    SatMod(f32),
    Tint(f32),
    Shade(f32),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ColorSpec {
    pub(super) base: ColorBase,
    pub(super) mods: Vec<ColorMod>,
}

/// Parse a colour element (`srgbClr`, `schemeClr`, …).
pub(super) fn parse_color(node: Node) -> Option<ColorSpec> {
    let base = match node.tag_name().name() {
        "srgbClr" => ColorBase::Rgb(Rgba::from_hex(node.attribute("val")?)?),
        "schemeClr" => match node.attribute("val")? {
            "phClr" => ColorBase::Placeholder,
            name => ColorBase::Scheme(name.to_string()),
        },
        "sysClr" => ColorBase::Rgb(
            node.attribute("lastClr")
                .and_then(Rgba::from_hex)
                .unwrap_or(match node.attribute("val") {
                    Some("window") => Rgba::WHITE,
                    _ => Rgba::BLACK,
                }),
        ),
        "prstClr" => ColorBase::Rgb(preset_color(node.attribute("val")?)),
        "scrgbClr" => {
            let channel = |name| {
                (fraction_attr(node, name).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u8
            };
            ColorBase::Rgb(Rgba::rgb(channel("r"), channel("g"), channel("b")))
        }
        "hslClr" => {
            let hue = attr_f32(node, "hue").unwrap_or(0.0) / 60_000.0;
            let sat = fraction_attr(node, "sat").unwrap_or(0.0);
            let lum = fraction_attr(node, "lum").unwrap_or(0.0);
            ColorBase::Rgb(from_hsl(hue, sat, lum, 255))
        }
        _ => return None,
    };
    let mods = node
        .children()
        .filter(Node::is_element)
        .filter_map(|m| {
            let value = fraction_attr(m, "val")?;
            Some(match m.tag_name().name() {
                "alpha" => ColorMod::Alpha(value),
                "lumMod" => ColorMod::LumMod(value),
                "lumOff" => ColorMod::LumOff(value),
                "satMod" => ColorMod::SatMod(value),
                "tint" => ColorMod::Tint(value),
                "shade" => ColorMod::Shade(value),
                _ => return None,
            })
        })
        .collect();
    Some(ColorSpec { base, mods })
}

/// The first colour element among `node`'s children.
pub(super) fn first_color(node: Node) -> Option<ColorSpec> {
    node.children()
        .filter(Node::is_element)
        .find_map(|n| parse_color(n))
}

pub(super) fn preset_color(name: &str) -> Rgba {
    match name {
        "white" => Rgba::WHITE,
        "red" => Rgba::rgb(255, 0, 0),
        "green" => Rgba::rgb(0, 128, 0),
        "lime" => Rgba::rgb(0, 255, 0),
        "blue" => Rgba::rgb(0, 0, 255),
        "yellow" => Rgba::rgb(255, 255, 0),
        "cyan" | "aqua" => Rgba::rgb(0, 255, 255),
        "magenta" | "fuchsia" => Rgba::rgb(255, 0, 255),
        "gray" | "grey" => Rgba::rgb(128, 128, 128),
        "silver" => Rgba::rgb(192, 192, 192),
        "orange" => Rgba::rgb(255, 165, 0),
        "navy" => Rgba::rgb(0, 0, 128),
        _ => Rgba::BLACK,
    }
}

pub(super) fn apply_mods(color: Rgba, mods: &[ColorMod]) -> Rgba {
    let mut color = color;
    for modifier in mods {
        match *modifier {
            ColorMod::Alpha(alpha) => color.a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
            ColorMod::Tint(tint) => {
                let toward_white = |c: u8| c as f32 + (255.0 - c as f32) * (1.0 - tint);
                color = rgb_f32(
                    toward_white(color.r),
                    toward_white(color.g),
                    toward_white(color.b),
                    color.a,
                );
            }
            ColorMod::Shade(shade) => {
                color = rgb_f32(
                    color.r as f32 * shade,
                    color.g as f32 * shade,
                    color.b as f32 * shade,
                    color.a,
                );
            }
            ColorMod::LumMod(_) | ColorMod::LumOff(_) | ColorMod::SatMod(_) => {
                let (h, mut s, mut l) = to_hsl(color);
                match *modifier {
                    ColorMod::LumMod(v) => l *= v,
                    ColorMod::LumOff(v) => l += v,
                    ColorMod::SatMod(v) => s *= v,
                    _ => {}
                }
                color = from_hsl(h, s.clamp(0.0, 1.0), l.clamp(0.0, 1.0), color.a);
            }
        }
    }
    color
}

pub(super) fn rgb_f32(r: f32, g: f32, b: f32, a: u8) -> Rgba {
    let channel = |c: f32| c.round().clamp(0.0, 255.0) as u8;
    Rgba {
        r: channel(r),
        g: channel(g),
        b: channel(b),
        a,
    }
}

/// Hue in degrees, saturation and lightness as fractions.
pub(super) fn to_hsl(color: Rgba) -> (f32, f32, f32) {
    let (r, g, b) = (
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f32::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

pub(super) fn from_hsl(hue: f32, s: f32, l: f32, a: u8) -> Rgba {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h = hue.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    rgb_f32((r + m) * 255.0, (g + m) * 255.0, (b + m) * 255.0, a)
}

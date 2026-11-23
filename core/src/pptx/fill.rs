//! Fill and line properties.

use super::*;

// ── Fills and lines ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub(super) enum FillSpec {
    None,
    Solid(ColorSpec),
    Gradient {
        stops: Vec<(f32, ColorSpec)>,
        angle: f32,
        radial: bool,
    },
    /// Relationship id of the image.
    Picture(String),
    /// `grpFill`: the enclosing group's fill, which is not tracked, so none.
    Group,
}

/// The fill described by a fill element itself (`solidFill`, `gradFill`, …).
pub(super) fn fill_spec(node: Node) -> Option<FillSpec> {
    Some(match node.tag_name().name() {
        "noFill" => FillSpec::None,
        "solidFill" => FillSpec::Solid(first_color(node)?),
        "gradFill" => {
            let stops = child(node, "gsLst")
                .into_iter()
                .flat_map(|list| children_named(list, "gs"))
                .filter_map(|stop| Some((fraction_attr(stop, "pos")?, first_color(stop)?)))
                .collect::<Vec<_>>();
            if stops.is_empty() {
                return None;
            }
            FillSpec::Gradient {
                stops,
                angle: child(node, "lin")
                    .and_then(|lin| attr_f32(lin, "ang"))
                    .map(|ang| ang / 60_000.0)
                    .unwrap_or(0.0),
                radial: child(node, "path").is_some(),
            }
        }
        // A pattern is drawn in its foreground colour: close enough to read.
        "pattFill" => FillSpec::Solid(child(node, "fgClr").and_then(first_color)?),
        "blipFill" => FillSpec::Picture(rel_attr(child(node, "blip")?, "embed")?.to_string()),
        "grpFill" => FillSpec::Group,
        _ => return None,
    })
}

/// The fill among `properties`'s children (`spPr`, `bgPr`, `tcPr`, `rPr`).
pub(super) fn find_fill(properties: Node) -> Option<FillSpec> {
    properties
        .children()
        .filter(Node::is_element)
        .find_map(|n| fill_spec(n))
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct LineSpec {
    pub(super) width: Option<f32>,
    pub(super) fill: Option<FillSpec>,
    pub(super) dash: Option<Dash>,
    pub(super) head_arrow: Option<bool>,
    pub(super) tail_arrow: Option<bool>,
}

impl LineSpec {
    pub(super) fn overlay(&mut self, other: &LineSpec) {
        overlay_option(&mut self.width, &other.width);
        overlay_option(&mut self.fill, &other.fill);
        overlay_option(&mut self.dash, &other.dash);
        overlay_option(&mut self.head_arrow, &other.head_arrow);
        overlay_option(&mut self.tail_arrow, &other.tail_arrow);
    }
}

pub(super) fn overlay_option<T: Clone>(base: &mut Option<T>, over: &Option<T>) {
    if over.is_some() {
        base.clone_from(over);
    }
}

pub(super) fn parse_line(ln: Node) -> LineSpec {
    let arrow = |name| {
        child(ln, name)
            .and_then(|end| end.attribute("type"))
            .map(|kind| kind != "none")
    };
    LineSpec {
        width: emu_attr(ln, "w"),
        fill: find_fill(ln),
        dash: child(ln, "prstDash")
            .and_then(|dash| dash.attribute("val"))
            .map(|val| match val {
                "dot" | "sysDot" => Dash::Dot,
                "dash" | "sysDash" => Dash::Dash,
                "lgDash" => Dash::LongDash,
                "dashDot" | "sysDashDot" | "lgDashDot" | "lgDashDotDot" | "sysDashDotDot" => {
                    Dash::DashDot
                }
                _ => Dash::Solid,
            }),
        head_arrow: arrow("headEnd"),
        tail_arrow: arrow("tailEnd"),
    }
}

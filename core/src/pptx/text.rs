//! Run, paragraph, list and body properties, and how they inherit.

use super::*;

// ── Text properties ──────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub(super) struct RunProps {
    pub(super) size: Option<f32>,
    pub(super) bold: Option<bool>,
    pub(super) italic: Option<bool>,
    pub(super) underline: Option<bool>,
    pub(super) strike: Option<bool>,
    pub(super) fill: Option<FillSpec>,
    pub(super) font: Option<String>,
    pub(super) baseline: Option<f32>,
    pub(super) all_caps: Option<bool>,
    pub(super) highlight: Option<ColorSpec>,
    pub(super) spacing: Option<f32>,
}

impl RunProps {
    pub(super) fn overlay(&mut self, other: &RunProps) {
        overlay_option(&mut self.size, &other.size);
        overlay_option(&mut self.bold, &other.bold);
        overlay_option(&mut self.italic, &other.italic);
        overlay_option(&mut self.underline, &other.underline);
        overlay_option(&mut self.strike, &other.strike);
        overlay_option(&mut self.fill, &other.fill);
        overlay_option(&mut self.font, &other.font);
        overlay_option(&mut self.baseline, &other.baseline);
        overlay_option(&mut self.all_caps, &other.all_caps);
        overlay_option(&mut self.highlight, &other.highlight);
        overlay_option(&mut self.spacing, &other.spacing);
    }
}

pub(super) fn parse_run_props(rpr: Node) -> RunProps {
    RunProps {
        size: attr_f32(rpr, "sz").map(|sz| sz / 100.0),
        bold: attr_bool(rpr, "b"),
        italic: attr_bool(rpr, "i"),
        underline: rpr.attribute("u").map(|u| u != "none"),
        strike: rpr.attribute("strike").map(|s| s != "noStrike"),
        fill: find_fill(rpr),
        font: child(rpr, "latin")
            .and_then(|latin| latin.attribute("typeface"))
            .map(str::to_string),
        baseline: fraction_attr(rpr, "baseline"),
        all_caps: rpr.attribute("cap").map(|cap| cap != "none"),
        highlight: child(rpr, "highlight").and_then(first_color),
        spacing: attr_f32(rpr, "spc").map(|spc| spc / 100.0),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum BulletKind {
    None,
    Char(String),
    AutoNumber { scheme: String, start: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum BulletSize {
    /// Relative to the first run's size.
    Relative(f32),
    Points(f32),
}

#[derive(Debug, Clone, Default)]
pub(super) struct ParaProps {
    pub(super) align: Option<Align>,
    pub(super) margin_left: Option<f32>,
    pub(super) indent: Option<f32>,
    pub(super) space_before: Option<Spacing>,
    pub(super) space_after: Option<Spacing>,
    pub(super) line_spacing: Option<Spacing>,
    pub(super) bullet: Option<BulletKind>,
    /// `Some(None)`: follow the text colour.
    pub(super) bullet_color: Option<Option<ColorSpec>>,
    pub(super) bullet_size: Option<BulletSize>,
    /// `Some(None)`: follow the text font.
    pub(super) bullet_font: Option<Option<String>>,
    pub(super) run: RunProps,
}

impl ParaProps {
    pub(super) fn overlay(&mut self, other: &ParaProps) {
        overlay_option(&mut self.align, &other.align);
        overlay_option(&mut self.margin_left, &other.margin_left);
        overlay_option(&mut self.indent, &other.indent);
        overlay_option(&mut self.space_before, &other.space_before);
        overlay_option(&mut self.space_after, &other.space_after);
        overlay_option(&mut self.line_spacing, &other.line_spacing);
        overlay_option(&mut self.bullet, &other.bullet);
        overlay_option(&mut self.bullet_color, &other.bullet_color);
        overlay_option(&mut self.bullet_size, &other.bullet_size);
        overlay_option(&mut self.bullet_font, &other.bullet_font);
        self.run.overlay(&other.run);
    }
}

pub(super) fn parse_spacing(node: Node) -> Option<Spacing> {
    if let Some(pct) = child(node, "spcPct") {
        return fraction_attr(pct, "val").map(Spacing::Lines);
    }
    child(node, "spcPts")
        .and_then(|pts| attr_f32(pts, "val"))
        .map(|val| Spacing::Points(val / 100.0))
}

pub(super) fn parse_para_props(ppr: Node) -> ParaProps {
    let mut props = ParaProps {
        align: ppr.attribute("algn").map(|algn| match algn {
            "ctr" => Align::Center,
            "r" => Align::Right,
            "just" | "dist" | "justLow" | "thaiDist" => Align::Justify,
            _ => Align::Left,
        }),
        margin_left: emu_attr(ppr, "marL"),
        indent: emu_attr(ppr, "indent"),
        space_before: child(ppr, "spcBef").and_then(parse_spacing),
        space_after: child(ppr, "spcAft").and_then(parse_spacing),
        line_spacing: child(ppr, "lnSpc").and_then(parse_spacing),
        run: child(ppr, "defRPr")
            .map(parse_run_props)
            .unwrap_or_default(),
        ..ParaProps::default()
    };
    for node in ppr.children().filter(Node::is_element) {
        match node.tag_name().name() {
            "buNone" => props.bullet = Some(BulletKind::None),
            "buChar" => {
                props.bullet = node
                    .attribute("char")
                    .map(|c| BulletKind::Char(c.to_string()))
            }
            "buAutoNum" => {
                props.bullet = Some(BulletKind::AutoNumber {
                    scheme: node.attribute("type").unwrap_or("arabicPeriod").to_string(),
                    start: attr_u32(node, "startAt").unwrap_or(1),
                })
            }
            // Picture bullets would need their image; a dot reads the same.
            "buBlip" => props.bullet = Some(BulletKind::Char("•".to_string())),
            "buClrTx" => props.bullet_color = Some(None),
            "buClr" => props.bullet_color = first_color(node).map(Some),
            "buSzTx" => props.bullet_size = Some(BulletSize::Relative(1.0)),
            "buSzPct" => props.bullet_size = fraction_attr(node, "val").map(BulletSize::Relative),
            "buSzPts" => {
                props.bullet_size = attr_f32(node, "val").map(|v| BulletSize::Points(v / 100.0))
            }
            "buFontTx" => props.bullet_font = Some(None),
            "buFont" => {
                props.bullet_font = node
                    .attribute("typeface")
                    .map(|face| Some(face.to_string()))
            }
            _ => {}
        }
    }
    props
}

/// Paragraph properties for outline levels 1–9.
#[derive(Debug, Clone, Default)]
pub(super) struct ListStyle {
    pub(super) levels: [ParaProps; 9],
}

impl ListStyle {
    pub(super) fn overlay(&mut self, other: &ListStyle) {
        for (level, over) in self.levels.iter_mut().zip(&other.levels) {
            level.overlay(over);
        }
    }
}

pub(super) fn parse_list_style(node: Node) -> ListStyle {
    let mut style = ListStyle::default();
    if let Some(default) = child(node, "defPPr").map(parse_para_props) {
        for level in &mut style.levels {
            level.overlay(&default);
        }
    }
    for (index, level) in style.levels.iter_mut().enumerate() {
        let name = format!("lvl{}pPr", index + 1);
        if let Some(props) = child(node, &name).map(parse_para_props) {
            level.overlay(&props);
        }
    }
    style
}

#[derive(Debug, Clone, Default)]
pub(super) struct TextStyles {
    pub(super) title: ListStyle,
    pub(super) body: ListStyle,
    pub(super) other: ListStyle,
}

pub(super) fn parse_text_styles(master: Node) -> TextStyles {
    let style = |name| {
        descend(master, &["txStyles", name])
            .map(parse_list_style)
            .unwrap_or_default()
    };
    TextStyles {
        title: style("titleStyle"),
        body: style("bodyStyle"),
        other: style("otherStyle"),
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct BodyProps {
    pub(super) insets: [Option<f32>; 4],
    pub(super) anchor: Option<Anchor>,
    pub(super) wrap: Option<bool>,
    /// Font scale and line-spacing reduction from `normAutofit`.
    pub(super) autofit: Option<(f32, f32)>,
}

impl BodyProps {
    pub(super) fn overlay(&mut self, other: &BodyProps) {
        for (inset, over) in self.insets.iter_mut().zip(&other.insets) {
            overlay_option(inset, over);
        }
        overlay_option(&mut self.anchor, &other.anchor);
        overlay_option(&mut self.wrap, &other.wrap);
        overlay_option(&mut self.autofit, &other.autofit);
    }
}

pub(super) fn parse_body_props(body: Node) -> BodyProps {
    let autofit = if let Some(norm) = child(body, "normAutofit") {
        Some((
            fraction_attr(norm, "fontScale").unwrap_or(1.0),
            fraction_attr(norm, "lnSpcReduction").unwrap_or(0.0),
        ))
    } else if child(body, "noAutofit").is_some() || child(body, "spAutoFit").is_some() {
        Some((1.0, 0.0))
    } else {
        None
    };
    BodyProps {
        insets: [
            emu_attr(body, "lIns"),
            emu_attr(body, "tIns"),
            emu_attr(body, "rIns"),
            emu_attr(body, "bIns"),
        ],
        anchor: body.attribute("anchor").map(parse_anchor),
        wrap: body.attribute("wrap").map(|wrap| wrap != "none"),
        autofit,
    }
}

pub(super) fn parse_anchor(value: &str) -> Anchor {
    match value {
        "ctr" => Anchor::Middle,
        "b" => Anchor::Bottom,
        _ => Anchor::Top,
    }
}

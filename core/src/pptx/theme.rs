//! The theme part: colour scheme, fonts and format styles.

use super::*;

// ── Theme ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(super) struct Theme {
    pub(super) colors: HashMap<String, Rgba>,
    pub(super) major_font: String,
    pub(super) minor_font: String,
    pub(super) fill_styles: Vec<FillSpec>,
    pub(super) background_styles: Vec<FillSpec>,
    pub(super) line_styles: Vec<LineSpec>,
}

impl Theme {
    /// The Office theme, for a deck whose master has none.
    pub(super) fn fallback() -> Self {
        let colors = [
            ("dk1", "000000"),
            ("lt1", "FFFFFF"),
            ("dk2", "44546A"),
            ("lt2", "E7E6E6"),
            ("accent1", "4472C4"),
            ("accent2", "ED7D31"),
            ("accent3", "A5A5A5"),
            ("accent4", "FFC000"),
            ("accent5", "5B9BD5"),
            ("accent6", "70AD47"),
            ("hlink", "0563C1"),
            ("folHlink", "954F72"),
        ]
        .into_iter()
        .filter_map(|(name, hex)| Some((name.to_string(), Rgba::from_hex(hex)?)))
        .collect();
        let placeholder = ColorSpec {
            base: ColorBase::Placeholder,
            mods: Vec::new(),
        };
        Self {
            colors,
            major_font: "Calibri Light".to_string(),
            minor_font: "Calibri".to_string(),
            fill_styles: vec![FillSpec::Solid(placeholder.clone()); 3],
            background_styles: vec![FillSpec::Solid(placeholder.clone()); 3],
            line_styles: [0.75, 1.5, 2.25]
                .into_iter()
                .map(|width| LineSpec {
                    width: Some(width),
                    fill: Some(FillSpec::Solid(placeholder.clone())),
                    ..LineSpec::default()
                })
                .collect(),
        }
    }
}

pub(super) fn parse_theme(root: Node) -> Theme {
    let mut theme = Theme::fallback();
    let Some(elements) = child(root, "themeElements") else {
        return theme;
    };
    if let Some(scheme) = child(elements, "clrScheme") {
        for entry in scheme.children().filter(Node::is_element) {
            if let Some(ColorSpec {
                base: ColorBase::Rgb(color),
                mods,
            }) = first_color(entry)
            {
                theme.colors.insert(
                    entry.tag_name().name().to_string(),
                    apply_mods(color, &mods),
                );
            }
        }
    }
    if let Some(fonts) = child(elements, "fontScheme") {
        let typeface = |which| {
            descend(fonts, &[which, "latin"])
                .and_then(|latin| latin.attribute("typeface"))
                .filter(|name| !name.is_empty())
                .map(str::to_string)
        };
        if let Some(major) = typeface("majorFont") {
            theme.major_font = major;
        }
        if let Some(minor) = typeface("minorFont") {
            theme.minor_font = minor;
        }
    }
    if let Some(formats) = child(elements, "fmtScheme") {
        let fills = |list| -> Option<Vec<FillSpec>> {
            let styles: Vec<_> = child(formats, list)?
                .children()
                .filter(Node::is_element)
                .filter_map(|n| fill_spec(n))
                .collect();
            (!styles.is_empty()).then_some(styles)
        };
        if let Some(styles) = fills("fillStyleLst") {
            theme.fill_styles = styles;
        }
        if let Some(styles) = fills("bgFillStyleLst") {
            theme.background_styles = styles;
        }
        if let Some(list) = child(formats, "lnStyleLst") {
            let styles: Vec<_> = children_named(list, "ln").map(parse_line).collect();
            if !styles.is_empty() {
                theme.line_styles = styles;
            }
        }
    }
    theme
}

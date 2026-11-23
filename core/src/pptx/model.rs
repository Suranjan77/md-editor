//! The resolved presentation model the viewer draws: slides, elements, fills, text. All lengths in points.

use super::*;

// ── Model ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);
    pub const WHITE: Rgba = Rgba::rgb(255, 255, 255);
    pub const TRANSPARENT: Rgba = Rgba {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub(super) fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim();
        if hex.len() != 6 {
            return None;
        }
        let value = u32::from_str_radix(hex, 16).ok()?;
        Some(Self::rgb(
            (value >> 16) as u8,
            (value >> 8) as u8,
            value as u8,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone)]
pub struct Presentation {
    /// Slide width in points.
    pub width: f32,
    /// Slide height in points.
    pub height: f32,
    pub slides: Vec<Slide>,
    /// The bytes of every image the slides reference, keyed by package part
    /// name — the value [`Picture::media`] and [`Fill::Picture`] carry.
    pub media: HashMap<String, Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct Slide {
    /// One-based position in the deck, as PowerPoint numbers slides.
    pub number: usize,
    /// Skipped in a slide show, but still part of the file.
    pub hidden: bool,
    /// Text of the slide's title placeholder, when it has one.
    pub title: Option<String>,
    pub background: Fill,
    /// Back to front: the master's shapes, the layout's, then the slide's own.
    pub elements: Vec<Element>,
}

#[derive(Debug, Clone)]
pub struct Element {
    /// Unrotated bounds on the slide.
    pub bounds: Rect,
    /// Clockwise rotation about the centre of `bounds`, in degrees.
    pub rotation: f32,
    pub flip_h: bool,
    pub flip_v: bool,
    pub kind: ElementKind,
}

#[derive(Debug, Clone)]
pub enum ElementKind {
    Shape(Shape),
    Picture(Picture),
    Table(Table),
    /// Content the viewer does not draw — a chart, a SmartArt graphic, an
    /// embedded object — shown as a labelled box so the gap is visible.
    Unsupported {
        label: String,
    },
}

#[derive(Debug, Clone)]
pub struct Shape {
    /// Outline, relative to the top-left corner of the element's bounds.
    pub paths: Vec<ShapePath>,
    pub fill: Fill,
    pub line: Option<Line>,
    pub text: Option<TextBody>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShapePath {
    pub commands: Vec<PathCommand>,
    pub filled: bool,
    pub stroked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathCommand {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    /// Two control points, then the end point.
    CubicTo(f32, f32, f32, f32, f32, f32),
    /// One control point, then the end point.
    QuadTo(f32, f32, f32, f32),
    Close,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Fill {
    None,
    Solid(Rgba),
    Gradient(Gradient),
    Picture { media: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    pub stops: Vec<GradientStop>,
    /// Direction the colours change along, clockwise from +x, in degrees.
    pub angle: f32,
    /// Radiates from the centre instead of running along `angle`.
    pub radial: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    /// Position along the gradient, 0 to 1.
    pub offset: f32,
    pub color: Rgba,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub color: Rgba,
    pub width: f32,
    pub dash: Dash,
    pub head_arrow: bool,
    pub tail_arrow: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dash {
    #[default]
    Solid,
    Dot,
    Dash,
    LongDash,
    DashDot,
}

#[derive(Debug, Clone)]
pub struct Picture {
    pub media: String,
    pub crop: Crop,
    pub line: Option<Line>,
}

/// Fractions of the source image cut away from each edge.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Crop {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

#[derive(Debug, Clone)]
pub struct Table {
    pub columns: Vec<f32>,
    pub rows: Vec<TableRow>,
}

#[derive(Debug, Clone)]
pub struct TableRow {
    /// Minimum height: a row grows to fit its text, as it does in PowerPoint.
    pub height: f32,
    pub cells: Vec<TableCell>,
}

#[derive(Debug, Clone)]
pub struct TableCell {
    pub text: Option<TextBody>,
    pub fill: Fill,
    pub borders: Borders,
    /// Columns this cell covers; 1 for an ordinary cell.
    pub column_span: usize,
    /// Rows this cell covers; 1 for an ordinary cell.
    pub row_span: usize,
    /// Covered by a neighbour's span, so it draws nothing of its own.
    pub merged: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Borders {
    pub left: Option<Line>,
    pub top: Option<Line>,
    pub right: Option<Line>,
    pub bottom: Option<Line>,
}

#[derive(Debug, Clone)]
pub struct TextBody {
    pub insets: Insets,
    pub anchor: Anchor,
    /// Whether lines wrap at the box's width.
    pub wrap: bool,
    pub paragraphs: Vec<Paragraph>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Insets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Anchor {
    #[default]
    Top,
    Middle,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spacing {
    /// A multiple of the line's natural height: `Lines(1.0)` is single spacing.
    Lines(f32),
    Points(f32),
}

#[derive(Debug, Clone)]
pub struct Paragraph {
    pub align: Align,
    /// Left edge of every line after the first, from the body's left inset.
    pub margin_left: f32,
    /// Offset of the first line from `margin_left`; negative hangs a bullet.
    pub indent: f32,
    pub space_before: Spacing,
    pub space_after: Spacing,
    pub line_spacing: Spacing,
    pub bullet: Option<Bullet>,
    /// Text runs. A run whose text is `"\n"` is a line break.
    pub runs: Vec<Run>,
    /// Size of the paragraph mark, which sets an empty paragraph's height.
    pub end_size: f32,
}

#[derive(Debug, Clone)]
pub struct Bullet {
    pub text: String,
    pub color: Rgba,
    pub size: f32,
    pub font: String,
}

#[derive(Debug, Clone)]
pub struct Run {
    pub text: String,
    pub style: RunStyle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunStyle {
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Rgba,
    /// Typeface as the file names it, with theme fonts resolved.
    pub font: String,
    /// Vertical shift as a fraction of the font size; positive raises.
    pub baseline: f32,
    pub highlight: Option<Rgba>,
    /// Extra space after each character, in points.
    pub spacing: f32,
}

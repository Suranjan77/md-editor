//! Read-only PowerPoint (`.pptx`) presentations.
//!
//! A `.pptx` file is a zip package of XML parts. This module reads one into a
//! [`Presentation`]: slides as lists of positioned elements whose geometry,
//! fills, lines and text styles are already *resolved*, so the native viewer
//! can draw them without knowing anything about the format.
//!
//! Resolution is most of the work. A shape rarely says how it looks: a title
//! placeholder takes its position from the slide layout, its size from the
//! slide master, its colour from the theme through the master's colour map,
//! and its alignment from the master's title style. The chain followed here,
//! lowest priority first, is:
//!
//! presentation `defaultTextStyle` → master `txStyles` → master placeholder →
//! layout placeholder → the shape's `style` font reference → the shape's own
//! `lstStyle` → paragraph and run properties.
//!
//! What is not modelled degrades instead of failing: charts, SmartArt and
//! embedded objects become [`ElementKind::Unsupported`] boxes, unknown preset
//! geometries draw as rectangles, and a slide whose XML cannot be read becomes
//! a slide saying so. Presentations are reference material in the same sense
//! PDFs are, so nothing here ever writes to the file.
//!
//! Every length in the model is in points (1/72 inch); the XML's English
//! Metric Units are converted on the way in.

use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use roxmltree::{Document, Node};

/// English Metric Units per point.
const EMU_PER_POINT: f64 = 12_700.0;
/// Largest single part — XML or media — read out of a package. The declared
/// size in the zip directory is checked first, and inflation is bounded too,
/// because a hostile archive can lie about it.
const MAX_PART_BYTES: u64 = 64 * 1024 * 1024;
/// Largest total of inflated bytes read from one package.
const MAX_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_SLIDES: usize = 5_000;
/// Groups nest; a malicious file could nest them until the stack overflows.
const MAX_TREE_DEPTH: usize = 64;
/// Font size when nothing in the style chain sets one.
const DEFAULT_FONT_SIZE: f32 = 18.0;
/// Line width when a line is drawn but its width is not given: 0.75pt.
const DEFAULT_LINE_WIDTH: f32 = 0.75;
/// Magic number of an OLE compound file. A `.pptx` saved with a password is
/// wrapped in one, and so is every legacy binary `.ppt`.
const COMPOUND_FILE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

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

    fn from_hex(hex: &str) -> Option<Self> {
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

// ── Entry points ─────────────────────────────────────────────────────

/// Read the presentation at `path`.
pub fn load_presentation(path: &Path) -> Result<Presentation, String> {
    let file =
        std::fs::File::open(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    parse_presentation(BufReader::new(file))
}

/// Read a presentation from any seekable byte source.
pub fn parse_presentation<R: Read + Seek>(mut reader: R) -> Result<Presentation, String> {
    let mut magic = [0u8; 8];
    if reader.read_exact(&mut magic).is_ok() && magic == COMPOUND_FILE_MAGIC {
        return Err(
            "This presentation is password-protected or in the old .ppt format, \
             neither of which can be opened"
                .to_string(),
        );
    }
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|e| format!("Cannot read the presentation: {e}"))?;
    let mut package = Package::open(reader)?;
    read_presentation(&mut package)
}

fn read_presentation<R: Read + Seek>(package: &mut Package<R>) -> Result<Presentation, String> {
    let root_rels = read_relationships(package, "")?;
    let main_part = root_rels
        .values()
        .find(|rel| rel.kind == "officeDocument" && !rel.external)
        .map(|rel| rel.target.clone())
        .ok_or("Not a PowerPoint presentation: the file has no main document")?;
    let presentation_xml = package
        .read_text(&main_part)?
        .ok_or("Not a PowerPoint presentation: its main document is missing")?;
    let presentation_doc = parse_xml(&presentation_xml, &main_part)?;
    let presentation = presentation_doc.root_element();
    if presentation.tag_name().name() != "presentation" {
        return Err("Not a PowerPoint presentation (is it a Word or Excel file?)".to_string());
    }
    let presentation_rels = read_relationships(package, &main_part)?;

    let (width, height) = child(presentation, "sldSz")
        .map(|size| {
            (
                emu_attr(size, "cx").unwrap_or(720.0),
                emu_attr(size, "cy").unwrap_or(540.0),
            )
        })
        .unwrap_or((720.0, 540.0));
    let default_style = child(presentation, "defaultTextStyle")
        .map(parse_list_style)
        .unwrap_or_default();
    let default_table_style = presentation_rels
        .values()
        .find(|rel| rel.kind == "tableStyles" && !rel.external)
        .and_then(|rel| package.read_text(&rel.target).ok().flatten())
        .and_then(|xml| {
            let doc = Document::parse(xml.trim_start_matches('\u{feff}')).ok()?;
            doc.root_element().attribute("def").map(str::to_string)
        })
        .unwrap_or_default();

    let slide_parts: Vec<String> = child(presentation, "sldIdLst")
        .into_iter()
        .flat_map(|list| children_named(list, "sldId"))
        .filter_map(|id| presentation_rels.get(rel_attr(id, "id")?))
        .filter(|rel| rel.kind == "slide" && !rel.external)
        .map(|rel| rel.target.clone())
        .take(MAX_SLIDES)
        .collect();

    // Discover the layouts, masters and themes the slides use, then read
    // every part once. Documents borrow their text, so all text is loaded
    // before any document is parsed.
    let mut rels: HashMap<String, Relationships> = HashMap::new();
    let mut theme_parts = HashSet::new();
    for slide in &slide_parts {
        let slide_rels = read_relationships(package, slide)?;
        if let Some(layout) = target_of(&slide_rels, "slideLayout")
            && !rels.contains_key(&layout)
        {
            let layout_rels = read_relationships(package, &layout)?;
            if let Some(master) = target_of(&layout_rels, "slideMaster")
                && !rels.contains_key(&master)
            {
                let master_rels = read_relationships(package, &master)?;
                if let Some(theme) = target_of(&master_rels, "theme") {
                    theme_parts.insert(theme);
                }
                rels.insert(master, master_rels);
            }
            rels.insert(layout, layout_rels);
        }
        rels.insert(slide.clone(), slide_rels);
    }

    let mut texts: HashMap<String, String> = HashMap::new();
    for part in rels.keys().chain(theme_parts.iter()) {
        if let Some(text) = package.read_text(part)? {
            texts.insert(part.clone(), text);
        }
    }
    // A part that is not well-formed XML is left out; whatever needed it
    // degrades (a slide says it could not be read, a layout is ignored).
    let docs: HashMap<String, Document> = texts
        .iter()
        .filter_map(|(part, text)| Some((part.clone(), parse_xml(text, part).ok()?)))
        .collect();

    let fallback_theme = Theme::fallback();
    let mut themes: HashMap<String, Theme> = HashMap::new();
    let mut master_styles: HashMap<String, TextStyles> = HashMap::new();
    for (part, part_rels) in &rels {
        let Some(root) = docs.get(part).map(Document::root_element) else {
            continue;
        };
        if root.tag_name().name() != "sldMaster" {
            continue;
        }
        if let Some(theme_root) = target_of(part_rels, "theme")
            .and_then(|theme| docs.get(&theme))
            .map(Document::root_element)
        {
            themes.insert(part.clone(), parse_theme(theme_root));
        }
        master_styles.insert(part.clone(), parse_text_styles(root));
    }

    let mut media_parts = HashSet::new();
    let mut slides = Vec::with_capacity(slide_parts.len());
    for (index, part) in slide_parts.iter().enumerate() {
        let number = index + 1;
        let (Some(doc), Some(slide_rels)) = (docs.get(part), rels.get(part)) else {
            slides.push(unreadable_slide(number, width, height));
            continue;
        };
        let layout_part = target_of(slide_rels, "slideLayout");
        let master_part = layout_part
            .as_ref()
            .and_then(|layout| rels.get(layout))
            .and_then(|layout_rels| target_of(layout_rels, "slideMaster"));
        let builder = SlideBuilder {
            theme: master_part
                .as_ref()
                .and_then(|master| themes.get(master))
                .unwrap_or(&fallback_theme),
            default_style: &default_style,
            text_styles: master_part.as_ref().and_then(|m| master_styles.get(m)),
            master: template(master_part.as_deref(), &docs, &rels),
            layout: template(layout_part.as_deref(), &docs, &rels),
            default_table_style: &default_table_style,
            color_map: HashMap::new(),
            number,
            title: None,
            media: &mut media_parts,
        };
        slides.push(builder.build(doc.root_element(), slide_rels));
    }

    // An image that cannot be read (too large, corrupt entry) is dropped; the
    // viewer draws its frame empty rather than refusing the whole deck.
    let mut media = HashMap::new();
    for part in media_parts {
        if let Ok(Some(bytes)) = package.read(&part) {
            media.insert(part, bytes);
        }
    }

    Ok(Presentation {
        width,
        height,
        slides,
        media,
    })
}

fn unreadable_slide(number: usize, width: f32, height: f32) -> Slide {
    Slide {
        number,
        hidden: false,
        title: None,
        background: Fill::Solid(Rgba::WHITE),
        elements: vec![Element {
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width,
                height,
            },
            rotation: 0.0,
            flip_h: false,
            flip_v: false,
            kind: ElementKind::Unsupported {
                label: "This slide could not be read".to_string(),
            },
        }],
    }
}

// ── Package and relationships ────────────────────────────────────────

struct Package<R> {
    archive: zip::ZipArchive<R>,
    bytes_read: u64,
}

impl<R: Read + Seek> Package<R> {
    fn open(reader: R) -> Result<Self, String> {
        let archive = zip::ZipArchive::new(reader)
            .map_err(|_| "Not a valid .pptx file: it is not a zip package".to_string())?;
        Ok(Self {
            archive,
            bytes_read: 0,
        })
    }

    /// The bytes of `part`, or `None` when the package has no such part.
    fn read(&mut self, part: &str) -> Result<Option<Vec<u8>>, String> {
        let file = match self.archive.by_name(part) {
            Ok(file) => file,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(format!("Cannot read {part}: {e}")),
        };
        if file.size() > MAX_PART_BYTES {
            return Err(format!("{part} is too large to open"));
        }
        let mut bytes = Vec::with_capacity(file.size() as usize);
        file.take(MAX_PART_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Cannot read {part}: {e}"))?;
        if bytes.len() as u64 > MAX_PART_BYTES {
            return Err(format!("{part} is too large to open"));
        }
        self.bytes_read += bytes.len() as u64;
        if self.bytes_read > MAX_PACKAGE_BYTES {
            return Err("The presentation is too large to open".to_string());
        }
        Ok(Some(bytes))
    }

    fn read_text(&mut self, part: &str) -> Result<Option<String>, String> {
        match self.read(part)? {
            Some(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| format!("{part} is not UTF-8 XML")),
            None => Ok(None),
        }
    }
}

#[derive(Debug, Clone)]
struct Relationship {
    /// Last segment of the relationship type URI: `slide`, `image`, `theme`…
    kind: String,
    /// Package part name for internal targets; the raw target otherwise.
    target: String,
    external: bool,
}

type Relationships = HashMap<String, Relationship>;

fn rels_part(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// Resolve a relationship target against the part that holds it, yielding a
/// package part name without a leading slash.
fn resolve_target(source_part: &str, target: &str) -> String {
    let target = percent_decode(&target.replace('\\', "/"));
    let mut segments: Vec<&str> = Vec::new();
    let relative = match target.strip_prefix('/') {
        Some(absolute) => absolute,
        None => {
            if let Some((dir, _)) = source_part.rsplit_once('/') {
                segments.extend(dir.split('/'));
            }
            target.as_str()
        }
    };
    for segment in relative.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(value) = u8::from_str_radix(&text[i + 1..i + 3], 16)
        {
            out.push(value);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

fn read_relationships<R: Read + Seek>(
    package: &mut Package<R>,
    part: &str,
) -> Result<Relationships, String> {
    let rels_name = rels_part(part);
    let Some(xml) = package.read_text(&rels_name)? else {
        return Ok(HashMap::new());
    };
    let Ok(doc) = parse_xml(&xml, &rels_name) else {
        return Ok(HashMap::new());
    };
    let mut rels = HashMap::new();
    for rel in children_named(doc.root_element(), "Relationship") {
        let (Some(id), Some(kind), Some(target)) = (
            rel.attribute("Id"),
            rel.attribute("Type"),
            rel.attribute("Target"),
        ) else {
            continue;
        };
        let external = rel.attribute("TargetMode") == Some("External");
        rels.insert(
            id.to_string(),
            Relationship {
                kind: kind.rsplit('/').next().unwrap_or(kind).to_string(),
                target: if external {
                    target.to_string()
                } else {
                    resolve_target(part, target)
                },
                external,
            },
        );
    }
    Ok(rels)
}

fn target_of(rels: &Relationships, kind: &str) -> Option<String> {
    rels.values()
        .find(|rel| rel.kind == kind && !rel.external)
        .map(|rel| rel.target.clone())
}

// ── XML helpers ──────────────────────────────────────────────────────

fn parse_xml<'i>(text: &'i str, part: &str) -> Result<Document<'i>, String> {
    Document::parse(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("{part} is not valid XML: {e}"))
}

fn is(node: &Node, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name
}

fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|n| is(n, name))
}

fn children_named<'a, 'i>(
    node: Node<'a, 'i>,
    name: &'static str,
) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children().filter(move |n| is(n, name))
}

fn descend<'a, 'i>(node: Node<'a, 'i>, path: &[&str]) -> Option<Node<'a, 'i>> {
    path.iter().try_fold(node, |node, name| child(node, name))
}

fn attr_f64(node: Node, name: &str) -> Option<f64> {
    node.attribute(name)?.trim().parse().ok()
}

fn attr_f32(node: Node, name: &str) -> Option<f32> {
    node.attribute(name)?.trim().parse().ok()
}

fn attr_u32(node: Node, name: &str) -> Option<u32> {
    node.attribute(name)?.trim().parse().ok()
}

fn attr_bool(node: Node, name: &str) -> Option<bool> {
    match node.attribute(name)? {
        "1" | "true" | "on" => Some(true),
        "0" | "false" | "off" => Some(false),
        _ => None,
    }
}

/// A length attribute in EMUs, as points.
fn emu_attr(node: Node, name: &str) -> Option<f32> {
    attr_f64(node, name).map(|emu| (emu / EMU_PER_POINT) as f32)
}

/// An attribute in thousandths of a percent (`100000` is 100%), as a fraction.
fn fraction_attr(node: Node, name: &str) -> Option<f32> {
    attr_f32(node, name).map(|value| value / 100_000.0)
}

/// A relationship-id attribute (`r:id`, `r:embed`). Matched by local name
/// with any namespace, since strict OOXML uses a different namespace URI.
fn rel_attr<'a>(node: Node<'a, '_>, local: &str) -> Option<&'a str> {
    node.attributes()
        .find(|attr| attr.name() == local && attr.namespace().is_some())
        .map(|attr| attr.value())
}

/// The `nvSpPr`/`nvPicPr`/… element of a shape.
fn non_visual<'a, 'i>(node: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name().starts_with("nv"))
}

fn is_hidden(node: Node) -> bool {
    non_visual(node)
        .and_then(|nv| child(nv, "cNvPr"))
        .and_then(|pr| attr_bool(pr, "hidden"))
        .unwrap_or(false)
}

// ── Colours ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum ColorBase {
    Rgb(Rgba),
    Scheme(String),
    /// `phClr`: the colour a theme style is instantiated with.
    Placeholder,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ColorMod {
    Alpha(f32),
    LumMod(f32),
    LumOff(f32),
    SatMod(f32),
    Tint(f32),
    Shade(f32),
}

#[derive(Debug, Clone, PartialEq)]
struct ColorSpec {
    base: ColorBase,
    mods: Vec<ColorMod>,
}

/// Parse a colour element (`srgbClr`, `schemeClr`, …).
fn parse_color(node: Node) -> Option<ColorSpec> {
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
fn first_color(node: Node) -> Option<ColorSpec> {
    node.children()
        .filter(Node::is_element)
        .find_map(|n| parse_color(n))
}

fn preset_color(name: &str) -> Rgba {
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

fn apply_mods(color: Rgba, mods: &[ColorMod]) -> Rgba {
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

fn rgb_f32(r: f32, g: f32, b: f32, a: u8) -> Rgba {
    let channel = |c: f32| c.round().clamp(0.0, 255.0) as u8;
    Rgba {
        r: channel(r),
        g: channel(g),
        b: channel(b),
        a,
    }
}

/// Hue in degrees, saturation and lightness as fractions.
fn to_hsl(color: Rgba) -> (f32, f32, f32) {
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

fn from_hsl(hue: f32, s: f32, l: f32, a: u8) -> Rgba {
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

// ── Fills and lines ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum FillSpec {
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
fn fill_spec(node: Node) -> Option<FillSpec> {
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
fn find_fill(properties: Node) -> Option<FillSpec> {
    properties
        .children()
        .filter(Node::is_element)
        .find_map(|n| fill_spec(n))
}

#[derive(Debug, Clone, PartialEq, Default)]
struct LineSpec {
    width: Option<f32>,
    fill: Option<FillSpec>,
    dash: Option<Dash>,
    head_arrow: Option<bool>,
    tail_arrow: Option<bool>,
}

impl LineSpec {
    fn overlay(&mut self, other: &LineSpec) {
        overlay_option(&mut self.width, &other.width);
        overlay_option(&mut self.fill, &other.fill);
        overlay_option(&mut self.dash, &other.dash);
        overlay_option(&mut self.head_arrow, &other.head_arrow);
        overlay_option(&mut self.tail_arrow, &other.tail_arrow);
    }
}

fn overlay_option<T: Clone>(base: &mut Option<T>, over: &Option<T>) {
    if over.is_some() {
        base.clone_from(over);
    }
}

fn parse_line(ln: Node) -> LineSpec {
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

// ── Theme ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Theme {
    colors: HashMap<String, Rgba>,
    major_font: String,
    minor_font: String,
    fill_styles: Vec<FillSpec>,
    background_styles: Vec<FillSpec>,
    line_styles: Vec<LineSpec>,
}

impl Theme {
    /// The Office theme, for a deck whose master has none.
    fn fallback() -> Self {
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

fn parse_theme(root: Node) -> Theme {
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

// ── Text properties ──────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
struct RunProps {
    size: Option<f32>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    fill: Option<FillSpec>,
    font: Option<String>,
    baseline: Option<f32>,
    all_caps: Option<bool>,
    highlight: Option<ColorSpec>,
    spacing: Option<f32>,
}

impl RunProps {
    fn overlay(&mut self, other: &RunProps) {
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

fn parse_run_props(rpr: Node) -> RunProps {
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
enum BulletKind {
    None,
    Char(String),
    AutoNumber { scheme: String, start: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum BulletSize {
    /// Relative to the first run's size.
    Relative(f32),
    Points(f32),
}

#[derive(Debug, Clone, Default)]
struct ParaProps {
    align: Option<Align>,
    margin_left: Option<f32>,
    indent: Option<f32>,
    space_before: Option<Spacing>,
    space_after: Option<Spacing>,
    line_spacing: Option<Spacing>,
    bullet: Option<BulletKind>,
    /// `Some(None)`: follow the text colour.
    bullet_color: Option<Option<ColorSpec>>,
    bullet_size: Option<BulletSize>,
    /// `Some(None)`: follow the text font.
    bullet_font: Option<Option<String>>,
    run: RunProps,
}

impl ParaProps {
    fn overlay(&mut self, other: &ParaProps) {
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

fn parse_spacing(node: Node) -> Option<Spacing> {
    if let Some(pct) = child(node, "spcPct") {
        return fraction_attr(pct, "val").map(Spacing::Lines);
    }
    child(node, "spcPts")
        .and_then(|pts| attr_f32(pts, "val"))
        .map(|val| Spacing::Points(val / 100.0))
}

fn parse_para_props(ppr: Node) -> ParaProps {
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
struct ListStyle {
    levels: [ParaProps; 9],
}

impl ListStyle {
    fn overlay(&mut self, other: &ListStyle) {
        for (level, over) in self.levels.iter_mut().zip(&other.levels) {
            level.overlay(over);
        }
    }
}

fn parse_list_style(node: Node) -> ListStyle {
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
struct TextStyles {
    title: ListStyle,
    body: ListStyle,
    other: ListStyle,
}

fn parse_text_styles(master: Node) -> TextStyles {
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
struct BodyProps {
    insets: [Option<f32>; 4],
    anchor: Option<Anchor>,
    wrap: Option<bool>,
    /// Font scale and line-spacing reduction from `normAutofit`.
    autofit: Option<(f32, f32)>,
}

impl BodyProps {
    fn overlay(&mut self, other: &BodyProps) {
        for (inset, over) in self.insets.iter_mut().zip(&other.insets) {
            overlay_option(inset, over);
        }
        overlay_option(&mut self.anchor, &other.anchor);
        overlay_option(&mut self.wrap, &other.wrap);
        overlay_option(&mut self.autofit, &other.autofit);
    }
}

fn parse_body_props(body: Node) -> BodyProps {
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

fn parse_anchor(value: &str) -> Anchor {
    match value {
        "ctr" => Anchor::Middle,
        "b" => Anchor::Bottom,
        _ => Anchor::Top,
    }
}

// ── Placeholders ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
struct Placeholder {
    kind: String,
    idx: Option<u32>,
}

fn placeholder(node: Node) -> Option<Placeholder> {
    let ph = descend(non_visual(node)?, &["nvPr", "ph"])?;
    Some(Placeholder {
        kind: ph.attribute("type").unwrap_or("obj").to_string(),
        idx: attr_u32(ph, "idx"),
    })
}

/// The master placeholder type a slide or layout placeholder inherits from.
fn master_kind(kind: &str) -> &str {
    match kind {
        "title" | "ctrTitle" => "title",
        "dt" | "ftr" | "sldNum" | "hdr" => kind,
        _ => "body",
    }
}

struct TemplatePart<'a, 'i> {
    root: Node<'a, 'i>,
    rels: &'a Relationships,
    placeholders: Vec<(Placeholder, Node<'a, 'i>)>,
}

fn template<'a, 'i>(
    part: Option<&str>,
    docs: &'a HashMap<String, Document<'i>>,
    rels: &'a HashMap<String, Relationships>,
) -> Option<TemplatePart<'a, 'i>> {
    let part = part?;
    let root = docs.get(part)?.root_element();
    let placeholders = descend(root, &["cSld", "spTree"])
        .map(|tree| {
            tree.descendants()
                .filter(|n| is(n, "sp") || is(n, "pic") || is(n, "graphicFrame"))
                .filter_map(|n| Some((placeholder(n)?, n)))
                .collect()
        })
        .unwrap_or_default();
    Some(TemplatePart {
        root,
        rels: rels.get(part)?,
        placeholders,
    })
}

// ── Geometry ─────────────────────────────────────────────────────────

/// A 2D affine map from a shape tree's coordinate space (EMUs) to slide
/// points, with the rotation and scale it applies kept alongside so an
/// element's size and angle can be carried through nested groups.
#[derive(Debug, Clone, Copy)]
struct Transform {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
    rotation: f64,
    scale_x: f64,
    scale_y: f64,
}

impl Transform {
    const SLIDE: Transform = Transform {
        a: 1.0 / EMU_PER_POINT,
        b: 0.0,
        c: 0.0,
        d: 1.0 / EMU_PER_POINT,
        e: 0.0,
        f: 0.0,
        rotation: 0.0,
        scale_x: 1.0 / EMU_PER_POINT,
        scale_y: 1.0 / EMU_PER_POINT,
    };

    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// `self` after `local`: maps a point through `local`, then `self`.
    fn then(&self, local: &Transform) -> Transform {
        Transform {
            a: self.a * local.a + self.c * local.b,
            b: self.b * local.a + self.d * local.b,
            c: self.a * local.c + self.c * local.d,
            d: self.b * local.c + self.d * local.d,
            e: self.a * local.e + self.c * local.f + self.e,
            f: self.b * local.e + self.d * local.f + self.f,
            rotation: self.rotation + local.rotation,
            scale_x: self.scale_x * local.scale_x,
            scale_y: self.scale_y * local.scale_y,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Xfrm {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    rotation: f64,
    flip_h: bool,
    flip_v: bool,
    /// `chOff`/`chExt` of a group: the child coordinate space.
    children: Option<(f64, f64, f64, f64)>,
}

fn parse_xfrm(xfrm: Node) -> Option<Xfrm> {
    let offset = child(xfrm, "off");
    let extent = child(xfrm, "ext")?;
    let children = child(xfrm, "chOff")
        .zip(child(xfrm, "chExt"))
        .map(|(off, ext)| {
            (
                attr_f64(off, "x").unwrap_or(0.0),
                attr_f64(off, "y").unwrap_or(0.0),
                attr_f64(ext, "cx").unwrap_or(0.0),
                attr_f64(ext, "cy").unwrap_or(0.0),
            )
        });
    Some(Xfrm {
        x: offset.and_then(|o| attr_f64(o, "x")).unwrap_or(0.0),
        y: offset.and_then(|o| attr_f64(o, "y")).unwrap_or(0.0),
        width: attr_f64(extent, "cx").unwrap_or(0.0).max(0.0),
        height: attr_f64(extent, "cy").unwrap_or(0.0).max(0.0),
        rotation: attr_f64(xfrm, "rot").unwrap_or(0.0) / 60_000.0,
        flip_h: attr_bool(xfrm, "flipH").unwrap_or(false),
        flip_v: attr_bool(xfrm, "flipV").unwrap_or(false),
        children,
    })
}

/// The transform a group's children are placed with.
fn group_transform(parent: &Transform, xfrm: &Xfrm) -> Transform {
    let (child_x, child_y, child_w, child_h) =
        xfrm.children
            .unwrap_or((xfrm.x, xfrm.y, xfrm.width, xfrm.height));
    let scale_x = if child_w > 0.0 {
        xfrm.width / child_w
    } else {
        1.0
    };
    let scale_y = if child_h > 0.0 {
        xfrm.height / child_h
    } else {
        1.0
    };
    let (center_x, center_y) = (xfrm.x + xfrm.width / 2.0, xfrm.y + xfrm.height / 2.0);
    let (sin, cos) = xfrm.rotation.to_radians().sin_cos();
    let flip_x = if xfrm.flip_h { -1.0 } else { 1.0 };
    let flip_y = if xfrm.flip_v { -1.0 } else { 1.0 };
    // Scale into the group's box, then flip and rotate about its centre.
    let tx = xfrm.x - center_x - scale_x * child_x;
    let ty = xfrm.y - center_y - scale_y * child_y;
    let local = Transform {
        a: cos * flip_x * scale_x,
        b: sin * flip_x * scale_x,
        c: -sin * flip_y * scale_y,
        d: cos * flip_y * scale_y,
        e: cos * flip_x * tx - sin * flip_y * ty + center_x,
        f: sin * flip_x * tx + cos * flip_y * ty + center_y,
        rotation: xfrm.rotation,
        scale_x,
        scale_y,
    };
    parent.then(&local)
}

/// Bounds and rotation on the slide of an element placed by `xfrm`.
fn place(xfrm: &Xfrm, transform: &Transform) -> (Rect, f32) {
    let (center_x, center_y) =
        transform.apply(xfrm.x + xfrm.width / 2.0, xfrm.y + xfrm.height / 2.0);
    let width = xfrm.width * transform.scale_x.abs();
    let height = xfrm.height * transform.scale_y.abs();
    (
        Rect {
            x: (center_x - width / 2.0) as f32,
            y: (center_y - height / 2.0) as f32,
            width: width as f32,
            height: height as f32,
        },
        ((xfrm.rotation + transform.rotation) % 360.0) as f32,
    )
}

/// Bezier handle length for a quarter ellipse, as a fraction of its radius.
const KAPPA: f32 = 0.552_284_8;

fn polygon(points: &[(f32, f32)]) -> ShapePath {
    let mut commands = Vec::with_capacity(points.len() + 1);
    for (i, &(x, y)) in points.iter().enumerate() {
        commands.push(if i == 0 {
            PathCommand::MoveTo(x, y)
        } else {
            PathCommand::LineTo(x, y)
        });
    }
    commands.push(PathCommand::Close);
    ShapePath {
        commands,
        filled: true,
        stroked: true,
    }
}

fn ellipse_path(w: f32, h: f32) -> ShapePath {
    let (rx, ry) = (w / 2.0, h / 2.0);
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    ShapePath {
        commands: vec![
            PathCommand::MoveTo(w, ry),
            PathCommand::CubicTo(w, ry + ky, rx + kx, h, rx, h),
            PathCommand::CubicTo(rx - kx, h, 0.0, ry + ky, 0.0, ry),
            PathCommand::CubicTo(0.0, ry - ky, rx - kx, 0.0, rx, 0.0),
            PathCommand::CubicTo(rx + kx, 0.0, w, ry - ky, w, ry),
            PathCommand::Close,
        ],
        filled: true,
        stroked: true,
    }
}

fn round_rect_path(w: f32, h: f32, radius: f32) -> ShapePath {
    let r = radius.clamp(0.0, w.min(h) / 2.0);
    let k = r * (1.0 - KAPPA);
    ShapePath {
        commands: vec![
            PathCommand::MoveTo(r, 0.0),
            PathCommand::LineTo(w - r, 0.0),
            PathCommand::CubicTo(w - k, 0.0, w, k, w, r),
            PathCommand::LineTo(w, h - r),
            PathCommand::CubicTo(w, h - k, w - k, h, w - r, h),
            PathCommand::LineTo(r, h),
            PathCommand::CubicTo(k, h, 0.0, h - k, 0.0, h - r),
            PathCommand::LineTo(0.0, r),
            PathCommand::CubicTo(0.0, k, k, 0.0, r, 0.0),
            PathCommand::Close,
        ],
        filled: true,
        stroked: true,
    }
}

/// Outline of a preset shape in a `w` × `h` box. Presets not listed draw as
/// their bounding rectangle.
fn preset_geometry(name: &str, adjust: &HashMap<String, f32>, w: f32, h: f32) -> Vec<ShapePath> {
    let short = w.min(h);
    let adj = |key: &str, default: f32| adjust.get(key).copied().unwrap_or(default) / 100_000.0;
    let rect = || polygon(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]);
    let path = match name {
        "roundRect" | "flowChartAlternateProcess" => {
            round_rect_path(w, h, short * adj("adj", 16_667.0))
        }
        "flowChartTerminator" => round_rect_path(w, h, short / 2.0),
        "ellipse" | "flowChartConnector" => ellipse_path(w, h),
        "line" | "straightConnector1" | "bentConnector2" | "bentConnector3" | "bentConnector4"
        | "bentConnector5" | "curvedConnector2" | "curvedConnector3" | "curvedConnector4"
        | "curvedConnector5" => ShapePath {
            commands: vec![PathCommand::MoveTo(0.0, 0.0), PathCommand::LineTo(w, h)],
            filled: false,
            stroked: true,
        },
        "triangle" | "flowChartExtract" => {
            polygon(&[(w * adj("adj", 50_000.0), 0.0), (w, h), (0.0, h)])
        }
        "rtTriangle" => polygon(&[(0.0, 0.0), (w, h), (0.0, h)]),
        "diamond" | "flowChartDecision" => {
            polygon(&[(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h), (0.0, h / 2.0)])
        }
        "parallelogram" | "flowChartInputOutput" => {
            let o = short * adj("adj", 25_000.0);
            polygon(&[(o, 0.0), (w, 0.0), (w - o, h), (0.0, h)])
        }
        "trapezoid" => {
            let o = short * adj("adj", 25_000.0);
            polygon(&[(o, 0.0), (w - o, 0.0), (w, h), (0.0, h)])
        }
        "pentagon" => polygon(&[
            (w / 2.0, 0.0),
            (w, h * 0.382),
            (w * 0.809, h),
            (w * 0.191, h),
            (0.0, h * 0.382),
        ]),
        "hexagon" => {
            let o = short * adj("adj", 25_000.0);
            polygon(&[
                (o, 0.0),
                (w - o, 0.0),
                (w, h / 2.0),
                (w - o, h),
                (o, h),
                (0.0, h / 2.0),
            ])
        }
        "octagon" => {
            let o = short * adj("adj", 29_289.0);
            polygon(&[
                (o, 0.0),
                (w - o, 0.0),
                (w, o),
                (w, h - o),
                (w - o, h),
                (o, h),
                (0.0, h - o),
                (0.0, o),
            ])
        }
        "homePlate" => {
            let o = short * adj("adj", 50_000.0);
            polygon(&[(0.0, 0.0), (w - o, 0.0), (w, h / 2.0), (w - o, h), (0.0, h)])
        }
        "chevron" => {
            let o = short * adj("adj", 50_000.0);
            polygon(&[
                (0.0, 0.0),
                (w - o, 0.0),
                (w, h / 2.0),
                (w - o, h),
                (0.0, h),
                (o, h / 2.0),
            ])
        }
        "rightArrow" | "leftArrow" => {
            let shaft = h * adj("adj1", 50_000.0);
            let head = short * adj("adj2", 50_000.0);
            let (top, bottom) = ((h - shaft) / 2.0, (h + shaft) / 2.0);
            let points = [
                (0.0, top),
                (w - head, top),
                (w - head, 0.0),
                (w, h / 2.0),
                (w - head, h),
                (w - head, bottom),
                (0.0, bottom),
            ];
            if name == "leftArrow" {
                polygon(&points.map(|(x, y)| (w - x, y)))
            } else {
                polygon(&points)
            }
        }
        "upArrow" | "downArrow" => {
            let shaft = w * adj("adj1", 50_000.0);
            let head = short * adj("adj2", 50_000.0);
            let (left, right) = ((w - shaft) / 2.0, (w + shaft) / 2.0);
            let points = [
                (left, h),
                (left, head),
                (0.0, head),
                (w / 2.0, 0.0),
                (w, head),
                (right, head),
                (right, h),
            ];
            if name == "downArrow" {
                polygon(&points.map(|(x, y)| (x, h - y)))
            } else {
                polygon(&points)
            }
        }
        "plus" | "flowChartSummingJunction" => {
            let o = short * adj("adj", 25_000.0);
            polygon(&[
                (o, 0.0),
                (w - o, 0.0),
                (w - o, o),
                (w, o),
                (w, h - o),
                (w - o, h - o),
                (w - o, h),
                (o, h),
                (o, h - o),
                (0.0, h - o),
                (0.0, o),
                (o, o),
            ])
        }
        _ => rect(),
    };
    vec![path]
}

fn adjust_values(prst_geom: Node) -> HashMap<String, f32> {
    child(prst_geom, "avLst")
        .into_iter()
        .flat_map(|list| children_named(list, "gd"))
        .filter_map(|gd| {
            let value = gd
                .attribute("fmla")?
                .strip_prefix("val ")?
                .trim()
                .parse()
                .ok()?;
            Some((gd.attribute("name")?.to_string(), value))
        })
        .collect()
}

/// Outline from a `custGeom`. Coordinates that are guide names rather than
/// numbers are not evaluated; a path using them is dropped.
fn custom_geometry(cust: Node, w: f32, h: f32) -> Vec<ShapePath> {
    let Some(list) = child(cust, "pathLst") else {
        return Vec::new();
    };
    children_named(list, "path")
        .filter_map(|path| custom_path(path, w, h))
        .collect()
}

fn custom_path(path: Node, w: f32, h: f32) -> Option<ShapePath> {
    let path_w = attr_f32(path, "w").filter(|v| *v > 0.0);
    let path_h = attr_f32(path, "h").filter(|v| *v > 0.0);
    let scale_x = path_w.map_or(1.0 / EMU_PER_POINT as f32, |pw| w / pw);
    let scale_y = path_h.map_or(1.0 / EMU_PER_POINT as f32, |ph| h / ph);
    let point = |pt: Node| -> Option<(f32, f32)> {
        Some((attr_f32(pt, "x")? * scale_x, attr_f32(pt, "y")? * scale_y))
    };
    let mut commands = Vec::new();
    let mut current = (0.0, 0.0);
    for command in path.children().filter(Node::is_element) {
        let points: Vec<Node> = children_named(command, "pt").collect();
        match command.tag_name().name() {
            "moveTo" => {
                current = point(*points.first()?)?;
                commands.push(PathCommand::MoveTo(current.0, current.1));
            }
            "lnTo" => {
                current = point(*points.first()?)?;
                commands.push(PathCommand::LineTo(current.0, current.1));
            }
            "cubicBezTo" => {
                let [a, b, c] = [
                    point(*points.first()?)?,
                    point(*points.get(1)?)?,
                    point(*points.get(2)?)?,
                ];
                commands.push(PathCommand::CubicTo(a.0, a.1, b.0, b.1, c.0, c.1));
                current = c;
            }
            "quadBezTo" => {
                let [a, b] = [point(*points.first()?)?, point(*points.get(1)?)?];
                commands.push(PathCommand::QuadTo(a.0, a.1, b.0, b.1));
                current = b;
            }
            "arcTo" => {
                let radius_x = attr_f32(command, "wR")? * scale_x;
                let radius_y = attr_f32(command, "hR")? * scale_y;
                let start = attr_f32(command, "stAng")? / 60_000.0;
                let sweep = attr_f32(command, "swAng")? / 60_000.0;
                current = arc_to(&mut commands, current, radius_x, radius_y, start, sweep);
            }
            "close" => commands.push(PathCommand::Close),
            _ => {}
        }
    }
    (!commands.is_empty()).then(|| ShapePath {
        commands,
        filled: path.attribute("fill") != Some("none"),
        stroked: attr_bool(path, "stroke") != Some(false),
    })
}

/// Append an elliptical arc that starts at `from`, as cubic segments of at
/// most 90°. Angles are clockwise degrees, as DrawingML measures them.
fn arc_to(
    commands: &mut Vec<PathCommand>,
    from: (f32, f32),
    radius_x: f32,
    radius_y: f32,
    start: f32,
    sweep: f32,
) -> (f32, f32) {
    let at = |angle: f32| {
        let radians = angle.to_radians();
        (radius_x * radians.cos(), radius_y * radians.sin())
    };
    let start_offset = at(start);
    let center = (from.0 - start_offset.0, from.1 - start_offset.1);
    let segments = (sweep.abs() / 90.0).ceil().max(1.0) as usize;
    let step = sweep / segments as f32;
    let handle = 4.0 / 3.0 * (step.to_radians() / 4.0).tan();
    let mut angle = start;
    let mut end = from;
    for _ in 0..segments {
        let (a0, a1) = (angle.to_radians(), (angle + step).to_radians());
        let p0 = (
            center.0 + radius_x * a0.cos(),
            center.1 + radius_y * a0.sin(),
        );
        let p1 = (
            center.0 + radius_x * a1.cos(),
            center.1 + radius_y * a1.sin(),
        );
        let c0 = (
            p0.0 - handle * radius_x * a0.sin(),
            p0.1 + handle * radius_y * a0.cos(),
        );
        let c1 = (
            p1.0 + handle * radius_x * a1.sin(),
            p1.1 - handle * radius_y * a1.cos(),
        );
        commands.push(PathCommand::CubicTo(c0.0, c0.1, c1.0, c1.1, p1.0, p1.1));
        angle += step;
        end = p1;
    }
    end
}

// ── Numbering ────────────────────────────────────────────────────────

fn roman(mut n: u32) -> String {
    const NUMERALS: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (value, numeral) in NUMERALS {
        while n >= value {
            out.push_str(numeral);
            n -= value;
        }
    }
    out
}

/// PowerPoint's alphabetic numbering repeats the letter: …, y, z, aa, bb.
fn alphabetic(n: u32) -> String {
    let n = n.max(1) - 1;
    let letter = (b'a' + (n % 26) as u8) as char;
    std::iter::repeat_n(letter, (n / 26) as usize + 1).collect()
}

/// The label for item `n` of an auto-numbered list in `scheme`
/// (`arabicPeriod`, `romanUcParenR`, …).
fn autonumber_label(scheme: &str, n: u32) -> String {
    let (number, punctuation) = if let Some(rest) = scheme.strip_prefix("romanUc") {
        (roman(n).to_uppercase(), rest)
    } else if let Some(rest) = scheme.strip_prefix("romanLc") {
        (roman(n), rest)
    } else if let Some(rest) = scheme.strip_prefix("alphaUc") {
        (alphabetic(n).to_uppercase(), rest)
    } else if let Some(rest) = scheme.strip_prefix("alphaLc") {
        (alphabetic(n), rest)
    } else {
        (
            n.to_string(),
            scheme.strip_prefix("arabic").unwrap_or("Period"),
        )
    };
    if punctuation.ends_with("ParenBoth") {
        format!("({number})")
    } else if punctuation.ends_with("ParenR") {
        format!("{number})")
    } else if punctuation.ends_with("Plain") {
        number
    } else if punctuation.ends_with("Minus") {
        format!("- {number} -")
    } else {
        format!("{number}.")
    }
}

// ── Table styles ─────────────────────────────────────────────────────

/// The built-in table styles the viewer approximates. PowerPoint does not
/// store built-in styles in the file, only their ids.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TableLook {
    Plain,
    Grid,
    /// "Medium Style 2": a solid header in the accent colour, tinted banded
    /// rows, and white rules between cells — PowerPoint's default table.
    Medium2 {
        accent: &'static str,
    },
}

fn table_look(style_id: &str) -> TableLook {
    match style_id.to_ascii_uppercase().as_str() {
        "" => TableLook::Plain,
        "{2D5ABB26-0587-4C30-8999-92F81FD0307C}" => TableLook::Plain,
        "{5940675A-B579-460E-94D1-54222C63F5DA}" => TableLook::Grid,
        "{073A0DAA-6AF3-43AB-8588-CEC1D06C72B9}" => TableLook::Medium2 { accent: "dk1" },
        "{21E4AEA4-8DFA-4A89-87EB-49C32662AFE8}" => TableLook::Medium2 { accent: "accent2" },
        "{F5AB1C69-6EDB-4FF4-983F-18BD219EF322}" => TableLook::Medium2 { accent: "accent3" },
        "{00A15C55-8517-42AA-B614-E9B94910E393}" => TableLook::Medium2 { accent: "accent4" },
        "{7DF18680-E054-41AD-8BC1-D1AEF772440D}" => TableLook::Medium2 { accent: "accent5" },
        "{93296810-A885-4BE3-A3E7-6D5BEEA58F35}" => TableLook::Medium2 { accent: "accent6" },
        // Medium Style 2 – Accent 1 itself, and any style not listed: most
        // built-in styles are accent-coloured banded tables like it.
        _ => TableLook::Medium2 { accent: "accent1" },
    }
}

// ── Slides ───────────────────────────────────────────────────────────

struct SlideBuilder<'a, 'i, 'm> {
    theme: &'a Theme,
    default_style: &'a ListStyle,
    text_styles: Option<&'a TextStyles>,
    master: Option<TemplatePart<'a, 'i>>,
    layout: Option<TemplatePart<'a, 'i>>,
    default_table_style: &'a str,
    color_map: HashMap<String, String>,
    number: usize,
    title: Option<String>,
    media: &'m mut HashSet<String>,
}

impl<'a, 'i> SlideBuilder<'a, 'i, '_> {
    fn build(mut self, slide: Node<'a, 'i>, rels: &Relationships) -> Slide {
        self.color_map = self.resolve_color_map(slide);
        let background = self.background(slide, rels);
        let mut elements = Vec::new();

        let slide_shows_templates = attr_bool(slide, "showMasterSp") != Some(false);
        if slide_shows_templates {
            let layout_shows_master = self
                .layout
                .as_ref()
                .is_none_or(|layout| attr_bool(layout.root, "showMasterSp") != Some(false));
            if layout_shows_master
                && let Some((root, template_rels)) = self
                    .master
                    .as_ref()
                    .map(|master| (master.root, master.rels))
                && let Some(tree) = descend(root, &["cSld", "spTree"])
            {
                self.walk(
                    tree,
                    template_rels,
                    &Transform::SLIDE,
                    0,
                    true,
                    &mut elements,
                );
            }
            if let Some((root, template_rels)) = self
                .layout
                .as_ref()
                .map(|layout| (layout.root, layout.rels))
                && let Some(tree) = descend(root, &["cSld", "spTree"])
            {
                self.walk(
                    tree,
                    template_rels,
                    &Transform::SLIDE,
                    0,
                    true,
                    &mut elements,
                );
            }
        }
        if let Some(tree) = descend(slide, &["cSld", "spTree"]) {
            self.walk(tree, rels, &Transform::SLIDE, 0, false, &mut elements);
        }

        Slide {
            number: self.number,
            hidden: attr_bool(slide, "show") == Some(false),
            title: self.title,
            background,
            elements,
        }
    }

    fn resolve_color_map(&self, slide: Node) -> HashMap<String, String> {
        let mut map = HashMap::new();
        let mut apply = |node: Option<Node>| {
            if let Some(node) = node {
                for attr in node.attributes() {
                    map.insert(attr.name().to_string(), attr.value().to_string());
                }
            }
        };
        apply(self.master.as_ref().and_then(|m| child(m.root, "clrMap")));
        apply(
            self.layout
                .as_ref()
                .and_then(|l| descend(l.root, &["clrMapOvr", "overrideClrMapping"])),
        );
        apply(descend(slide, &["clrMapOvr", "overrideClrMapping"]));
        map
    }

    // ── Colour and fill resolution ──

    fn scheme_color(&self, name: &str) -> Rgba {
        let mapped = self
            .color_map
            .get(name)
            .map(String::as_str)
            .unwrap_or(match name {
                "bg1" => "lt1",
                "tx1" => "dk1",
                "bg2" => "lt2",
                "tx2" => "dk2",
                other => other,
            });
        self.theme
            .colors
            .get(mapped)
            .copied()
            .unwrap_or(Rgba::BLACK)
    }

    fn color(&self, spec: &ColorSpec, placeholder: Option<Rgba>) -> Rgba {
        let base = match &spec.base {
            ColorBase::Rgb(color) => *color,
            ColorBase::Scheme(name) => self.scheme_color(name),
            ColorBase::Placeholder => placeholder.unwrap_or(Rgba::BLACK),
        };
        apply_mods(base, &spec.mods)
    }

    fn fill(&mut self, spec: &FillSpec, rels: &Relationships, placeholder: Option<Rgba>) -> Fill {
        match spec {
            FillSpec::None | FillSpec::Group => Fill::None,
            FillSpec::Solid(color) => Fill::Solid(self.color(color, placeholder)),
            FillSpec::Gradient {
                stops,
                angle,
                radial,
            } => Fill::Gradient(Gradient {
                stops: stops
                    .iter()
                    .map(|(offset, color)| GradientStop {
                        offset: offset.clamp(0.0, 1.0),
                        color: self.color(color, placeholder),
                    })
                    .collect(),
                angle: *angle,
                radial: *radial,
            }),
            FillSpec::Picture(id) => match rels.get(id) {
                Some(rel) if !rel.external => {
                    self.media.insert(rel.target.clone());
                    Fill::Picture {
                        media: rel.target.clone(),
                    }
                }
                _ => Fill::None,
            },
        }
    }

    /// A theme style reference (`fillRef`, `bgRef`): the numbered style from
    /// the theme, instantiated with the reference's colour.
    fn style_fill(&mut self, reference: Node, rels: &Relationships) -> Option<Fill> {
        let index = attr_u32(reference, "idx")? as usize;
        let spec = match index {
            0 => return Some(Fill::None),
            1..=999 => self.theme.fill_styles.get(index - 1),
            _ => self.theme.background_styles.get(index - 1001),
        }?
        .clone();
        let placeholder = first_color(reference).map(|color| self.color(&color, None));
        Some(self.fill(&spec, rels, placeholder))
    }

    fn line(&mut self, chain: &[Node], style: Option<Node>, rels: &Relationships) -> Option<Line> {
        let mut spec = LineSpec::default();
        let mut placeholder = None;
        if let Some(reference) = style.and_then(|s| child(s, "lnRef"))
            && let Some(index) = attr_u32(reference, "idx").filter(|i| *i > 0)
            && let Some(theme_line) = self.theme.line_styles.get(index as usize - 1)
        {
            spec = theme_line.clone();
            placeholder = first_color(reference).map(|color| self.color(&color, None));
        }
        for properties in chain.iter().rev() {
            if let Some(ln) = child(*properties, "ln") {
                spec.overlay(&parse_line(ln));
            }
        }
        let color = match self.fill(spec.fill.as_ref()?, rels, placeholder) {
            Fill::Solid(color) => color,
            Fill::Gradient(gradient) => gradient.stops.first()?.color,
            _ => return None,
        };
        Some(Line {
            color,
            width: spec.width.unwrap_or(DEFAULT_LINE_WIDTH),
            dash: spec.dash.unwrap_or_default(),
            head_arrow: spec.head_arrow.unwrap_or(false),
            tail_arrow: spec.tail_arrow.unwrap_or(false),
        })
    }

    fn background(&mut self, slide: Node<'a, 'i>, rels: &Relationships) -> Fill {
        let mut sources = vec![(slide, rels)];
        if let Some(layout) = &self.layout {
            sources.push((layout.root, layout.rels));
        }
        if let Some(master) = &self.master {
            sources.push((master.root, master.rels));
        }
        for (root, source_rels) in sources {
            let Some(bg) = descend(root, &["cSld", "bg"]) else {
                continue;
            };
            if let Some(spec) = child(bg, "bgPr").and_then(find_fill) {
                return self.fill(&spec, source_rels, None);
            }
            if let Some(fill) = child(bg, "bgRef").and_then(|r| self.style_fill(r, source_rels)) {
                return fill;
            }
        }
        Fill::Solid(Rgba::WHITE)
    }

    // ── Shape tree ──

    fn walk(
        &mut self,
        tree: Node<'a, 'i>,
        rels: &Relationships,
        transform: &Transform,
        depth: usize,
        template: bool,
        out: &mut Vec<Element>,
    ) {
        if depth > MAX_TREE_DEPTH {
            return;
        }
        for node in tree.children().filter(Node::is_element) {
            let name = node.tag_name().name();
            // A template's placeholders only shape the slide's own; they
            // are never drawn themselves.
            if template
                && matches!(name, "sp" | "pic" | "graphicFrame")
                && placeholder(node).is_some()
            {
                continue;
            }
            if matches!(name, "sp" | "cxnSp" | "pic" | "graphicFrame" | "grpSp") && is_hidden(node)
            {
                continue;
            }
            let element = match name {
                "sp" | "cxnSp" => self.shape(node, rels, transform, template),
                "pic" => self.picture(node, rels, transform),
                "graphicFrame" => self.graphic_frame(node, rels, transform),
                "grpSp" => {
                    if let Some(xfrm) = descend(node, &["grpSpPr", "xfrm"]).and_then(parse_xfrm) {
                        let inner = group_transform(transform, &xfrm);
                        self.walk(node, rels, &inner, depth + 1, template, out);
                    } else {
                        self.walk(node, rels, transform, depth + 1, template, out);
                    }
                    None
                }
                // Alternate content offers a newer representation and a
                // fallback; the fallback is the one every reader supports.
                "AlternateContent" => {
                    if let Some(branch) = child(node, "Fallback").or_else(|| child(node, "Choice"))
                    {
                        self.walk(branch, rels, transform, depth + 1, template, out);
                    }
                    None
                }
                _ => None,
            };
            out.extend(element);
        }
    }

    /// The layout and master placeholders a slide placeholder inherits from.
    fn inherited(&self, ph: Option<&Placeholder>) -> (Option<Node<'a, 'i>>, Option<Node<'a, 'i>>) {
        let Some(ph) = ph else {
            return (None, None);
        };
        let layout = self.layout.as_ref().and_then(|layout| {
            let by_idx = ph.idx.and_then(|idx| {
                layout
                    .placeholders
                    .iter()
                    .find(|(candidate, _)| candidate.idx == Some(idx))
            });
            by_idx
                .or_else(|| {
                    layout.placeholders.iter().find(|(candidate, _)| {
                        master_kind(&candidate.kind) == master_kind(&ph.kind)
                    })
                })
                .map(|(_, node)| *node)
        });
        let kind = layout
            .and_then(placeholder)
            .map(|layout_ph| layout_ph.kind)
            .unwrap_or_else(|| ph.kind.clone());
        let master = self.master.as_ref().and_then(|master| {
            master
                .placeholders
                .iter()
                .find(|(candidate, _)| master_kind(&candidate.kind) == master_kind(&kind))
                .map(|(_, node)| *node)
        });
        (layout, master)
    }

    fn shape(
        &mut self,
        node: Node<'a, 'i>,
        rels: &Relationships,
        transform: &Transform,
        template: bool,
    ) -> Option<Element> {
        let ph = placeholder(node);
        let (layout_ph, master_ph) = self.inherited(ph.as_ref());
        let chain: Vec<Node> = [Some(node), layout_ph, master_ph]
            .into_iter()
            .flatten()
            .filter_map(|n| child(n, "spPr"))
            .collect();

        let xfrm = chain
            .iter()
            .find_map(|pr| child(*pr, "xfrm").and_then(parse_xfrm))?;
        let (bounds, rotation) = place(&xfrm, transform);
        let style = child(node, "style");

        let fill = match chain.iter().find_map(|pr| find_fill(*pr)) {
            Some(spec) => self.fill(&spec, rels, None),
            None => style
                .and_then(|s| child(s, "fillRef"))
                .and_then(|reference| self.style_fill(reference, rels))
                .unwrap_or(Fill::None),
        };
        let line = self.line(&chain, style, rels);

        let text = child(node, "txBody").and_then(|body| {
            let (levels, body_props) =
                self.shape_text_style(body, ph.as_ref(), layout_ph, master_ph, style);
            self.text_body(body, &levels, &body_props)
        });
        if !template
            && let (Some(ph), Some(text)) = (&ph, &text)
            && master_kind(&ph.kind) == "title"
            && self.title.is_none()
        {
            let title: String = text
                .paragraphs
                .iter()
                .flat_map(|p| {
                    p.runs
                        .iter()
                        .map(|r| if r.text == "\n" { " " } else { r.text.as_str() })
                })
                .collect();
            let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
            if !title.is_empty() {
                self.title = Some(title);
            }
        }

        if fill == Fill::None && line.is_none() && text.is_none() {
            return None;
        }
        let paths = chain
            .iter()
            .find_map(|pr| {
                if let Some(prst) = child(*pr, "prstGeom") {
                    let name = prst.attribute("prst").unwrap_or("rect");
                    Some(preset_geometry(
                        name,
                        &adjust_values(prst),
                        bounds.width,
                        bounds.height,
                    ))
                } else {
                    child(*pr, "custGeom")
                        .map(|cust| custom_geometry(cust, bounds.width, bounds.height))
                        .filter(|paths| !paths.is_empty())
                }
            })
            .unwrap_or_else(|| {
                preset_geometry("rect", &HashMap::new(), bounds.width, bounds.height)
            });

        Some(Element {
            bounds,
            rotation,
            flip_h: xfrm.flip_h,
            flip_v: xfrm.flip_v,
            kind: ElementKind::Shape(Shape {
                paths,
                fill,
                line,
                text,
            }),
        })
    }

    /// The list style and body properties a shape's text starts from, before
    /// its own paragraphs and runs.
    fn shape_text_style(
        &self,
        body: Node,
        ph: Option<&Placeholder>,
        layout_ph: Option<Node>,
        master_ph: Option<Node>,
        style: Option<Node>,
    ) -> (ListStyle, BodyProps) {
        let mut levels = self.default_style.clone();
        if let (Some(ph), Some(styles)) = (ph, self.text_styles) {
            levels.overlay(match master_kind(&ph.kind) {
                "title" => &styles.title,
                "body" => &styles.body,
                _ => &styles.other,
            });
        }
        let mut body_props = BodyProps::default();
        for inherited in [master_ph, layout_ph].into_iter().flatten() {
            if let Some(inherited_body) = child(inherited, "txBody") {
                if let Some(list) = child(inherited_body, "lstStyle") {
                    levels.overlay(&parse_list_style(list));
                }
                if let Some(props) = child(inherited_body, "bodyPr") {
                    body_props.overlay(&parse_body_props(props));
                }
            }
        }
        if let Some(reference) = style.and_then(|s| child(s, "fontRef")) {
            let font_ref = RunProps {
                font: match reference.attribute("idx") {
                    Some("major") => Some("+mj-lt".to_string()),
                    Some("minor") => Some("+mn-lt".to_string()),
                    _ => None,
                },
                fill: first_color(reference).map(FillSpec::Solid),
                ..RunProps::default()
            };
            for level in &mut levels.levels {
                level.run.overlay(&font_ref);
            }
        }
        if let Some(list) = child(body, "lstStyle") {
            levels.overlay(&parse_list_style(list));
        }
        if let Some(props) = child(body, "bodyPr") {
            body_props.overlay(&parse_body_props(props));
        }
        (levels, body_props)
    }

    fn picture(
        &mut self,
        node: Node<'a, 'i>,
        rels: &Relationships,
        transform: &Transform,
    ) -> Option<Element> {
        let ph = placeholder(node);
        let (layout_ph, master_ph) = self.inherited(ph.as_ref());
        let chain: Vec<Node> = [Some(node), layout_ph, master_ph]
            .into_iter()
            .flatten()
            .filter_map(|n| child(n, "spPr"))
            .collect();
        let xfrm = chain
            .iter()
            .find_map(|pr| child(*pr, "xfrm").and_then(parse_xfrm))?;
        let (bounds, rotation) = place(&xfrm, transform);
        let blip_fill = child(node, "blipFill")?;
        let target = child(blip_fill, "blip")
            .and_then(|blip| rel_attr(blip, "embed"))
            .and_then(|id| rels.get(id))
            .filter(|rel| !rel.external)?
            .target
            .clone();

        let extension = target.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let kind = if matches!(extension.as_str(), "emf" | "wmf" | "svg" | "pdf") {
            ElementKind::Unsupported {
                label: format!("{} picture", extension.to_uppercase()),
            }
        } else {
            self.media.insert(target.clone());
            let crop = child(blip_fill, "srcRect").map_or(Crop::default(), |src| Crop {
                left: fraction_attr(src, "l").unwrap_or(0.0),
                top: fraction_attr(src, "t").unwrap_or(0.0),
                right: fraction_attr(src, "r").unwrap_or(0.0),
                bottom: fraction_attr(src, "b").unwrap_or(0.0),
            });
            let line = self.line(&chain, child(node, "style"), rels);
            ElementKind::Picture(Picture {
                media: target,
                crop,
                line,
            })
        };
        Some(Element {
            bounds,
            rotation,
            flip_h: xfrm.flip_h,
            flip_v: xfrm.flip_v,
            kind,
        })
    }

    fn graphic_frame(
        &mut self,
        node: Node<'a, 'i>,
        rels: &Relationships,
        transform: &Transform,
    ) -> Option<Element> {
        let xfrm = child(node, "xfrm").and_then(parse_xfrm).or_else(|| {
            let (layout_ph, master_ph) = self.inherited(placeholder(node).as_ref());
            [layout_ph, master_ph]
                .into_iter()
                .flatten()
                .find_map(|n| descend(n, &["spPr", "xfrm"]).and_then(parse_xfrm))
        })?;
        let (bounds, rotation) = place(&xfrm, transform);
        let data = descend(node, &["graphic", "graphicData"])?;
        let uri = data.attribute("uri").unwrap_or("");
        let kind = if let Some(table) = child(data, "tbl") {
            ElementKind::Table(self.table(table, rels))
        } else if uri.ends_with("/chart") || uri.contains("chart") {
            ElementKind::Unsupported {
                label: "Chart".to_string(),
            }
        } else if uri.contains("diagram") {
            ElementKind::Unsupported {
                label: "SmartArt graphic".to_string(),
            }
        } else {
            ElementKind::Unsupported {
                label: "Embedded object".to_string(),
            }
        };
        Some(Element {
            bounds,
            rotation,
            flip_h: false,
            flip_v: false,
            kind,
        })
    }

    fn table(&mut self, table: Node<'a, 'i>, rels: &Relationships) -> Table {
        let properties = child(table, "tblPr");
        let flag = |name| properties.and_then(|p| attr_bool(p, name)).unwrap_or(false);
        let (first_row, last_row, first_col, last_col, band_row) = (
            flag("firstRow"),
            flag("lastRow"),
            flag("firstCol"),
            flag("lastCol"),
            flag("bandRow"),
        );
        let look = table_look(
            properties
                .and_then(|p| child(p, "tableStyleId"))
                .and_then(|id| id.text())
                .unwrap_or(self.default_table_style)
                .trim(),
        );

        let columns: Vec<f32> = child(table, "tblGrid")
            .into_iter()
            .flat_map(|grid| children_named(grid, "gridCol"))
            .map(|col| emu_attr(col, "w").unwrap_or(0.0))
            .collect();
        let row_nodes: Vec<Node> = children_named(table, "tr").collect();
        let row_count = row_nodes.len();

        let mut rows = Vec::with_capacity(row_count);
        for (row_index, row) in row_nodes.into_iter().enumerate() {
            let mut cells = Vec::new();
            for (col_index, cell) in children_named(row, "tc").enumerate() {
                let emphasized = (first_row && row_index == 0)
                    || (last_row && row_index + 1 == row_count)
                    || (first_col && col_index == 0)
                    || (last_col && col_index + 1 == columns.len());
                let data_row = row_index.saturating_sub(usize::from(first_row));
                let banded = band_row && !emphasized && data_row % 2 == 0;
                cells.push(self.table_cell(cell, rels, look, emphasized, banded));
            }
            rows.push(TableRow {
                height: emu_attr(row, "h").unwrap_or(0.0),
                cells,
            });
        }
        Table { columns, rows }
    }

    fn table_cell(
        &mut self,
        cell: Node<'a, 'i>,
        rels: &Relationships,
        look: TableLook,
        emphasized: bool,
        banded: bool,
    ) -> TableCell {
        let properties = child(cell, "tcPr");

        let (style_fill, style_border, style_text) = match look {
            TableLook::Plain => (Fill::None, None, None),
            TableLook::Grid => (
                Fill::None,
                Some(Line {
                    color: self.scheme_color("tx1"),
                    width: 1.0,
                    dash: Dash::Solid,
                    head_arrow: false,
                    tail_arrow: false,
                }),
                None,
            ),
            TableLook::Medium2 { accent } => {
                let accent_color = self.scheme_color(accent);
                let tint = |amount| apply_mods(accent_color, &[ColorMod::Tint(amount)]);
                let fill = if emphasized {
                    accent_color
                } else if banded {
                    tint(0.4)
                } else {
                    tint(0.2)
                };
                let text = emphasized.then(|| RunProps {
                    bold: Some(true),
                    fill: Some(FillSpec::Solid(ColorSpec {
                        base: ColorBase::Scheme("lt1".to_string()),
                        mods: Vec::new(),
                    })),
                    ..RunProps::default()
                });
                (
                    Fill::Solid(fill),
                    Some(Line {
                        color: self.scheme_color("lt1"),
                        width: 1.0,
                        dash: Dash::Solid,
                        head_arrow: false,
                        tail_arrow: false,
                    }),
                    text,
                )
            }
        };

        let fill = match properties.and_then(find_fill) {
            Some(spec) => self.fill(&spec, rels, None),
            None => style_fill,
        };
        let mut border = |name| match properties.and_then(|p| child(p, name)) {
            Some(ln) => {
                let spec = parse_line(ln);
                match spec.fill {
                    Some(fill_spec) => match self.fill(&fill_spec, rels, None) {
                        Fill::Solid(color) => Some(Line {
                            color,
                            width: spec.width.unwrap_or(DEFAULT_LINE_WIDTH),
                            dash: spec.dash.unwrap_or_default(),
                            head_arrow: false,
                            tail_arrow: false,
                        }),
                        _ => None,
                    },
                    None => style_border.clone(),
                }
            }
            None => style_border.clone(),
        };
        let borders = Borders {
            left: border("lnL"),
            top: border("lnT"),
            right: border("lnR"),
            bottom: border("lnB"),
        };

        let mut levels = self.default_style.clone();
        if let Some(text_props) = &style_text {
            for level in &mut levels.levels {
                level.run.overlay(text_props);
            }
        }
        let inset = |name, default: f32| {
            properties
                .and_then(|p| emu_attr(p, name))
                .unwrap_or(default)
        };
        let body_props = BodyProps {
            insets: [
                Some(inset("marL", 7.2)),
                Some(inset("marT", 3.6)),
                Some(inset("marR", 7.2)),
                Some(inset("marB", 3.6)),
            ],
            anchor: properties
                .and_then(|p| p.attribute("anchor"))
                .map(parse_anchor),
            wrap: Some(true),
            autofit: None,
        };
        let text = child(cell, "txBody").and_then(|body| {
            if let Some(list) = child(body, "lstStyle") {
                levels.overlay(&parse_list_style(list));
            }
            self.text_body(body, &levels, &body_props)
        });

        TableCell {
            text,
            fill,
            borders,
            column_span: attr_u32(cell, "gridSpan").unwrap_or(1).max(1) as usize,
            row_span: attr_u32(cell, "rowSpan").unwrap_or(1).max(1) as usize,
            merged: attr_bool(cell, "hMerge").unwrap_or(false)
                || attr_bool(cell, "vMerge").unwrap_or(false),
        }
    }

    // ── Text ──

    /// Resolve a text body's paragraphs over the list style they inherit.
    /// Returns `None` for a body with no visible text, which is how an empty
    /// placeholder looks outside PowerPoint's editing view.
    fn text_body(&mut self, body: Node, levels: &ListStyle, props: &BodyProps) -> Option<TextBody> {
        let (font_scale, spacing_reduction) = props.autofit.unwrap_or((1.0, 0.0));
        let mut counters: [Option<u32>; 9] = [None; 9];
        let mut paragraphs = Vec::new();
        let mut has_text = false;

        for paragraph in children_named(body, "p") {
            let own = child(paragraph, "pPr");
            let level = own.and_then(|p| attr_u32(p, "lvl")).unwrap_or(0).min(8) as usize;
            let mut para = levels.levels[level].clone();
            if let Some(own) = own {
                para.overlay(&parse_para_props(own));
            }

            let mut runs = Vec::new();
            for node in paragraph.children().filter(Node::is_element) {
                let name = node.tag_name().name();
                if !matches!(name, "r" | "fld" | "br") {
                    continue;
                }
                let mut run_props = para.run.clone();
                if let Some(rpr) = child(node, "rPr") {
                    run_props.overlay(&parse_run_props(rpr));
                }
                let text = match name {
                    "br" => "\n".to_string(),
                    "fld" if node.attribute("type") == Some("slidenum") => self.number.to_string(),
                    _ => child(node, "t")
                        .and_then(|t| t.text())
                        .unwrap_or("")
                        .replace(['\t', '\u{b}'], "    "),
                };
                if text.is_empty() {
                    continue;
                }
                runs.push(self.run(text, &run_props, font_scale));
            }

            let mut end_props = para.run.clone();
            if let Some(end) = child(paragraph, "endParaRPr") {
                end_props.overlay(&parse_run_props(end));
            }
            let visible = runs.iter().any(|run| !run.text.trim().is_empty());
            has_text |= visible;

            let bullet = match &para.bullet {
                Some(BulletKind::AutoNumber { scheme, start }) if visible => {
                    let n = counters[level].map_or(*start, |n| n + 1);
                    counters[level] = Some(n);
                    counters[level + 1..].fill(None);
                    Some(autonumber_label(scheme, n))
                }
                Some(BulletKind::Char(text)) if visible => {
                    counters[level..].fill(None);
                    Some(text.clone())
                }
                _ => {
                    if visible {
                        counters[level..].fill(None);
                    }
                    None
                }
            }
            .map(|text| self.bullet(text, &para, runs.first()));

            let line_spacing = match para.line_spacing.unwrap_or(Spacing::Lines(1.0)) {
                Spacing::Lines(lines) => Spacing::Lines((lines - spacing_reduction).max(0.1)),
                points => points,
            };
            paragraphs.push(Paragraph {
                align: para.align.unwrap_or_default(),
                margin_left: para.margin_left.unwrap_or(0.0),
                indent: para.indent.unwrap_or(0.0),
                space_before: para.space_before.unwrap_or(Spacing::Points(0.0)),
                space_after: para.space_after.unwrap_or(Spacing::Points(0.0)),
                line_spacing,
                bullet,
                runs,
                end_size: end_props.size.unwrap_or(DEFAULT_FONT_SIZE) * font_scale,
            });
        }

        if !has_text {
            return None;
        }
        let [left, top, right, bottom] = props.insets;
        Some(TextBody {
            insets: Insets {
                left: left.unwrap_or(7.2),
                top: top.unwrap_or(3.6),
                right: right.unwrap_or(7.2),
                bottom: bottom.unwrap_or(3.6),
            },
            anchor: props.anchor.unwrap_or_default(),
            wrap: props.wrap.unwrap_or(true),
            paragraphs,
        })
    }

    fn font_name(&self, typeface: Option<&str>) -> String {
        match typeface {
            Some(face) if face.starts_with("+mj") => self.theme.major_font.clone(),
            Some(face) if face.starts_with("+mn") || face.is_empty() => {
                self.theme.minor_font.clone()
            }
            Some(face) => face.to_string(),
            None => self.theme.minor_font.clone(),
        }
    }

    fn run(&mut self, text: String, props: &RunProps, font_scale: f32) -> Run {
        let color = match &props.fill {
            Some(FillSpec::Solid(color)) => self.color(color, None),
            Some(FillSpec::Gradient { stops, .. }) => stops
                .first()
                .map_or(Rgba::BLACK, |(_, color)| self.color(color, None)),
            Some(FillSpec::None) => Rgba::TRANSPARENT,
            _ => self.scheme_color("tx1"),
        };
        let text = if props.all_caps == Some(true) {
            text.to_uppercase()
        } else {
            text
        };
        Run {
            text,
            style: RunStyle {
                size: props.size.unwrap_or(DEFAULT_FONT_SIZE) * font_scale,
                bold: props.bold.unwrap_or(false),
                italic: props.italic.unwrap_or(false),
                underline: props.underline.unwrap_or(false),
                strike: props.strike.unwrap_or(false),
                color,
                font: self.font_name(props.font.as_deref()),
                baseline: props.baseline.unwrap_or(0.0),
                highlight: props.highlight.as_ref().map(|c| self.color(c, None)),
                spacing: props.spacing.unwrap_or(0.0) * font_scale,
            },
        }
    }

    fn bullet(&self, text: String, para: &ParaProps, first_run: Option<&Run>) -> Bullet {
        let text_size = first_run.map_or(DEFAULT_FONT_SIZE, |run| run.style.size);
        Bullet {
            text,
            color: match &para.bullet_color {
                Some(Some(color)) => self.color(color, None),
                _ => first_run.map_or(Rgba::BLACK, |run| run.style.color),
            },
            size: match para.bullet_size {
                Some(BulletSize::Points(points)) => points,
                Some(BulletSize::Relative(ratio)) => text_size * ratio,
                None => text_size,
            },
            font: match &para.bullet_font {
                Some(Some(face)) => self.font_name(Some(face)),
                _ => first_run.map_or_else(
                    || self.theme.minor_font.clone(),
                    |run| run.style.font.clone(),
                ),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    const FIXTURE: &str = "../tests-fixtures/pptx/fixture.pptx";

    fn fixture() -> Presentation {
        load_presentation(Path::new(FIXTURE)).expect("fixture parses")
    }

    fn shapes(slide: &Slide) -> Vec<(&Element, &Shape)> {
        slide
            .elements
            .iter()
            .filter_map(|element| match &element.kind {
                ElementKind::Shape(shape) => Some((element, shape)),
                _ => None,
            })
            .collect()
    }

    fn text_of(body: &TextBody) -> Vec<String> {
        body.paragraphs
            .iter()
            .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect())
            .collect()
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.01,
            "expected {expected}, got {actual}"
        );
    }

    /// A package from `(part name, content)` pairs, stored uncompressed.
    fn package(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, content) in parts {
            writer.start_file(*name, options).unwrap();
            writer.write_all(content.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;

    /// A one-slide package with `shapes` as the slide's shape tree.
    fn single_slide(shapes: &str) -> Vec<u8> {
        let root_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#;
        let presentation = format!(
            r#"<p:presentation {NS}><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="9144000" cy="5143500"/></p:presentation>"#
        );
        let presentation_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#;
        let slide =
            format!(r#"<p:sld {NS}><p:cSld><p:spTree>{shapes}</p:spTree></p:cSld></p:sld>"#);
        package(&[
            ("_rels/.rels", root_rels),
            ("ppt/presentation.xml", &presentation),
            ("ppt/_rels/presentation.xml.rels", presentation_rels),
            ("ppt/slides/slide1.xml", &slide),
        ])
    }

    fn rect_shape(id: u32, extra: &str, xfrm: &str) -> String {
        format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="s{id}" {extra}/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm {xfrm}><a:off x="1270000" y="1270000"/><a:ext cx="1270000" cy="635000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></p:spPr></p:sp>"#
        )
    }

    #[test]
    fn reads_slide_size_and_count() {
        let deck = fixture();
        assert_eq!(deck.slides.len(), 5);
        assert_close(deck.width, 720.0);
        assert_close(deck.height, 540.0);
        let numbers: Vec<usize> = deck.slides.iter().map(|s| s.number).collect();
        assert_eq!(numbers, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn title_placeholder_inherits_position_and_style() {
        let deck = fixture();
        let slide = &deck.slides[0];
        assert_eq!(slide.title.as_deref(), Some("Fixture Deck"));
        let (title_element, title) = shapes(slide)[0];
        // Geometry comes from the title layout's placeholder.
        assert_close(title_element.bounds.x, 54.0);
        assert_close(title_element.bounds.y, 167.75);
        assert_close(title_element.bounds.width, 612.0);
        let body = title.text.as_ref().unwrap();
        let paragraph = &body.paragraphs[0];
        // Alignment and size come from the master's title style, the font
        // from the theme's major font.
        assert_eq!(paragraph.align, Align::Center);
        assert_close(paragraph.runs[0].style.size, 44.0);
        assert_eq!(paragraph.runs[0].style.font, "Calibri");
        assert_eq!(body.anchor, Anchor::Middle);

        let (_, subtitle) = shapes(slide)[1];
        let run = &subtitle.text.as_ref().unwrap().paragraphs[0];
        // The layout switches the master's bullet off and tints the text.
        assert!(run.bullet.is_none());
        assert_eq!(run.runs[0].style.color, Rgba::rgb(64, 64, 64));
    }

    #[test]
    fn body_placeholder_levels_bullets_and_runs() {
        let deck = fixture();
        let (element, body) = shapes(&deck.slides[1])[1];
        // No geometry on the slide or its layout: the master's body frame.
        assert_close(element.bounds.x, 36.0);
        assert_close(element.bounds.y, 126.0);
        let body = body.text.as_ref().unwrap();
        let first = &body.paragraphs[0];
        assert_eq!(first.bullet.as_ref().unwrap().text, "•");
        assert_close(first.runs[0].style.size, 32.0);
        let second = &body.paragraphs[1];
        assert_eq!(second.bullet.as_ref().unwrap().text, "–");
        assert_close(second.margin_left, 58.5);
        assert_close(second.runs[0].style.size, 28.0);
        let mixed = &body.paragraphs[2].runs;
        assert_eq!(mixed.len(), 3);
        assert!(!mixed[0].style.bold);
        assert!(mixed[1].style.bold);
        assert_eq!(mixed[2].style.color, Rgba::rgb(0xC0, 0, 0));
    }

    #[test]
    fn shapes_resolve_theme_styles_groups_and_pictures() {
        let deck = fixture();
        let slide = &deck.slides[2];
        assert_eq!(slide.background, Fill::Solid(Rgba::rgb(0xF5, 0xF2, 0xEC)));

        let all = shapes(slide);
        let (_, styled) = all[0];
        // fillRef 3 is the theme's gradient in accent 1; fontRef makes the
        // text lt1, lnRef 1 gives the theme's thinnest line.
        assert!(matches!(&styled.fill, Fill::Gradient(g) if g.stops.len() == 2));
        assert_close(styled.line.as_ref().unwrap().width, 0.75);
        let text = styled.text.as_ref().unwrap();
        assert_eq!(text.anchor, Anchor::Middle);
        assert_eq!(text.paragraphs[0].runs[0].style.color, Rgba::WHITE);

        let (oval_element, oval) = all[1];
        assert_eq!(oval.fill, Fill::Solid(Rgba::rgb(0xC0, 0x50, 0x4D)));
        assert_close(oval_element.rotation, 30.0);

        // The group is drawn at half size from its own origin.
        let (square, _) = all[2];
        assert_close(square.bounds.x, 72.0);
        assert_close(square.bounds.y, 216.0);
        assert_close(square.bounds.width, 36.0);
        let (triangle, _) = all[3];
        assert_close(triangle.bounds.x, 126.0);

        let (_, connector) = all[4];
        let line = connector.line.as_ref().unwrap();
        assert_close(line.width, 3.0);
        assert_eq!(line.color, Rgba::rgb(0x2C, 0x6E, 0x72));
        assert!(connector.paths.iter().all(|path| !path.filled));

        let picture = slide
            .elements
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::Picture(p) => Some(p),
                _ => None,
            })
            .unwrap();
        assert_eq!(picture.media, "ppt/media/image1.png");
        assert!(deck.media[&picture.media].starts_with(b"\x89PNG"));
    }

    #[test]
    fn tables_merge_cells_and_apply_the_default_style() {
        let deck = fixture();
        let slide = &deck.slides[3];
        let table = slide
            .elements
            .iter()
            .find_map(|e| match &e.kind {
                ElementKind::Table(t) => Some(t),
                _ => None,
            })
            .unwrap();
        assert_eq!(table.columns, vec![120.0, 120.0, 120.0]);
        assert_eq!(table.rows.len(), 3);
        let header = &table.rows[0].cells;
        assert_eq!(header[0].fill, Fill::Solid(Rgba::rgb(0x4F, 0x81, 0xBD)));
        let header_run = &header[0].text.as_ref().unwrap().paragraphs[0].runs[0];
        assert!(header_run.style.bold);
        assert_eq!(header_run.style.color, Rgba::WHITE);
        assert_eq!(header[1].column_span, 2);
        assert!(header[2].merged);
        assert_eq!(table.rows[1].cells[0].row_span, 2);
        assert!(table.rows[2].cells[0].merged);
        // Banded body rows alternate between two tints of the accent.
        assert_ne!(table.rows[1].cells[1].fill, table.rows[2].cells[1].fill);

        assert!(
            slide
                .elements
                .iter()
                .any(|e| matches!(&e.kind, ElementKind::Unsupported { label } if label == "Chart"))
        );
    }

    #[test]
    fn text_features_numbering_fields_and_gradients() {
        let deck = fixture();
        let slide = &deck.slides[4];
        let Fill::Gradient(gradient) = &slide.background else {
            panic!("expected a gradient background, got {:?}", slide.background);
        };
        assert_eq!(gradient.stops.len(), 2);
        assert_close(gradient.angle, 270.0);

        let all = shapes(slide);
        let body = all[0].1.text.as_ref().unwrap();
        assert_eq!(body.anchor, Anchor::Bottom);
        assert_eq!(body.paragraphs[0].align, Align::Center);
        assert_eq!(text_of(body)[0], "Centered first line\nafter a break");
        let numbers: Vec<&str> = body.paragraphs[1..]
            .iter()
            .map(|p| p.bullet.as_ref().unwrap().text.as_str())
            .collect();
        assert_eq!(numbers, ["1.", "2.", "3."]);
        assert_close(body.paragraphs[1].runs[0].style.size, 14.0);

        let formula = all[1].1.text.as_ref().unwrap();
        assert!(!formula.wrap);
        let runs = &formula.paragraphs[0].runs;
        assert_close(runs[1].style.baseline, 0.3);
        assert_eq!(runs.last().unwrap().text, "5");
    }

    #[test]
    fn rejects_files_that_are_not_presentations() {
        let err = parse_presentation(Cursor::new(b"just some text".to_vec())).unwrap_err();
        assert!(err.contains("not a zip"), "{err}");

        let mut compound = COMPOUND_FILE_MAGIC.to_vec();
        compound.extend_from_slice(&[0; 512]);
        let err = parse_presentation(Cursor::new(compound)).unwrap_err();
        assert!(err.contains("password-protected"), "{err}");

        let word = package(&[
            (
                "_rels/.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
            ),
            ("word/document.xml", "<w:document xmlns:w=\"urn:w\"/>"),
        ]);
        let err = parse_presentation(Cursor::new(word)).unwrap_err();
        assert!(err.contains("Not a PowerPoint presentation"), "{err}");
    }

    #[test]
    fn hidden_shapes_are_skipped_and_fallback_content_is_used() {
        let tree = format!(
            "{}{}<mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><mc:Choice Requires=\"p14\">{}</mc:Choice><mc:Fallback>{}</mc:Fallback></mc:AlternateContent>",
            rect_shape(2, "", ""),
            rect_shape(3, r#"hidden="1""#, ""),
            rect_shape(4, "", r#"rot="5400000""#),
            rect_shape(5, "", r#"flipH="1""#),
        );
        let deck = parse_presentation(Cursor::new(single_slide(&tree))).unwrap();
        let elements = &deck.slides[0].elements;
        assert_eq!(elements.len(), 2);
        assert_close(elements[0].bounds.x, 100.0);
        assert_close(elements[0].bounds.width, 100.0);
        assert_close(elements[0].bounds.height, 50.0);
        // The fallback's flipped rectangle, not the choice's rotated one.
        assert!(elements[1].flip_h);
        assert_close(elements[1].rotation, 0.0);
    }

    #[test]
    fn rotated_groups_place_children_around_the_group_centre() {
        // A 200×100pt group at the origin, rotated 90°, holding a child in
        // its top-left quarter. Rotating about the group centre (100, 50)
        // moves that child's centre from (50, 25) to (125, 0).
        let tree = r#"<p:grpSp><p:nvGrpSpPr><p:cNvPr id="2" name="g"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm rot="5400000"><a:off x="0" y="0"/><a:ext cx="2540000" cy="1270000"/><a:chOff x="0" y="0"/><a:chExt cx="2540000" cy="1270000"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id="3" name="c"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1270000" cy="635000"/></a:xfrm><a:solidFill><a:srgbClr val="00FF00"/></a:solidFill></p:spPr></p:sp></p:grpSp>"#;
        let deck = parse_presentation(Cursor::new(single_slide(tree))).unwrap();
        let element = &deck.slides[0].elements[0];
        assert_close(element.bounds.x + element.bounds.width / 2.0, 125.0);
        assert_close(element.bounds.y + element.bounds.height / 2.0, 0.0);
        assert_close(element.rotation, 90.0);
    }

    #[test]
    fn relationship_targets_resolve_against_their_part() {
        assert_eq!(
            resolve_target("ppt/slides/slide1.xml", "../media/image%201.png"),
            "ppt/media/image 1.png"
        );
        assert_eq!(
            resolve_target("ppt/slides/slide1.xml", "/ppt/x.xml"),
            "ppt/x.xml"
        );
        assert_eq!(
            resolve_target("", "ppt/presentation.xml"),
            "ppt/presentation.xml"
        );
        assert_eq!(
            rels_part("ppt/slides/slide1.xml"),
            "ppt/slides/_rels/slide1.xml.rels"
        );
        assert_eq!(rels_part(""), "_rels/.rels");
    }

    #[test]
    fn colour_modifiers_follow_drawingml() {
        let accent = Rgba::rgb(0x44, 0x72, 0xC4);
        // "Lighter 40%" as PowerPoint writes it.
        let lighter = apply_mods(accent, &[ColorMod::LumMod(0.6), ColorMod::LumOff(0.4)]);
        assert!(lighter.r > accent.r && lighter.g > accent.g && lighter.b > accent.b);
        assert_eq!(
            apply_mods(Rgba::BLACK, &[ColorMod::Tint(0.75)]),
            Rgba::rgb(64, 64, 64)
        );
        assert_eq!(
            apply_mods(Rgba::WHITE, &[ColorMod::Shade(0.5)]),
            Rgba::rgb(128, 128, 128)
        );
        assert_eq!(apply_mods(Rgba::WHITE, &[ColorMod::Alpha(0.5)]).a, 128);
    }

    #[test]
    fn autonumber_labels() {
        assert_eq!(autonumber_label("arabicPeriod", 3), "3.");
        assert_eq!(autonumber_label("arabicParenR", 3), "3)");
        assert_eq!(autonumber_label("romanUcPeriod", 14), "XIV.");
        assert_eq!(autonumber_label("romanLcParenBoth", 4), "(iv)");
        assert_eq!(autonumber_label("alphaLcParenR", 28), "bb)");
        assert_eq!(autonumber_label("arabicPlain", 7), "7");
    }
}

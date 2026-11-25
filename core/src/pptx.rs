//! Read-only PowerPoint (`.pptx`) presentations: zip package loader, XML resolution, and slide assembly.

mod error;
pub use error::{PptxError, PptxResult};
use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use roxmltree::{Document, Node};

/// English Metric Units per point.
const EMU_PER_POINT: f64 = 12_700.0;
/// Largest single part — XML or media — read out of a package.
const MAX_PART_BYTES: u64 = 64 * 1024 * 1024;
/// Largest total of inflated bytes read from one package.
const MAX_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_SLIDES: usize = 5_000;
/// Maximum group nesting depth.
const MAX_TREE_DEPTH: usize = 64;
/// Font size when nothing in the style chain sets one.
const DEFAULT_FONT_SIZE: f32 = 18.0;
/// Line width when a line is drawn but its width is not given: 0.75pt.
const DEFAULT_LINE_WIDTH: f32 = 0.75;
/// Magic number of an OLE compound file (password-protected `.pptx` or `.ppt`).
const COMPOUND_FILE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

mod color;
mod fill;
mod geometry;
mod model;
mod numbering;
mod package;
mod placeholder;
mod slide;
mod slide_content;
mod table_style;
mod text;
mod theme;
mod xml;

use color::*;
use fill::*;
use geometry::*;
pub use model::*;
use numbering::*;
use package::*;
use placeholder::*;
use slide::*;
use table_style::*;
use text::*;
use theme::*;
use xml::*;

// ── Entry points ─────────────────────────────────────────────────────

/// Read the presentation at `path`.
pub fn load_presentation(path: &Path) -> PptxResult<Presentation> {
    let file = std::fs::File::open(path).map_err(|e| PptxError::Io {
        context: format!("Cannot open {}", path.display()),
        source: e,
    })?;
    parse_presentation(BufReader::new(file))
}

/// Read a presentation from any seekable byte source.
pub fn parse_presentation<R: Read + Seek>(mut reader: R) -> PptxResult<Presentation> {
    let mut magic = [0u8; 8];
    if reader.read_exact(&mut magic).is_ok() && magic == COMPOUND_FILE_MAGIC {
        return Err(
            "This presentation is password-protected or in the old .ppt format, \
             neither of which can be opened"
                .into(),
        );
    }
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|e| PptxError::from(format!("Cannot read the presentation: {e}")))?;
    let mut package = Package::open(reader)?;
    read_presentation(&mut package)
}

fn read_presentation<R: Read + Seek>(package: &mut Package<R>) -> PptxResult<Presentation> {
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
        return Err(PptxError::from(
            "Not a PowerPoint presentation (is it a Word or Excel file?)".to_string(),
        ));
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
    let default_table_style = read_default_table_style(package, &presentation_rels);

    let slide_parts: Vec<String> = child(presentation, "sldIdLst")
        .into_iter()
        .flat_map(|list| children_named(list, "sldId"))
        .filter_map(|id| presentation_rels.get(rel_attr(id, "id")?))
        .filter(|rel| rel.kind == "slide" && !rel.external)
        .map(|rel| rel.target.clone())
        .take(MAX_SLIDES)
        .collect();

    let (rels, theme_parts) = discover_slide_relationships(package, &slide_parts)?;

    let mut texts: HashMap<String, String> = HashMap::new();
    for part in rels.keys().chain(theme_parts.iter()) {
        if let Some(text) = package.read_text(part)? {
            texts.insert(part.clone(), text);
        }
    }
    let docs: HashMap<String, Document> = texts
        .iter()
        .filter_map(|(part, text)| Some((part.clone(), parse_xml(text, part).ok()?)))
        .collect();

    let (slides, media_parts) = build_presentation_slides(
        &slide_parts,
        &docs,
        &rels,
        &default_style,
        &default_table_style,
        (width, height),
    );

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

fn read_default_table_style<R: Read + Seek>(
    package: &mut Package<R>,
    presentation_rels: &Relationships,
) -> String {
    presentation_rels
        .values()
        .find(|rel| rel.kind == "tableStyles" && !rel.external)
        .and_then(|rel| package.read_text(&rel.target).ok().flatten())
        .and_then(|xml| {
            let doc = Document::parse(xml.trim_start_matches('\u{feff}')).ok()?;
            doc.root_element().attribute("def").map(str::to_string)
        })
        .unwrap_or_default()
}

fn discover_slide_relationships<R: Read + Seek>(
    package: &mut Package<R>,
    slide_parts: &[String],
) -> PptxResult<(HashMap<String, Relationships>, HashSet<String>)> {
    let mut rels: HashMap<String, Relationships> = HashMap::new();
    let mut theme_parts = HashSet::new();
    for slide in slide_parts {
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
    Ok((rels, theme_parts))
}

fn build_presentation_slides<'i>(
    slide_parts: &[String],
    docs: &HashMap<String, Document<'i>>,
    rels: &HashMap<String, Relationships>,
    default_style: &ListStyle,
    default_table_style: &str,
    (width, height): (f32, f32),
) -> (Vec<Slide>, HashSet<String>) {
    let fallback_theme = Theme::fallback();
    let mut themes: HashMap<String, Theme> = HashMap::new();
    let mut master_styles: HashMap<String, TextStyles> = HashMap::new();
    for (part, part_rels) in rels {
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
            default_style,
            text_styles: master_part.as_ref().and_then(|m| master_styles.get(m)),
            master: template(master_part.as_deref(), docs, rels),
            layout: template(layout_part.as_deref(), docs, rels),
            default_table_style,
            color_map: HashMap::new(),
            number,
            title: None,
            media: &mut media_parts,
        };
        slides.push(builder.build(doc.root_element(), slide_rels));
    }
    (slides, media_parts)
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

#[cfg(test)]
mod tests;

//! Presentation slides, prepared for drawing.
//!
//! [`md_editor_core::pptx`] reads a `.pptx` file into resolved slide content.
//! This module does the rest of the work that does not depend on the window —
//! decoding and cropping pictures, and laying out every text body and table
//! with the shaper the canvas draws with — once per load, off the UI thread.
//! Drawing a slide ([`draw`]) is then only painting shapes and glyph runs
//! that are already positioned.
//!
//! Layout is in points rather than pixels. The shaper's widths scale
//! linearly with size, so one layout serves every zoom level, and resizing
//! the window never shapes a deck's text again.

pub mod draw;
mod fonts;
#[cfg(test)]
mod preview;
pub mod text;

use std::collections::HashMap;
use std::fmt;
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use iced::widget::image::Handle;
use md_editor_core::pptx::{Crop, Element, ElementKind, Fill, Slide};

use text::{LaidTable, LaidText};

/// Longest side, in pixels, a picture is kept at. A photo straight off a
/// camera is many times larger than a slide ever shows it, and a deck of them
/// at full resolution would hold hundreds of megabytes.
const MAX_IMAGE_SIDE: u32 = 4096;

/// What drawing an element needs beyond the element itself.
pub enum Prepared {
    Nothing,
    Shape {
        text: Option<LaidText>,
        /// Key into [`LoadedDeck::images`] for a picture fill.
        image: Option<String>,
    },
    /// Key into [`LoadedDeck::images`]; `None` when the picture could not be
    /// decoded.
    Picture(Option<String>),
    Table(LaidTable),
}

pub struct PreparedSlide {
    /// Key into [`LoadedDeck::images`] for a picture background.
    pub background: Option<String>,
    /// Parallel to the slide's elements.
    pub elements: Vec<Prepared>,
    /// Ranges of the elements that can share one canvas layer, bottom first.
    /// Never empty: the first layer carries the background.
    pub layers: Vec<Range<usize>>,
}

pub struct LoadedDeck {
    /// Distinct for every load, so a canvas's cached drawing is never reused
    /// for a different deck.
    pub id: u64,
    /// Slide width in points.
    pub width: f32,
    /// Slide height in points.
    pub height: f32,
    pub slides: Vec<Slide>,
    /// Parallel to `slides`.
    pub prepared: Vec<PreparedSlide>,
    pub images: HashMap<String, Handle>,
}

impl fmt::Debug for LoadedDeck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadedDeck")
            .field("id", &self.id)
            .field("slides", &self.slides.len())
            .field("images", &self.images.len())
            .finish_non_exhaustive()
    }
}

/// Read the presentation at `path` and prepare every slide for drawing.
pub fn load_deck(path: &Path) -> Result<LoadedDeck, String> {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    let mut presentation = md_editor_core::pptx::load_presentation(path)?;
    let media = std::mem::take(&mut presentation.media);
    let metrics = fonts::Shaper;
    let mut images = Images::new(&media);
    let prepared = presentation
        .slides
        .iter()
        .map(|slide| PreparedSlide {
            background: match &slide.background {
                Fill::Picture { media } => images.handle(media, Crop::default()),
                _ => None,
            },
            elements: slide
                .elements
                .iter()
                .map(|element| prepare(element, &metrics, &mut images))
                .collect(),
            layers: layer_ranges(&slide.elements),
        })
        .collect();

    Ok(LoadedDeck {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        width: presentation.width,
        height: presentation.height,
        slides: presentation.slides,
        prepared,
        images: images.handles,
    })
}

fn prepare(element: &Element, metrics: &fonts::Shaper, images: &mut Images) -> Prepared {
    match &element.kind {
        ElementKind::Shape(shape) => Prepared::Shape {
            text: shape
                .text
                .as_ref()
                .map(|body| text::layout_text(body, element.bounds.width, metrics)),
            image: match &shape.fill {
                Fill::Picture { media } => images.handle(media, Crop::default()),
                _ => None,
            },
        },
        ElementKind::Picture(picture) => {
            Prepared::Picture(images.handle(&picture.media, picture.crop))
        }
        ElementKind::Table(table) => Prepared::Table(text::layout_table(table, metrics)),
        ElementKind::Unsupported { .. } => Prepared::Nothing,
    }
}

/// The passes a canvas layer paints in, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Pass {
    Shapes,
    Images,
    Text,
}

/// The first and last pass an element paints in.
fn passes(element: &Element) -> (Pass, Pass) {
    match &element.kind {
        ElementKind::Shape(shape) => {
            let image = matches!(shape.fill, Fill::Picture { .. });
            let lowest = if !image && (shape.fill != Fill::None || shape.line.is_some()) {
                Pass::Shapes
            } else if image {
                Pass::Images
            } else {
                Pass::Text
            };
            let highest = if shape.text.is_some() {
                Pass::Text
            } else if image {
                Pass::Images
            } else {
                Pass::Shapes
            };
            (lowest, highest.max(lowest))
        }
        ElementKind::Picture(_) => (Pass::Images, Pass::Images),
        ElementKind::Table(_) | ElementKind::Unsupported { .. } => (Pass::Shapes, Pass::Text),
    }
}

/// Split a slide's elements into runs that can share one canvas layer.
///
/// Within a layer the renderer paints every shape, then every image, then
/// every glyph, whatever order they were drawn in. Slides stack freely — a
/// caption band over a photo, a card over a text box — so a slide is drawn
/// as a stack of layers, and a new one starts wherever an element would
/// otherwise be painted under something drawn before it.
pub fn layer_ranges(elements: &[Element]) -> Vec<Range<usize>> {
    let mut layers = Vec::new();
    let mut start = 0;
    let mut top = Pass::Shapes;
    for (index, element) in elements.iter().enumerate() {
        let (lowest, highest) = passes(element);
        if index > start && lowest < top {
            layers.push(start..index);
            start = index;
            top = Pass::Shapes;
        }
        top = top.max(highest);
    }
    layers.push(start..elements.len());
    layers
}

/// Decoded pictures, keyed by media part and crop.
struct Images<'m> {
    media: &'m HashMap<String, Vec<u8>>,
    decoded: HashMap<String, Option<image::DynamicImage>>,
    handles: HashMap<String, Handle>,
}

impl<'m> Images<'m> {
    fn new(media: &'m HashMap<String, Vec<u8>>) -> Self {
        Self {
            media,
            decoded: HashMap::new(),
            handles: HashMap::new(),
        }
    }

    /// The key of `media` cut by `crop`, decoding it on first use. `None`
    /// when the part is missing or is not an image this build can decode.
    fn handle(&mut self, media: &str, crop: Crop) -> Option<String> {
        let key = if crop == Crop::default() {
            media.to_string()
        } else {
            format!(
                "{media}#{}:{}:{}:{}",
                crop.left, crop.top, crop.right, crop.bottom
            )
        };
        if self.handles.contains_key(&key) {
            return Some(key);
        }
        let bytes = self.media.get(media);
        let source = self
            .decoded
            .entry(media.to_string())
            .or_insert_with(|| bytes.and_then(|bytes| image::load_from_memory(bytes).ok()))
            .as_ref()?;
        let (width, height) = (source.width(), source.height());
        if width == 0 || height == 0 {
            return None;
        }
        // Negative crops pad the picture in PowerPoint; they are drawn uncut.
        let cut = |fraction: f32, extent: u32| (fraction.clamp(0.0, 1.0) * extent as f32) as u32;
        let (left, top) = (cut(crop.left, width), cut(crop.top, height));
        let (right, bottom) = (cut(crop.right, width), cut(crop.bottom, height));
        let mut picture = source.crop_imm(
            left.min(width - 1),
            top.min(height - 1),
            width.saturating_sub(left + right).max(1),
            height.saturating_sub(top + bottom).max(1),
        );
        if picture.width().max(picture.height()) > MAX_IMAGE_SIDE {
            picture = picture.resize(
                MAX_IMAGE_SIDE,
                MAX_IMAGE_SIDE,
                image::imageops::FilterType::Triangle,
            );
        }
        let rgba = picture.into_rgba8();
        let (w, h) = rgba.dimensions();
        self.handles
            .insert(key.clone(), Handle::from_rgba(w, h, rgba.into_raw()));
        Some(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use md_editor_core::pptx::{Picture, Rect, Rgba, Shape};

    fn element(kind: ElementKind) -> Element {
        Element {
            bounds: Rect::default(),
            rotation: 0.0,
            flip_h: false,
            flip_v: false,
            kind,
        }
    }

    fn filled_shape() -> Element {
        element(ElementKind::Shape(Shape {
            paths: Vec::new(),
            fill: Fill::Solid(Rgba::BLACK),
            line: None,
            text: None,
        }))
    }

    fn text_box() -> Element {
        let body = md_editor_core::pptx::TextBody {
            insets: Default::default(),
            anchor: Default::default(),
            wrap: true,
            paragraphs: Vec::new(),
        };
        element(ElementKind::Shape(Shape {
            paths: Vec::new(),
            fill: Fill::None,
            line: None,
            text: Some(body),
        }))
    }

    fn picture() -> Element {
        element(ElementKind::Picture(Picture {
            media: "ppt/media/a.png".to_string(),
            crop: Crop::default(),
            line: None,
        }))
    }

    #[test]
    fn layers_split_where_an_element_would_paint_under_an_earlier_one() {
        // A shape over text, and a shape over a picture, each need a layer
        // of their own; a picture over a shape and text over anything do not.
        let elements = [
            text_box(),
            filled_shape(),
            picture(),
            filled_shape(),
            text_box(),
        ];
        assert_eq!(layer_ranges(&elements), vec![0..1, 1..3, 3..5]);
        assert_eq!(layer_ranges(&[]), vec![0..0]);
        assert_eq!(
            layer_ranges(&[filled_shape(), picture(), text_box()]),
            vec![0..3]
        );
    }
}

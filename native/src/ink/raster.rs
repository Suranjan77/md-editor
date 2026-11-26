//! A handwritten page as a picture: its strokes rasterized for a note that embeds the `.ink` file.

use super::document::{InkDocument, StrokeKind};
use super::stroke::{self, StrokeOptions, Vec2};
use crate::raster::Decoded;

/// Space kept around the ink, in page units (logical pixels).
const MARGIN: f32 = 16.0;
/// Pixels per page unit: sharp on a 2× display.
const DENSITY: f32 = 2.0;
/// Longest side of the bitmap; a huge page renders at a lower density.
const MAX_SIDE_PX: f32 = 4096.0;

/// Render `doc` cropped to its ink plus a margin, on a transparent ground (a
/// note shows it on its own background, as the ink window does). The layout
/// size is the ink's size in logical pixels. `None` for a page with no ink.
pub fn render(doc: &InkDocument) -> Option<Decoded> {
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
    let (min, max) = outlines
        .iter()
        .filter_map(|(outline, ..)| stroke::bounds(outline))
        .reduce(|(a0, a1), (b0, b1)| {
            (
                Vec2::new(a0.x.min(b0.x), a0.y.min(b0.y)),
                Vec2::new(a1.x.max(b1.x), a1.y.max(b1.y)),
            )
        })?;
    let origin = Vec2::new(min.x - MARGIN, min.y - MARGIN);
    let (width, height) = (max.x - min.x + 2.0 * MARGIN, max.y - min.y + 2.0 * MARGIN);
    let density = DENSITY.min(MAX_SIDE_PX / width.max(height).max(1.0));
    let pixel_width = (width * density).ceil().max(1.0) as u32;
    let pixel_height = (height * density).ceil().max(1.0) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(pixel_width, pixel_height)?;
    let transform = resvg::tiny_skia::Transform::from_row(
        density,
        0.0,
        0.0,
        density,
        -origin.x * density,
        -origin.y * density,
    );

    // Highlighter under writing, as on the canvas.
    for pass in [StrokeKind::Highlighter, StrokeKind::Pen] {
        for (outline, color, _) in outlines.iter().filter(|(.., kind)| *kind == pass) {
            let mut path = resvg::tiny_skia::PathBuilder::new();
            path.move_to(outline[0].x, outline[0].y);
            for vertex in &outline[1..] {
                path.line_to(vertex.x, vertex.y);
            }
            path.close();
            let Some(path) = path.finish() else {
                continue;
            };
            let mut paint = resvg::tiny_skia::Paint::default();
            paint.set_color_rgba8(
                channel(color[0]),
                channel(color[1]),
                channel(color[2]),
                channel(color[3]),
            );
            paint.anti_alias = true;
            pixmap.fill_path(
                &path,
                &paint,
                resvg::tiny_skia::FillRule::Winding,
                transform,
                None,
            );
        }
    }

    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let c = pixel.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Some(Decoded {
        rgba,
        pixel_width,
        pixel_height,
        width,
        height,
    })
}

fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests;

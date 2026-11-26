//! Tests for `ink::raster`.

use super::*;
use crate::ink::document::InkStroke;
use crate::ink::stroke::InkPoint;

fn line(from: (f32, f32), to: (f32, f32), color: [f32; 4], kind: StrokeKind) -> InkStroke {
    let points: Vec<InkPoint> = (0..=20)
        .map(|i| {
            let t = i as f32 / 20.0;
            InkPoint::new(
                from.0 + (to.0 - from.0) * t,
                from.1 + (to.1 - from.1) * t,
                0.8,
            )
        })
        .collect();
    InkStroke::from_points(&points, 6.0, color, kind)
}

fn pixel(image: &Decoded, x: f32, y: f32) -> [u8; 4] {
    let density = image.pixel_width as f32 / image.width;
    let (px, py) = ((x * density) as usize, (y * density) as usize);
    let i = (py * image.pixel_width as usize + px) * 4;
    image.rgba[i..i + 4].try_into().unwrap()
}

#[test]
fn a_page_renders_cropped_to_its_ink_with_a_transparent_ground() {
    let mut doc = InkDocument::new();
    // Far from the origin: the picture starts at the ink, not at (0, 0).
    doc.push(line(
        (1000.0, 500.0),
        (1200.0, 500.0),
        [1.0, 0.0, 0.0, 1.0],
        StrokeKind::Pen,
    ));
    doc.push(line(
        (1000.0, 600.0),
        (1200.0, 600.0),
        [0.0, 0.0, 1.0, 1.0],
        StrokeKind::Pen,
    ));
    let image = render(&doc).expect("ink to show");

    // The ink is 200 wide and 100 tall between centre lines, plus at most the
    // pen's width (tapering and pressure make it thinner; smoothing ends a
    // stroke slightly short, as on the canvas), plus the margins.
    let ink_w = image.width - 2.0 * MARGIN;
    let ink_h = image.height - 2.0 * MARGIN;
    assert!((185.0..=206.0).contains(&ink_w), "{ink_w}");
    assert!((100.0..=106.0).contains(&ink_h), "{ink_h}");
    assert_eq!(image.pixel_width, (image.width * DENSITY).ceil() as u32);
    assert_eq!(
        image.rgba.len(),
        (image.pixel_width * image.pixel_height * 4) as usize
    );

    let top = MARGIN + (ink_h - 100.0) / 2.0; // the first stroke's centre line
    let red = pixel(&image, image.width / 2.0, top);
    assert!(red[0] > 200 && red[2] < 60 && red[3] > 200, "{red:?}");
    let blue = pixel(&image, image.width / 2.0, top + 100.0);
    assert!(blue[2] > 200 && blue[0] < 60, "{blue:?}");
    assert_eq!(
        pixel(&image, image.width / 2.0, top + 50.0)[3],
        0,
        "between the lines: clear"
    );
}

#[test]
fn highlighter_sits_under_writing_and_an_empty_page_is_nothing() {
    assert!(render(&InkDocument::new()).is_none());

    let mut doc = InkDocument::new();
    doc.push(line(
        (0.0, 0.0),
        (100.0, 0.0),
        [0.0, 0.0, 1.0, 1.0],
        StrokeKind::Pen,
    ));
    // Drawn later but painted first.
    doc.push(line(
        (0.0, 0.0),
        (100.0, 0.0),
        [1.0, 1.0, 0.0, 1.0],
        StrokeKind::Highlighter,
    ));
    let image = render(&doc).unwrap();
    let centre = pixel(&image, image.width / 2.0, image.height / 2.0);
    assert!(centre[2] > 200 && centre[0] < 60, "pen on top: {centre:?}");
}

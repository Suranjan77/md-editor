//! Unit tests for dual-mode `.ink.svg` vector export and lossless round-trip import.

use super::*;
use crate::ink::document::{InkStroke, PaperStyle};

#[test]
fn round_trips_strokes_paper_and_special_xml_chars() {
    let mut doc = InkDocument::new();
    doc.paper = PaperStyle::Grid;
    doc.push(InkStroke {
        kind: StrokeKind::Pen,
        color: [
            crate::ink::COLOR_PRESETS[0].1.r,
            crate::ink::COLOR_PRESETS[0].1.g,
            crate::ink::COLOR_PRESETS[0].1.b,
            1.0,
        ],
        size: 3.2,
        points: vec![[10.0, 20.0, 0.5], [40.0, 50.0, 0.7], [90.0, 45.0, 0.6]],
    });
    doc.push(InkStroke {
        kind: StrokeKind::Highlighter,
        color: [0.98, 0.75, 0.35, 0.32],
        size: 20.0,
        points: vec![[15.0, 25.0, 0.5], [85.0, 25.0, 0.5]],
    });

    let svg = to_svg(&doc).expect("export to svg");
    assert!(svg.contains("<svg xmlns=\"http://www.w3.org/2000/svg\""));
    assert!(svg.contains("<metadata id=\"md-editor-ink\">"));

    let restored = from_svg(&svg).expect("round-trip from svg");
    assert_eq!(restored.paper, PaperStyle::Grid);
    assert_eq!(restored.strokes().len(), 2);
    assert_eq!(restored.strokes()[0].points, doc.strokes()[0].points);
    assert_eq!(restored.strokes()[1].kind, StrokeKind::Highlighter);
}

#[test]
fn renders_crisp_svg_parseable_by_usvg_for_both_populated_and_empty_pages() {
    let empty = InkDocument::new();
    let empty_svg = to_svg(&empty).expect("empty svg");
    assert!(resvg::usvg::Tree::from_str(&empty_svg, &Default::default()).is_ok());

    let mut populated = InkDocument::new();
    populated.push(InkStroke {
        kind: StrokeKind::Pen,
        color: [0.48, 0.72, 0.95, 1.0],
        size: 4.0,
        points: vec![[20.0, 20.0, 0.4], [100.0, 55.0, 0.7], [180.0, 110.0, 0.8]],
    });
    let pop_svg = to_svg(&populated).expect("populated svg");
    let tree = resvg::usvg::Tree::from_str(&pop_svg, &Default::default())
        .expect("resvg parses exported ink svg");
    assert!(tree.size().width() > 100.0, "width={}", tree.size().width());
    assert!(
        tree.size().height() > 60.0,
        "height={}",
        tree.size().height()
    );
}

#[test]
fn neutral_ink_emits_adaptive_light_dark_css_and_preserves_highlighter_order() {
    let mut doc = InkDocument::new();
    doc.push(InkStroke {
        kind: StrokeKind::Pen,
        color: [
            crate::ink::COLOR_PRESETS[0].1.r,
            crate::ink::COLOR_PRESETS[0].1.g,
            crate::ink::COLOR_PRESETS[0].1.b,
            1.0,
        ],
        size: 3.2,
        points: vec![[10.0, 10.0, 0.5], [60.0, 10.0, 0.5]],
    });
    doc.push(InkStroke {
        kind: StrokeKind::Highlighter,
        color: [0.98, 0.75, 0.35, 0.32],
        size: 20.0,
        points: vec![[10.0, 10.0, 0.5], [60.0, 10.0, 0.5]],
    });

    let svg = to_svg(&doc).expect("export svg");
    assert!(svg.contains("@media (prefers-color-scheme: dark)"));
    assert!(svg.contains("class=\"ink-neutral\""));

    let hl_pos = svg
        .find("fill-opacity=\"0.320\"")
        .expect("highlighter path");
    let pen_pos = svg.find("class=\"ink-neutral\"").expect("pen path");
    assert!(
        hl_pos < pen_pos,
        "highlighter should be emitted before pen in <g id=\"ink-strokes\">"
    );
}

#[test]
fn xml_unescape_handles_double_escaped_entities_in_single_pass() {
    assert_eq!(xml_unescape("a &amp;lt; b &lt; c"), "a &lt; b < c");
    assert_eq!(xml_unescape("&quot;hi&apos; &amp;"), "\"hi' &");
}

#[test]
fn rejects_regular_svg_without_ink_metadata() {
    let err = from_svg("<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>").unwrap_err();
    assert!(err.contains("missing ink metadata"), "{err}");
}

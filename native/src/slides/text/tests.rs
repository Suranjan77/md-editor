//! Tests for `text`.

use super::*;
use md_editor_core::pptx::{Borders, Bullet, Fill, Insets, Run, RunStyle, TableCell, TableRow};

/// Every character is half its font size wide, in one default font.
struct Fixed;

impl TextMetrics for Fixed {
    fn font(&self, _: &str, _: bool, _: bool) -> Font {
        Font::DEFAULT
    }
    fn width(&self, text: &str, _: Font, size: f32) -> f32 {
        text.chars().count() as f32 * size * 0.5
    }
    fn vertical(&self, _: Font) -> (f32, f32) {
        (0.8, 0.2)
    }
}

fn run(text: &str, size: f32) -> Run {
    Run {
        text: text.to_string(),
        style: RunStyle {
            size,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            color: Rgba::BLACK,
            font: "Arial".to_string(),
            baseline: 0.0,
            highlight: None,
            spacing: 0.0,
        },
    }
}

fn paragraph(runs: Vec<Run>) -> Paragraph {
    Paragraph {
        align: Align::Left,
        margin_left: 0.0,
        indent: 0.0,
        space_before: Spacing::Points(0.0),
        space_after: Spacing::Points(0.0),
        line_spacing: Spacing::Lines(1.0),
        bullet: None,
        runs,
        end_size: 10.0,
    }
}

fn body(paragraphs: Vec<Paragraph>) -> TextBody {
    TextBody {
        insets: Insets::default(),
        anchor: Anchor::Top,
        wrap: true,
        paragraphs,
    }
}

fn texts(laid: &LaidText) -> Vec<(&str, f32)> {
    laid.runs.iter().map(|r| (r.text.as_str(), r.x)).collect()
}

#[test]
fn wraps_words_at_the_box_width() {
    // Characters are 5pt wide: "aaaa " and "bbbb" fill 45 of 50pt.
    let laid = layout_text(
        &body(vec![paragraph(vec![run("aaaa bbbb cccc", 10.0)])]),
        50.0,
        &Fixed,
    );
    assert_eq!(texts(&laid), [("aaaa bbbb", 0.0), ("cccc", 0.0)]);
    assert!(laid.runs[1].y > laid.runs[0].y);
    assert!(
        (laid.height - 24.0).abs() < 1e-3,
        "two 12pt lines, got {}",
        laid.height
    );
}

#[test]
fn aligns_lines_within_the_box() {
    let mut centered = paragraph(vec![run("ab", 10.0)]);
    centered.align = Align::Center;
    let mut right = paragraph(vec![run("ab", 10.0)]);
    right.align = Align::Right;
    let laid = layout_text(&body(vec![centered, right]), 50.0, &Fixed);
    assert_eq!(texts(&laid), [("ab", 20.0), ("ab", 40.0)]);
}

#[test]
fn hanging_bullets_sit_left_of_wrapped_text() {
    let mut item = paragraph(vec![run("aaaa bbbb", 10.0)]);
    item.margin_left = 20.0;
    item.indent = -20.0;
    item.bullet = Some(Bullet {
        text: "•".to_string(),
        color: Rgba::BLACK,
        size: 10.0,
        font: "Arial".to_string(),
    });
    let laid = layout_text(&body(vec![item]), 60.0, &Fixed);
    assert_eq!(texts(&laid), [("•", 0.0), ("aaaa", 20.0), ("bbbb", 20.0)]);
}

#[test]
fn breaks_words_wider_than_a_line() {
    let laid = layout_text(
        &body(vec![paragraph(vec![run("aaaaaaaaaaaa", 10.0)])]),
        25.0,
        &Fixed,
    );
    let pieces: Vec<&str> = laid.runs.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(pieces, ["aaaaa", "aaaaa", "aa"]);
}

#[test]
fn breaks_and_empty_paragraphs_take_a_line_each() {
    let laid = layout_text(
        &body(vec![
            paragraph(vec![run("one", 10.0), run("\n", 10.0), run("two", 10.0)]),
            paragraph(Vec::new()),
        ]),
        100.0,
        &Fixed,
    );
    assert_eq!(texts(&laid), [("one", 0.0), ("two", 0.0)]);
    assert!(
        (laid.height - 36.0).abs() < 1e-3,
        "three 12pt lines, got {}",
        laid.height
    );
}

#[test]
fn runs_of_different_sizes_share_a_baseline() {
    let laid = layout_text(
        &body(vec![paragraph(vec![run("big ", 20.0), run("small", 10.0)])]),
        200.0,
        &Fixed,
    );
    let (big, small) = (&laid.runs[0], &laid.runs[1]);
    assert_eq!(small.x, 40.0);
    // With ascent 0.8 and descent 0.2 the glyph box's top sits 0.8 of
    // the size above the baseline, so both runs end at the same line.
    let baseline = |r: &PlacedRun| r.y + r.size * 0.8;
    assert!((baseline(big) - baseline(small)).abs() < 1e-3);
}

#[test]
fn letter_spacing_places_each_character() {
    let mut spaced = run("ab c", 10.0);
    spaced.style.spacing = 2.0;
    let laid = layout_text(&body(vec![paragraph(vec![spaced])]), 100.0, &Fixed);
    // Each character advances 5pt plus 2pt of tracking; the space between
    // the words takes its advance but draws nothing.
    assert_eq!(texts(&laid), [("a", 0.0), ("b", 7.0), ("c", 21.0)]);
}

#[test]
fn anchoring_places_text_in_its_box() {
    let mut text = body(vec![paragraph(vec![run("a", 10.0)])]);
    text.insets = Insets {
        left: 0.0,
        top: 4.0,
        right: 0.0,
        bottom: 4.0,
    };
    assert_eq!(anchor_offset(&text, 100.0, 12.0), 4.0);
    text.anchor = Anchor::Middle;
    assert_eq!(anchor_offset(&text, 100.0, 12.0), 44.0);
    text.anchor = Anchor::Bottom;
    assert_eq!(anchor_offset(&text, 100.0, 12.0), 84.0);
}

#[test]
fn table_rows_grow_to_fit_their_text() {
    let cell = |text: Option<&str>| TableCell {
        text: text.map(|t| body(vec![paragraph(vec![run(t, 10.0)])])),
        fill: Fill::None,
        borders: Borders::default(),
        column_span: 1,
        row_span: 1,
        merged: false,
    };
    let table = Table {
        columns: vec![25.0, 50.0],
        rows: vec![
            TableRow {
                height: 5.0,
                cells: vec![cell(Some("aaaa bbbb")), cell(None)],
            },
            TableRow {
                height: 30.0,
                cells: vec![cell(Some("a")), cell(Some("b"))],
            },
        ],
    };
    let laid = layout_table(&table, &Fixed);
    assert_eq!(laid.column_x, [0.0, 25.0, 75.0]);
    // The first row wraps to two 12pt lines; the second keeps its height.
    assert_eq!(laid.row_y, [0.0, 24.0, 54.0]);
    assert!(laid.cells[0][1].is_none());
}

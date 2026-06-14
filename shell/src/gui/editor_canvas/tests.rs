use md3_editor::layout::{ConcealMode, Measurer, StyledLine};
use md3_editor::parse::LineKind;
use md3_editor::style::{Span, SpanKind};

use super::{
    LINE_HEIGHT, MAX_READING_WIDTH, MIN_PAGE_MARGIN, MonoMeasurer, content_left, content_width,
};

#[test]
fn reading_column_caps_and_centers_on_wide_panes() {
    assert_eq!(content_width(1200.0), MAX_READING_WIDTH);
    assert_eq!(content_left(1200.0), 180.0);
    assert_eq!(content_width(800.0), 800.0 - MIN_PAGE_MARGIN * 2.0);
    assert_eq!(content_left(800.0), MIN_PAGE_MARGIN);
}

#[test]
fn image_height_comes_from_rendered_asset() {
    let measurer = MonoMeasurer::default();
    measurer.set_image_size("plot.png".to_string(), 400.0, 300.0);
    let line = StyledLine {
        display: "![plot](plot.png)".to_string(),
        conceal: ConcealMode::Concealed,
        kind: LineKind::Paragraph,
        spans: vec![Span {
            range: 0..17,
            kind: SpanKind::Image {
                url: "plot.png".to_string(),
            },
        }],
    };

    let measured = measurer.measure(&line, 800.0);
    assert!(measured.height > f64::from(LINE_HEIGHT * 10.0));
}

#[test]
fn inline_math_stays_on_text_row_when_it_fits() {
    let measurer = MonoMeasurer::default();
    measurer.set_math_size("x^2".to_string(), 40.0, 24.0);
    let line = StyledLine {
        display: "value $x^2$ here".to_string(),
        conceal: ConcealMode::Concealed,
        kind: LineKind::Paragraph,
        spans: vec![
            Span {
                range: 0..6,
                kind: SpanKind::Text,
            },
            Span {
                range: 6..11,
                kind: SpanKind::Math,
            },
            Span {
                range: 11..16,
                kind: SpanKind::Text,
            },
        ],
    };

    let measured = measurer.measure(&line, 800.0);
    assert_eq!(measured.rows, 1);
    assert_eq!(measured.height, f64::from(LINE_HEIGHT + 6.0));
}

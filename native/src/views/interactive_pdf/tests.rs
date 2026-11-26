//! Tests for `interactive_pdf`.

use super::*;
use md_editor_core::pdf::{PdfPageText, PdfRect, PdfTextLine};

fn page(text: &str, lines: &[(usize, usize)]) -> PdfPageText {
    PdfPageText {
        offsets: Default::default(),
        page_index: 0,
        page_width: 600.0,
        page_height: 800.0,
        text: text.to_string(),
        chars: Vec::new(),
        lines: lines
            .iter()
            .map(|&(start, end)| PdfTextLine {
                start_text_index: start,
                end_text_index: end,
                bbox: PdfRect {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
            })
            .collect(),
    }
}

#[test]
fn a_double_click_selects_the_word_under_it() {
    let text = page("the gradient's norm, (x_1)", &[]);
    // "gradient's" spans 4..=13; a click anywhere inside takes all of it.
    assert_eq!(word_around(&text, 6), Some((4, 13)));
    assert_eq!(word_around(&text, 13), Some((4, 13)));
    // "x_1" inside parentheses: the brackets are not part of the word.
    assert_eq!(word_around(&text, 23), Some((22, 24)));
    // A space or comma selects only itself.
    assert_eq!(word_around(&text, 3), Some((3, 3)));
    assert_eq!(word_around(&text, 19), Some((19, 19)));
    assert_eq!(word_around(&text, 99), None);
}

#[test]
fn a_quoted_word_leaves_its_quotes_out() {
    let text = page("say 'adam' now", &[]);
    assert_eq!(word_around(&text, 6), Some((5, 8)));
}

#[test]
fn a_triple_click_selects_the_line_under_it() {
    let text = page("first line\nsecond line", &[(0, 10), (11, 22)]);
    assert_eq!(line_around(&text, 3), Some((0, 9)));
    assert_eq!(line_around(&text, 15), Some((11, 21)));
    assert_eq!(
        line_around(&text, 10),
        None,
        "the line break belongs to no line"
    );
}

#[test]
fn an_alt_drag_region_is_a_clamped_fraction_of_the_page_either_way_round() {
    use iced::{Point, Size};
    let size = Size::new(600.0, 800.0);
    let region = region_fraction(Point::new(450.0, 600.0), Point::new(150.0, 200.0), size).unwrap();
    assert_eq!(
        (region.x, region.y, region.width, region.height),
        (0.25, 0.25, 0.5, 0.5),
        "dragged up-left"
    );
    let past_edge =
        region_fraction(Point::new(300.0, 400.0), Point::new(900.0, -50.0), size).unwrap();
    assert_eq!(
        (past_edge.x, past_edge.y, past_edge.width, past_edge.height),
        (0.5, 0.0, 0.5, 0.5)
    );
    assert_eq!(
        region_fraction(Point::new(10.0, 10.0), Point::new(14.0, 300.0), size),
        None,
        "too narrow to mean a region"
    );
}

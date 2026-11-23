use super::*;
use crate::pdf::{PdfTextChar, PdfTextLine};

const CW: f32 = 6.0;
const CH: f32 = 10.0;
const PAGE_W: f32 = 600.0;
const PAGE_H: f32 = 800.0;

/// Build a page from `(text, left_x, baseline_y)` lines. `y` is bottom-left
/// origin (the char convention). A newline char terminates each line.
fn page(index: u16, lines: &[(&str, f32, f32)]) -> PdfPageText {
    let mut text = String::new();
    let mut chars: Vec<PdfTextChar> = Vec::new();
    let mut text_lines = Vec::new();
    for (s, x0, y) in lines {
        let start = chars.len();
        let mut x = *x0;
        for c in s.chars() {
            text.push(c);
            chars.push(PdfTextChar {
                char_index: chars.len() as u32,
                text_index: chars.len(),
                ch: c,
                bbox: PdfRect {
                    x,
                    y: *y,
                    width: CW,
                    height: CH,
                },
            });
            x += CW;
        }
        text.push('\n');
        chars.push(PdfTextChar {
            char_index: chars.len() as u32,
            text_index: chars.len(),
            ch: '\n',
            bbox: PdfRect {
                x,
                y: *y,
                width: 0.0,
                height: 0.0,
            },
        });
        let end = chars.len();
        text_lines.push(PdfTextLine {
            start_text_index: start,
            end_text_index: end,
            bbox: PdfRect {
                x: *x0,
                y: *y,
                width: s.chars().count() as f32 * CW,
                height: CH,
            },
        });
    }
    PdfPageText {
        page_index: index,
        page_width: PAGE_W,
        page_height: PAGE_H,
        text,
        chars,
        lines: text_lines,
    }
}

#[test]
fn paren_number_finder() {
    let got = find_paren_numbers("see (3.14) and (12) and (3.14a) but not (x) or ()");
    let labels: Vec<&str> = got.iter().map(|(_, _, l)| l.as_str()).collect();
    assert_eq!(labels, vec!["3.14", "12", "3.14"]);
}

#[test]
fn equation_reference_resolves_to_right_aligned_label() {
    // p0: prose mentions (3.14); p1: the display equation with the label
    // right-aligned (x in the right margin).
    let p0 = page(0, &[("as shown in (3.14) we have", 50.0, 700.0)]);
    let label_x = PAGE_W * 0.7; // right margin
    let p1 = page(1, &[("E = mc^2", 80.0, 500.0), ("(3.14)", label_x, 500.0)]);
    let links = resolve_references(&[p0, p1], &[]);
    let eq: Vec<_> = links
        .iter()
        .filter(|l| l.kind == ReferenceKind::Equation)
        .collect();
    assert_eq!(
        eq.len(),
        1,
        "exactly one call-site link, not the label itself"
    );
    assert_eq!(eq[0].src_page, 0);
    assert_eq!(eq[0].dest_page, 1);
    assert_eq!(eq[0].label, "3.14");
}

#[test]
fn ambiguous_equation_label_is_dropped() {
    // Same label right-aligned on two pages ⇒ unsafe ⇒ no link.
    let x = PAGE_W * 0.7;
    let p0 = page(0, &[("ref (1)", 50.0, 700.0)]);
    let p1 = page(1, &[("(1)", x, 500.0)]);
    let p2 = page(2, &[("(1)", x, 400.0)]);
    let links = resolve_references(&[p0, p1, p2], &[]);
    assert!(links.iter().all(|l| l.kind != ReferenceKind::Equation));
}

#[test]
fn interval_without_target_is_not_linked() {
    // "(3.14)" appears only in prose, no right-aligned label ⇒ no target.
    let p0 = page(0, &[("the interval (3.14) is open", 50.0, 700.0)]);
    let links = resolve_references(&[p0], &[]);
    assert!(links.is_empty());
}

#[test]
fn figure_reference_resolves_but_caption_does_not_selflink() {
    let p0 = page(0, &[("see Fig. 2 for details", 50.0, 700.0)]);
    let p1 = page(1, &[("Figure 2: the architecture", 50.0, 500.0)]);
    let links = resolve_references(&[p0, p1], &[]);
    let figs: Vec<_> = links
        .iter()
        .filter(|l| l.kind == ReferenceKind::Figure)
        .collect();
    assert_eq!(figs.len(), 1);
    assert_eq!(figs[0].src_page, 0);
    assert_eq!(figs[0].dest_page, 1);
    assert_eq!(figs[0].label, "figure 2");
}

#[test]
fn citation_year_is_not_an_equation() {
    // A right-aligned "(2003)" must not become an equation target, and a
    // prose "(2003)" citation year must not link.
    let x = PAGE_W * 0.7;
    let p0 = page(0, &[("Strang (2003) and others", 50.0, 700.0)]);
    let p1 = page(1, &[("(2003)", x, 500.0)]);
    let links = resolve_references(&[p0, p1], &[]);
    assert!(links.iter().all(|l| l.kind != ReferenceKind::Equation));
}

#[test]
fn dotted_figure_numbers_stay_distinct() {
    // Figure 1.1 and Figure 1.2 must not collapse to "figure 1".
    let p0 = page(0, &[("see Figure 1.2 and Figure 1.1", 50.0, 700.0)]);
    let p1 = page(
        1,
        &[
            ("Figure 1.1 first", 50.0, 600.0),
            ("Figure 1.2 second", 50.0, 400.0),
        ],
    );
    let links = resolve_references(&[p0, p1], &[]);
    let labels: std::collections::HashSet<_> = links
        .iter()
        .filter(|l| l.kind == ReferenceKind::Figure)
        .map(|l| l.label.clone())
        .collect();
    assert!(labels.contains("figure 1.1"), "got {labels:?}");
    assert!(labels.contains("figure 1.2"), "got {labels:?}");
}

#[test]
fn table_reference_resolves() {
    let p0 = page(0, &[("results in Table 3 confirm", 50.0, 700.0)]);
    let p1 = page(1, &[("Table 3 Summary of results", 50.0, 500.0)]);
    let links = resolve_references(&[p0, p1], &[]);
    let t: Vec<_> = links
        .iter()
        .filter(|l| l.kind == ReferenceKind::Table)
        .collect();
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].label, "table 3");
}

#[test]
fn section_reference_resolves_via_toc() {
    let toc = vec![TocEntry {
        title: "3.2 Gradient Descent".to_string(),
        page_index: Some(42),
        children: vec![],
    }];
    let p0 = page(0, &[("recall Section 3.2 for the method", 50.0, 700.0)]);
    let links = resolve_references(&[p0], &toc);
    let s: Vec<_> = links
        .iter()
        .filter(|l| l.kind == ReferenceKind::Section)
        .collect();
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].dest_page, 42);
    assert_eq!(s[0].label, "3.2");
}

#[test]
fn bare_chapter_number_is_not_a_section_target() {
    let toc = vec![TocEntry {
        title: "5 Optimization".to_string(),
        page_index: Some(10),
        children: vec![],
    }];
    let p0 = page(0, &[("see Section 5 later", 50.0, 700.0)]);
    let links = resolve_references(&[p0], &toc);
    assert!(links.iter().all(|l| l.kind != ReferenceKind::Section));
}

#[test]
fn callsite_bbox_is_top_left_and_over_the_token() {
    let p0 = page(0, &[("xx (1.1) yy", 50.0, 700.0)]);
    let x = PAGE_W * 0.7;
    let p1 = page(1, &[("(1.1)", x, 500.0)]);
    let links = resolve_references(&[p0, p1], &[]);
    let l = links
        .iter()
        .find(|l| l.kind == ReferenceKind::Equation)
        .unwrap();
    // "(1.1)" starts after "xx " = 3 chars ⇒ x ≈ 50 + 3*CW.
    assert!(
        (l.bbox.x - (50.0 + 3.0 * CW)).abs() < 1.0,
        "bbox.x={}",
        l.bbox.x
    );
    // top-left y for a line with baseline y=700,h=10 ⇒ 800-(700+10)=90.
    assert!((l.bbox.y - 90.0).abs() < 1.0, "bbox.y={}", l.bbox.y);
}

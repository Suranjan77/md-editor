#![cfg(feature = "pdfium")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use md_pdf::CharBox;
use md_pdf::render::PdfRenderer;
use md_pdf::select::select;
use std::path::{Path, PathBuf};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests-fixtures/pdf")
}

fn renderer() -> Option<PdfRenderer> {
    // Dev machines can point at a local libpdfium via PDFIUM_LIB_DIR; otherwise
    // bind the system library, or skip (not fail) when neither is present.
    let lib_dir = std::env::var_os("PDFIUM_LIB_DIR").map(PathBuf::from);
    PdfRenderer::new(lib_dir.as_deref())
        .or_else(|_| PdfRenderer::new(None))
        .ok()
}

fn group_into_lines(chars: &[CharBox]) -> Vec<Vec<CharBox>> {
    let mut lines = Vec::new();
    let mut current_line: Vec<CharBox> = Vec::new();
    for c in chars {
        if c.ch == '\r' || c.ch == '\n' {
            continue;
        }
        if let Some(last) = current_line.last() {
            let mid_y_c = (c.y0 + c.y1) / 2.0;
            let mid_y_l = (last.y0 + last.y1) / 2.0;
            if (mid_y_c - mid_y_l).abs() > 4.0 {
                lines.push(current_line);
                current_line = Vec::new();
            }
        }
        current_line.push(*c);
    }
    if !current_line.is_empty() {
        lines.push(current_line);
    }
    lines
}

#[test]
fn test_real_glyph_selection() {
    let Some(r) = renderer() else {
        eprintln!("skipping: libpdfium not available");
        return;
    };

    let fixtures = vec![
        ("single-page.pdf", 3),   // 3 lines
        ("tight-leading.pdf", 6), // 6 lines
        ("two-column.pdf", 8),    // 4 lines per col, 2 cols = 8 lines total
    ];

    for (name, expected_lines) in fixtures {
        let path = fixture_dir().join(name);
        let chars = r.page_chars(&path, 0).expect("should load page chars");
        assert!(!chars.is_empty(), "chars should not be empty for {name}");

        // 1. Every box satisfies bounds rules
        for c in &chars {
            assert!(c.x0 < c.x1, "x0 < x1 for {} in {name}", c.ch);
            assert!(c.y0 <= c.y1, "y0 <= y1 for {} in {name}", c.ch);
            // bounds check within page points ± 1pt (612x792)
            assert!(
                c.x0 >= -1.0 && c.x1 <= 613.0,
                "x bounds violation for {} in {name}: x0={}, x1={}",
                c.ch,
                c.x0,
                c.x1
            );
            assert!(
                c.y0 >= -1.0 && c.y1 <= 793.0,
                "y bounds violation for {} in {name}: y0={}, y1={}",
                c.ch,
                c.y0,
                c.y1
            );
        }

        let lines = group_into_lines(&chars);
        assert_eq!(
            lines.len(),
            expected_lines,
            "unexpected line count for {name}"
        );

        // 2. Same-line drag on third line
        // (Single page third line: "Used for...", Tight leading third line: "Tight-leading line 3", Two column third line: "Col 1 Line 3")
        let line3_chars = &lines[2];
        let first = line3_chars.first().unwrap();
        let last = line3_chars.last().unwrap();

        let drag_start = (first.x0, (first.y0 + first.y1) / 2.0);
        let drag_end = (last.x1, (last.y0 + last.y1) / 2.0);

        let sel = select(&chars, drag_start, drag_end).expect("selection should succeed");

        let expected_text: String = line3_chars.iter().map(|c| c.ch).collect();
        // Compare trimmed since there might be trailing whitespace synthesized
        assert_eq!(
            sel.text.trim(),
            expected_text.trim(),
            "text mismatch on third line of {name}"
        );
        assert_eq!(
            sel.quads.len(),
            1,
            "quad count should be 1 for same-line drag in {name}"
        );

        // Check vertical band of the selection quad
        let line_y0 = line3_chars
            .iter()
            .map(|c| c.y0)
            .fold(f32::INFINITY, f32::min);
        let line_y1 = line3_chars
            .iter()
            .map(|c| c.y1)
            .fold(f32::NEG_INFINITY, f32::max);
        let quad = &sel.quads[0];
        assert!(
            quad.y0 >= line_y0 - 1.0,
            "quad y0 too high in {name}: quad.y0={}, line_y0={}",
            quad.y0,
            line_y0
        );
        assert!(
            quad.y1 <= line_y1 + 1.0,
            "quad y1 too low in {name}: quad.y1={}, line_y1={}",
            quad.y1,
            line_y1
        );

        // 3. For two-column.pdf: drag inside column 2 only, assert no column-1 text
        if name == "two-column.pdf" {
            // Lines 4 to 7 are "Col 2 Line 1" through "Col 2 Line 4"
            let col2_line = &lines[5]; // "Col 2 Line 2"
            let first = col2_line.first().unwrap();
            let last = col2_line.last().unwrap();

            let drag_start = (first.x0, (first.y0 + first.y1) / 2.0);
            let drag_end = (last.x1, (last.y0 + last.y1) / 2.0);

            let sel =
                select(&chars, drag_start, drag_end).expect("selection on col 2 should succeed");
            assert!(
                !sel.text.contains("Col 1"),
                "selection in col 2 contained col 1 text: {}",
                sel.text
            );
            let expected_col2_text: String = col2_line.iter().map(|c| c.ch).collect();
            assert_eq!(sel.text.trim(), expected_col2_text.trim());
        }
    }
}

#[test]
fn test_rotated_pages() {
    let Some(r) = renderer() else {
        eprintln!("skipping: libpdfium not available");
        return;
    };

    let path = fixture_dir().join("rotated-pages.pdf");
    for page in 0..4 {
        let chars = r.page_chars(&path, page).expect("should load page chars");
        assert!(
            !chars.is_empty(),
            "chars should not be empty for page {page} of rotated-pages.pdf"
        );

        // Bounds checks
        for c in &chars {
            assert!(c.x0 < c.x1, "x0 < x1 for {} on page {page}", c.ch);
            assert!(c.y0 <= c.y1, "y0 <= y1 for {} on page {page}", c.ch);
        }

        // Whole page drag: from first char to last char
        let first = chars.first().unwrap();
        let last = chars.last().unwrap();

        let drag_start = (first.x0, (first.y0 + first.y1) / 2.0);
        let drag_end = (last.x1, (last.y0 + last.y1) / 2.0);

        let sel = select(&chars, drag_start, drag_end).expect("selection should succeed");
        let expected_text: String = chars
            .iter()
            .filter(|c| c.ch != '\r' && c.ch != '\n')
            .map(|c| c.ch)
            .collect();
        assert_eq!(
            sel.text.trim(),
            expected_text.trim(),
            "text mismatch on page {page} of rotated-pages.pdf"
        );
    }
}

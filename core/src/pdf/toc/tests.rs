//! Headless unit tests for table-of-contents parsing, heading depth, indent bands, and page-delta learning.

use super::*;

fn page_with_lines(page_index: u16, lines: &[(&str, f32, f32, f32)]) -> PdfPageText {
    let mut text = String::new();
    let mut text_lines = Vec::new();
    for (idx, (content, x, y, h)) in lines.iter().enumerate() {
        if idx > 0 {
            text.push('\n');
        }
        let start = text.chars().count();
        text.push_str(content);
        let end = text.chars().count();
        text_lines.push(PdfTextLine {
            start_text_index: start,
            end_text_index: end,
            bbox: PdfRect {
                x: *x,
                y: *y,
                width: 200.0,
                height: *h,
            },
        });
    }
    PdfPageText {
        page_index,
        page_width: 612.0,
        page_height: 792.0,
        text,
        chars: Vec::new(),
        lines: text_lines,
    }
}

#[test]
fn parse_leader_line_handles_all_leader_styles_and_digit_bounds() {
    // ASCII dots
    assert_eq!(
        parse_leader_line("1.1 Overview ........ 7", true),
        Some(("1.1 Overview".to_string(), 7))
    );
    // Middle dots (U+00B7)
    assert_eq!(
        parse_leader_line("Background ······ 19", true),
        Some(("Background".to_string(), 19))
    );
    // Horizontal ellipsis (U+2026) — >= 2 leader glyphs
    assert_eq!(
        parse_leader_line("Methodology …… 42", true),
        Some(("Methodology".to_string(), 42))
    );
    // Tabs and spaces: rejected when require_leader = true, accepted when false
    assert_eq!(parse_leader_line("Appendix A\t\t108", true), None);
    assert_eq!(
        parse_leader_line("Appendix A\t\t108", false),
        Some(("Appendix A".to_string(), 108))
    );
    assert_eq!(parse_leader_line("References     215", true), None);
    assert_eq!(
        parse_leader_line("References     215", false),
        Some(("References".to_string(), 215))
    );

    // Single dot is NOT a real leader when require_leader = true
    assert_eq!(parse_leader_line("Section . 5", true), None);

    // Digit bounds: 1..=6 digits accepted, 0 or >=7 rejected
    assert_eq!(parse_leader_line("No trailing digits ....", true), None);
    assert_eq!(
        parse_leader_line("Six Digit Max .... 999999", true),
        Some(("Six Digit Max".to_string(), 999_999))
    );
    assert_eq!(
        parse_leader_line("Seven Digits Overflow .... 1000000", true),
        None
    );

    // Number-only or symbol-only title rejected
    assert_eq!(parse_leader_line("1234 .... 56", true), None);
    assert_eq!(parse_leader_line("--- .... 12", true), None);
}

#[test]
fn strip_leader_tail_preserves_numbered_titles_and_removes_leaders() {
    assert_eq!(
        strip_leader_tail("  2.3 Gradient Descent .......... 84  "),
        "2.3 Gradient Descent"
    );
    assert_eq!(strip_leader_tail("Bibliography ···· 310"), "Bibliography");
    assert_eq!(strip_leader_tail("Index …… 405"), "Index");
    assert_eq!(strip_leader_tail("Conclusion . . . \t 99"), "Conclusion");

    // Legitimate trailing numbers with no >=2-dot leader stay intact
    assert_eq!(strip_leader_tail("Chapter 5"), "Chapter 5");
    assert_eq!(strip_leader_tail("Part 2"), "Part 2");
    assert_eq!(strip_leader_tail(" Theorem 1.4 "), "Theorem 1.4");
    assert_eq!(strip_leader_tail("Preface"), "Preface");
}

#[test]
fn clean_link_title_joins_wrapped_lines_and_drops_trailing_page_number() {
    assert_eq!(
        clean_link_title("3.2 Variational\nInference and Bounds\n47"),
        "3.2 Variational Inference and Bounds"
    );
    assert_eq!(
        clean_link_title("  Introduction  \r\n  ........ 12 \r\n"),
        "Introduction"
    );
    // Keeps a trailing number that is part of a single-line heading with no leader
    assert_eq!(clean_link_title("Chapter 9"), "Chapter 9");
    // Multi-line where the final line is the right-column page number
    assert_eq!(clean_link_title("Chapter 9\n120"), "Chapter 9");
}

#[test]
fn numbered_heading_depth_covers_parts_appendices_sections_and_rejections() {
    // Roman parts, appendices, chapters, sections (case-insensitive)
    assert_eq!(numbered_heading_depth("Part I Foundations"), Some(1));
    assert_eq!(numbered_heading_depth("PART IV Advanced Systems"), Some(1));
    assert_eq!(numbered_heading_depth("Appendix A Proofs"), Some(1));
    assert_eq!(numbered_heading_depth("appendix C"), Some(1));
    assert_eq!(numbered_heading_depth("Chapter 12 Synthesis"), Some(1));
    assert_eq!(numbered_heading_depth("Section 3 Preliminaries"), Some(1));

    // Keyword without an identifier is rejected
    assert_eq!(numbered_heading_depth("Chapter "), None);
    assert_eq!(numbered_heading_depth("Appendix ---"), None);

    // Deep dotted numeric sections
    assert_eq!(numbered_heading_depth("1. Overview"), Some(1));
    assert_eq!(numbered_heading_depth("2.4 Optimization"), Some(2));
    assert_eq!(numbered_heading_depth("2.4.1 Convex Case"), Some(3));
    assert_eq!(numbered_heading_depth("2.4.1.3 Subgradients"), Some(4));

    // Bare 1-2 digit + Capitalized word
    assert_eq!(numbered_heading_depth("1 Introduction"), Some(1));
    assert_eq!(numbered_heading_depth("99 Summary"), Some(1));

    // Non-heading rejections
    assert_eq!(numbered_heading_depth("100 Years of Solitude"), None);
    assert_eq!(numbered_heading_depth("2024 Annual Report"), None);
    assert_eq!(numbered_heading_depth("3 blind mice"), None);
    assert_eq!(numbered_heading_depth("3.14159"), None);
    assert_eq!(numbered_heading_depth("1.2."), None);
    assert_eq!(numbered_heading_depth("Plain Unnumbered Heading"), None);
}

#[test]
fn indent_bands_and_entry_level_cluster_and_cap_levels() {
    // Indents within 2pt merge; sorted ascending; capped at MAX_TOC_LEVELS (4).
    let raw_indents = [72.0, 73.4, 96.0, 97.9, 120.0, 144.0, 168.0];
    let bands = indent_bands(&raw_indents);
    assert_eq!(bands.len(), MAX_TOC_LEVELS);
    assert_eq!(bands, vec![72.0, 96.0, 120.0, 144.0]);

    // Unnumbered entries resolve via indent band (with 2pt tolerance)
    assert_eq!(entry_level("Preface", 72.5, &bands), 0);
    assert_eq!(entry_level("Subtopic", 95.0, &bands), 1);
    assert_eq!(entry_level("Deep Note", 120.0, &bands), 2);
    assert_eq!(entry_level("Overflow Indent", 200.0, &bands), 3);

    // Numbered entries use numbering depth regardless of visual x-indent
    assert_eq!(entry_level("1. Introduction", 144.0, &bands), 0);
    assert_eq!(entry_level("1.2.3 Details", 72.0, &bands), 2);
    assert_eq!(entry_level("1.2.3.4.5 Very Deep", 72.0, &bands), 3);
}

#[test]
fn learn_page_delta_handles_front_matter_single_vote_and_consensus() {
    // Front matter occupies physical pages 0..=4 (TOC at page 1), so printed
    // page 1 starts at physical index 5 (delta = +4).
    let pages = vec![
        page_with_lines(0, &[("Title Page", 72.0, 700.0, 24.0)]),
        page_with_lines(1, &[("Contents", 72.0, 700.0, 18.0)]),
        page_with_lines(2, &[("Preface iv", 72.0, 700.0, 12.0)]),
        page_with_lines(3, &[("Notation", 72.0, 700.0, 12.0)]),
        page_with_lines(4, &[("List of Figures", 72.0, 700.0, 12.0)]),
        page_with_lines(5, &[("Introduction", 72.0, 700.0, 16.0)]),
        page_with_lines(6, &[("Body prose", 72.0, 700.0, 10.0)]),
        page_with_lines(7, &[("Methodology", 72.0, 700.0, 16.0)]),
        page_with_lines(8, &[("Experiments", 72.0, 700.0, 16.0)]),
    ];

    // Single vote must be rejected so one accidental match cannot skew the TOC.
    let single = [("Introduction", 1), ("Unmatched Chapter", 10)];
    assert_eq!(learn_page_delta(&pages, 1, &single), None);

    // Multi-vote consensus (+4) wins even with an outlier coincidental match.
    let entries = [
        ("Notation", 99),    // outlier at page 3 => delta -96 (1 vote)
        ("Introduction", 1), // page 5 - 1 = +4
        ("Methodology", 3),  // page 7 - 3 = +4
        ("Experiments", 4),  // page 8 - 4 = +4
    ];
    assert_eq!(learn_page_delta(&pages, 1, &entries), Some(4));

    // Matches on or before `last_toc` are ignored.
    let before_toc = [("Title Page", 1), ("Contents", 2)];
    assert_eq!(learn_page_delta(&pages, 1, &before_toc), None);
}

#[test]
fn recover_toc_from_texts_prefers_printed_toc_and_falls_back_to_synthesis() {
    // Case 1: Printed TOC with front-matter offset (+2)
    let with_printed_toc = vec![
        page_with_lines(0, &[("Book Cover", 72.0, 700.0, 22.0)]),
        page_with_lines(
            1,
            &[
                ("Table of Contents", 72.0, 720.0, 18.0),
                ("1. Foundations ........ 1", 72.0, 680.0, 11.0),
                ("1.1 Axioms ........ 2", 92.0, 660.0, 11.0),
                ("2. Applications ........ 3", 72.0, 640.0, 11.0),
            ],
        ),
        page_with_lines(2, &[("Preface", 72.0, 700.0, 14.0)]),
        page_with_lines(3, &[("1. Foundations", 72.0, 700.0, 16.0)]),
        page_with_lines(4, &[("1.1 Axioms", 72.0, 700.0, 14.0)]),
        page_with_lines(5, &[("2. Applications", 72.0, 700.0, 16.0)]),
    ];
    let recovered = recover_toc_from_texts(&with_printed_toc);
    assert_eq!(recovered.len(), 2);
    assert_eq!(recovered[0].title, "1. Foundations");
    assert_eq!(recovered[0].page_index, Some(3));
    assert_eq!(recovered[0].children.len(), 1);
    assert_eq!(recovered[0].children[0].title, "1.1 Axioms");
    assert_eq!(recovered[0].children[0].page_index, Some(4));
    assert_eq!(recovered[1].title, "2. Applications");
    assert_eq!(recovered[1].page_index, Some(5));

    // Case 2: No printed TOC -> typographic synthesis picks taller headings
    let without_printed_toc = vec![
        page_with_lines(
            0,
            &[
                ("Abstract Overview", 72.0, 700.0, 18.0),
                ("regular body text line one", 72.0, 660.0, 10.0),
                ("regular body text line two", 72.0, 640.0, 10.0),
            ],
        ),
        page_with_lines(
            1,
            &[
                ("Experimental Setup", 72.0, 700.0, 18.0),
                ("more regular body text", 72.0, 660.0, 10.0),
            ],
        ),
    ];
    let synthesized = recover_toc_from_texts(&without_printed_toc);
    assert_eq!(synthesized.len(), 2);
    assert_eq!(synthesized[0].title, "Abstract Overview");
    assert_eq!(synthesized[0].page_index, Some(0));
    assert_eq!(synthesized[1].title, "Experimental Setup");
    assert_eq!(synthesized[1].page_index, Some(1));
}

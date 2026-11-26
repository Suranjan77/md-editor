//! Tests for `pdf`.

use super::*;

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_pdf_render_bucket() {
    // Exact powers of 1.4 land on their own bucket.
    assert_eq!(pdf_render_bucket(1.0), 1.0);
    assert!((pdf_render_bucket(1.4) - 1.4).abs() < 1e-4);

    // Zoom levels round up to the next bucket (never upscaled > 1.4x).
    assert!((pdf_render_bucket(1.1) - 1.4).abs() < 1e-4);
    assert!((pdf_render_bucket(1.5) - 1.4 * 1.4).abs() < 1e-3);

    // A whole band of nearby zooms shares one bucket, so they reuse the
    // same cached bitmap instead of forcing re-renders.
    let b = pdf_render_bucket(1.05);
    assert_eq!(b, pdf_render_bucket(1.2));
    assert_eq!(b, pdf_render_bucket(1.4));

    // Buckets stay >= zoom (so the bitmap is never upscaled), within
    // 1.4x (so it is never wastefully oversampled).
    for &z in &[0.5_f32, 0.75, 0.9, 1.0, 1.3, 2.0, 3.3, 4.0] {
        let s = pdf_render_bucket(z);
        assert!(s >= z - 1e-4, "bucket {s} below zoom {z}");
        assert!(s < z * 1.4 + 1e-4, "bucket {s} too far above zoom {z}");
    }
}

// --- synthesize_toc (windowless: no pdfium) -------------------------

/// Build a `PdfPageText` from `(text, height)` line specs. The page `text`
/// is the lines joined by '\n', and each `PdfTextLine` spans its slice with
/// a bbox of the given height.
fn page_fixture(page_index: u16, lines: &[(&str, f32)]) -> PdfPageText {
    let mut text = String::new();
    let mut text_lines = Vec::new();
    for (i, (s, h)) in lines.iter().enumerate() {
        if i > 0 {
            text.push('\n');
        }
        let start = text.chars().count();
        text.push_str(s);
        let end = text.chars().count();
        text_lines.push(PdfTextLine {
            start_text_index: start,
            end_text_index: end,
            bbox: PdfRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: *h,
            },
        });
    }
    PdfPageText {
        offsets: Default::default(),
        page_index,
        page_width: 600.0,
        page_height: 800.0,
        text,
        chars: Vec::new(),
        lines: text_lines,
    }
}

/// Build a `PdfPageText` from `(text, x, y, height)` line specs, for tests
/// that need real geometry (column layouts, row pairing).
fn page_fixture_xy(page_index: u16, lines: &[(&str, f32, f32, f32)]) -> PdfPageText {
    let mut text = String::new();
    let mut text_lines = Vec::new();
    for (i, (s, x, y, h)) in lines.iter().enumerate() {
        if i > 0 {
            text.push('\n');
        }
        let start = text.chars().count();
        text.push_str(s);
        let end = text.chars().count();
        text_lines.push(PdfTextLine {
            start_text_index: start,
            end_text_index: end,
            bbox: PdfRect {
                x: *x,
                y: *y,
                width: 100.0,
                height: *h,
            },
        });
    }
    PdfPageText {
        offsets: Default::default(),
        page_index,
        page_width: 595.0,
        page_height: 842.0,
        text,
        chars: Vec::new(),
        lines: text_lines,
    }
}

fn flatten_titles(entries: &[TocEntry], out: &mut Vec<String>) {
    for e in entries {
        out.push(e.title.clone());
        flatten_titles(&e.children, out);
    }
}

#[test]
fn test_synthesize_toc_picks_large_lines() {
    // Body text at 10pt, two headings at 16pt.
    let pages = vec![
        page_fixture(
            0,
            &[
                ("Big Heading One", 16.0),
                ("some body text here", 10.0),
                ("more body text here", 10.0),
            ],
        ),
        page_fixture(
            1,
            &[("Big Heading Two", 16.0), ("yet more body text", 10.0)],
        ),
    ];
    let toc = synthesize_toc(&pages);
    let mut titles = Vec::new();
    flatten_titles(&toc, &mut titles);
    assert_eq!(titles, vec!["Big Heading One", "Big Heading Two"]);
    assert_eq!(toc[0].page_index, Some(0));
    assert_eq!(toc[1].page_index, Some(1));
}

#[test]
fn test_synthesize_toc_drops_running_header() {
    // A 16pt running header repeats on every page and must not appear,
    // while a genuine 16pt heading on page 1 does.
    let mut pages = Vec::new();
    for p in 0..4u16 {
        let mut lines = vec![
            ("My Document Title", 16.0), // running header on every page
            ("body line one here", 10.0),
            ("body line two here", 10.0),
        ];
        if p == 1 {
            lines.insert(1, ("Introduction", 16.0));
        }
        pages.push(page_fixture(p, &lines));
    }
    let toc = synthesize_toc(&pages);
    let mut titles = Vec::new();
    flatten_titles(&toc, &mut titles);
    assert!(
        !titles.iter().any(|t| t == "My Document Title"),
        "running header should be dropped, got {titles:?}"
    );
    assert!(titles.iter().any(|t| t == "Introduction"));
}

#[test]
fn test_synthesize_toc_levels_nest() {
    // 18pt > 14pt > 10pt body: the 14pt heading nests under the 18pt one.
    let pages = vec![page_fixture(
        0,
        &[
            ("Chapter", 18.0),
            ("Section", 14.0),
            ("body text content", 10.0),
            ("more body content", 10.0),
            ("even more content", 10.0),
            ("still more content", 10.0),
        ],
    )];
    let toc = synthesize_toc(&pages);
    assert_eq!(toc.len(), 1);
    assert_eq!(toc[0].title, "Chapter");
    assert_eq!(toc[0].children.len(), 1);
    assert_eq!(toc[0].children[0].title, "Section");
}

#[test]
fn test_synthesize_toc_empty_when_uniform() {
    // All lines the same size => no headings.
    let pages = vec![page_fixture(
        0,
        &[
            ("line one here", 10.0),
            ("line two here", 10.0),
            ("line three here", 10.0),
        ],
    )];
    assert!(synthesize_toc(&pages).is_empty());
}

#[test]
fn test_numbered_heading_depth() {
    assert_eq!(numbered_heading_depth("1. Introduction"), Some(1));
    assert_eq!(numbered_heading_depth("1.2 Background"), Some(2));
    assert_eq!(numbered_heading_depth("1.2.3 Details"), Some(3));
    assert_eq!(numbered_heading_depth("Chapter 4 Results"), Some(1));
    assert_eq!(numbered_heading_depth("Appendix B"), Some(1));
    // Bare integer + capitalized word is a common chapter style.
    assert_eq!(numbered_heading_depth("1 Introduction"), Some(1));
    assert_eq!(numbered_heading_depth("12 Conclusions"), Some(1));
    // Bare numbers / years / quantities must NOT count as headings.
    assert_eq!(numbered_heading_depth("2020 was a year"), None);
    assert_eq!(numbered_heading_depth("1 apple"), None);
    assert_eq!(numbered_heading_depth("1.5kg of flour"), None);
    assert_eq!(numbered_heading_depth("just text"), None);
}

// --- printed-TOC recovery (windowless) -----------------------------

#[test]
fn test_parse_leader_line() {
    // Dotted leader with a page number.
    assert_eq!(
        parse_leader_line("Introduction ......... 12", true),
        Some(("Introduction".to_string(), 12))
    );
    // Space-only leader is accepted only when leaders are not required.
    assert_eq!(
        parse_leader_line("Methods            34", false),
        Some(("Methods".to_string(), 34))
    );
    assert_eq!(parse_leader_line("Methods            34", true), None);
    // Prose ending in a number is rejected when a leader is required.
    assert_eq!(parse_leader_line("we saw 3 cats", true), None);
    // A title needs letters, so a number-only row is rejected.
    assert_eq!(parse_leader_line("12 ..... 34", true), None);
}

#[test]
fn test_strip_leader_tail() {
    assert_eq!(strip_leader_tail("Introduction ...... 12"), "Introduction");
    // No real leader: a title that ends in a number is preserved.
    assert_eq!(strip_leader_tail("Chapter 5"), "Chapter 5");
    assert_eq!(strip_leader_tail("Conclusion"), "Conclusion");
}

#[test]
fn test_detect_toc_pages() {
    let pages = vec![
        page_fixture(
            0,
            &[
                ("Contents", 16.0),
                ("Introduction ...... 1", 10.0),
                ("Methods ...... 3", 10.0),
            ],
        ),
        page_fixture(1, &[("Introduction", 14.0), ("body here", 10.0)]),
    ];
    assert_eq!(detect_toc_pages(&pages), vec![0]);
}

#[test]
fn test_parse_printed_toc_maps_pages_via_learned_delta() {
    // A "Contents" page lists two entries by their printed page numbers; the
    // body pages carry matching headings, so the printed->physical offset is
    // learned (here delta = 0) and applied.
    let pages = vec![
        page_fixture(
            0,
            &[
                ("Contents", 16.0),
                ("Introduction ......... 1", 10.0),
                ("Methods ......... 3", 10.0),
                ("Results ......... 5", 10.0),
            ],
        ),
        page_fixture(1, &[("Introduction", 14.0), ("body text here", 10.0)]),
        page_fixture(2, &[("filler page text", 10.0)]),
        page_fixture(3, &[("Methods", 14.0), ("more body text", 10.0)]),
        page_fixture(4, &[("more filler text", 10.0)]),
        page_fixture(5, &[("Results", 14.0), ("yet more body", 10.0)]),
    ];
    let toc = parse_printed_toc(&pages);
    let mut titles = Vec::new();
    flatten_titles(&toc, &mut titles);
    assert_eq!(titles, vec!["Introduction", "Methods", "Results"]);
    assert_eq!(toc[0].page_index, Some(1));
    assert_eq!(toc[1].page_index, Some(3));
    assert_eq!(toc[2].page_index, Some(5));
}

#[test]
fn test_parse_printed_toc_two_column_layout() {
    // The real-world failure: a contents page whose page numbers sit in a
    // separate right-hand column on the same row as the title (no leaders),
    // including a wrapped title that spans two rows.
    let pages = vec![
        page_fixture_xy(
            0,
            &[
                ("Contents", 51.0, 744.0, 46.0),
                ("Introduction", 51.0, 704.0, 14.0),
                ("1", 539.0, 704.0, 14.0),
                ("Methods", 51.0, 663.0, 14.0),
                ("3", 538.0, 663.0, 14.0),
                ("Advanced topics in", 51.0, 642.0, 14.0),
                ("5", 538.0, 642.0, 14.0),
                ("computing", 87.0, 629.0, 14.0), // wrapped continuation, no number
            ],
        ),
        page_fixture_xy(1, &[("Introduction", 51.0, 700.0, 20.0)]),
        page_fixture_xy(2, &[("filler", 51.0, 700.0, 10.0)]),
        page_fixture_xy(3, &[("Methods", 51.0, 700.0, 20.0)]),
        page_fixture_xy(4, &[("filler", 51.0, 700.0, 10.0)]),
        page_fixture_xy(5, &[("Advanced topics in computing", 51.0, 700.0, 20.0)]),
    ];
    let toc = parse_printed_toc(&pages);
    let mut titles = Vec::new();
    flatten_titles(&toc, &mut titles);
    assert_eq!(
        titles,
        vec!["Introduction", "Methods", "Advanced topics in computing"]
    );
    assert_eq!(toc[0].page_index, Some(1));
    assert_eq!(toc[1].page_index, Some(3));
    assert_eq!(toc[2].page_index, Some(5));
}

#[test]
fn test_parse_printed_toc_positional_fallback() {
    // No body heading matches the entries, so the offset can't be learned;
    // the parser falls back to "content starts right after the contents
    // page" (here delta 0) rather than discarding the recovered titles.
    let pages = vec![
        page_fixture(
            0,
            &[
                ("Contents", 16.0),
                ("Alpha ......... 1", 10.0),
                ("Bravo ......... 3", 10.0),
                ("Charlie ......... 5", 10.0),
            ],
        ),
        page_fixture(1, &[("unrelated text", 10.0)]),
        page_fixture(2, &[("more unrelated text", 10.0)]),
        page_fixture(3, &[("still unrelated", 10.0)]),
        page_fixture(4, &[("yet more text", 10.0)]),
        page_fixture(5, &[("final unrelated", 10.0)]),
    ];
    let toc = parse_printed_toc(&pages);
    let mut titles = Vec::new();
    flatten_titles(&toc, &mut titles);
    assert_eq!(titles, vec!["Alpha", "Bravo", "Charlie"]);
    assert_eq!(toc[0].page_index, Some(1));
    assert_eq!(toc[1].page_index, Some(3));
    assert_eq!(toc[2].page_index, Some(5));
}

#[test]
fn test_synthesize_toc_numbered_same_size() {
    // Headings share the body font size but are detected by numbering, and
    // nest by numbering depth.
    let pages = vec![page_fixture(
        0,
        &[
            ("1. Introduction", 10.0),
            ("1.1 Background here", 10.0),
            ("body text content", 10.0),
            ("more body content", 10.0),
            ("2. Methods", 10.0),
        ],
    )];
    let toc = synthesize_toc(&pages);
    assert_eq!(toc.len(), 2, "two top-level numbered headings");
    assert_eq!(toc[0].title, "1. Introduction");
    assert_eq!(toc[0].children.len(), 1);
    assert_eq!(toc[0].children[0].title, "1.1 Background here");
    assert_eq!(toc[1].title, "2. Methods");
}

#[test]
fn test_synthesize_toc_caps_heading_same_size() {
    // ALL-CAPS short line at body size is treated as a heading.
    let pages = vec![page_fixture(
        0,
        &[
            ("INTRODUCTION", 10.0),
            ("the body text here", 10.0),
            ("still more body text", 10.0),
        ],
    )];
    let toc = synthesize_toc(&pages);
    let mut titles = Vec::new();
    flatten_titles(&toc, &mut titles);
    assert_eq!(titles, vec!["INTRODUCTION"]);
}

#[test]
fn test_synthesize_toc_per_page_fallback() {
    // No size/number/caps signal across multiple pages => per-page outline,
    // one entry per page labeled by its most prominent (tallest) line, with
    // the running header excluded.
    let pages = vec![
        page_fixture(0, &[("Running Header", 10.0), ("alpha content line", 12.0)]),
        page_fixture(1, &[("Running Header", 10.0), ("bravo content line", 12.0)]),
    ];
    let toc = synthesize_toc(&pages);
    assert_eq!(toc.len(), 2);
    assert_eq!(toc[0].title, "alpha content line");
    assert_eq!(toc[0].page_index, Some(0));
    assert_eq!(toc[1].title, "bravo content line");
    assert_eq!(toc[1].page_index, Some(1));
}

#[test]
fn test_pdf_search() {
    let _guard = TEST_LOCK.lock().unwrap();
    let pdfium = bind_pdfium().unwrap();
    let doc = pdfium
        .load_pdf_from_file("../tests-fixtures/pdf/dummy.pdf", None)
        .unwrap();
    let page = doc.pages().get(0).unwrap();
    let text_page = page.text().unwrap();
    let page_height = page.height().value;
    let page_width = page.width().value;
    println!("Page size: {} x {}", page_width, page_height);
    let options = PdfSearchOptions::new();
    let search = text_page.search("dummy", &options).unwrap();
    let mut count = 0;
    for segments in search.iter(PdfSearchDirection::SearchForward) {
        count += 1;
        println!("Match {}:", count);
        for segment in segments.iter() {
            let bounds = segment.bounds();
            let ry = bounds.bottom().value;
            let rh = bounds.height().value;
            let view_y = page_height - ry - rh;
            println!(
                "  Segment bounds: left={}, bottom={}, width={}, height={}",
                bounds.left().value,
                ry,
                bounds.width().value,
                rh
            );
            println!("  Expected view y (zoom=1.0): {}", view_y);
        }
    }
    assert!(count > 0, "No matches found!");
}

#[test]
fn test_pdf_renderer_search() {
    let _guard = TEST_LOCK.lock().unwrap();
    let renderer = PdfRenderer::new().unwrap();
    let path = "../tests-fixtures/pdf/dummy.pdf";

    // Test non-regex search
    let results = renderer.search_text(path, "dummy", false, false).unwrap();
    println!("Non-regex results: {results:?}");
    assert!(
        !results.is_empty(),
        "Non-regex search for 'dummy' should return matches"
    );
    for match_info in &results {
        assert!(match_info.context.to_lowercase().contains("dummy"));
        assert!(!match_info.rects.is_empty());
    }

    // Test regex search
    let regex_results = renderer
        .search_text(path, "dum[a-z]+y", true, false)
        .unwrap();
    println!("Regex results: {regex_results:?}");
    assert!(
        !regex_results.is_empty(),
        "Regex search for 'dum[a-z]+y' should return matches"
    );
    assert_eq!(
        results.len(),
        regex_results.len(),
        "Regex and non-regex search count should match"
    );
}

#[test]
fn test_pdf_text_extraction_and_hashing() {
    let _guard = TEST_LOCK.lock().unwrap();
    let path = "../tests-fixtures/pdf/dummy.pdf";
    let (id, size, modified) = compute_provisional_id(std::path::Path::new(path)).unwrap();
    assert!(!id.is_empty(), "Document hash must not be empty");
    assert!(size > 0, "Document size must be > 0");
    assert!(modified.is_some(), "Modified time should be available");

    let renderer = PdfRenderer::new().unwrap();
    let text_layer = renderer.get_page_text(path, 0).unwrap();
    println!("Extracted text: {}", text_layer.text);
    assert!(
        text_layer.text.to_lowercase().contains("dummy"),
        "Extracted text should contain 'dummy'"
    );
    assert!(
        !text_layer.chars.is_empty(),
        "Chars list should not be empty"
    );
    assert!(
        !text_layer.lines.is_empty(),
        "Lines list should not be empty"
    );
    for line in &text_layer.lines {
        assert!(
            line.end_text_index > line.start_text_index,
            "PDF text lines must use non-empty exclusive ranges"
        );
        assert!(
            line.bbox.width > 0.0 && line.bbox.height > 0.0,
            "PDF text lines must be backed by visible glyph bounds"
        );
        assert!(
            line.end_text_index <= text_layer.text.chars().count(),
            "PDF text line range must stay within extracted text"
        );
        let line_chars = text_layer
            .chars
            .iter()
            .filter(|c| {
                c.text_index >= line.start_text_index
                    && c.text_index < line.end_text_index
                    && c.bbox.width > 0.0
                    && c.bbox.height > 0.0
            })
            .count();
        assert!(line_chars > 0, "Every PDF text line must be selectable");
    }

    // Verify bounding boxes coordinates
    for c in &text_layer.chars {
        assert!(c.bbox.width >= 0.0);
        assert!(c.bbox.height >= 0.0);
    }

    // Test character merging
    let merged = merge_char_rects(&text_layer.chars[..5]);
    assert!(!merged.is_empty(), "Merged rects list should not be empty");
}

#[test]
fn password_load_error_is_distinct() {
    let err = pdf_load_error(PdfiumError::PdfiumLibraryInternalError(
        PdfiumInternalError::PasswordError,
    ));
    assert_eq!(err, PdfError::PasswordRequired);
    assert_eq!(err.to_string(), "Password required to open PDF");
}

#[test]
fn pdf_renderer_commands_visible_window_and_priority_render() {
    let _guard = TEST_LOCK.lock().unwrap();
    let renderer = PdfRenderer::new().unwrap();
    let dummy = "../tests-fixtures/pdf/dummy.pdf";
    let large = "../tests-fixtures/pdf/large-500-pages.pdf";

    assert_eq!(renderer.page_count(dummy).unwrap(), 1);
    assert_eq!(renderer.page_count(large).unwrap(), 500);

    let img = renderer.render_page(dummy, 0, 1.0).unwrap();
    assert!(img.width() > 0 && img.height() > 0);

    // Restrict visible range on `large` to pages 10..=12 (buffered window 8..=14).
    renderer.set_visible_range(10, 12, large);
    assert_eq!(
        renderer.render_page(large, 0, 0.5).unwrap_err(),
        PdfError::Skipped
    );
    assert_eq!(
        renderer.render_page(large, 25, 0.5).unwrap_err(),
        PdfError::Skipped
    );
    // Inside buffered window succeeds.
    assert!(renderer.render_page(large, 10, 0.5).is_ok());
    assert!(renderer.render_page(large, 14, 0.5).is_ok());
    // A different document path is not skipped by `large`'s visible range.
    assert!(renderer.render_page(dummy, 0, 0.5).is_ok());

    // Priority render bypasses the visible-range filter even far outside 8..=14.
    let priority_img = renderer.render_page_priority(large, 250, 0.5).unwrap();
    assert!(priority_img.width() > 0 && priority_img.height() > 0);
}

#[test]
fn pdf_search_empty_invalid_regex_and_cancellation() {
    let _guard = TEST_LOCK.lock().unwrap();
    let renderer = PdfRenderer::new().unwrap();
    let dummy = "../tests-fixtures/pdf/dummy.pdf";
    let large = "../tests-fixtures/pdf/large-500-pages.pdf";

    // Empty and whitespace-only queries short-circuit cleanly.
    assert!(
        renderer
            .search_text(dummy, "", false, false)
            .unwrap()
            .is_empty()
    );
    assert!(
        renderer
            .search_text(dummy, "   \t ", true, false)
            .unwrap()
            .is_empty()
    );

    // Invalid regex returns a typed PdfError::Other.
    let err = renderer
        .search_text(dummy, "[unclosed(", true, false)
        .unwrap_err();
    assert!(
        matches!(&err, PdfError::Other(msg) if msg.contains("Invalid PDF search regex")),
        "unexpected error: {err:?}"
    );

    // Search cancellation when the command sender disconnects mid-search.
    let pdfium = bind_pdfium().unwrap();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let (_prio_tx, prio_rx) = std::sync::mpsc::channel();
    let visible = std::sync::Arc::new(std::sync::Mutex::new(None));
    let mut worker = worker::Worker::new(&pdfium, cmd_rx, prio_rx, visible);

    let (resp_tx, resp_rx) = std::sync::mpsc::sync_channel(1);
    cmd_tx
        .send(PdfCommand::SearchText(
            large.to_string(),
            "nonexistent_term_xyz".to_string(),
            false,
            false,
            resp_tx,
        ))
        .unwrap();
    // Disconnect the sender before running the queued command so `Worker` sees
    // the abandoned channel and cancels the 500-page scan immediately.
    let cmd = cmd_rx_from_worker(&mut worker);
    drop(cmd_tx);
    worker.handle_command(cmd);
    assert_eq!(resp_rx.recv().unwrap().unwrap_err(), PdfError::WorkerGone);
}

fn cmd_rx_from_worker(worker: &mut worker::Worker<'_>) -> PdfCommand {
    worker.receiver.recv().unwrap()
}

struct TestTempDir(PathBuf);

impl TestTempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("md-editor-pdf-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn worker_three_document_lru_cache_promotes_hits_and_evicts_oldest() {
    // 1. Pure LRU helper check.
    let mut cache: VecDeque<(String, usize)> = VecDeque::new();
    for (k, v) in [("a", 1), ("b", 2), ("c", 3)] {
        worker::lru_get_or_insert_with(&mut cache, worker::DOC_CACHE_CAPACITY, k, || Ok(v))
            .unwrap();
    }
    assert_eq!(
        cache.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        vec!["a", "b", "c"]
    );
    // Access "a" -> promoted to most-recent ("b" becomes oldest).
    assert_eq!(
        *worker::lru_get_or_insert_with(&mut cache, worker::DOC_CACHE_CAPACITY, "a", || {
            panic!("must hit cache")
        })
        .unwrap(),
        1
    );
    assert_eq!(
        cache.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        vec!["b", "c", "a"]
    );
    // Insert 4th document "d" -> evicts "b" (the oldest).
    worker::lru_get_or_insert_with(&mut cache, worker::DOC_CACHE_CAPACITY, "d", || Ok(4)).unwrap();
    assert_eq!(cache.len(), 3);
    assert_eq!(
        cache.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        vec!["c", "a", "d"]
    );

    // 2. Real `Worker` check with 4 distinct PDF files on disk.
    let _guard = TEST_LOCK.lock().unwrap();
    let pdfium = bind_pdfium().unwrap();
    let tmp = TestTempDir::new();
    let src_bytes = std::fs::read("../tests-fixtures/pdf/dummy.pdf").unwrap();
    let paths: Vec<String> = (1..=4)
        .map(|n| {
            let p = tmp.path().join(format!("doc_{n}.pdf"));
            std::fs::write(&p, &src_bytes).unwrap();
            p.to_string_lossy().to_string()
        })
        .collect();

    let (_cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let (_prio_tx, prio_rx) = std::sync::mpsc::channel();
    let visible = std::sync::Arc::new(std::sync::Mutex::new(None));
    let mut worker = worker::Worker::new(&pdfium, cmd_rx, prio_rx, visible);

    for p in &paths[0..3] {
        assert_eq!(worker.page_count(p).unwrap(), 1);
    }
    assert_eq!(worker.documents.len(), 3);
    // Touch paths[0] so it moves to MRU and paths[1] becomes the eviction victim.
    assert_eq!(worker.page_count(&paths[0]).unwrap(), 1);
    assert_eq!(
        worker
            .documents
            .iter()
            .map(|(p, _)| p.as_str())
            .collect::<Vec<_>>(),
        vec![paths[1].as_str(), paths[2].as_str(), paths[0].as_str()]
    );

    // Opening paths[3] evicts paths[1].
    assert_eq!(worker.page_count(&paths[3]).unwrap(), 1);
    assert_eq!(worker.documents.len(), 3);
    assert_eq!(
        worker
            .documents
            .iter()
            .map(|(p, _)| p.as_str())
            .collect::<Vec<_>>(),
        vec![paths[2].as_str(), paths[0].as_str(), paths[3].as_str()]
    );
}

#[test]
fn pdfium_candidates_prefer_the_installed_layout() {
    let lib = Path::new("libpdfium.so");
    let candidates = pdfium_candidates(Some(Path::new("/usr/bin")), lib);
    assert_eq!(
        candidates[0],
        PathBuf::from("/usr/lib/md-editor/libpdfium.so")
    );
    assert_eq!(candidates[1], PathBuf::from("/usr/bin/libpdfium.so"));
    assert_eq!(
        candidates[2],
        PathBuf::from("/usr/bin/resources/libpdfium.so")
    );
    assert!(candidates[3].ends_with("pdfium/libpdfium.so"));

    // Without an executable path only the source-tree fallback remains.
    assert_eq!(pdfium_candidates(None, lib).len(), 1);
}

/// PERF-2: a render queued behind whole-document work is served first. The
/// worker used to be one FIFO, so a page render waited for every scan queued
/// ahead of it, and opening a bookmark-less PDF showed a blank pane for
/// seconds.
#[test]
fn renders_jump_ahead_of_queued_background_work() {
    let _guard = TEST_LOCK.lock().unwrap();
    let renderer = PdfRenderer::new().unwrap();
    let dummy = "../tests-fixtures/pdf/dummy.pdf";
    let large = "../tests-fixtures/pdf/large-500-pages.pdf";
    let query = "no-such-text-anywhere";

    // Warm the document cache, and learn how long one full-document search takes.
    let started = std::time::Instant::now();
    renderer.search_text(large, query, false, false).unwrap();
    let one_search = started.elapsed();

    let finished = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..3 {
            scope.spawn(|| {
                renderer.search_text(large, query, false, false).unwrap();
                finished.lock().unwrap().push("search");
            });
        }
        // Let all three reach the worker: one runs, two wait in the queue.
        std::thread::sleep(one_search / 4 + std::time::Duration::from_millis(5));
        renderer.render_page(dummy, 0, 0.5).unwrap();
        finished.lock().unwrap().push("render");
    });

    let order = finished.into_inner().unwrap();
    let render_at = order.iter().position(|e| *e == "render").unwrap();
    assert!(
        render_at < 3,
        "the render waited for every queued search: {order:?} (one search took {one_search:?})"
    );
}

#[test]
fn background_lane_holds_whole_document_work_only() {
    let (tx, _rx) = std::sync::mpsc::sync_channel(1);
    assert!(PdfCommand::SearchText("a".into(), "q".into(), false, false, tx).is_background());
    let (tx, _rx) = std::sync::mpsc::sync_channel(1);
    assert!(PdfCommand::GetPageTextBackground("a".into(), 0, tx).is_background());
    let (tx, _rx) = std::sync::mpsc::sync_channel(1);
    assert!(!PdfCommand::GetPageText("a".into(), 0, tx).is_background());
    let (tx, _rx) = std::sync::mpsc::sync_channel(1);
    assert!(!PdfCommand::RenderPage("a".into(), 0, 1.0, tx).is_background());
    let (tx, _rx) = std::sync::mpsc::sync_channel(1);
    assert!(!PdfCommand::PageSizes("a".into(), tx).is_background());
}

/// One page of "MMMMMM" in 36 pt Helvetica at user-space (60, 120) on a
/// 400×300 pt page, with the given `/Rotate` and crop box.
fn write_geometry_fixture(
    path: &Path,
    rotation: PdfPageRenderRotation,
    crop: Option<(f32, f32, f32, f32)>,
) {
    let pdfium = bind_pdfium().unwrap();
    let mut doc = pdfium.create_new_pdf().unwrap();
    let font = doc.fonts_mut().helvetica();
    let mut page = doc
        .pages_mut()
        .create_page_at_end(PdfPagePaperSize::new_custom(
            PdfPoints::new(400.0),
            PdfPoints::new(300.0),
        ))
        .unwrap();
    page.objects_mut()
        .create_text_object(
            PdfPoints::new(60.0),
            PdfPoints::new(120.0),
            "MMMMMM",
            font,
            PdfPoints::new(36.0),
        )
        .unwrap();
    page.set_rotation(rotation);
    if let Some((bottom, left, top, right)) = crop {
        page.boundaries_mut()
            .set_crop(pdfium_render::prelude::PdfRect::new_from_values(
                bottom, left, top, right,
            ))
            .unwrap();
    }
    drop(page);
    doc.save_to_file(path).unwrap();
}

/// Share of dark pixels inside a bottom-left-origin rect (displayed-box
/// points) of a page rendered at `scale`.
fn ink_in(image: &image::RgbaImage, rect: &PdfRect, page_height: f32, scale: f32) -> f32 {
    let x0 = (rect.x * scale).max(0.0) as u32;
    let x1 = ((rect.x + rect.width) * scale).min(image.width() as f32) as u32;
    let y0 = ((page_height - rect.y - rect.height) * scale).max(0.0) as u32;
    let y1 = ((page_height - rect.y) * scale).min(image.height() as f32) as u32;
    let (mut dark, mut total) = (0u32, 0u32);
    for y in y0..y1 {
        for x in x0..x1 {
            total += 1;
            let p = image.get_pixel(x, y);
            if u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 300 {
                dark += 1;
            }
        }
    }
    if total == 0 {
        0.0
    } else {
        dark as f32 / total as f32
    }
}

/// Text, selection and search rects must sit on the glyphs in the rendered
/// bitmap on rotated pages and pages whose crop box does not start at the
/// origin, not only on plain pages. They used to be raw user space, so
/// highlights and search hits were drawn beside the text on such pages.
#[test]
fn text_and_search_rects_sit_on_the_rendered_glyphs_whatever_the_page_geometry() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dir = std::env::temp_dir().join(format!("md-pdf-geometry-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    /// Crop box as (bottom, left, top, right).
    type Crop = Option<(f32, f32, f32, f32)>;
    let cases: [(&str, PdfPageRenderRotation, Crop); 5] = [
        ("plain", PdfPageRenderRotation::None, None),
        ("rotate-90", PdfPageRenderRotation::Degrees90, None),
        ("rotate-180", PdfPageRenderRotation::Degrees180, None),
        (
            "cropped",
            PdfPageRenderRotation::None,
            Some((50.0, 40.0, 280.0, 380.0)),
        ),
        (
            "rotate-270-cropped",
            PdfPageRenderRotation::Degrees270,
            Some((50.0, 40.0, 280.0, 380.0)),
        ),
    ];

    let renderer = PdfRenderer::new().unwrap();
    let scale = 2.0;
    for (name, rotation, crop) in cases {
        let path = dir.join(format!("{name}.pdf"));
        write_geometry_fixture(&path, rotation, crop);
        let path = path.to_string_lossy().to_string();

        let text = renderer.get_page_text(&path, 0).unwrap();
        let glyphs: Vec<_> = text.chars.iter().filter(|c| c.ch == 'M').cloned().collect();
        assert_eq!(glyphs.len(), 6, "{name}: {:?}", text.text);
        // The box around every glyph. On a quarter-turned page the run reads
        // vertically, so it is not one merged line.
        let on_text = &glyphs
            .iter()
            .skip(1)
            .fold(glyphs[0].bbox.clone(), |acc, c| {
                let (x0, y0) = (acc.x.min(c.bbox.x), acc.y.min(c.bbox.y));
                let x1 = (acc.x + acc.width).max(c.bbox.x + c.bbox.width);
                let y1 = (acc.y + acc.height).max(c.bbox.y + c.bbox.height);
                PdfRect {
                    x: x0,
                    y: y0,
                    width: x1 - x0,
                    height: y1 - y0,
                }
            });

        let image = renderer.render_page(&path, 0, scale).unwrap().to_rgba8();
        let inside = ink_in(&image, on_text, text.page_height, scale);
        assert!(
            inside > 0.15,
            "{name}: the text rect {on_text:?} covers no glyphs ({inside:.3} dark)"
        );
        // The same box moved across the run, by its own thickness, must be blank.
        let vertical = on_text.height > on_text.width;
        let beside = if vertical {
            let step = on_text.width * 1.5;
            PdfRect {
                x: if on_text.x > text.page_width / 2.0 {
                    on_text.x - step
                } else {
                    on_text.x + step
                },
                ..on_text.clone()
            }
        } else {
            let step = on_text.height * 1.5;
            PdfRect {
                y: if on_text.y > text.page_height / 2.0 {
                    on_text.y - step
                } else {
                    on_text.y + step
                },
                ..on_text.clone()
            }
        };
        let outside = ink_in(&image, &beside, text.page_height, scale);
        assert!(
            outside < 0.01,
            "{name}: ink beside the text rect ({outside:.3})"
        );

        let hits = renderer.search_text(&path, "MMM", false, false).unwrap();
        let hit = hits.first().and_then(|m| m.rects.first()).expect("a hit");
        let overlap_x = (hit.x + hit.width).min(on_text.x + on_text.width) - hit.x.max(on_text.x);
        let overlap_y = (hit.y + hit.height).min(on_text.y + on_text.height) - hit.y.max(on_text.y);
        assert!(
            overlap_x > 0.0 && overlap_y > 0.0,
            "{name}: search hit {hit:?} is not on the text {on_text:?}"
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn text_range_slices_by_char_and_clamps() {
    let mut page = page_fixture(0, &[("αβγ δ", 10.0)]);
    page.text = "αβγ δ€".to_string();
    page.offsets = Default::default();
    assert_eq!(page.text_range(1, 3), "βγ");
    assert_eq!(page.text_range(4, 99), "δ€");
    assert_eq!(page.text_range(5, 2), "");
}

#[test]
fn a_page_region_renders_the_text_under_it_from_the_top_left() {
    let _guard = TEST_LOCK.lock().unwrap();
    let renderer = PdfRenderer::new().unwrap();
    let path = "../tests-fixtures/pdf/dummy.pdf";
    let (_, page_h) = renderer.page_sizes(path).unwrap()[0];
    let hit = renderer.search_text(path, "dummy", false, false).unwrap()[0].rects[0].clone();
    // Search rects are y-up from the bottom; regions are measured from the top.
    let over_text = PageRegion {
        x: hit.x - 2.0,
        y: page_h - (hit.y + hit.height) - 2.0,
        width: hit.width + 4.0,
        height: hit.height + 4.0,
    };
    let image = renderer.render_region(path, 0, over_text, 400).unwrap();
    assert_eq!(image.rgba.len(), (image.width * image.height * 4) as usize);
    let expected_w = (over_text.width * (400.0 / over_text.width).clamp(1.0, 8.0)).round();
    assert!(
        (image.width as f32 - expected_w).abs() <= 2.0,
        "{} vs {expected_w}",
        image.width
    );
    let dark = |img: &RegionImage| img.rgba.chunks(4).filter(|p| p[0] < 128).count();
    assert!(dark(&image) > 0, "the word's glyphs are in the crop");

    let blank_corner = PageRegion {
        x: 0.0,
        y: page_h - 20.0,
        width: 20.0,
        height: 20.0,
    };
    let corner = renderer.render_region(path, 0, blank_corner, 100).unwrap();
    assert_eq!(dark(&corner), 0, "the bottom-left margin is empty");

    let off_page = PageRegion {
        x: -500.0,
        y: -500.0,
        width: 100.0,
        height: 100.0,
    };
    assert!(renderer.render_region(path, 0, off_page, 100).is_err());
    assert!(renderer.render_region(path, 999, over_text, 100).is_err());
}

#[test]
fn a_pdf_replaced_on_disk_is_read_again_not_served_from_the_cache() {
    let _guard = TEST_LOCK.lock().unwrap();
    let renderer = PdfRenderer::new().unwrap();
    let dir = std::env::temp_dir().join(format!("md_pdf_replaced_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("paper.pdf");
    std::fs::copy("../tests-fixtures/pdf/dummy.pdf", &path).unwrap();
    let path_str = path.to_string_lossy().to_string();
    let before = renderer.page_count(&path_str).unwrap();

    std::fs::copy("../tests-fixtures/pdf/large-500-pages.pdf", &path).unwrap();
    let after = renderer.page_count(&path_str).unwrap();
    assert_ne!(
        before, after,
        "the new file's pages, not the cached document's"
    );
    assert_eq!(after, 500);
    let _ = std::fs::remove_dir_all(&dir);
}

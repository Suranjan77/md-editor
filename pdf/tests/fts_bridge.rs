//! PDF text → FTS index bridge (plan §3.4: "FTS5 for markdown + extracted
//! PDF text"). Proves the vault's `TextExtractor` seam composes with the
//! real pdfium-backed renderer over the fixture corpus — the same wiring
//! the shell will own in production.
#![cfg(feature = "pdfium")]

use std::path::{Path, PathBuf};

use md_pdf::render::PdfRenderer;
use md_vault::{SearchIndex, TextExtractor};

/// The production-shaped adapter: all pages concatenated; any failure
/// (corrupt file, unreadable page) yields `None` so the index records the
/// file without retrying it every pass.
struct PdfiumExtractor {
    renderer: PdfRenderer,
}

impl TextExtractor for PdfiumExtractor {
    fn extract(&self, abs_path: &Path) -> Option<String> {
        let pages = self.renderer.page_count(abs_path).ok()?;
        let mut text = String::new();
        for page in 0..u32::from(pages) {
            text.push_str(&self.renderer.extract_text(abs_path, page).ok()?);
            text.push('\n');
        }
        Some(text)
    }
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests-fixtures/pdf")
}

fn extractor() -> Option<PdfiumExtractor> {
    // Dev machines can point at a local libpdfium via PDFIUM_LIB_DIR; otherwise
    // bind the system library, or skip (not fail) when neither is present.
    let lib_dir = std::env::var_os("PDFIUM_LIB_DIR").map(PathBuf::from);
    PdfRenderer::new(lib_dir.as_deref())
        .or_else(|_| PdfRenderer::new(None))
        .ok()
        .map(|renderer| PdfiumExtractor { renderer })
}

fn ok<T, E: std::fmt::Display>(r: Result<T, E>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn real_pdf_text_lands_in_the_search_index() {
    let Some(extractor) = extractor() else {
        eprintln!("skipping: libpdfium not available");
        return;
    };
    let dir = ok(tempfile::tempdir());
    let root = dir.path();
    ok(std::fs::copy(
        fixture_dir().join("multipage-outline.pdf"),
        root.join("paper.pdf"),
    ));
    ok(std::fs::write(
        root.join("note.md"),
        "markdown about parsers",
    ));

    let mut index = ok(SearchIndex::open_in_memory());
    let report = ok(index.sync_with(root, Some(&extractor)));
    assert_eq!(report.indexed, 2, "markdown and pdf both indexed");

    // Search for a word the renderer actually extracted, so the test
    // doesn't bake in fixture content.
    let text = match extractor.extract(&root.join("paper.pdf")) {
        Some(t) => t,
        None => panic!("fixture extraction failed"),
    };
    let Some(word) = text
        .split_whitespace()
        .find(|w| w.len() >= 4 && w.chars().all(char::is_alphanumeric))
    else {
        panic!("fixture yielded no searchable word: {text:?}");
    };
    let hits = ok(index.search(word, 10));
    assert!(
        hits.iter().any(|h| h.path == Path::new("paper.pdf")),
        "searching {word:?} finds the pdf, got {hits:?}"
    );

    // Cold-start guarantee holds for PDFs too: nothing re-extracted.
    let report = ok(index.sync_with(root, Some(&extractor)));
    assert_eq!(report.indexed, 0);
    assert_eq!(report.unchanged, 2);
}

#[test]
fn corrupt_pdf_is_recorded_without_poisoning_the_index() {
    let Some(extractor) = extractor() else {
        eprintln!("skipping: libpdfium not available");
        return;
    };
    let dir = ok(tempfile::tempdir());
    let root = dir.path();
    ok(std::fs::copy(
        fixture_dir().join("corrupt.pdf"),
        root.join("broken.pdf"),
    ));
    ok(std::fs::write(root.join("note.md"), "healthy markdown"));

    let mut index = ok(SearchIndex::open_in_memory());
    let report = ok(index.sync_with(root, Some(&extractor)));
    assert_eq!(report.indexed, 2, "corrupt file recorded, sync not aborted");
    assert_eq!(ok(index.search("healthy", 10)).len(), 1);

    let report = ok(index.sync_with(root, Some(&extractor)));
    assert_eq!(report.unchanged, 2, "corrupt file not re-extracted");
}

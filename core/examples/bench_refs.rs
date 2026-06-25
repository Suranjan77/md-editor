//! Spike benchmark for the PDF internal-reference feature.
//!
//! Measures where the time actually goes so we can pick an affordance tier:
//!   * `get_toc`            — for bookmark-less PDFs this runs build_page_text
//!                            over the WHOLE document (the Tier-B upfront scan).
//!   * per-page get_page_text loop — the same per-glyph `loose_bounds()` cost,
//!                            serialized; this is the dominant extraction cost.
//!   * extract_document_text — full text, NO bboxes (search-index path), as a
//!                            floor to isolate the loose_bounds overhead.
//!   * search_text          — pdfium native search (the Tier-A on-demand path).
//!
//! Run: cargo run --release --example bench_refs -- <file1.pdf> [file2.pdf ...]

use md_editor_core::pdf::PdfRenderer;
use std::time::Instant;

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: bench_refs <file.pdf> [more.pdf ...]");
        std::process::exit(2);
    }

    let renderer = PdfRenderer::new().expect("bind pdfium");

    for path in &paths {
        let name = std::path::Path::new(path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        println!("\n=== {name} ===");

        let pages = match renderer.page_count(path) {
            Ok(p) => p,
            Err(e) => {
                println!("  page_count failed: {e}");
                continue;
            }
        };
        println!("  pages: {pages}");

        // Tier-B upfront cost: get_toc. For bookmark-less PDFs this internally
        // does build_page_text over every page + the recovery heuristics.
        let t = Instant::now();
        let toc = renderer.get_toc(path);
        let toc_ms = t.elapsed().as_secs_f64() * 1e3;
        let (n_toc, synthetic) = match &toc {
            Ok((entries, synthetic)) => (count_entries(entries), *synthetic),
            Err(_) => (0, false),
        };
        println!(
            "  get_toc:               {toc_ms:8.1} ms   ({n_toc} entries, {})",
            if synthetic { "RECOVERED (full scan)" } else { "embedded (early-out)" }
        );

        // Dominant extraction cost: full per-page text WITH bboxes.
        let t = Instant::now();
        let mut chars_total = 0usize;
        for i in 0..pages {
            if let Ok(pt) = renderer.get_page_text(path, i) {
                chars_total += pt.chars.len();
            }
        }
        let pt_ms = t.elapsed().as_secs_f64() * 1e3;
        println!(
            "  get_page_text x{pages:<4}    {pt_ms:8.1} ms   ({:.2} ms/page, {chars_total} glyphs, {:.1} us/glyph)",
            pt_ms / pages as f64,
            if chars_total > 0 { pt_ms * 1e3 / chars_total as f64 } else { 0.0 }
        );

        // Floor: full text, no bboxes (isolates the loose_bounds overhead).
        let t = Instant::now();
        let txt_len = renderer.extract_document_text(path).map(|s| s.len()).unwrap_or(0);
        let txt_ms = t.elapsed().as_secs_f64() * 1e3;
        println!(
            "  extract_document_text: {txt_ms:8.1} ms   ({txt_len} bytes, no bboxes)"
        );

        // Tier-A on-demand cost: one native search for an equation-style token.
        let t = Instant::now();
        let hits = renderer.search_text(path, "(1.1)", false, false).map(|m| m.len()).unwrap_or(0);
        let search_ms = t.elapsed().as_secs_f64() * 1e3;
        println!(
            "  search_text \"(1.1)\":   {search_ms:8.1} ms   ({hits} hits) [Tier-A per-click]"
        );
    }
}

fn count_entries(entries: &[md_editor_core::pdf::TocEntry]) -> usize {
    entries.iter().map(|e| 1 + count_entries(&e.children)).sum()
}

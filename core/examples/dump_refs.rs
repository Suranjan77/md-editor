//! Spike validation: resolve internal references for real PDFs and print a
//! sample so we can eyeball precision before wiring any UI (mirrors how TOC
//! recovery was de-risked).
//!
//! Run: cargo run --release --example dump_refs -- <file.pdf> [more.pdf ...]

use md_editor_core::pdf::PdfRenderer;
use md_editor_core::references::{resolve_references, ReferenceKind};
use std::time::Instant;

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: dump_refs <file.pdf> [more.pdf ...]");
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
        let toc = renderer.get_toc(path).map(|(t, _)| t).unwrap_or_default();

        let t = Instant::now();
        let mut page_texts = Vec::with_capacity(pages as usize);
        for i in 0..pages {
            if let Ok(pt) = renderer.get_page_text(path, i) {
                page_texts.push(pt);
            }
        }
        let scan_ms = t.elapsed().as_secs_f64() * 1e3;

        let t = Instant::now();
        let links = resolve_references(&page_texts, &toc);
        let resolve_ms = t.elapsed().as_secs_f64() * 1e3;

        let (mut eq, mut fig, mut tab, mut sec) = (0, 0, 0, 0);
        for l in &links {
            match l.kind {
                ReferenceKind::Equation => eq += 1,
                ReferenceKind::Figure => fig += 1,
                ReferenceKind::Table => tab += 1,
                ReferenceKind::Section => sec += 1,
            }
        }
        println!(
            "  pages {pages}, scan {scan_ms:.0} ms, resolve {resolve_ms:.1} ms",
        );
        println!(
            "  links: {} total — {eq} eq, {fig} fig, {tab} tab, {sec} sec",
            links.len()
        );

        // Sample a few of each kind with the surrounding source-line text.
        for kind in [
            ReferenceKind::Equation,
            ReferenceKind::Figure,
            ReferenceKind::Table,
            ReferenceKind::Section,
        ] {
            let mut shown = 0;
            for l in links.iter().filter(|l| l.kind == kind) {
                if shown >= 4 {
                    break;
                }
                let needle = match l.kind {
                    ReferenceKind::Equation => format!("({})", l.label),
                    _ => l.label.clone(),
                };
                let ctx = source_context(&page_texts, l.src_page, &needle);
                println!(
                    "    [{:>3}] p{:<4} → p{:<4} {:<8} {:<7}  …{}…",
                    "",
                    l.src_page,
                    l.dest_page,
                    kind.as_str(),
                    l.label,
                    ctx
                );
                shown += 1;
            }
        }
    }
}

/// Grab a short window of text around the first occurrence of `label` on
/// `page`, for human sanity-checking that the link sits on a real reference.
fn source_context(
    pages: &[md_editor_core::pdf::PdfPageText],
    page: u16,
    needle: &str,
) -> String {
    let Some(pt) = pages.iter().find(|p| p.page_index == page) else {
        return String::new();
    };
    let flat: String = pt.text.chars().map(|c| if c == '\n' { ' ' } else { c }).collect();
    let hay = flat.to_lowercase();
    if let Some(byte_pos) = hay.find(&needle.to_lowercase()) {
        let char_pos = hay[..byte_pos].chars().count();
        flat.chars()
            .skip(char_pos.saturating_sub(30))
            .take(needle.chars().count() + 60)
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        String::new()
    }
}

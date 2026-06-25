//! Recognise *internal* cross-references in a PDF's text layer (numbered
//! equations, figures, tables, sections) and resolve each in-prose mention to
//! the page/position of its target — *without modifying the PDF*.
//!
//! This module is pure: it operates only on [`PdfPageText`] (already extracted
//! by the single pdfium worker thread) plus the recovered table of contents. It
//! never touches pdfium, so it is cheap, `Send`, and unit-testable windowlessly.
//! See `pdf-text-scan-costs` / `pdf-toc-recovery` for the surrounding design.
//!
//! Strategy: a one-pass scan builds a *target map* (label → location) for each
//! reference family, then a second pass over the same text finds *call-sites*
//! (mentions) and emits a link only when a call-site's label matches a unique
//! target. "No target ⇒ no link" is what keeps precision high and stray numbers
//! (intervals, quantities, years) from becoming bogus links.

use crate::pdf::{merge_char_rects, PdfPageText, PdfRect, TocEntry};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Version of the resolution algorithm. Bump whenever the resolver's output for
/// the same input would change (new rules, coordinate/aim tweaks) so stale
/// cached results (keyed by document id) are discarded and recomputed.
pub const RESOLVER_VERSION: u32 = 2;

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum ReferenceKind {
    Equation,
    Figure,
    Table,
    Section,
}

impl ReferenceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Equation => "Equation",
            Self::Figure => "Figure",
            Self::Table => "Table",
            Self::Section => "Section",
        }
    }
}

/// One resolved reference: a clickable region (`bbox`, top-left origin like
/// [`crate::pdf::LinkInfo`]) on `src_page` that points at `dest_page`/`dest_y`.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ReferenceLink {
    pub src_page: u16,
    /// Call-site rectangle in PDF points, **top-left origin** — matches the
    /// convention of `LinkInfo`/`get_page_links` so these drop straight into
    /// the existing hit-test (`link_at`) and right-click preview path.
    pub bbox: PdfRect,
    pub dest_page: u32,
    /// Target Y in PDF points, **top-left origin**, for the preview crop.
    /// `None` for section targets (whose precise Y we don't know — page top).
    pub dest_y: Option<f32>,
    pub label: String,
    pub kind: ReferenceKind,
}

/// A reference target: where a label lives in the document.
#[derive(Clone, Copy)]
struct Target {
    page: u16,
    /// Top-left-origin Y of the target line's centre, when known.
    dest_y: Option<f32>,
}

/// Fraction of the page width past which a token is considered to sit in the
/// right margin. Equation numbers are right-aligned, so a `(n)` token whose
/// left edge is past this is treated as a *label* (target), never a call-site.
const RIGHT_MARGIN_FRAC: f32 = 0.62;

/// How far (PDF points) to shift a figure/table preview target off its caption
/// toward the artwork. ~⅓ of the preview window (`PREVIEW_WINDOW_PT` = 300pt),
/// so the caption lands near the window edge and the figure/table fills the
/// rest of the frame instead of being clipped.
const CAPTION_PREVIEW_SHIFT: f32 = 110.0;

/// Resolve every recognised internal reference in the document.
///
/// `pages` is the full document text (in page order). `toc` is the recovered or
/// embedded outline, used to locate section targets by their leading number.
pub fn resolve_references(pages: &[PdfPageText], toc: &[TocEntry]) -> Vec<ReferenceLink> {
    let equations = build_equation_targets(pages);
    let captions = build_caption_targets(pages);
    let sections = build_section_targets(toc);

    let mut links = Vec::new();
    for page in pages {
        scan_equation_callsites(page, &equations, &mut links);
        scan_caption_callsites(page, &captions, &mut links);
        scan_section_callsites(page, &sections, &mut links);
    }
    dedup_links(&mut links);
    links
}

/// Drop duplicate links that land on the same spot (same page, same target,
/// overlapping rect) — e.g. when two keyword spellings match one mention.
fn dedup_links(links: &mut Vec<ReferenceLink>) {
    let mut seen = std::collections::HashSet::new();
    links.retain(|l| {
        seen.insert((
            l.src_page,
            l.dest_page,
            l.label.clone(),
            l.bbox.x.round() as i32,
            l.bbox.y.round() as i32,
        ))
    });
}

// ---------------------------------------------------------------------------
// Coordinate / char-range helpers
// ---------------------------------------------------------------------------

/// Convert a bottom-left-origin rect (the char/`loose_bounds` convention) to the
/// top-left-origin convention used by `LinkInfo` and the preview crop.
fn to_top_left(r: &PdfRect, page_height: f32) -> PdfRect {
    PdfRect {
        x: r.x,
        y: page_height - (r.y + r.height),
        width: r.width,
        height: r.height,
    }
}

/// Merged rect (top-left origin) covering the chars at `[start, end)` of the
/// page text. `page.text` and `page.chars` are 1:1, so a char index into the
/// string indexes the bbox array directly.
fn range_rect(page: &PdfPageText, start: usize, end: usize) -> Option<PdfRect> {
    let slice = page.chars.get(start..end)?;
    // A call-site token is short and on one visual line, so the first merged
    // segment is the whole token; take it as the clickable rect.
    let first = merge_char_rects(slice).into_iter().next()?;
    Some(to_top_left(&first, page.page_height))
}

/// Left edge (x, in points) of the char range, used for right-margin tests.
fn range_left_x(page: &PdfPageText, start: usize, end: usize) -> Option<f32> {
    page.chars
        .get(start..end)?
        .iter()
        .filter(|c| c.bbox.width > 0.0)
        .map(|c| c.bbox.x)
        .fold(None, |acc: Option<f32>, x| Some(acc.map_or(x, |a| a.min(x))))
}

/// Top-left-origin Y of the centre of the char range (a target line).
fn range_center_y_top_left(page: &PdfPageText, start: usize, end: usize) -> Option<f32> {
    let slice = page.chars.get(start..end)?;
    let (mut y_min, mut y_max) = (f32::MAX, f32::MIN);
    for c in slice.iter().filter(|c| c.bbox.height > 0.0) {
        y_min = y_min.min(c.bbox.y);
        y_max = y_max.max(c.bbox.y + c.bbox.height);
    }
    if y_min > y_max {
        return None;
    }
    let center_bottom_left = (y_min + y_max) / 2.0;
    Some(page.page_height - center_bottom_left)
}

/// Byte offset → char index within `page.text` (== index into `page.chars`).
fn char_index_at_byte(text: &str, byte: usize) -> usize {
    text[..byte].chars().count()
}

/// True when the char at `char_idx` begins a (trimmed) text line — i.e. the
/// preceding non-space char is a newline, or it is the document start. Used to
/// tell a *caption* ("Figure 3: …" at line start) from a *reference*
/// ("see Figure 3" mid-line).
fn at_line_start(page: &PdfPageText, char_idx: usize) -> bool {
    for c in page.chars[..char_idx].iter().rev() {
        match c.ch {
            ' ' | '\t' => continue,
            '\n' | '\r' => return true,
            _ => return false,
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Target builders
// ---------------------------------------------------------------------------

/// Insert `label → target`, but drop the label entirely if it is seen a second
/// time: a label that occurs at two locations is ambiguous (e.g. equation
/// numbering that restarts each chapter) and is unsafe to link.
fn insert_unique(map: &mut HashMap<String, Option<Target>>, label: String, t: Target) {
    map.entry(label)
        .and_modify(|slot| *slot = None)
        .or_insert(Some(t));
}

/// Drop ambiguous (`None`) entries, leaving only labels with a unique target.
fn finalize(map: HashMap<String, Option<Target>>) -> HashMap<String, Target> {
    map.into_iter().filter_map(|(k, v)| v.map(|t| (k, t))).collect()
}

/// Equation labels: a parenthesised dotted number, optionally with a trailing
/// letter (`(3.14)`, `(12)`, `(3.14a)`), sitting in the right margin beside a
/// display equation. The right-margin test is what distinguishes a *label* from
/// an in-prose mention of the same token.
fn build_equation_targets(pages: &[PdfPageText]) -> HashMap<String, Target> {
    let mut map: HashMap<String, Option<Target>> = HashMap::new();
    for page in pages {
        let right_min = page.page_width * RIGHT_MARGIN_FRAC;
        for (s, e, label) in find_paren_numbers(&page.text) {
            let cs = char_index_at_byte(&page.text, s);
            let ce = char_index_at_byte(&page.text, e);
            let Some(x) = range_left_x(page, cs, ce) else {
                continue;
            };
            if x < right_min {
                continue; // not right-aligned ⇒ not an equation label
            }
            let dest_y = range_center_y_top_left(page, cs, ce);
            insert_unique(&mut map, label, Target { page: page.page_index, dest_y });
        }
    }
    finalize(map)
}

/// Figure/Table caption targets: a line beginning `Figure N` / `Fig. N` /
/// `Table N`. Keyed `"figure 3"` / `"table 3"`.
fn build_caption_targets(pages: &[PdfPageText]) -> HashMap<String, Target> {
    let mut map: HashMap<String, Option<Target>> = HashMap::new();
    for page in pages {
        for line in &page.lines {
            let text = line_slice(page, line.start_text_index, line.end_text_index);
            let trimmed = text.trim_start();
            if let Some(label) = caption_label(trimmed) {
                let dest_y = range_center_y_top_left(
                    page,
                    line.start_text_index,
                    line.end_text_index,
                );
                insert_unique(&mut map, label, Target { page: page.page_index, dest_y });
            }
        }
    }
    finalize(map)
}

/// Section targets, drawn from the (embedded or recovered) TOC: a section number
/// at the start of a TOC title maps to that entry's page. Keyed `"3.2"`.
fn build_section_targets(toc: &[TocEntry]) -> HashMap<String, Target> {
    let mut map: HashMap<String, Option<Target>> = HashMap::new();
    collect_section_targets(toc, &mut map);
    finalize(map)
}

fn collect_section_targets(entries: &[TocEntry], map: &mut HashMap<String, Option<Target>>) {
    for e in entries {
        if let Some(num) = leading_section_number(&e.title) {
            if let Some(page) = e.page_index {
                insert_unique(
                    map,
                    num,
                    Target {
                        page: page as u16,
                        dest_y: None,
                    },
                );
            }
        }
        collect_section_targets(&e.children, map);
    }
}

// ---------------------------------------------------------------------------
// Call-site scanners
// ---------------------------------------------------------------------------

fn push_link(
    links: &mut Vec<ReferenceLink>,
    page: &PdfPageText,
    cs: usize,
    ce: usize,
    target: &Target,
    label: String,
    kind: ReferenceKind,
) {
    // Don't link a reference to itself (same page, overlapping line).
    if let Some(bbox) = range_rect(page, cs, ce) {
        links.push(ReferenceLink {
            src_page: page.page_index,
            bbox,
            dest_page: target.page as u32,
            dest_y: target.dest_y,
            label,
            kind,
        });
    }
}

fn scan_equation_callsites(
    page: &PdfPageText,
    targets: &HashMap<String, Target>,
    links: &mut Vec<ReferenceLink>,
) {
    let right_min = page.page_width * RIGHT_MARGIN_FRAC;
    for (s, e, label) in find_paren_numbers(&page.text) {
        let Some(target) = targets.get(&label) else {
            continue;
        };
        let cs = char_index_at_byte(&page.text, s);
        let ce = char_index_at_byte(&page.text, e);
        // Skip the label itself (right-aligned occurrence) and any occurrence
        // that *is* the target line.
        if let Some(x) = range_left_x(page, cs, ce) {
            if x >= right_min && page.page_index == target.page {
                continue;
            }
        }
        push_link(links, page, cs, ce, target, label, ReferenceKind::Equation);
    }
}

fn scan_caption_callsites(
    page: &PdfPageText,
    targets: &HashMap<String, Target>,
    links: &mut Vec<ReferenceLink>,
) {
    for (s, e, label, kind) in find_caption_refs(&page.text) {
        let Some(target) = targets.get(&label) else {
            continue;
        };
        let cs = char_index_at_byte(&page.text, s);
        // Skip the caption itself (the target line begins with this token).
        if at_line_start(page, cs) && page.page_index == target.page {
            continue;
        }
        let ce = char_index_at_byte(&page.text, e);
        // A caption is text adjacent to a (non-text) figure/table. The preview
        // centres on `dest_y`, so aim it at the artwork rather than the caption:
        // figures conventionally caption *below* the image (shift the target up,
        // i.e. smaller top-left y), tables caption *above* (shift down). This
        // keeps the figure/table itself in frame instead of clipped at the top.
        let aimed = Target {
            page: target.page,
            dest_y: target.dest_y.map(|y| match kind {
                ReferenceKind::Figure => y - CAPTION_PREVIEW_SHIFT,
                ReferenceKind::Table => y + CAPTION_PREVIEW_SHIFT,
                _ => y,
            }),
        };
        push_link(links, page, cs, ce, &aimed, label, kind);
    }
}

fn scan_section_callsites(
    page: &PdfPageText,
    targets: &HashMap<String, Target>,
    links: &mut Vec<ReferenceLink>,
) {
    for (s, e, label) in find_section_refs(&page.text) {
        let Some(target) = targets.get(&label) else {
            continue;
        };
        let cs = char_index_at_byte(&page.text, s);
        let ce = char_index_at_byte(&page.text, e);
        push_link(links, page, cs, ce, target, label, ReferenceKind::Section);
    }
}

// ---------------------------------------------------------------------------
// Token finders (byte-offset based, regex-free for hot-path clarity)
// ---------------------------------------------------------------------------

/// Find `(<dotted-number>[letter])` tokens, returning `(byte_start, byte_end,
/// normalized_label)` where the label is the inner number, e.g. `"3.14"`.
fn find_paren_numbers(text: &str) -> Vec<(usize, usize, String)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'(' {
            let start = i;
            let mut j = i + 1;
            let mut saw_digit = false;
            let mut ok = true;
            while j < bytes.len() && bytes[j] != b')' {
                let c = bytes[j];
                if c.is_ascii_digit() {
                    saw_digit = true;
                } else if c == b'.' {
                    // dotted separator, fine
                } else if c.is_ascii_alphabetic() && j + 1 < bytes.len() && bytes[j + 1] == b')' {
                    // trailing equation-variant letter, e.g. (3.14a)
                } else {
                    ok = false;
                    break;
                }
                j += 1;
            }
            if ok && saw_digit && j < bytes.len() && bytes[j] == b')' {
                let inner = &text[start + 1..j];
                // Strip a trailing variant letter for the lookup key.
                let label: String = inner.trim_end_matches(|c: char| c.is_ascii_alphabetic()).to_string();
                // Reject bare 4+-digit numbers: these are years in citations
                // (`(2003)`), not equation labels. Dotted numbers (`3.14`) and
                // short ints (`(12)`) are kept.
                let digits = label.chars().filter(|c| c.is_ascii_digit()).count();
                if !label.is_empty() && (label.contains('.') || digits <= 3) {
                    out.push((start, j + 1, label));
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Recognise a caption *target* line: returns the normalised label for a line
/// starting with `Figure N[.M]`, `Fig. N[.M]`, or `Table N[.M]`. Numbers are
/// dotted-aware so `Figure 1.1` and `Figure 1.2` stay distinct (keyed
/// `"figure 1.1"`).
fn caption_label(trimmed: &str) -> Option<String> {
    for (kw, family) in [("figure", "figure"), ("fig", "figure"), ("table", "table")] {
        if let Some(num) = match_keyword_dotted(trimmed, kw) {
            return Some(format!("{family} {num}"));
        }
    }
    None
}

/// If `s` starts (case-insensitively) with `kw`, optional `.`, spaces, then a
/// (possibly dotted) number, return that number string. `match_keyword_dotted("Fig. 3: foo", "fig")` → `"3"`.
fn match_keyword_dotted(s: &str, kw: &str) -> Option<String> {
    let lower = s.to_ascii_lowercase();
    let rest = lower.strip_prefix(kw)?;
    // "fig" is tried after "figure"; on a "figure…" string the dotted parse
    // below starts on 'u' and fails, so there is no double match.
    let rest = rest.trim_start_matches('.').trim_start();
    take_dotted(rest, 0).map(|(_, label)| label)
}

/// Find figure/table *references* in prose: `Fig. 3`, `Figure 1.1`, `Table 2.3`.
/// Returns `(byte_start, byte_end, label, kind)`.
fn find_caption_refs(text: &str) -> Vec<(usize, usize, String, ReferenceKind)> {
    let mut out = Vec::new();
    for (kw, family, kind) in [
        ("figure", "figure", ReferenceKind::Figure),
        ("fig", "figure", ReferenceKind::Figure),
        ("table", "table", ReferenceKind::Table),
    ] {
        find_keyword_dotted_refs(text, kw, |start, end, num| {
            out.push((start, end, format!("{family} {num}"), kind));
        });
    }
    out
}

/// Find section references: `Section 3.2`, `Sec. 3.2`, `§3.2`.
/// Returns `(byte_start, byte_end, "3.2")`.
fn find_section_refs(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    for kw in ["section", "sec", "§"] {
        find_keyword_dotted_refs(text, kw, |start, end, num| {
            out.push((start, end, num));
        });
    }
    out
}

/// Scan for `<kw>[.] <dotted-number>` occurrences (word-boundary aware),
/// invoking `f(start, end, number_string)` for each. Case-insensitive on ASCII.
fn find_keyword_dotted_refs(text: &str, kw: &str, mut f: impl FnMut(usize, usize, String)) {
    scan_keyword(text, kw, |after_kw_byte, kw_start| {
        let (end, label) = take_dotted(text, after_kw_byte)?;
        f(kw_start, end, label);
        Some(())
    });
}

/// Core keyword scanner. Finds each case-insensitive occurrence of `kw` that
/// starts on a word boundary, skips an optional `.` and surrounding spaces, and
/// calls `body(byte_after_separators, kw_start_byte)`.
fn scan_keyword(text: &str, kw: &str, mut body: impl FnMut(usize, usize) -> Option<()>) {
    let lower = text.to_ascii_lowercase();
    let lb = lower.as_bytes();
    let kb = kw.as_bytes();
    if kb.is_empty() || kb.len() > lb.len() {
        return;
    }
    let mut i = 0;
    while i + kb.len() <= lb.len() {
        if &lb[i..i + kb.len()] == kb {
            // Word boundary before the keyword (keywords starting with a letter).
            let boundary = i == 0
                || !lb[i - 1].is_ascii_alphanumeric()
                || !kb[0].is_ascii_alphanumeric();
            if boundary {
                let mut j = i + kb.len();
                // optional '.' then spaces (and a possible non-breaking space)
                if j < lb.len() && lb[j] == b'.' {
                    j += 1;
                }
                while j < lb.len() && (lb[j] == b' ' || lb[j] == b'\t' || lb[j] == 0xa0) {
                    j += 1;
                }
                if body(j, i).is_some() {
                    i += kb.len();
                    continue;
                }
            }
        }
        i += 1;
    }
}

/// Parse a dotted number (`3`, `3.2`, `3.2.1`) at `byte`, returning
/// `(end_byte, label)`. Requires at least one digit.
fn take_dotted(text: &str, byte: usize) -> Option<(usize, String)> {
    let b = text.as_bytes();
    let mut j = byte;
    let mut saw_digit = false;
    while j < b.len() {
        if b[j].is_ascii_digit() {
            saw_digit = true;
            j += 1;
        } else if b[j] == b'.' && j + 1 < b.len() && b[j + 1].is_ascii_digit() {
            j += 1;
        } else {
            break;
        }
    }
    if !saw_digit {
        return None;
    }
    Some((j, text[byte..j].to_string()))
}

/// Leading dotted section number of a heading/title, e.g. `"3.2 Foo"` → `"3.2"`.
/// Rejects bare integers (a chapter title `"5 Foo"` is too coarse to target by
/// number reliably) to keep section linking precise.
fn leading_section_number(title: &str) -> Option<String> {
    let t = title.trim_start();
    let (_end, label) = take_dotted(t, 0)?;
    if label.contains('.') {
        Some(label)
    } else {
        None
    }
}

/// Text of a char range `[start, end)` of a page (1:1 string/char mapping).
fn line_slice(page: &PdfPageText, start: usize, end: usize) -> String {
    page.text.chars().skip(start).take(end - start).collect()
}

#[cfg(test)]
mod tests {
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
                    bbox: PdfRect { x, y: *y, width: CW, height: CH },
                });
                x += CW;
            }
            text.push('\n');
            chars.push(PdfTextChar {
                char_index: chars.len() as u32,
                text_index: chars.len(),
                ch: '\n',
                bbox: PdfRect { x, y: *y, width: 0.0, height: 0.0 },
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
        assert_eq!(eq.len(), 1, "exactly one call-site link, not the label itself");
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
            &[("Figure 1.1 first", 50.0, 600.0), ("Figure 1.2 second", 50.0, 400.0)],
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
        let t: Vec<_> = links.iter().filter(|l| l.kind == ReferenceKind::Table).collect();
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
        let s: Vec<_> = links.iter().filter(|l| l.kind == ReferenceKind::Section).collect();
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
        let l = links.iter().find(|l| l.kind == ReferenceKind::Equation).unwrap();
        // "(1.1)" starts after "xx " = 3 chars ⇒ x ≈ 50 + 3*CW.
        assert!((l.bbox.x - (50.0 + 3.0 * CW)).abs() < 1.0, "bbox.x={}", l.bbox.x);
        // top-left y for a line with baseline y=700,h=10 ⇒ 800-(700+10)=90.
        assert!((l.bbox.y - 90.0).abs() < 1.0, "bbox.y={}", l.bbox.y);
    }
}

//! Page text extraction and table-of-contents recovery: embedded bookmarks, printed contents pages, dot leaders, and a typographic fallback.

use super::*;

/// The embedded outline (bookmarks) of a document, empty when there are none.
pub(super) fn embedded_toc(doc: &PdfDocument) -> Vec<TocEntry> {
    let mut bookmarks = Vec::new();
    let mut current = doc.bookmarks().root();
    while let Some(bookmark) = current {
        current = bookmark.next_sibling();
        bookmarks.push(bookmark);
    }
    PdfRenderer::parse_bookmarks(&bookmarks)
}

/// Extract the text layer of every page, in page order. This is the one-time
/// full-document scan shared by TOC recovery and reference resolution.
pub(super) fn build_all_page_texts(doc: &PdfDocument) -> Vec<PdfPageText> {
    let pages_handle = doc.pages();
    let page_count = pages_handle.len();
    let mut page_texts = Vec::new();
    for i in 0..page_count {
        let page = match pages_handle.get(i) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if let Ok(pt) = build_page_text(&page, i as u16) {
            page_texts.push(pt);
        }
    }
    page_texts
}

/// Recover an outline for a bookmark-less document from its page text, best
/// source first: a printed contents page via its link annotations, then its
/// dot-leader text, and only failing that a typographic heuristic.
pub(super) fn recover_toc(doc: &PdfDocument, page_texts: &[PdfPageText]) -> Vec<TocEntry> {
    let toc_pages = detect_toc_pages(page_texts);
    if !toc_pages.is_empty() {
        let linked = harvest_linked_toc(doc, page_texts, &toc_pages);
        if linked.len() >= 2 {
            return linked;
        }
    }
    let printed = parse_printed_toc(page_texts);
    if printed.len() >= 2 {
        return printed;
    }
    synthesize_toc(page_texts)
}

/// Recover an outline from page text alone (no `PdfDocument`), for callers that
/// have already extracted the text and want section structure without a second
/// scan or the doc handle. Skips the link-annotation path (which needs the doc);
/// uses the printed-contents text parser, then the typographic heuristic.
pub fn recover_toc_from_texts(page_texts: &[PdfPageText]) -> Vec<TocEntry> {
    let printed = parse_printed_toc(page_texts);
    if printed.len() >= 2 {
        return printed;
    }
    synthesize_toc(page_texts)
}

/// Extract a page's text, per-character bounding boxes, and grouped text lines.
///
/// Shared by the `GetPageText` command and the auto-TOC synthesis path so the
/// line-grouping heuristic lives in exactly one place.
pub(super) fn build_page_text(page: &PdfPage, index: u16) -> Result<PdfPageText, String> {
    let text_page = page.text().map_err(|e| e.to_string())?;

    let page_width = page.width().value;
    let page_height = page.height().value;

    let mut text = String::new();
    let mut chars = Vec::new();
    let mut text_index = 0usize;

    for c in text_page.chars().iter() {
        if let Some(ch) = c.unicode_char() {
            let char_index = c.index() as u32;
            text.push(ch);

            let bbox = match c.loose_bounds() {
                Ok(rect) => PdfRect {
                    x: rect.left().value,
                    y: rect.bottom().value,
                    width: rect.width().value,
                    height: rect.height().value,
                },
                Err(_) => PdfRect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                },
            };

            chars.push(PdfTextChar {
                char_index,
                text_index,
                ch,
                bbox,
            });
            text_index += 1;
        }
    }

    // Group into lines
    let mut lines = Vec::new();
    let mut current_line: Option<PdfTextLine> = None;

    for c in chars
        .iter()
        .filter(|c| c.bbox.width > 0.0 && c.bbox.height > 0.0)
    {
        match &mut current_line {
            Some(line) => {
                let line_y_min = line.bbox.y;
                let line_y_max = line.bbox.y + line.bbox.height;
                let c_y_min = c.bbox.y;
                let c_y_max = c.bbox.y + c.bbox.height;

                let overlap = line_y_max.min(c_y_max) - line_y_min.max(c_y_min);
                let min_h = line.bbox.height.min(c.bbox.height);

                if overlap > 0.0 && overlap > 0.3 * min_h {
                    // Merge bbox
                    let x_min = line.bbox.x.min(c.bbox.x);
                    let x_max = (line.bbox.x + line.bbox.width).max(c.bbox.x + c.bbox.width);
                    let y_min = line.bbox.y.min(c.bbox.y);
                    let y_max = (line.bbox.y + line.bbox.height).max(c.bbox.y + c.bbox.height);

                    line.bbox.x = x_min;
                    line.bbox.y = y_min;
                    line.bbox.width = x_max - x_min;
                    line.bbox.height = y_max - y_min;
                    line.end_text_index = c.text_index + 1;
                } else {
                    lines.push(current_line.take().expect("matched Some above"));
                    current_line = Some(PdfTextLine {
                        start_text_index: c.text_index,
                        end_text_index: c.text_index + 1,
                        bbox: c.bbox.clone(),
                    });
                }
            }
            None => {
                current_line = Some(PdfTextLine {
                    start_text_index: c.text_index,
                    end_text_index: c.text_index + 1,
                    bbox: c.bbox.clone(),
                });
            }
        }
    }
    if let Some(line) = current_line {
        lines.push(line);
    }

    Ok(PdfPageText {
        page_index: index,
        page_width,
        page_height,
        text,
        chars,
        lines,
    })
}

/// Text of a single line, sliced out of the page's character buffer.
pub(super) fn line_text(page: &PdfPageText, line: &PdfTextLine) -> String {
    page.text
        .chars()
        .skip(line.start_text_index)
        .take(line.end_text_index.saturating_sub(line.start_text_index))
        .collect::<String>()
        .trim()
        .to_string()
}

/// Nesting depth of a numbered-heading prefix, or `None` when the line does not
/// begin with a recognizable section number.
///
/// Recognizes dotted numeric prefixes (`1.`, `1.2`, `1.2.3`) and keyworded
/// headings (`Chapter 4`, `Section 2`, `Part III`, `Appendix B`). A bare number
/// with no dot (e.g. `2020 was a year`) is intentionally rejected to avoid
/// matching years and quantities. The returned depth drives heading nesting:
/// `1.` → 1, `1.2` → 2, `Chapter 4` → 1.
pub(super) fn numbered_heading_depth(title: &str) -> Option<usize> {
    let t = title.trim_start();

    // Keyworded headings.
    let lower = t.to_lowercase();
    for kw in ["chapter ", "section ", "part ", "appendix "] {
        if lower.starts_with(kw) {
            let rest = t[kw.len()..].trim_start();
            if rest.chars().next().is_some_and(|c| c.is_alphanumeric()) {
                return Some(1);
            }
        }
    }

    // Dotted numeric prefix: runs of ASCII digits separated by '.'.
    let mut depth = 0usize;
    let mut saw_digit = false;
    let mut consumed = 0usize;
    for c in t.chars() {
        if c.is_ascii_digit() {
            saw_digit = true;
            consumed += 1;
        } else if c == '.' && saw_digit {
            depth += 1;
            saw_digit = false;
            consumed += 1;
        } else {
            break;
        }
    }
    if saw_digit {
        depth += 1; // trailing segment with no dot, e.g. the "2" in "1.2"
    }
    if depth == 0 {
        return None;
    }
    // The number must be followed by whitespace and real heading text.
    let after = &t[consumed..];
    let has_text =
        matches!(after.chars().next(), Some(c) if c.is_whitespace()) && !after.trim().is_empty();
    if !has_text {
        return None;
    }
    if t[..consumed].contains('.') {
        // Dotted section number: "1." -> 1, "1.2" -> 2.
        return Some(depth);
    }
    // Bare integer with no dot. Accept as a top-level heading only when it is a
    // plausible section index (<= 2 digits, so years like "2020" are rejected)
    // followed by a capitalized word ("1 Introduction"), which separates real
    // headings from quantities ("1 apple") and sentences ("2020 was a year").
    if consumed <= 2
        && after
            .trim_start()
            .chars()
            .next()
            .is_some_and(|c| c.is_uppercase())
    {
        return Some(1);
    }
    None
}

/// True when a line reads like a heading title rather than mathematics or
/// decorative glyphs. Rejects strings dominated by symbols and any containing
/// Private Use Area code points (font-specific math brackets, ligatures, icons),
/// which are a common source of junk entries on equation-heavy pages.
pub(super) fn is_plausible_title(title: &str) -> bool {
    // A heading is a single line; embedded newlines/control chars mark captured
    // multi-line content such as a display equation, not a title. Private Use
    // Area code points are font-specific math brackets, ligatures, or icons.
    if title
        .chars()
        .any(|c| c.is_control() || ('\u{E000}'..='\u{F8FF}').contains(&c))
    {
        return false;
    }
    let letters = title.chars().filter(|c| c.is_alphabetic()).count();
    let non_space = title.chars().filter(|c| !c.is_whitespace()).count();
    // At least three letters, and letters must form the majority of glyphs.
    letters >= 3 && letters * 2 >= non_space
}

/// True for a short line set entirely in uppercase letters (e.g. `INTRODUCTION`),
/// a common heading style that carries no extra glyph height.
pub(super) fn is_caps_heading(title: &str) -> bool {
    let letters = title.chars().filter(|c| c.is_alphabetic()).count();
    letters >= 3
        && title
            .chars()
            .filter(|c| c.is_alphabetic())
            .all(|c| c.is_uppercase())
}

/// Vertical window, in PDF points, shown around a reference's destination in the
/// right-click link preview. ~⅓ of a US-Letter page: enough context to read the
/// referenced line/figure while keeping it comfortably zoomed.
pub(super) const PREVIEW_WINDOW_PT: f32 = 300.0;

/// Maximum heading-nesting depth used across every TOC-recovery strategy.
pub(super) const MAX_TOC_LEVELS: usize = 4;
/// How many leading pages to scan when looking for a printed contents page.
pub(super) const MAX_TOC_SCAN: usize = 24;
/// Characters that act as TOC leaders between a title and its page number.
pub(super) const LEADER_CHARS: [char; 5] = ['.', ' ', '\u{00b7}', '\u{2026}', '\t'];

/// Whitespace-collapsed, lowercased form of a heading for cross-page matching.
pub(super) fn normalize_heading(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// True for a line that is itself the "Contents" / "Table of Contents" title.
pub(super) fn is_contents_heading(text: &str) -> bool {
    let low = normalize_heading(text);
    low == "contents" || low == "table of contents" || low.starts_with("table of contents")
}

/// Strip a trailing dot-leader and page number from a heading-like string,
/// e.g. `"Introduction ........ 12"` -> `"Introduction"`. A trailing number is
/// only removed when preceded by a real leader (>= 2 dots), so titles that
/// legitimately end in a number (`"Chapter 5"`) are preserved.
pub(super) fn strip_leader_tail(text: &str) -> String {
    let t = text.trim();
    let digits = t.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return t.to_string();
    }
    let head = &t[..t.len() - digits];
    let dots = head
        .chars()
        .rev()
        .take_while(|c| LEADER_CHARS.contains(c))
        .filter(|c| *c != ' ' && *c != '\t')
        .count();
    if dots >= 2 {
        head.trim_end_matches(|c: char| LEADER_CHARS.contains(&c))
            .trim()
            .to_string()
    } else {
        t.to_string()
    }
}

/// Clean the text under a TOC link annotation into a title. The link rect often
/// spans the whole row, so the captured text wraps across visual lines (joined
/// here with spaces) and ends with the right-column page number (dropped, since
/// the link's destination already gives the exact page).
pub(super) fn clean_link_title(raw: &str) -> String {
    let mut parts: Vec<&str> = raw
        .split(['\n', '\r'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if parts
        .last()
        .is_some_and(|s| s.chars().all(|c| c.is_ascii_digit()))
    {
        parts.pop();
    }
    strip_leader_tail(&parts.join(" "))
}

/// Parse a printed table-of-contents line into `(title, printed_page_number)`.
///
/// Matches `Some Section Title .......... 23` and, with section numbering,
/// `1.2 Some Section .... 23`. A trailing integer is required. When
/// `require_leader` is true a run of >= 2 leader dots must separate the title
/// from the number, so ordinary prose ending in a number is not misread; this
/// is relaxed on pages already identified as a TOC (where space-only leaders
/// are common).
pub(super) fn parse_leader_line(text: &str, require_leader: bool) -> Option<(String, u32)> {
    let t = text.trim();
    let digit_count = t.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digit_count == 0 || digit_count > 6 {
        return None;
    }
    let split = t.len() - digit_count; // digits are ASCII, so byte == char here
    let page: u32 = t[split..].parse().ok()?;
    let head = &t[..split];
    let dot_count = head
        .chars()
        .rev()
        .take_while(|c| LEADER_CHARS.contains(c))
        .filter(|c| *c != ' ' && *c != '\t')
        .count();
    if require_leader && dot_count < 2 {
        return None;
    }
    let title = head
        .trim_end_matches(|c: char| LEADER_CHARS.contains(&c))
        .trim();
    // A real entry has a title with letters (rejects dotted number-only rows).
    if title.is_empty() || !title.chars().any(|c| c.is_alphabetic()) {
        return None;
    }
    Some((title.to_string(), page))
}

/// Assemble a flat list of `(title, page_index, level)` entries (in document
/// order, `level` 0 = top) into a nested tree, attaching each entry under the
/// most recent entry at a strictly shallower level.
pub(super) fn build_toc_tree(items: Vec<(String, Option<u32>, usize)>) -> Vec<TocEntry> {
    let mut roots: Vec<TocEntry> = Vec::new();
    // Path of child-indices from a root down to the last entry at each level.
    let mut path: Vec<(usize, Vec<usize>)> = Vec::new();
    for (title, page_index, level) in items {
        let entry = TocEntry {
            title,
            page_index,
            children: Vec::new(),
        };
        while path.last().map(|(l, _)| *l >= level).unwrap_or(false) {
            path.pop();
        }
        let new_path = if let Some((_, parent_path)) = path.last().cloned() {
            let mut node = &mut roots;
            for &idx in &parent_path {
                node = &mut node[idx].children;
            }
            node.push(entry);
            let mut p = parent_path;
            p.push(node.len() - 1);
            p
        } else {
            roots.push(entry);
            vec![roots.len() - 1]
        };
        path.push((level, new_path));
    }
    roots
}

/// Distinct left-edge indents (ascending, merged within 2pt, capped at
/// [`MAX_TOC_LEVELS`]) used to map an entry's x position to a nesting level.
pub(super) fn indent_bands(indents: &[f32]) -> Vec<f32> {
    let mut v: Vec<f32> = indents.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v.dedup_by(|a, b| (*a - *b).abs() < 2.0);
    v.truncate(MAX_TOC_LEVELS);
    v
}

/// Nesting level (0 = outermost) for an entry at left edge `x`.
pub(super) fn level_for_indent(bands: &[f32], x: f32) -> usize {
    bands
        .iter()
        .rposition(|&i| x >= i - 2.0)
        .unwrap_or(0)
        .min(bands.len().saturating_sub(1))
}

/// Level for a flat entry: its section-number depth when numbered, else its
/// indent band. Shared by the link- and text-based printed-TOC strategies.
pub(super) fn entry_level(title: &str, indent: f32, bands: &[f32]) -> usize {
    numbered_heading_depth(title)
        .map(|d| (d - 1).min(MAX_TOC_LEVELS - 1))
        .unwrap_or_else(|| level_for_indent(bands, indent))
}

/// One recovered contents-page entry. `real` is true when the page number came
/// from an actual pairing (embedded leader or number column) rather than being
/// inherited by a standalone heading whose number failed to pair.
pub(super) struct TocRow {
    pub(super) title: String,
    pub(super) page: u32,
    pub(super) indent: f32,
    pub(super) real: bool,
}

/// Extract entries from one contents page.
///
/// Handles two layouts that defeat a naive per-line parse:
///  * **Dot leaders** — `Some Title .......... 23` on a single line.
///  * **Two-column** — the page number sits in a separate right-hand column on
///    the *same row* as its title (no leader at all), which is how most
///    professionally typeset documents lay out their contents.
///
/// Title lines whose row carries no page number are treated as wrapped
/// continuations and merged into the preceding entry.
pub(super) fn extract_toc_entries(page: &PdfPageText) -> Vec<TocRow> {
    struct Line {
        text: String,
        x: f32,
        y: f32,
        h: f32,
        num: Option<u32>,
    }
    // A page-number column lives in the right portion of the page; section
    // numbers and footers do not, so left-half numerics are ignored.
    let right_min = page.page_width * 0.5;
    let mut lines: Vec<Line> = Vec::new();
    let mut num_col: Vec<(f32, u32, f32)> = Vec::new(); // (y, value, height)
    for line in &page.lines {
        let t = line_text(page, line);
        if t.is_empty() || is_contents_heading(&t) {
            continue;
        }
        if let Ok(v) = t.parse::<u32>() {
            if t.len() <= 4 && line.bbox.x > right_min {
                num_col.push((line.bbox.y, v, line.bbox.height));
            }
            continue; // a numeric-only line is never a title
        }
        lines.push(Line {
            text: t,
            x: line.bbox.x,
            y: line.bbox.y,
            h: line.bbox.height,
            num: None,
        });
    }
    let has_column = !num_col.is_empty();

    // Resolve each line's title and (when present) its page number: from an
    // embedded leader, else from the number column at the same row. An embedded
    // number always requires a real dot leader — without one, a trailing integer
    // is far more likely a year, equation, or reference number (as in a paper's
    // bibliography) than a page reference.
    let mut titled: Vec<Line> = Vec::with_capacity(lines.len());
    for mut l in lines {
        if let Some((title, n)) = parse_leader_line(&l.text, true) {
            l.text = title;
            l.num = Some(n);
        } else if has_column {
            let tol = (l.h.max(1.0) * 0.7).max(2.0);
            l.num = num_col
                .iter()
                .find(|(ny, _, nh)| (ny - l.y).abs() <= tol.max(nh * 0.7))
                .map(|(_, v, _)| *v);
        }
        titled.push(l);
    }

    // Walk rows top-to-bottom; a numbered row starts an entry, an unnumbered row
    // continues the previous one.
    titled.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<TocRow> = Vec::new();
    let mut pending: Option<TocRow> = None;
    for l in titled {
        match l.num {
            Some(n) => {
                if let Some(p) = pending.take() {
                    out.push(p);
                }
                pending = Some(TocRow {
                    title: l.text.trim().to_string(),
                    page: n,
                    indent: l.x,
                    real: true,
                });
            }
            None => {
                // A row with no page number is normally a wrapped continuation
                // of the entry above it. But if it is plainly a heading in its
                // own right (numbered or ALL-CAPS), keep it as its own entry —
                // its number simply failed to pair — inheriting the previous
                // entry's page so it stays navigable. Inherited entries are
                // flagged `real = false` so they don't count toward TOC-page
                // detection (front-matter prose has such subheadings too).
                let standalone =
                    numbered_heading_depth(&l.text).is_some() || is_caps_heading(l.text.trim());
                if standalone {
                    let page = pending.as_ref().map(|p| p.page).unwrap_or(0);
                    if let Some(p) = pending.take() {
                        out.push(p);
                    }
                    pending = Some(TocRow {
                        title: l.text.trim().to_string(),
                        page,
                        indent: l.x,
                        real: false,
                    });
                } else if let Some(p) = pending.as_mut() {
                    p.title.push(' ');
                    p.title.push_str(l.text.trim());
                }
            }
        }
    }
    if let Some(p) = pending.take() {
        out.push(p);
    }
    out
}

/// Indices of leading pages that look like a printed table of contents: a page
/// carrying a "Contents" heading alongside entries, or one with a strong run of
/// page-numbered entries (covers multi-page TOCs with no repeated heading).
pub(super) fn detect_toc_pages(pages: &[PdfPageText]) -> Vec<usize> {
    let scan = pages.len().min(MAX_TOC_SCAN);
    let mut candidates: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut heading_candidate: Option<usize> = None;
    for (i, page) in pages.iter().enumerate().take(scan) {
        let has_heading = page
            .lines
            .iter()
            .any(|l| is_contents_heading(&line_text(page, l)));
        // Count only entries with a genuinely paired page number; inherited
        // standalone headings (common in front-matter prose) don't qualify.
        let real = extract_toc_entries(page).iter().filter(|e| e.real).count();
        if (has_heading && real >= 2) || real >= 5 {
            candidates.insert(i);
            if has_heading && heading_candidate.is_none() {
                heading_candidate = Some(i);
            }
        }
    }
    // A real TOC occupies a contiguous run of pages; isolated later matches are
    // body pages that merely contain numbered lists/figures. Anchor on the first
    // "Contents" page when present (else the first candidate) and extend forward
    // only while pages stay TOC-like.
    let Some(anchor) = heading_candidate.or_else(|| candidates.iter().min().copied()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut p = anchor;
    while candidates.contains(&p) {
        out.push(p);
        p += 1;
    }
    out
}

/// Learn the constant offset `physical_index - printed_page_number` by matching
/// a few TOC titles to headings on the body pages they point at. Returns `None`
/// unless at least two entries agree, so a single coincidental match cannot
/// misalign the whole outline.
pub(super) fn learn_page_delta(
    pages: &[PdfPageText],
    last_toc: usize,
    entries: &[(&str, u32)],
) -> Option<i64> {
    let mut votes: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
    for (title, printed) in entries.iter().take(40) {
        let norm = normalize_heading(title);
        if norm.len() < 4 {
            continue;
        }
        for page in pages.iter().filter(|p| (p.page_index as usize) > last_toc) {
            let matched = page
                .lines
                .iter()
                .any(|l| normalize_heading(&line_text(page, l)) == norm);
            if matched {
                let delta = page.page_index as i64 - *printed as i64;
                *votes.entry(delta).or_default() += 1;
                break; // first matching page wins for this entry
            }
        }
    }
    votes
        .into_iter()
        .filter(|(_, c)| *c >= 2)
        .max_by_key(|(_, c)| *c)
        .map(|(d, _)| d)
}

/// Recover a printed table of contents from its link annotations: for each link
/// on a detected TOC page, the anchored text becomes the title and the link's
/// destination gives the exact page index. The most reliable strategy when it
/// applies, since the page mapping is taken from the document, not guessed.
pub(super) fn harvest_linked_toc(
    doc: &PdfDocument,
    page_texts: &[PdfPageText],
    toc_pages: &[usize],
) -> Vec<TocEntry> {
    let toc_set: std::collections::HashSet<usize> = toc_pages.iter().copied().collect();
    let pages = doc.pages();
    struct Raw {
        title: String,
        dest: u32,
        indent: f32,
        page_rank: usize, // position of the TOC page in reading order
        top: f32,         // top edge, for top-to-bottom ordering within a page
    }
    let mut raws: Vec<Raw> = Vec::new();
    let mut seen: std::collections::HashSet<(String, u32)> = std::collections::HashSet::new();
    for (rank, &pi) in toc_pages.iter().enumerate() {
        let page = match pages.get(pi as i32) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let ptext = page_texts.iter().find(|p| p.page_index as usize == pi);
        for link in page.links().iter() {
            let rect = match link.rect() {
                Ok(r) => r,
                Err(_) => continue,
            };
            let mut dest_page: Option<u32> = None;
            if let Some(PdfAction::LocalDestination(ref local)) = link.action()
                && let Ok(dest) = local.destination()
            {
                dest_page = dest.page_index().ok().map(|i| i as u32);
            }
            if dest_page.is_none()
                && let Some(dest) = link.destination()
            {
                dest_page = dest.page_index().ok().map(|i| i as u32);
            }
            let Some(dest) = dest_page else {
                continue;
            };
            if toc_set.contains(&(dest as usize)) {
                continue; // intra-TOC navigation link, not an entry
            }
            let Some(pt) = ptext else { continue };
            let title = clean_link_title(&text_in_rect(
                pt,
                rect.left().value,
                rect.bottom().value,
                rect.right().value,
                rect.top().value,
            ));
            if !is_plausible_title(&title) {
                continue;
            }
            if !seen.insert((title.clone(), dest)) {
                continue;
            }
            raws.push(Raw {
                title,
                dest,
                indent: rect.left().value,
                page_rank: rank,
                top: rect.top().value,
            });
        }
    }
    if raws.len() < 2 {
        return Vec::new();
    }
    // Link annotations are not necessarily stored in reading order, so sort into
    // it (page, then top-to-bottom) before nesting by indent.
    raws.sort_by(|a, b| {
        a.page_rank.cmp(&b.page_rank).then(
            b.top
                .partial_cmp(&a.top)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    let bands = indent_bands(&raws.iter().map(|r| r.indent).collect::<Vec<_>>());
    let flat = raws
        .into_iter()
        .map(|r| {
            let level = entry_level(&r.title, r.indent, &bands);
            (r.title, Some(r.dest), level)
        })
        .collect();
    build_toc_tree(flat)
}

/// Recover a printed table of contents from its text: parse dot-leader lines on
/// detected TOC pages into `(title, printed page number)`, then map printed
/// numbers to physical indices via a learned offset ([`learn_page_delta`]).
/// Pure and windowless so it can be unit-tested without pdfium.
pub fn parse_printed_toc(pages: &[PdfPageText]) -> Vec<TocEntry> {
    let toc_pages = detect_toc_pages(pages);
    let Some(&last_toc) = toc_pages.iter().max() else {
        return Vec::new();
    };
    let mut raws: Vec<TocRow> = Vec::new();
    for &pi in &toc_pages {
        raws.extend(extract_toc_entries(&pages[pi]));
    }
    raws.retain(|r| is_plausible_title(&r.title));
    if raws.len() < 2 {
        return Vec::new();
    }
    // Map printed page numbers to physical indices. Prefer an offset learned by
    // matching titles to the body headings they point at; otherwise assume the
    // content begins on the page right after the contents (a sound default for
    // documents whose body headings don't match their TOC text verbatim).
    let entries: Vec<(&str, u32)> = raws.iter().map(|r| (r.title.as_str(), r.page)).collect();
    let min_printed = raws.iter().map(|r| r.page).min().unwrap_or(1);
    let delta = learn_page_delta(pages, last_toc, &entries)
        .unwrap_or_else(|| last_toc as i64 + 1 - min_printed as i64);

    let bands = indent_bands(&raws.iter().map(|r| r.indent).collect::<Vec<_>>());
    let total_pages = pages.len() as i64;
    let flat: Vec<(String, Option<u32>, usize)> = raws
        .into_iter()
        .map(|r| {
            let phys = (r.page as i64 + delta).clamp(0, total_pages - 1) as u32;
            let level = entry_level(&r.title, r.indent, &bands);
            (r.title, Some(phys), level)
        })
        .collect();
    // A genuine table of contents references at least a few distinct places in
    // the document. When the recovered targets collapse onto one or two pages,
    // the "TOC" is really a bibliography/index whose numbers are years or
    // citation indices (not pages), or a misdetected body page — discard it.
    let distinct: std::collections::HashSet<Option<u32>> =
        flat.iter().map(|(_, p, _)| *p).collect();
    if distinct.len() < 3 {
        return Vec::new();
    }
    build_toc_tree(flat)
}

/// Concatenate, in reading order, the text of every character whose center
/// falls inside the given rect (PDF points, bottom-left origin). Used to read
/// the title a TOC link annotation sits on top of.
pub(super) fn text_in_rect(
    page: &PdfPageText,
    left: f32,
    bottom: f32,
    right: f32,
    top: f32,
) -> String {
    let mut s = String::new();
    for c in &page.chars {
        let cx = c.bbox.x + c.bbox.width * 0.5;
        let cy = c.bbox.y + c.bbox.height * 0.5;
        if cx >= left && cx <= right && cy >= bottom && cy <= top {
            s.push(c.ch);
        }
    }
    s.trim().to_string()
}

/// Synthesize a table of contents from page text when a PDF has no embedded
/// bookmark tree.
///
/// A line is treated as a heading when any signal fires: it is meaningfully
/// taller than the document's body text, it begins with a section number
/// (`1.2`, `Chapter 4`), or it is a short ALL-CAPS line. Numbered headings nest
/// by their numbering depth; the rest nest by relative font size. Running
/// headers/footers (identical text repeated on many pages) are dropped.
///
/// When no headings are found at all, a multi-page document falls back to a
/// per-page outline (each page labeled by its most prominent line) so the
/// reader always gets something faster than scrolling. The result flows through
/// the same downstream pipeline as embedded bookmarks.
///
/// Pure and windowless so it can be unit-tested without pdfium.
pub fn synthesize_toc(pages: &[PdfPageText]) -> Vec<TocEntry> {
    const HEADING_RATIO: f32 = 1.15;
    const MAX_HEADING_WORDS: usize = 12;
    const HEADER_REPEAT_FRACTION: f32 = 0.25;
    const MAX_LEVELS: usize = 4;
    const MAX_ENTRIES: usize = 500;
    const MAX_LABEL_CHARS: usize = 80;

    if pages.is_empty() {
        return Vec::new();
    }

    // Collect every line's height to find the document body size.
    let mut heights: Vec<f32> = Vec::new();
    for page in pages {
        for line in &page.lines {
            if line.bbox.height > 0.0 {
                heights.push(line.bbox.height);
            }
        }
    }
    if heights.is_empty() {
        return Vec::new();
    }

    // Body size = median line height (robust against a few large headings).
    let body_size = {
        let mut sorted = heights.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        sorted[sorted.len() / 2]
    };
    let heading_threshold = body_size * HEADING_RATIO;

    // Count identical text occurrences across pages to spot running
    // headers/footers, which should never become TOC entries.
    let mut text_page_counts: std::collections::HashMap<String, std::collections::HashSet<u16>> =
        std::collections::HashMap::new();
    for page in pages {
        for line in &page.lines {
            let t = line_text(page, line);
            if !t.is_empty() {
                text_page_counts
                    .entry(t)
                    .or_default()
                    .insert(page.page_index);
            }
        }
    }
    let repeat_page_threshold = ((pages.len() as f32) * HEADER_REPEAT_FRACTION)
        .ceil()
        .max(2.0) as usize;
    let is_running_header = |title: &str| -> bool {
        text_page_counts
            .get(title)
            .map(|set| set.len() >= repeat_page_threshold)
            .unwrap_or(false)
    };

    // Gather candidate headings. `level` is `Some` when known from numbering,
    // otherwise resolved from font size after the sweep.
    struct Candidate {
        title: String,
        page_index: u32,
        size: f32,
        level: Option<usize>,
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    'outer: for page in pages {
        for line in &page.lines {
            let title = line_text(page, line);
            if title.is_empty() || title.split_whitespace().count() > MAX_HEADING_WORDS {
                continue;
            }
            if is_running_header(&title) || !is_plausible_title(&title) {
                continue;
            }
            let size = line.bbox.height;
            let num_depth = numbered_heading_depth(&title);
            let by_size = size >= heading_threshold;
            let by_caps = is_caps_heading(&title) && size >= body_size * 0.95;
            // A section number alone isn't enough: enumerated body lists ("1.
            // Zero-order approximation…", "2. The remainder…") share the form of
            // a numbered heading. Accept numbering only for a short, non-prose
            // line (real headings don't run to a sentence ending in a period).
            let by_number = num_depth.is_some()
                && title.split_whitespace().count() <= 8
                && !title.trim_end().ends_with('.');
            if !by_size && !by_caps && !by_number {
                continue;
            }
            candidates.push(Candidate {
                title,
                page_index: page.page_index as u32,
                size,
                level: num_depth.map(|d| (d - 1).min(MAX_LEVELS - 1)),
            });
            if candidates.len() >= MAX_ENTRIES {
                break 'outer;
            }
        }
    }

    // A genuine document outline is spread across the document. When many
    // "headings" instead cluster on just a page or two, they are really an
    // enumerated list — a reference section, index, or glossary — or scan/math
    // noise, not a table of contents. Fabricating an outline from those is worse
    // than offering none, so bail rather than emit a page-long bogus TOC.
    let distinct_pages: std::collections::HashSet<u32> =
        candidates.iter().map(|c| c.page_index).collect();
    if candidates.len() >= 6 && distinct_pages.len() <= 2 {
        return Vec::new();
    }
    // Conversely, a real outline is sparse — at most a heading or two per page.
    // When candidates are far denser than that, the heuristic is misfiring on an
    // equation-, theorem-, or caption-heavy document (typical of papers with no
    // real contents page); an empty outline beats a hundreds-of-entries dump.
    if candidates.len() > 2 * pages.len() + 4 {
        return Vec::new();
    }

    if candidates.is_empty() {
        // Fallback: a per-page outline, but only when there are several pages to
        // navigate (a one-entry TOC for a single page is pointless).
        if pages.len() < 2 {
            return Vec::new();
        }
        let mut out: Vec<TocEntry> = Vec::new();
        for page in pages {
            let label = page
                .lines
                .iter()
                .map(|line| (line.bbox.height, line_text(page, line)))
                .filter(|(_, t)| !t.is_empty() && !is_running_header(t) && is_plausible_title(t))
                .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(_, t)| t)
                .unwrap_or_else(|| format!("Page {}", page.page_index + 1));
            let label: String = label.chars().take(MAX_LABEL_CHARS).collect();
            out.push(TocEntry {
                title: label,
                page_index: Some(page.page_index as u32),
                children: Vec::new(),
            });
            if out.len() >= MAX_ENTRIES {
                break;
            }
        }
        return out;
    }

    // Cluster distinct heading sizes (descending) into <= MAX_LEVELS levels.
    let mut distinct_sizes: Vec<f32> = candidates.iter().map(|c| c.size).collect();
    distinct_sizes.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    distinct_sizes.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    distinct_sizes.truncate(MAX_LEVELS);

    // Map a heading size to a level (0 = largest/top). Larger or equal to the
    // n-th distinct size means level n.
    let level_for = |size: f32| -> usize {
        for (i, s) in distinct_sizes.iter().enumerate() {
            if size >= *s - 0.5 {
                return i;
            }
        }
        distinct_sizes.len().saturating_sub(1)
    };

    // Numbered headings nest by their numbering depth; everything else by
    // relative font size. Build the tree with the shared stack-based assembler.
    let flat = candidates
        .into_iter()
        .map(|c| {
            let level = c.level.unwrap_or_else(|| level_for(c.size));
            (c.title, Some(c.page_index), level)
        })
        .collect();
    build_toc_tree(flat)
}

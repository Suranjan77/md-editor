//! Page text extraction and table-of-contents recovery from bookmarks, link annotations, and printed contents pages.

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
        let Ok(page) = pages_handle.get(i) else {
            continue;
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
pub(super) fn build_page_text(page: &PdfPage, index: u16) -> PdfResult<PdfPageText> {
    let text_page = page.text().map_err(|e| PdfError::Other(e.to_string()))?;

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
                    let prev = std::mem::replace(
                        line,
                        PdfTextLine {
                            start_text_index: c.text_index,
                            end_text_index: c.text_index + 1,
                            bbox: c.bbox.clone(),
                        },
                    );
                    lines.push(prev);
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
    let split = t.len() - digit_count;
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
    let mut path: Vec<(usize, Vec<usize>)> = Vec::new();
    for (title, page_index, level) in items {
        let entry = TocEntry {
            title,
            page_index,
            children: Vec::new(),
        };
        while path.last().is_some_and(|(l, _)| *l >= level) {
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
pub(super) fn extract_toc_entries(page: &PdfPageText) -> Vec<TocRow> {
    struct Line {
        text: String,
        x: f32,
        y: f32,
        h: f32,
        num: Option<u32>,
    }
    let right_min = page.page_width * 0.5;
    let mut lines: Vec<Line> = Vec::new();
    let mut num_col: Vec<(f32, u32, f32)> = Vec::new();
    for line in &page.lines {
        let t = line_text(page, line);
        if t.is_empty() || is_contents_heading(&t) {
            continue;
        }
        if let Ok(v) = t.parse::<u32>() {
            if t.len() <= 4 && line.bbox.x > right_min {
                num_col.push((line.bbox.y, v, line.bbox.height));
            }
            continue;
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
                let standalone =
                    numbered_heading_depth(&l.text).is_some() || is_caps_heading(l.text.trim());
                if standalone {
                    let page = pending.as_ref().map_or(0, |p| p.page);
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

/// Indices of leading pages that look like a printed table of contents.
pub(super) fn detect_toc_pages(pages: &[PdfPageText]) -> Vec<usize> {
    let scan = pages.len().min(MAX_TOC_SCAN);
    let mut candidates: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut heading_candidate: Option<usize> = None;
    for (i, page) in pages.iter().enumerate().take(scan) {
        let has_heading = page
            .lines
            .iter()
            .any(|l| is_contents_heading(&line_text(page, l)));
        let real = extract_toc_entries(page).iter().filter(|e| e.real).count();
        if (has_heading && real >= 2) || real >= 5 {
            candidates.insert(i);
            if has_heading && heading_candidate.is_none() {
                heading_candidate = Some(i);
            }
        }
    }
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
/// TOC titles to headings on the body pages they point at. Requires >= 2 votes.
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
                break;
            }
        }
    }
    votes
        .into_iter()
        .filter(|(_, c)| *c >= 2)
        .max_by_key(|(_, c)| *c)
        .map(|(d, _)| d)
}

/// Recover a printed table of contents from its link annotations.
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
        page_rank: usize,
        top: f32,
    }
    let mut raws: Vec<Raw> = Vec::new();
    let mut seen: std::collections::HashSet<(String, u32)> = std::collections::HashSet::new();
    for (rank, &pi) in toc_pages.iter().enumerate() {
        let Ok(page) = pages.get(pi as i32) else {
            continue;
        };
        let ptext = page_texts.iter().find(|p| p.page_index as usize == pi);
        for link in page.links().iter() {
            let Ok(rect) = link.rect() else {
                continue;
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
                continue;
            }
            let Some(pt) = ptext else { continue };
            let title = clean_link_title(&text_in_rect(
                pt,
                rect.left().value,
                rect.bottom().value,
                rect.right().value,
                rect.top().value,
            ));
            if !is_plausible_title(&title) || !seen.insert((title.clone(), dest)) {
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

/// Recover a printed table of contents from its text. Pure and windowless.
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
    let distinct: std::collections::HashSet<Option<u32>> =
        flat.iter().map(|(_, p, _)| *p).collect();
    if distinct.len() < 3 {
        return Vec::new();
    }
    build_toc_tree(flat)
}

/// Concatenate, in reading order, the text of every character whose center
/// falls inside the given rect (PDF points, bottom-left origin).
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

#[cfg(test)]
mod tests;

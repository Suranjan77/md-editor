//! Typographic table-of-contents synthesis and heading classification heuristics.

use super::toc::{build_toc_tree, line_text};
use super::{PdfPageText, TocEntry};
use std::collections::{HashMap, HashSet};

const HEADING_RATIO: f32 = 1.15;
const MAX_HEADING_WORDS: usize = 12;
const HEADER_REPEAT_FRACTION: f32 = 0.25;
const MAX_LEVELS: usize = 4;
const MAX_ENTRIES: usize = 500;
const MAX_LABEL_CHARS: usize = 80;

/// Nesting depth of a numbered-heading prefix, or `None` when the line does not
/// begin with a recognizable section number.
///
/// Recognizes dotted numeric prefixes (`1.`, `1.2`, `1.2.3`) and keyworded
/// headings (`Chapter 4`, `Section 2`, `Part III`, `Appendix B`). A bare number
/// with no dot (e.g. `2020 was a year`) is intentionally rejected unless it is
/// a short section index followed by a capitalized word (`1 Introduction`).
pub(crate) fn numbered_heading_depth(title: &str) -> Option<usize> {
    let t = title.trim_start();

    let lower = t.to_lowercase();
    for kw in ["chapter ", "section ", "part ", "appendix "] {
        if lower.starts_with(kw) {
            let rest = t[kw.len()..].trim_start();
            if rest.chars().next().is_some_and(|c| c.is_alphanumeric()) {
                return Some(1);
            }
        }
    }

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
        depth += 1;
    }
    if depth == 0 {
        return None;
    }
    let after = &t[consumed..];
    let has_text =
        matches!(after.chars().next(), Some(c) if c.is_whitespace()) && !after.trim().is_empty();
    if !has_text {
        return None;
    }
    if t[..consumed].contains('.') {
        return Some(depth);
    }
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
/// Private Use Area code points (font-specific math brackets, ligatures, icons).
pub(crate) fn is_plausible_title(title: &str) -> bool {
    if title
        .chars()
        .any(|c| c.is_control() || ('\u{E000}'..='\u{F8FF}').contains(&c))
    {
        return false;
    }
    let letters = title.chars().filter(|c| c.is_alphabetic()).count();
    let non_space = title.chars().filter(|c| !c.is_whitespace()).count();
    letters >= 3 && letters * 2 >= non_space
}

/// True for a short line set entirely in uppercase letters (e.g. `INTRODUCTION`),
/// a common heading style that carries no extra glyph height.
pub(crate) fn is_caps_heading(title: &str) -> bool {
    let letters = title.chars().filter(|c| c.is_alphabetic()).count();
    letters >= 3
        && title
            .chars()
            .filter(|c| c.is_alphabetic())
            .all(|c| c.is_uppercase())
}

struct HeadingCandidate {
    title: String,
    page_index: u32,
    size: f32,
    level: Option<usize>,
}

/// Synthesize a table of contents from page text when a PDF has no embedded
/// bookmark tree or printed contents page. Pure and windowless.
pub fn synthesize_toc(pages: &[PdfPageText]) -> Vec<TocEntry> {
    let Some(body_size) = median_line_height(pages) else {
        return Vec::new();
    };
    let running_headers = running_header_set(pages);
    let candidates = collect_heading_candidates(pages, body_size, &running_headers);

    let distinct_pages: HashSet<u32> = candidates.iter().map(|c| c.page_index).collect();
    if candidates.len() >= 6 && distinct_pages.len() <= 2 {
        return Vec::new();
    }
    if candidates.len() > 2 * pages.len() + 4 {
        return Vec::new();
    }

    if candidates.is_empty() {
        return per_page_fallback_toc(pages, &running_headers);
    }

    let distinct_sizes = cluster_heading_sizes(&candidates);
    let flat = candidates
        .into_iter()
        .map(|c| {
            let level = c
                .level
                .unwrap_or_else(|| level_for_size(&distinct_sizes, c.size));
            (c.title, Some(c.page_index), level)
        })
        .collect();
    build_toc_tree(flat)
}

fn median_line_height(pages: &[PdfPageText]) -> Option<f32> {
    let mut heights: Vec<f32> = pages
        .iter()
        .flat_map(|page| page.lines.iter())
        .map(|line| line.bbox.height)
        .filter(|h| *h > 0.0)
        .collect();
    if heights.is_empty() {
        return None;
    }
    heights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(heights[heights.len() / 2])
}

fn running_header_set(pages: &[PdfPageText]) -> HashSet<String> {
    let mut counts: HashMap<String, HashSet<u16>> = HashMap::new();
    for page in pages {
        for line in &page.lines {
            let t = line_text(page, line);
            if !t.is_empty() {
                counts.entry(t).or_default().insert(page.page_index);
            }
        }
    }
    let repeat_threshold = ((pages.len() as f32) * HEADER_REPEAT_FRACTION)
        .ceil()
        .max(2.0) as usize;
    counts
        .into_iter()
        .filter_map(|(title, set)| (set.len() >= repeat_threshold).then_some(title))
        .collect()
}

fn collect_heading_candidates(
    pages: &[PdfPageText],
    body_size: f32,
    running_headers: &HashSet<String>,
) -> Vec<HeadingCandidate> {
    let heading_threshold = body_size * HEADING_RATIO;
    let mut candidates = Vec::new();
    for page in pages {
        for line in &page.lines {
            let title = line_text(page, line);
            if title.is_empty() || title.split_whitespace().count() > MAX_HEADING_WORDS {
                continue;
            }
            if running_headers.contains(&title) || !is_plausible_title(&title) {
                continue;
            }
            let size = line.bbox.height;
            let num_depth = numbered_heading_depth(&title);
            let by_size = size >= heading_threshold;
            let by_caps = is_caps_heading(&title) && size >= body_size * 0.95;
            let by_number = num_depth.is_some()
                && title.split_whitespace().count() <= 8
                && !title.trim_end().ends_with('.');
            if !by_size && !by_caps && !by_number {
                continue;
            }
            candidates.push(HeadingCandidate {
                title,
                page_index: page.page_index as u32,
                size,
                level: num_depth.map(|d| (d - 1).min(MAX_LEVELS - 1)),
            });
            if candidates.len() >= MAX_ENTRIES {
                return candidates;
            }
        }
    }
    candidates
}

fn per_page_fallback_toc(
    pages: &[PdfPageText],
    running_headers: &HashSet<String>,
) -> Vec<TocEntry> {
    if pages.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for page in pages.iter().take(MAX_ENTRIES) {
        let label = page
            .lines
            .iter()
            .map(|line| (line.bbox.height, line_text(page, line)))
            .filter(|(_, t)| !t.is_empty() && !running_headers.contains(t) && is_plausible_title(t))
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, t)| t)
            .unwrap_or_else(|| format!("Page {}", page.page_index + 1));
        let label: String = label.chars().take(MAX_LABEL_CHARS).collect();
        out.push(TocEntry {
            title: label,
            page_index: Some(page.page_index as u32),
            children: Vec::new(),
        });
    }
    out
}

fn cluster_heading_sizes(candidates: &[HeadingCandidate]) -> Vec<f32> {
    let mut sizes: Vec<f32> = candidates.iter().map(|c| c.size).collect();
    sizes.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    sizes.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    sizes.truncate(MAX_LEVELS);
    sizes
}

fn level_for_size(distinct_sizes: &[f32], size: f32) -> usize {
    for (i, s) in distinct_sizes.iter().enumerate() {
        if size >= *s - 0.5 {
            return i;
        }
    }
    distinct_sizes.len().saturating_sub(1)
}

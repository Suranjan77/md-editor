//! Text selection over a page's character geometry (plan §3.3 "text
//! selection"): pure math from char boxes + two drag points to the selected
//! text and its per-line highlight quads. No pdfium here — the impure half
//! ([`crate::render`]) supplies [`CharBox`]es; this module is what makes the
//! selection semantics testable with synthetic glyph grids.
//!
//! Coordinate space: page points, origin **top-left** (the annotation
//! store's convention) — the renderer flips pdfium's bottom-left rects
//! before they get here.

/// One character's glyph box on a page, in reading order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharBox {
    pub ch: char,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// One line's slice of a selection — the rectangle to tint, page points,
/// top-left origin. Mirrors the shape of an annotation quad.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelRect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// A resolved selection: what to paint and what was selected.
#[derive(Debug, Clone, PartialEq)]
pub struct TextSelection {
    /// One rect per visual line touched, in line order.
    pub quads: Vec<SelRect>,
    /// Selected characters, `\n` between lines.
    pub text: String,
}

/// A maximal run of consecutive chars forming one visual line.
struct Line {
    start: usize,
    end: usize, // exclusive
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
}

/// True when the char belongs to the line's band: its vertical center lies
/// inside the band, or the band's center inside the char. Any-overlap is
/// not enough — pdfium *loose* boxes span full line height, so tightly-
/// leaded lines overlap by a point or two and an any-overlap rule chains
/// the whole page into one band (selection painted on line 1 wherever the
/// drag was). Overlap *ratios* are not enough either — pdfium synthesizes
/// space chars whose boxes can be degenerate (zero height), and a ratio
/// rule splits the visual line at every such space, after which same-line
/// drags resolve to a single caret (nothing draws). Center containment
/// accepts both degenerate boxes and font-size outliers while still
/// rejecting the next line down.
fn joins_line(line: &Line, c: &CharBox) -> bool {
    let c_mid = (c.y0 + c.y1) / 2.0;
    let line_mid = (line.y0 + line.y1) / 2.0;
    (c_mid >= line.y0 && c_mid <= line.y1) || (line_mid >= c.y0 && line_mid <= c.y1)
}

/// Group reading-ordered chars into visual lines: a char extends the current
/// line while it shares the line's vertical band. Column breaks reset the
/// band (the next column's first line sits back at the top), so multi-column
/// order is preserved, not merged.
fn lines(chars: &[CharBox]) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    for (i, c) in chars.iter().enumerate() {
        match out.last_mut() {
            Some(line) if joins_line(line, c) => {
                line.end = i + 1;
                line.x0 = line.x0.min(c.x0);
                line.x1 = line.x1.max(c.x1);
                line.y0 = line.y0.min(c.y0);
                line.y1 = line.y1.max(c.y1);
            }
            _ => out.push(Line {
                start: i,
                end: i + 1,
                x0: c.x0,
                x1: c.x1,
                y0: c.y0,
                y1: c.y1,
            }),
        }
    }
    out
}

/// Caret position (0..=chars.len()) for a page point: above all text means
/// the document start, below means the end, otherwise the nearest line and
/// the gap nearest `x` within it.
fn position(chars: &[CharBox], lines: &[Line], p: (f32, f32)) -> usize {
    let Some(first) = lines.first() else {
        return 0;
    };
    let Some(last) = lines.last() else {
        return 0;
    };
    if p.1 < first.y0 {
        return 0;
    }
    if p.1 > last.y1 {
        return chars.len();
    }
    // Nearest band vertically; horizontal distance breaks ties so that on
    // rows holding several bands (two-column layouts, or lines split by
    // odd glyph boxes) the band *under the cursor* wins, not the first one
    // in reading order.
    let mut best = first;
    let mut best_key = (f32::MAX, f32::MAX);
    for line in lines {
        let dy = if p.1 >= line.y0 && p.1 <= line.y1 {
            0.0
        } else {
            (p.1 - line.y0).abs().min((p.1 - line.y1).abs())
        };
        let dx = if p.0 >= line.x0 && p.0 <= line.x1 {
            0.0
        } else {
            (p.0 - line.x0).abs().min((p.0 - line.x1).abs())
        };
        if (dy, dx) < best_key {
            best_key = (dy, dx);
            best = line;
        }
    }
    let mut pos = best.start;
    for c in &chars[best.start..best.end] {
        if p.0 >= (c.x0 + c.x1) / 2.0 {
            pos += 1;
        } else {
            break;
        }
    }
    pos
}

/// Resolve a drag between two page points into a selection. `None` when the
/// page has no text or the points straddle no characters. Argument order
/// does not matter (backward drags select the same range).
pub fn select(chars: &[CharBox], a: (f32, f32), b: (f32, f32)) -> Option<TextSelection> {
    if chars.is_empty() {
        return None;
    }
    let lines = lines(chars);
    let (lo, hi) = {
        let (pa, pb) = (position(chars, &lines, a), position(chars, &lines, b));
        (pa.min(pb), pa.max(pb))
    };
    selection_for(chars, &lines, lo, hi)
}

/// The selection a drag over `range` would produce: per-line union quads and
/// the covered text. `None` for empty or out-of-bounds ranges.
pub fn range_selection(chars: &[CharBox], range: std::ops::Range<usize>) -> Option<TextSelection> {
    if range.end > chars.len() {
        return None;
    }
    selection_for(chars, &lines(chars), range.start, range.end)
}

/// Case-insensitive occurrences of `needle` in the page's char stream, as
/// char-index ranges (feed them to [`range_selection`] for quads). Folding
/// is per-scalar (`char::to_lowercase`, first scalar), which covers the
/// practical alphabets; locale-grade folding is not this module's business.
pub fn find(chars: &[CharBox], needle: &str) -> Vec<std::ops::Range<usize>> {
    let fold = |c: char| c.to_lowercase().next().unwrap_or(c);
    let needle: Vec<char> = needle.chars().map(fold).collect();
    if needle.is_empty() || needle.len() > chars.len() {
        return Vec::new();
    }
    let haystack: Vec<char> = chars.iter().map(|c| fold(c.ch)).collect();
    let mut out = Vec::new();
    for start in 0..=(haystack.len() - needle.len()) {
        if haystack[start..start + needle.len()] == needle[..] {
            out.push(start..start + needle.len());
        }
    }
    out
}

fn selection_for(chars: &[CharBox], lines: &[Line], lo: usize, hi: usize) -> Option<TextSelection> {
    if lo >= hi {
        return None;
    }
    let mut quads = Vec::new();
    let mut text = String::new();
    for line in lines {
        let start = line.start.max(lo);
        let end = line.end.min(hi);
        if start >= end {
            continue;
        }
        let slice = &chars[start..end];
        let mut quad = SelRect {
            x0: f32::MAX,
            y0: f32::MAX,
            x1: f32::MIN,
            y1: f32::MIN,
        };
        for c in slice {
            quad.x0 = quad.x0.min(c.x0);
            quad.y0 = quad.y0.min(c.y0);
            quad.x1 = quad.x1.max(c.x1);
            quad.y1 = quad.y1.max(c.y1);
        }
        quads.push(quad);
        if !text.is_empty() {
            text.push('\n');
        }
        text.extend(slice.iter().map(|c| c.ch));
    }
    Some(TextSelection { quads, text })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic line of 10×12 glyphs starting at (x, top).
    fn line_of(text: &str, x: f32, top: f32) -> Vec<CharBox> {
        text.chars()
            .enumerate()
            .map(|(i, ch)| CharBox {
                ch,
                x0: x + i as f32 * 10.0,
                y0: top,
                x1: x + (i + 1) as f32 * 10.0,
                y1: top + 12.0,
            })
            .collect()
    }

    /// Two lines: "hello world" at y 0, "second line" at y 20.
    fn two_lines() -> Vec<CharBox> {
        let mut chars = line_of("hello world", 50.0, 0.0);
        chars.extend(line_of("second line", 50.0, 20.0));
        chars
    }

    fn sel(chars: &[CharBox], a: (f32, f32), b: (f32, f32)) -> TextSelection {
        match select(chars, a, b) {
            Some(s) => s,
            None => panic!("expected a selection between {a:?} and {b:?}"),
        }
    }

    #[test]
    fn single_line_drag_selects_the_straddled_chars() {
        let chars = line_of("hello", 0.0, 0.0);
        // Left half of 'e' (x 12) to right half of 'l' #2 (x 38): "ell".
        let s = sel(&chars, (12.0, 6.0), (38.0, 6.0));
        assert_eq!(s.text, "ell");
        assert_eq!(s.quads.len(), 1);
        assert_eq!(
            s.quads[0],
            SelRect {
                x0: 10.0,
                y0: 0.0,
                x1: 40.0,
                y1: 12.0
            }
        );
    }

    #[test]
    fn backward_drag_selects_the_same_range() {
        let chars = line_of("hello", 0.0, 0.0);
        assert_eq!(
            select(&chars, (38.0, 6.0), (12.0, 6.0)),
            select(&chars, (12.0, 6.0), (38.0, 6.0))
        );
    }

    #[test]
    fn multi_line_selection_yields_one_quad_per_line_and_newlines() {
        let chars = two_lines();
        // From mid-'w' on line 1 (x 115) down to mid-'c' on line 2 (x 75).
        let s = sel(&chars, (111.0, 6.0), (79.0, 26.0));
        assert_eq!(s.text, "world\nsec");
        assert_eq!(s.quads.len(), 2);
        assert_eq!(s.quads[0].y0, 0.0);
        assert_eq!(s.quads[1].y0, 20.0);
        assert!(s.quads[0].x0 > s.quads[1].x0, "line 1 starts at 'w'");
    }

    #[test]
    fn drag_into_the_margin_clamps_to_doc_ends() {
        let chars = two_lines();
        // Above everything to below everything: the whole text.
        let s = sel(&chars, (0.0, -100.0), (500.0, 100.0));
        assert_eq!(s.text, "hello world\nsecond line");
        // A drag ending below the last line reaches the end even at x 0.
        let s = sel(&chars, (79.0, 22.0), (0.0, 90.0));
        assert_eq!(s.text, "ond line");
    }

    #[test]
    fn empty_page_and_zero_width_drags_select_nothing() {
        assert_eq!(select(&[], (0.0, 0.0), (10.0, 10.0)), None);
        let chars = two_lines();
        // Same caret position on both ends (gap between 'h' and 'e').
        assert_eq!(select(&chars, (61.0, 6.0), (62.0, 6.0)), None);
    }

    #[test]
    fn between_lines_snaps_to_the_nearest_band() {
        let chars = two_lines();
        // y 13.0 is just under line 1 (band 0–12): snaps to line 1, so a
        // drag from there to line 1's start selects within line 1 only.
        let s = sel(&chars, (50.0, 6.0), (161.0, 13.0));
        assert_eq!(s.text, "hello world");
        assert_eq!(s.quads.len(), 1);
    }

    #[test]
    fn find_locates_case_insensitive_matches_with_quads() {
        let chars = two_lines(); // "hello world" then "second line" at y 20
        let hits = find(&chars, "SECOND");
        assert_eq!(hits, vec![11..17]);
        let s = match range_selection(&chars, hits[0].clone()) {
            Some(s) => s,
            None => panic!("match range yields no selection"),
        };
        assert_eq!(s.text, "second");
        assert_eq!(s.quads.len(), 1);
        assert_eq!(s.quads[0].y0, 20.0, "quad sits on the second line");
    }

    #[test]
    fn find_returns_every_occurrence_and_nothing_for_absent_text() {
        let chars = line_of("abcabcab", 0.0, 0.0);
        assert_eq!(find(&chars, "ab"), vec![0..2, 3..5, 6..8]);
        assert!(find(&chars, "zz").is_empty());
        assert!(find(&chars, "").is_empty());
        assert!(find(&[], "a").is_empty());
        assert!(range_selection(&chars, 5..99).is_none(), "oob is None");
    }

    #[test]
    fn tightly_leaded_lines_do_not_merge_into_one_band() {
        // Loose boxes 14pt tall on a 12pt line step: consecutive lines
        // overlap by 2pt, the shape real pdfium loose bounds take in
        // single-spaced text. Selection on line 3 must land on line 3,
        // not resolve into line 1 through a page-wide merged band.
        let mut chars: Vec<CharBox> = Vec::new();
        for (i, line) in ["first line", "second line", "third line"]
            .iter()
            .enumerate()
        {
            chars.extend(line.chars().enumerate().map(|(j, ch)| CharBox {
                ch,
                x0: j as f32 * 10.0,
                y0: i as f32 * 12.0,
                x1: (j + 1) as f32 * 10.0,
                y1: i as f32 * 12.0 + 14.0,
            }));
        }
        assert_eq!(lines(&chars).len(), 3, "three bands despite 2pt overlap");
        // Drag across "third" on line 3 (band y 24–38, mid y 31).
        let s = sel(&chars, (1.0, 31.0), (49.0, 31.0));
        assert_eq!(s.text, "third");
        assert_eq!(s.quads.len(), 1);
        assert_eq!(s.quads[0].y0, 24.0, "quad sits on line 3, not line 1");
    }

    #[test]
    fn degenerate_space_boxes_do_not_split_the_line() {
        // pdfium-synthesized spaces can carry zero-height boxes. The line
        // must stay one band, and a same-line drag *past* the space must
        // still select (a split band used to resolve every caret into the
        // first segment — same-line drags drew nothing).
        let mut chars = line_of("ab", 0.0, 0.0);
        chars.push(CharBox {
            ch: ' ',
            x0: 20.0,
            y0: 6.0,
            x1: 30.0,
            y1: 6.0, // zero height
        });
        chars.extend(line_of("cd", 30.0, 0.0));
        assert_eq!(lines(&chars).len(), 1, "one visual line");
        // Drag from inside 'a' to inside 'd' (x 2 → x 48), all at y 6.
        let s = sel(&chars, (2.0, 6.0), (48.0, 6.0));
        assert_eq!(s.text, "ab cd");
        assert_eq!(s.quads.len(), 1);
    }

    #[test]
    fn same_row_columns_resolve_carets_by_x() {
        // Two columns sharing one visual row merge into one band (adjacent
        // in reading order, same vertical extent) — caret resolution must
        // still land by x, so a drag inside column B selects column B text.
        let mut chars = line_of("left", 0.0, 0.0);
        chars.extend(line_of("right", 300.0, 0.0));
        let s = sel(&chars, (301.0, 6.0), (349.0, 6.0));
        assert_eq!(s.text, "right");
        assert_eq!(s.quads.len(), 1);
        assert!(s.quads[0].x0 >= 300.0, "quad sits in column B");
    }

    #[test]
    fn column_break_does_not_merge_bands() {
        // Column 2 restarts at the top: same y as column 1's first line.
        let mut chars = line_of("colA", 0.0, 0.0);
        chars.extend(line_of("colA2", 0.0, 20.0));
        chars.extend(line_of("colB", 300.0, 0.0));
        let l = lines(&chars);
        assert_eq!(l.len(), 3, "column restart is a new line, not a merge");
        assert_eq!(l[2].start, 9);
    }
}

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
    y0: f32,
    y1: f32,
}

/// Group reading-ordered chars into visual lines: a char extends the current
/// line while its vertical extent overlaps the line's band. Column breaks
/// reset the band (the next column's first line sits back at the top, no
/// overlap), so multi-column order is preserved, not merged.
fn lines(chars: &[CharBox]) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    for (i, c) in chars.iter().enumerate() {
        match out.last_mut() {
            Some(line) if c.y0 < line.y1 && c.y1 > line.y0 => {
                line.end = i + 1;
                line.y0 = line.y0.min(c.y0);
                line.y1 = line.y1.max(c.y1);
            }
            _ => out.push(Line {
                start: i,
                end: i + 1,
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
    let mut best = first;
    let mut best_d = f32::MAX;
    for line in lines {
        let d = if p.1 >= line.y0 && p.1 <= line.y1 {
            0.0
        } else {
            (p.1 - line.y0).abs().min((p.1 - line.y1).abs())
        };
        if d < best_d {
            best_d = d;
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
    if lo == hi {
        return None;
    }

    let mut quads = Vec::new();
    let mut text = String::new();
    for line in &lines {
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

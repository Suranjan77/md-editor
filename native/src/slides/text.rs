//! Line breaking and placement for slide text.
//!
//! Lays out a [`TextBody`] closely enough to PowerPoint that a slide reads as
//! the same slide: paragraphs with their spacing, hanging bullets and
//! automatic numbers, greedy word wrapping at the box's width, alignment,
//! runs of different sizes sharing a baseline, superscripts, highlights, and
//! underline and strike rules. Justification, tab stops, and East Asian line
//! breaking rules are not modelled; justified text is set flush left.
//!
//! Positions are points relative to the text box's inner top-left corner,
//! inside its insets. The vertical anchor is applied when drawing
//! ([`anchor_offset`]), because only then is the height of the box known —
//! a table cell is as tall as the tallest cell in its row.

use iced::Font;
use md_editor_core::pptx::{Align, Anchor, Paragraph, Rgba, Spacing, Table, TextBody};

/// Single line spacing, as a multiple of the largest font size on the line.
const SINGLE_SPACING: f32 = 1.2;
/// Size of superscript and subscript text relative to its run.
const SCRIPT_SCALE: f32 = 2.0 / 3.0;
/// Gap between a bullet and its text when the bullet does not hang, as a
/// multiple of the bullet's size.
const BULLET_GAP: f32 = 0.3;
/// Slack when deciding whether a word fits, so rounding in the shaper's
/// widths never pushes a word that fits exactly onto the next line.
const FIT_EPSILON: f32 = 0.01;

/// What layout needs from the font system. The app answers with the real
/// shaper; tests answer with fixed-width stand-ins.
pub trait TextMetrics {
    /// The font a run named `typeface` is set in.
    fn font(&self, typeface: &str, bold: bool, italic: bool) -> Font;
    /// Advance width of `text` at `size` points, trailing spaces included.
    fn width(&self, text: &str, font: Font, size: f32) -> f32;
    /// Ascent and descent per unit of font size.
    fn vertical(&self, font: Font) -> (f32, f32);
}

/// A run of text with one style, placed on a line.
#[derive(Debug, Clone)]
pub struct PlacedRun {
    pub text: String,
    pub x: f32,
    /// Top of an iced text box one font size tall, which centres the glyph
    /// extents vertically; chosen so the glyphs land on the line's baseline.
    pub y: f32,
    pub size: f32,
    pub font: Font,
    pub color: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: Rgba,
}

#[derive(Debug, Clone, Default)]
pub struct LaidText {
    /// Bullets and text, in drawing order.
    pub runs: Vec<PlacedRun>,
    /// Underlines and strike-throughs.
    pub rules: Vec<TextRect>,
    /// Highlighter marks, drawn behind the text.
    pub highlights: Vec<TextRect>,
    /// Height of all the lines and the spacing between paragraphs.
    pub height: f32,
}

/// A table's grid once every row has grown to fit its text.
#[derive(Debug, Clone, Default)]
pub struct LaidTable {
    /// Left edge of each column, then the right edge of the last.
    pub column_x: Vec<f32>,
    /// Top edge of each row, then the bottom edge of the last.
    pub row_y: Vec<f32>,
    /// Laid-out text per cell, `None` for empty and merged cells.
    pub cells: Vec<Vec<Option<LaidText>>>,
}

/// Lay out `body` in a box `box_width` points wide.
pub fn layout_text(body: &TextBody, box_width: f32, metrics: &dyn TextMetrics) -> LaidText {
    let inner = (box_width - body.insets.left - body.insets.right).max(1.0);
    let wrap_width = if body.wrap { inner } else { f32::INFINITY };
    let mut laid = LaidText::default();
    let mut y = 0.0;
    let count = body.paragraphs.len();
    for (index, paragraph) in body.paragraphs.iter().enumerate() {
        let size = lead_size(paragraph);
        // Space before the first paragraph would push text off a box's top
        // edge; PowerPoint ignores it there.
        if index > 0 {
            y += spacing(paragraph.space_before, size);
        }
        y = layout_paragraph(paragraph, inner, wrap_width, y, metrics, &mut laid);
        if index + 1 < count {
            y += spacing(paragraph.space_after, size);
        }
    }
    laid.height = y;
    laid
}

/// Distance from the top of a box `box_height` points tall to where text
/// `text_height` points tall starts, honouring the body's anchor. Negative
/// when text overflows a middle- or bottom-anchored box, as in PowerPoint.
pub fn anchor_offset(body: &TextBody, box_height: f32, text_height: f32) -> f32 {
    let inner = box_height - body.insets.top - body.insets.bottom;
    body.insets.top
        + match body.anchor {
            Anchor::Top => 0.0,
            Anchor::Middle => (inner - text_height) / 2.0,
            Anchor::Bottom => inner - text_height,
        }
}

/// Lay out every cell of `table`, growing rows to fit their text.
pub fn layout_table(table: &Table, metrics: &dyn TextMetrics) -> LaidTable {
    let columns = table.columns.len();
    let mut column_x = Vec::with_capacity(columns + 1);
    column_x.push(0.0);
    for width in &table.columns {
        column_x.push(column_x.last().copied().unwrap_or(0.0) + width);
    }

    let mut heights: Vec<f32> = table.rows.iter().map(|row| row.height).collect();
    let mut spanning = Vec::new();
    let mut cells = Vec::with_capacity(table.rows.len());
    for (r, row) in table.rows.iter().enumerate() {
        let mut laid_row = Vec::with_capacity(row.cells.len());
        for (c, cell) in row.cells.iter().enumerate() {
            if cell.merged || c >= columns {
                laid_row.push(None);
                continue;
            }
            let right = (c + cell.column_span).min(columns);
            let laid = cell.text.as_ref().map(|body| {
                let laid = layout_text(body, column_x[right] - column_x[c], metrics);
                let needed = laid.height + body.insets.top + body.insets.bottom;
                if cell.row_span > 1 {
                    spanning.push((r, cell.row_span, needed));
                } else {
                    heights[r] = heights[r].max(needed);
                }
                laid
            });
            laid_row.push(laid);
        }
        cells.push(laid_row);
    }
    // A cell spanning rows only needs the rows together to be tall enough;
    // the last of them takes up any shortfall.
    for (first, span, needed) in spanning {
        let end = (first + span).min(heights.len());
        let available: f32 = heights[first..end].iter().sum();
        if needed > available && end > first {
            heights[end - 1] += needed - available;
        }
    }

    let mut row_y = Vec::with_capacity(heights.len() + 1);
    row_y.push(0.0);
    for height in heights {
        row_y.push(row_y.last().copied().unwrap_or(0.0) + height);
    }
    LaidTable {
        column_x,
        row_y,
        cells,
    }
}

/// The size the paragraph's spacing is measured against: its first run's.
fn lead_size(paragraph: &Paragraph) -> f32 {
    paragraph
        .runs
        .iter()
        .find(|run| run.text != "\n")
        .map_or(paragraph.end_size, |run| run.style.size)
}

fn spacing(spacing: Spacing, size: f32) -> f32 {
    match spacing {
        Spacing::Lines(lines) => lines * size * SINGLE_SPACING,
        Spacing::Points(points) => points,
    }
}

/// A word and the whitespace after it, the unit lines break between.
struct Word {
    run: usize,
    text: String,
    /// The spaces that followed the word, drawn only between words.
    trailing: String,
    width: f32,
    space: f32,
    /// Extra space after every character, from the run's letter spacing.
    tracking: f32,
    font: Font,
    /// Size the glyphs are drawn at: smaller for super- and subscripts.
    drawn_size: f32,
    /// A forced line break rather than text.
    is_break: bool,
}

fn split_words(paragraph: &Paragraph, max_width: f32, metrics: &dyn TextMetrics) -> Vec<Word> {
    let mut words = Vec::new();
    for (index, run) in paragraph.runs.iter().enumerate() {
        let style = &run.style;
        let font = metrics.font(&style.font, style.bold, style.italic);
        let drawn_size = if style.baseline == 0.0 {
            style.size
        } else {
            style.size * SCRIPT_SCALE
        };
        let tracking = style.spacing;
        if run.text == "\n" {
            words.push(Word {
                run: index,
                text: String::new(),
                trailing: String::new(),
                width: 0.0,
                space: 0.0,
                tracking: 0.0,
                font,
                drawn_size: style.size,
                is_break: true,
            });
            continue;
        }
        for piece in run.text.split_inclusive(' ') {
            let text = piece.trim_end_matches(' ');
            let trailing = &piece[text.len()..];
            let space = if trailing.is_empty() {
                0.0
            } else {
                metrics.width(trailing, font, drawn_size) + tracking * trailing.len() as f32
            };
            let width =
                metrics.width(text, font, drawn_size) + tracking * text.chars().count() as f32;
            if width > max_width && text.chars().count() > 1 {
                // A word wider than a whole line is broken between
                // characters, or it would run out of its box.
                let chunks = break_word(text, font, drawn_size, tracking, max_width, metrics);
                let last = chunks.len() - 1;
                for (i, (chunk, chunk_width)) in chunks.into_iter().enumerate() {
                    words.push(Word {
                        run: index,
                        text: chunk,
                        trailing: if i == last {
                            trailing.to_string()
                        } else {
                            String::new()
                        },
                        width: chunk_width,
                        space: if i == last { space } else { 0.0 },
                        tracking,
                        font,
                        drawn_size,
                        is_break: false,
                    });
                }
            } else {
                words.push(Word {
                    run: index,
                    text: text.to_string(),
                    trailing: trailing.to_string(),
                    width,
                    space,
                    tracking,
                    font,
                    drawn_size,
                    is_break: false,
                });
            }
        }
    }
    words
}

/// Split `text` into pieces no wider than `max_width`, each at least one
/// character long.
fn break_word(
    text: &str,
    font: Font,
    size: f32,
    tracking: f32,
    max_width: f32,
    metrics: &dyn TextMetrics,
) -> Vec<(String, f32)> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_width = 0.0;
    for ch in text.chars() {
        let ch_width = metrics.width(ch.encode_utf8(&mut [0; 4]), font, size) + tracking;
        if !current.is_empty() && current_width + ch_width > max_width + FIT_EPSILON {
            chunks.push((std::mem::take(&mut current), current_width));
            current_width = 0.0;
        }
        current.push(ch);
        current_width += ch_width;
    }
    if !current.is_empty() {
        chunks.push((current, current_width));
    }
    chunks
}

struct Line {
    /// Left edge of the line's text, before alignment.
    start: f32,
    /// Words on the line and their offsets from `start`.
    items: Vec<(usize, f32)>,
    cursor: f32,
    /// Size of the break that ended an empty line, which sets its height.
    break_size: Option<f32>,
}

impl Line {
    fn new(start: f32) -> Self {
        Self {
            start,
            items: Vec::new(),
            cursor: 0.0,
            break_size: None,
        }
    }
}

fn layout_paragraph(
    paragraph: &Paragraph,
    inner: f32,
    wrap_width: f32,
    top: f32,
    metrics: &dyn TextMetrics,
    laid: &mut LaidText,
) -> f32 {
    let words = split_words(
        paragraph,
        (wrap_width - paragraph.margin_left).max(1.0),
        metrics,
    );

    let bullet_x = paragraph.margin_left + paragraph.indent;
    let bullet = paragraph.bullet.as_ref().map(|bullet| {
        let font = metrics.font(&bullet.font, false, false);
        (bullet, font, metrics.width(&bullet.text, font, bullet.size))
    });
    let first_start = match &bullet {
        // A hanging bullet sits in the indent; text starts at the margin
        // unless the bullet is wider than the space it hangs in.
        Some((_, _, width)) if paragraph.indent < 0.0 => {
            paragraph.margin_left.max(bullet_x + width)
        }
        Some((bullet, _, width)) => bullet_x + width + bullet.size * BULLET_GAP,
        None => bullet_x,
    };

    let mut lines = Vec::new();
    let mut line = Line::new(first_start);
    for (index, word) in words.iter().enumerate() {
        if word.is_break {
            line.break_size = Some(word.drawn_size);
            lines.push(std::mem::replace(
                &mut line,
                Line::new(paragraph.margin_left),
            ));
            continue;
        }
        if !line.items.is_empty()
            && line.start + line.cursor + word.width > wrap_width + FIT_EPSILON
        {
            lines.push(std::mem::replace(
                &mut line,
                Line::new(paragraph.margin_left),
            ));
        }
        line.items.push((index, line.cursor));
        line.cursor += word.width + word.space;
    }
    lines.push(line);

    let mut y = top;
    for (line_index, line) in lines.iter().enumerate() {
        let tallest = line.items.iter().map(|&(i, _)| &words[i]).max_by(|a, b| {
            let size = |w: &Word| paragraph.runs[w.run].style.size;
            size(a).total_cmp(&size(b))
        });
        let size = tallest.map_or_else(
            || line.break_size.unwrap_or(paragraph.end_size),
            |word| paragraph.runs[word.run].style.size,
        );
        let line_font = tallest.map_or(Font::DEFAULT, |word| word.font);
        let height = match paragraph.line_spacing {
            Spacing::Lines(lines) => size * SINGLE_SPACING * lines,
            Spacing::Points(points) => points,
        };
        let (ascent, descent) = metrics.vertical(line_font);
        let baseline = y + height * ascent / (ascent + descent).max(0.01);

        let natural = line.items.last().map_or(0.0, |&(i, x)| x + words[i].width);
        let room = inner - line.start;
        let shift = match paragraph.align {
            Align::Center => (room - natural) / 2.0,
            Align::Right => room - natural,
            Align::Left | Align::Justify => 0.0,
        };
        let origin = line.start + shift;

        if line_index == 0
            && let Some((bullet, font, _)) = &bullet
        {
            laid.runs.push(PlacedRun {
                text: bullet.text.clone(),
                x: bullet_x + shift,
                y: box_top(baseline, bullet.size, *font, metrics),
                size: bullet.size,
                font: *font,
                color: bullet.color,
            });
        }

        let mut k = 0;
        while k < line.items.len() {
            let (first_word, first_x) = line.items[k];
            let run = words[first_word].run;
            let mut text = String::new();
            let mut end = k;
            while end < line.items.len() && words[line.items[end].0].run == run {
                let word = &words[line.items[end].0];
                text.push_str(&word.text);
                let next_same_run = line
                    .items
                    .get(end + 1)
                    .is_some_and(|&(next, _)| words[next].run == run);
                if next_same_run {
                    text.push_str(&word.trailing);
                }
                end += 1;
            }
            let (last_word, last_x) = line.items[end - 1];
            let width = last_x + words[last_word].width - first_x;
            let x = origin + first_x;

            let style = &paragraph.runs[run].style;
            let word = &words[first_word];
            let run_baseline = baseline - style.baseline * style.size;
            if let Some(color) = style.highlight {
                laid.highlights.push(TextRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                });
            }
            if style.color.a > 0 && !text.trim().is_empty() {
                let top = box_top(run_baseline, word.drawn_size, word.font, metrics);
                if word.tracking == 0.0 {
                    laid.runs.push(PlacedRun {
                        text,
                        x,
                        y: top,
                        size: word.drawn_size,
                        font: word.font,
                        color: style.color,
                    });
                } else {
                    // iced text has no letter spacing, so tracked text is
                    // placed a character at a time.
                    let mut char_x = x;
                    for ch in text.chars() {
                        let glyph = ch.to_string();
                        let advance =
                            metrics.width(&glyph, word.font, word.drawn_size) + word.tracking;
                        if !ch.is_whitespace() {
                            laid.runs.push(PlacedRun {
                                text: glyph,
                                x: char_x,
                                y: top,
                                size: word.drawn_size,
                                font: word.font,
                                color: style.color,
                            });
                        }
                        char_x += advance;
                    }
                }
            }
            let thickness = (word.drawn_size * 0.06).max(0.5);
            if style.underline {
                laid.rules.push(TextRect {
                    x,
                    y: run_baseline + word.drawn_size * 0.1,
                    width,
                    height: thickness,
                    color: style.color,
                });
            }
            if style.strike {
                laid.rules.push(TextRect {
                    x,
                    y: run_baseline - word.drawn_size * 0.3,
                    width,
                    height: thickness,
                    color: style.color,
                });
            }
            k = end;
        }
        y += height;
    }
    y
}

/// Top of a one-font-size-tall iced text box whose glyphs sit on `baseline`.
/// The shaper centres a face's ascent-to-descent extent in the box.
fn box_top(baseline: f32, size: f32, font: Font, metrics: &dyn TextMetrics) -> f32 {
    let (ascent, descent) = metrics.vertical(font);
    baseline - size * ((1.0 - (ascent + descent)) / 2.0 + ascent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use md_editor_core::pptx::{Borders, Bullet, Fill, Insets, Run, RunStyle, TableCell, TableRow};

    /// Every character is half its font size wide, in one default font.
    struct Fixed;

    impl TextMetrics for Fixed {
        fn font(&self, _: &str, _: bool, _: bool) -> Font {
            Font::DEFAULT
        }
        fn width(&self, text: &str, _: Font, size: f32) -> f32 {
            text.chars().count() as f32 * size * 0.5
        }
        fn vertical(&self, _: Font) -> (f32, f32) {
            (0.8, 0.2)
        }
    }

    fn run(text: &str, size: f32) -> Run {
        Run {
            text: text.to_string(),
            style: RunStyle {
                size,
                bold: false,
                italic: false,
                underline: false,
                strike: false,
                color: Rgba::BLACK,
                font: "Arial".to_string(),
                baseline: 0.0,
                highlight: None,
                spacing: 0.0,
            },
        }
    }

    fn paragraph(runs: Vec<Run>) -> Paragraph {
        Paragraph {
            align: Align::Left,
            margin_left: 0.0,
            indent: 0.0,
            space_before: Spacing::Points(0.0),
            space_after: Spacing::Points(0.0),
            line_spacing: Spacing::Lines(1.0),
            bullet: None,
            runs,
            end_size: 10.0,
        }
    }

    fn body(paragraphs: Vec<Paragraph>) -> TextBody {
        TextBody {
            insets: Insets::default(),
            anchor: Anchor::Top,
            wrap: true,
            paragraphs,
        }
    }

    fn texts(laid: &LaidText) -> Vec<(&str, f32)> {
        laid.runs.iter().map(|r| (r.text.as_str(), r.x)).collect()
    }

    #[test]
    fn wraps_words_at_the_box_width() {
        // Characters are 5pt wide: "aaaa " and "bbbb" fill 45 of 50pt.
        let laid = layout_text(
            &body(vec![paragraph(vec![run("aaaa bbbb cccc", 10.0)])]),
            50.0,
            &Fixed,
        );
        assert_eq!(texts(&laid), [("aaaa bbbb", 0.0), ("cccc", 0.0)]);
        assert!(laid.runs[1].y > laid.runs[0].y);
        assert!(
            (laid.height - 24.0).abs() < 1e-3,
            "two 12pt lines, got {}",
            laid.height
        );
    }

    #[test]
    fn aligns_lines_within_the_box() {
        let mut centered = paragraph(vec![run("ab", 10.0)]);
        centered.align = Align::Center;
        let mut right = paragraph(vec![run("ab", 10.0)]);
        right.align = Align::Right;
        let laid = layout_text(&body(vec![centered, right]), 50.0, &Fixed);
        assert_eq!(texts(&laid), [("ab", 20.0), ("ab", 40.0)]);
    }

    #[test]
    fn hanging_bullets_sit_left_of_wrapped_text() {
        let mut item = paragraph(vec![run("aaaa bbbb", 10.0)]);
        item.margin_left = 20.0;
        item.indent = -20.0;
        item.bullet = Some(Bullet {
            text: "•".to_string(),
            color: Rgba::BLACK,
            size: 10.0,
            font: "Arial".to_string(),
        });
        let laid = layout_text(&body(vec![item]), 60.0, &Fixed);
        assert_eq!(texts(&laid), [("•", 0.0), ("aaaa", 20.0), ("bbbb", 20.0)]);
    }

    #[test]
    fn breaks_words_wider_than_a_line() {
        let laid = layout_text(
            &body(vec![paragraph(vec![run("aaaaaaaaaaaa", 10.0)])]),
            25.0,
            &Fixed,
        );
        let pieces: Vec<&str> = laid.runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(pieces, ["aaaaa", "aaaaa", "aa"]);
    }

    #[test]
    fn breaks_and_empty_paragraphs_take_a_line_each() {
        let laid = layout_text(
            &body(vec![
                paragraph(vec![run("one", 10.0), run("\n", 10.0), run("two", 10.0)]),
                paragraph(Vec::new()),
            ]),
            100.0,
            &Fixed,
        );
        assert_eq!(texts(&laid), [("one", 0.0), ("two", 0.0)]);
        assert!(
            (laid.height - 36.0).abs() < 1e-3,
            "three 12pt lines, got {}",
            laid.height
        );
    }

    #[test]
    fn runs_of_different_sizes_share_a_baseline() {
        let laid = layout_text(
            &body(vec![paragraph(vec![run("big ", 20.0), run("small", 10.0)])]),
            200.0,
            &Fixed,
        );
        let (big, small) = (&laid.runs[0], &laid.runs[1]);
        assert_eq!(small.x, 40.0);
        // With ascent 0.8 and descent 0.2 the glyph box's top sits 0.8 of
        // the size above the baseline, so both runs end at the same line.
        let baseline = |r: &PlacedRun| r.y + r.size * 0.8;
        assert!((baseline(big) - baseline(small)).abs() < 1e-3);
    }

    #[test]
    fn letter_spacing_places_each_character() {
        let mut spaced = run("ab c", 10.0);
        spaced.style.spacing = 2.0;
        let laid = layout_text(&body(vec![paragraph(vec![spaced])]), 100.0, &Fixed);
        // Each character advances 5pt plus 2pt of tracking; the space between
        // the words takes its advance but draws nothing.
        assert_eq!(texts(&laid), [("a", 0.0), ("b", 7.0), ("c", 21.0)]);
    }

    #[test]
    fn anchoring_places_text_in_its_box() {
        let mut text = body(vec![paragraph(vec![run("a", 10.0)])]);
        text.insets = Insets {
            left: 0.0,
            top: 4.0,
            right: 0.0,
            bottom: 4.0,
        };
        assert_eq!(anchor_offset(&text, 100.0, 12.0), 4.0);
        text.anchor = Anchor::Middle;
        assert_eq!(anchor_offset(&text, 100.0, 12.0), 44.0);
        text.anchor = Anchor::Bottom;
        assert_eq!(anchor_offset(&text, 100.0, 12.0), 84.0);
    }

    #[test]
    fn table_rows_grow_to_fit_their_text() {
        let cell = |text: Option<&str>| TableCell {
            text: text.map(|t| body(vec![paragraph(vec![run(t, 10.0)])])),
            fill: Fill::None,
            borders: Borders::default(),
            column_span: 1,
            row_span: 1,
            merged: false,
        };
        let table = Table {
            columns: vec![25.0, 50.0],
            rows: vec![
                TableRow {
                    height: 5.0,
                    cells: vec![cell(Some("aaaa bbbb")), cell(None)],
                },
                TableRow {
                    height: 30.0,
                    cells: vec![cell(Some("a")), cell(Some("b"))],
                },
            ],
        };
        let laid = layout_table(&table, &Fixed);
        assert_eq!(laid.column_x, [0.0, 25.0, 75.0]);
        // The first row wraps to two 12pt lines; the second keeps its height.
        assert_eq!(laid.row_y, [0.0, 24.0, 54.0]);
        assert!(laid.cells[0][1].is_none());
    }
}

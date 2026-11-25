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
    let lines = wrap_lines(&words, paragraph.margin_left, first_start, wrap_width);

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

        // Consecutive words from the same run are placed as one segment.
        let row = LineRow {
            origin: line.start + shift,
            y,
            height,
            baseline,
        };
        let mut k = 0;
        while k < line.items.len() {
            let run = words[line.items[k].0].run;
            let end = k + line.items[k..]
                .iter()
                .take_while(|&&(i, _)| words[i].run == run)
                .count();
            place_segment(&line.items[k..end], &words, paragraph, row, metrics, laid);
            k = end;
        }
        y += height;
    }
    y
}

/// Greedy line breaking: the first line starts at `first_start` (after any
/// bullet), the rest at the paragraph margin.
fn wrap_lines(words: &[Word], margin_left: f32, first_start: f32, wrap_width: f32) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut line = Line::new(first_start);
    for (index, word) in words.iter().enumerate() {
        if word.is_break {
            line.break_size = Some(word.drawn_size);
            lines.push(std::mem::replace(&mut line, Line::new(margin_left)));
            continue;
        }
        if !line.items.is_empty()
            && line.start + line.cursor + word.width > wrap_width + FIT_EPSILON
        {
            lines.push(std::mem::replace(&mut line, Line::new(margin_left)));
        }
        line.items.push((index, line.cursor));
        line.cursor += word.width + word.space;
    }
    lines.push(line);
    lines
}

/// Where one laid-out line sits: its left edge after alignment, its top,
/// height and baseline.
#[derive(Clone, Copy)]
struct LineRow {
    origin: f32,
    y: f32,
    height: f32,
    baseline: f32,
}

/// Place one same-run stretch of a line: its highlight, its text and any
/// underline or strike rule.
fn place_segment(
    items: &[(usize, f32)],
    words: &[Word],
    paragraph: &Paragraph,
    row: LineRow,
    metrics: &dyn TextMetrics,
    laid: &mut LaidText,
) {
    let (first_word, first_x) = items[0];
    let (last_word, last_x) = items[items.len() - 1];
    let mut text = String::new();
    for (n, &(index, _)) in items.iter().enumerate() {
        text.push_str(&words[index].text);
        if n + 1 < items.len() {
            text.push_str(&words[index].trailing);
        }
    }
    let width = last_x + words[last_word].width - first_x;
    let x = row.origin + first_x;

    let word = &words[first_word];
    let style = &paragraph.runs[word.run].style;
    let run_baseline = row.baseline - style.baseline * style.size;
    if let Some(color) = style.highlight {
        laid.highlights.push(TextRect {
            x,
            y: row.y,
            width,
            height: row.height,
            color,
        });
    }
    if style.color.a > 0 && !text.trim().is_empty() {
        let top = box_top(run_baseline, word.drawn_size, word.font, metrics);
        push_text(laid, text, x, top, word, style.color, metrics);
    }
    let thickness = (word.drawn_size * 0.06).max(0.5);
    let mut rule = |y| {
        laid.rules.push(TextRect {
            x,
            y,
            width,
            height: thickness,
            color: style.color,
        })
    };
    if style.underline {
        rule(run_baseline + word.drawn_size * 0.1);
    }
    if style.strike {
        rule(run_baseline - word.drawn_size * 0.3);
    }
}

/// Push a run of text; tracked text is placed a character at a time, since
/// iced text has no letter spacing.
fn push_text(
    laid: &mut LaidText,
    text: String,
    x: f32,
    top: f32,
    word: &Word,
    color: Rgba,
    metrics: &dyn TextMetrics,
) {
    if word.tracking == 0.0 {
        laid.runs.push(PlacedRun {
            text,
            x,
            y: top,
            size: word.drawn_size,
            font: word.font,
            color,
        });
        return;
    }
    let mut char_x = x;
    for ch in text.chars() {
        let glyph = ch.to_string();
        let advance = metrics.width(&glyph, word.font, word.drawn_size) + word.tracking;
        if !ch.is_whitespace() {
            laid.runs.push(PlacedRun {
                text: glyph,
                x: char_x,
                y: top,
                size: word.drawn_size,
                font: word.font,
                color,
            });
        }
        char_x += advance;
    }
}

/// Top of a one-font-size-tall iced text box whose glyphs sit on `baseline`.
/// The shaper centres a face's ascent-to-descent extent in the box.
fn box_top(baseline: f32, size: f32, font: Font, metrics: &dyn TextMetrics) -> f32 {
    let (ascent, descent) = metrics.vertical(font);
    baseline - size * ((1.0 - (ascent + descent)) / 2.0 + ascent)
}

#[cfg(test)]
mod tests;

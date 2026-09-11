//! Inline layout: breaking a line's spans into visual rows.
//!
//! This is the only place rows are broken. Line heights, painting, caret
//! placement, selection and hit testing all read a [`Flow`], so they cannot
//! disagree about where text is.
//!
//! Rows break greedily between words; whitespace after a word hangs past the
//! right edge instead of forcing a break, and a word wider than a whole row
//! is broken between characters. Every item on a row shares the row's
//! baseline, so mixed font sizes and inline equations line up.

use std::ops::Range;

use super::measure::{measure_char_width, span_font};
use super::metrics::*;
use super::spans::{math_source, source_col_after_span, span_is_editing};
use super::{MathCache, Measure};
use crate::editor::highlight::StyledLine;

/// A line broken into rows. Positions are relative to the line's text origin:
/// the left edge of the text column and the top of the line's body.
pub(super) struct Flow<'a> {
    line: &'a StyledLine,
    pub rows: Vec<Row>,
    /// In source order; rows never decrease.
    pub items: Vec<Item>,
}

pub(super) struct Row {
    pub top: f32,
    pub height: f32,
    pub baseline: f32,
    /// Index range into [`Flow::items`].
    items: Range<usize>,
}

pub(super) struct Item {
    pub span_idx: usize,
    pub row: usize,
    pub x: f32,
    /// Horizontal space taken, including any trailing whitespace or gap.
    pub advance: f32,
    pub font_size: f32,
    /// Source columns covered.
    pub start_col: usize,
    pub end_col: usize,
    pub kind: ItemKind,
}

pub(super) enum ItemKind {
    /// Characters of a span's visible text.
    Text(TextPart),
    /// A span that shows nothing, such as a concealed `**` marker.
    Hidden,
    Checkbox {
        checked: bool,
    },
    /// A rendered inline equation.
    Math {
        width: f32,
        height: f32,
    },
}

pub(super) struct TextPart {
    /// Whether the span shows its markdown source.
    pub revealed: bool,
    /// Byte range within the span's visible text.
    pub bytes: Range<usize>,
    /// Index, within the span's visible text, of the first character.
    first_char: usize,
    span_start_col: usize,
    span_end_col: usize,
    pub font: iced::Font,
}

impl TextPart {
    /// Source column of the character at `char_idx` of the span's visible
    /// text. Visible text that is longer than its source clamps to its end.
    fn col_of(&self, char_idx: usize) -> usize {
        (self.span_start_col + char_idx).min(self.span_end_col)
    }
}

/// Where the caret is drawn for a column.
pub(super) struct CaretSpot {
    pub x: f32,
    pub row: usize,
    pub font_size: f32,
}

impl<'a> Flow<'a> {
    pub fn build<R: Measure>(
        line: &'a StyledLine,
        math_cache: &MathCache,
        available_width: f32,
        block_editing: bool,
        active_col: Option<usize>,
    ) -> Self {
        let mut builder = Builder {
            items: Vec::new(),
            rows: Vec::new(),
            row: RowMetrics::default(),
            row_start: 0,
            x: 0.0,
            max_w: wrap_width(available_width),
        };

        let mut span_start_col = 0;
        for (span_idx, span) in line.spans.iter().enumerate() {
            let span_end_col = source_col_after_span(span, span_start_col);
            let revealed = span_is_editing(line, span_idx, block_editing, active_col);
            let font_size = span.font_size;
            let atom = |advance, kind| Item {
                span_idx,
                row: 0,
                x: 0.0,
                advance,
                font_size,
                start_col: span_start_col,
                end_col: span_end_col,
                kind,
            };

            if span.is_checkbox && !revealed {
                builder.place_atom(
                    atom(
                        CHECKBOX_ADVANCE,
                        ItemKind::Checkbox {
                            checked: span.is_checked,
                        },
                    ),
                    CHECKBOX_ADVANCE,
                );
            } else if span.is_math && !line.is_math_block && !revealed {
                let tex = math_source(span);
                if let Some(math) = math_cache.get(tex).filter(|_| !span.is_syntax) {
                    let kind = ItemKind::Math {
                        width: math.width,
                        height: math.height,
                    };
                    builder.place_atom(atom(math.width + INLINE_MATH_GAP, kind), math.width);
                } else if tex.is_empty() || span.is_syntax {
                    builder.place_atom(atom(0.0, ItemKind::Hidden), 0.0);
                } else {
                    builder.place_text::<R>(line, span_idx, revealed, span_start_col, span_end_col);
                }
            } else if span.visible_text(revealed).is_empty() {
                builder.place_atom(atom(0.0, ItemKind::Hidden), 0.0);
            } else {
                builder.place_text::<R>(line, span_idx, revealed, span_start_col, span_end_col);
            }

            span_start_col = span_end_col;
        }

        builder.finish(line)
    }

    /// Total height of the rows.
    pub fn height(&self) -> f32 {
        self.rows
            .last()
            .map_or(BASE_LINE_HEIGHT, |row| row.top + row.height)
    }

    pub fn row_items(&self, row: usize) -> &[Item] {
        &self.items[self.rows[row].items.clone()]
    }

    /// The visible text of a text item.
    pub fn text(&self, part: &TextPart, span_idx: usize) -> &'a str {
        &self.line.spans[span_idx].visible_text(part.revealed)[part.bytes.clone()]
    }

    /// Top of a text line box of `font_size` sitting on `row`'s baseline.
    pub fn text_top(&self, row: usize, font_size: f32) -> f32 {
        self.rows[row].baseline - font_size * BASELINE_FACTOR
    }

    /// The line inline widgets (checkboxes, equations) are centred on.
    pub fn axis(&self, row: usize) -> f32 {
        self.rows[row].baseline - MATH_AXIS_HEIGHT
    }

    /// Where the caret goes for source column `col`.
    pub fn caret<R: Measure>(&self, col: usize) -> CaretSpot {
        let Some(last) = self.items.last() else {
            return CaretSpot {
                x: 0.0,
                row: 0,
                font_size: DEFAULT_FONT_SIZE,
            };
        };
        // The first item still covering `col`; a column on a boundary belongs
        // to the item that starts there.
        let item = self
            .items
            .iter()
            .find(|item| col < item.end_col)
            .unwrap_or(last);
        let x = if col >= item.end_col {
            item.x + item.advance
        } else {
            item.x + self.offset_in_item::<R>(item, col)
        };
        CaretSpot {
            x,
            row: item.row,
            font_size: item.font_size,
        }
    }

    /// X offset of column `col` from the start of `item`, which must cover it.
    fn offset_in_item<R: Measure>(&self, item: &Item, col: usize) -> f32 {
        let ItemKind::Text(part) = &item.kind else {
            return 0.0;
        };
        let mut offset = 0.0;
        for (i, ch) in self.text(part, item.span_idx).chars().enumerate() {
            if part.col_of(part.first_char + i) >= col {
                break;
            }
            offset += measure_char_width::<R>(ch, item.font_size, part.font);
        }
        offset
    }

    /// Row at a y offset, clamped to the first and last rows.
    pub fn row_at(&self, y: f32) -> usize {
        self.rows
            .iter()
            .position(|row| y < row.top + row.height)
            .unwrap_or(self.rows.len().saturating_sub(1))
    }

    /// Source column nearest the point (`x`, `y`).
    pub fn col_at<R: Measure>(&self, x: f32, y: f32) -> usize {
        if self.items.is_empty() {
            return 0;
        }
        let row = self.row_at(y);
        let items = self.row_items(row);

        for item in items {
            match &item.kind {
                ItemKind::Text(part) => {
                    let mut cx = item.x;
                    for (i, ch) in self.text(part, item.span_idx).chars().enumerate() {
                        let cw = measure_char_width::<R>(ch, item.font_size, part.font);
                        if x < cx + cw / 2.0 {
                            return part.col_of(part.first_char + i);
                        }
                        cx += cw;
                    }
                }
                ItemKind::Checkbox { .. } | ItemKind::Math { .. } => {
                    if x < item.x + item.advance / 2.0 {
                        return item.start_col;
                    }
                }
                ItemKind::Hidden => {}
            }
        }

        // Past the end of the row.
        let last = &items[items.len() - 1];
        if row + 1 == self.rows.len() {
            return last.end_col;
        }
        // The end column of a wrapped row is drawn at the start of the next
        // one, so stay before the row's last character instead.
        match &last.kind {
            ItemKind::Text(part) => {
                let chars = self.text(part, last.span_idx).chars().count();
                part.col_of(part.first_char + chars.saturating_sub(1))
            }
            _ => last.start_col,
        }
    }

    /// The span under (`x`, `y`), if any.
    pub fn span_at(&self, x: f32, y: f32) -> Option<usize> {
        if self.items.is_empty() {
            return None;
        }
        self.row_items(self.row_at(y))
            .iter()
            .find(|item| x >= item.x && x < item.x + item.advance)
            .map(|item| item.span_idx)
    }

    /// Horizontal extents, per row, of source columns `from..to`.
    pub fn range_extents<R: Measure>(&self, from: usize, to: usize) -> Vec<(usize, f32, f32)> {
        let mut extents = Vec::new();
        for (row_idx, row) in self.rows.iter().enumerate() {
            let items = &self.items[row.items.clone()];
            let (Some(first), Some(last)) = (items.first(), items.last()) else {
                continue;
            };
            let start = from.max(first.start_col);
            let end = to.min(last.end_col);
            if start >= end {
                continue;
            }
            let x0 = self.x_in_row::<R>(items, start);
            let x1 = self.x_in_row::<R>(items, end);
            extents.push((row_idx, x0, x1));
        }
        extents
    }

    /// X of `col` among `items` of one row; columns past the row map to its
    /// end rather than to the next row.
    fn x_in_row<R: Measure>(&self, items: &[Item], col: usize) -> f32 {
        for item in items {
            if col < item.end_col {
                return item.x + self.offset_in_item::<R>(item, col.max(item.start_col));
            }
        }
        items.last().map_or(0.0, |item| item.x + item.advance)
    }
}

/// Vertical extent of what sits on a row, around its baseline.
#[derive(Default)]
struct RowMetrics {
    ascent: f32,
    descent: f32,
    /// Minimum height from the font sizes on the row.
    step: f32,
    has_math: bool,
}

impl RowMetrics {
    fn add_text(&mut self, font_size: f32) {
        self.step = self.step.max(visual_line_step(font_size));
        self.ascent = self.ascent.max(font_size * BASELINE_FACTOR);
        self.descent = self
            .descent
            .max(font_size * (LINE_BOX_FACTOR - BASELINE_FACTOR));
    }

    /// Something `height` tall centred on the row's axis.
    fn add_centered(&mut self, height: f32) {
        self.ascent = self.ascent.max(height / 2.0 + MATH_AXIS_HEIGHT);
        self.descent = self.descent.max(height / 2.0 - MATH_AXIS_HEIGHT);
    }
}

struct Builder {
    items: Vec<Item>,
    rows: Vec<Row>,
    row: RowMetrics,
    row_start: usize,
    x: f32,
    max_w: f32,
}

impl Builder {
    /// Close the current row and start a new one.
    fn break_row(&mut self) {
        let metrics = std::mem::take(&mut self.row);
        let top = self.rows.last().map_or(0.0, |row| row.top + row.height);
        let content = metrics.ascent + metrics.descent;
        let padding = if metrics.has_math {
            INLINE_MATH_ROW_PADDING
        } else {
            0.0
        };
        let height = metrics.step.max(content + padding).max(BASE_LINE_HEIGHT);
        self.rows.push(Row {
            top,
            height,
            baseline: top + (height - content) / 2.0 + metrics.ascent,
            items: self.row_start..self.items.len(),
        });
        self.row_start = self.items.len();
        self.x = 0.0;
    }

    /// Move to a new row if something `fit_width` wide doesn't fit here.
    /// Zero-width things never break, so hanging whitespace can't strand them
    /// on a row of their own.
    fn fit(&mut self, fit_width: f32) {
        if fit_width > 0.0 && self.x > 0.0 && self.x + fit_width > self.max_w {
            self.break_row();
        }
    }

    fn push(&mut self, mut item: Item) {
        item.row = self.rows.len();
        item.x = self.x;
        self.x += item.advance;
        self.items.push(item);
    }

    /// Place an unbreakable item whose visible part is `fit_width` wide.
    fn place_atom(&mut self, item: Item, fit_width: f32) {
        self.fit(fit_width);
        match &item.kind {
            ItemKind::Checkbox { .. } => {
                self.row.step = self.row.step.max(visual_line_step(item.font_size));
                self.row.add_centered(CHECKBOX_SIZE);
            }
            ItemKind::Math { height, .. } => {
                self.row.add_centered(*height);
                self.row.has_math = true;
            }
            // Concealed text still holds its row open at its own size, so an
            // empty line is as tall and as aligned as a line of text.
            ItemKind::Hidden | ItemKind::Text(_) => self.row.add_text(item.font_size),
        }
        self.push(item);
    }

    /// Place a span's visible text, word by word.
    fn place_text<R: Measure>(
        &mut self,
        line: &StyledLine,
        span_idx: usize,
        revealed: bool,
        span_start_col: usize,
        span_end_col: usize,
    ) {
        let span = &line.spans[span_idx];
        let text = span.visible_text(revealed);
        let font = span_font(span, line);
        let font_size = span.font_size;
        let char_w = |ch: char| measure_char_width::<R>(ch, font_size, font);
        let total_chars = text.chars().count();

        let make = |bytes: Range<usize>, first_char: usize, chars: usize, advance: f32| {
            let part = TextPart {
                revealed,
                bytes,
                first_char,
                span_start_col,
                span_end_col,
                font,
            };
            let start_col = part.col_of(first_char);
            let end_col = if first_char + chars == total_chars {
                span_end_col
            } else {
                part.col_of(first_char + chars)
            };
            Item {
                span_idx,
                row: 0,
                x: 0.0,
                advance,
                font_size,
                start_col,
                end_col,
                kind: ItemKind::Text(part),
            }
        };

        for word in words(text) {
            let chars = || {
                text[word.bytes.clone()]
                    .char_indices()
                    .map(move |(b, ch)| (word.bytes.start + b, ch))
            };
            // Trailing whitespace hangs, so only the word itself has to fit.
            let (word_w, total_w) = chars().fold((0.0, 0.0), |(ink, all), (_, ch)| {
                let w = char_w(ch);
                if ch.is_whitespace() {
                    (ink, all + w)
                } else {
                    (all + w, all + w)
                }
            });

            if word_w <= self.max_w {
                self.fit(word_w);
                self.row.add_text(font_size);
                let item = make(
                    word.bytes.clone(),
                    word.first_char,
                    word.char_count,
                    total_w,
                );
                self.push(item);
                continue;
            }

            // Wider than a row: break between characters.
            self.fit(word_w);
            let mut chunk_start: Option<(usize, usize)> = None;
            let mut chunk_chars = 0;
            let mut chunk_w = 0.0;
            for (char_offset, (byte, ch)) in chars().enumerate() {
                let w = char_w(ch);
                if chunk_start.is_some() && !ch.is_whitespace() && self.x + chunk_w + w > self.max_w
                {
                    let (start_byte, start_char) = chunk_start.take().unwrap();
                    self.row.add_text(font_size);
                    self.push(make(start_byte..byte, start_char, chunk_chars, chunk_w));
                    self.break_row();
                    chunk_chars = 0;
                    chunk_w = 0.0;
                }
                if chunk_start.is_none() {
                    chunk_start = Some((byte, word.first_char + char_offset));
                }
                chunk_chars += 1;
                chunk_w += w;
            }
            if let Some((start_byte, start_char)) = chunk_start {
                self.row.add_text(font_size);
                self.push(make(
                    start_byte..word.bytes.end,
                    start_char,
                    chunk_chars,
                    chunk_w,
                ));
            }
        }
    }

    fn finish<'a>(mut self, line: &'a StyledLine) -> Flow<'a> {
        if self.items.is_empty() {
            self.row.add_text(DEFAULT_FONT_SIZE);
        }
        self.break_row();
        Flow {
            line,
            rows: self.rows,
            items: self.items,
        }
    }
}

/// A word and the whitespace after it.
struct Word {
    bytes: Range<usize>,
    first_char: usize,
    char_count: usize,
}

/// Split `text` into words, each carrying its trailing whitespace. Leading
/// whitespace forms a word of its own.
fn words(text: &str) -> Vec<Word> {
    let mut words = Vec::new();
    let mut start = (0, 0);
    let mut char_idx = 0;
    let mut after_space = false;

    for (byte, ch) in text.char_indices() {
        if ch.is_whitespace() {
            after_space = true;
        } else if after_space {
            words.push(Word {
                bytes: start.0..byte,
                first_char: start.1,
                char_count: char_idx - start.1,
            });
            start = (byte, char_idx);
            after_space = false;
        }
        char_idx += 1;
    }
    if char_idx > start.1 {
        words.push(Word {
            bytes: start.0..text.len(),
            first_char: start.1,
            char_count: char_idx - start.1,
        });
    }
    words
}

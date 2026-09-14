//! Inline layout: breaking a line's spans into visual rows.
//!
//! This is the only place rows are broken. Line heights, painting, caret
//! placement, selection and hit testing all read a [`Flow`], so they cannot
//! disagree about where text is.
//!
//! Rows break greedily at Unicode line-break opportunities (UAX #14: between
//! words, after hyphens and dashes, inside URLs); whitespace after a break
//! hangs past the right edge instead of forcing one, and a word wider than a
//! whole row is broken between grapheme clusters. Wrapped rows of a list item
//! hang past its marker. A wrapped heading nobody is editing is balanced: its
//! rows are made as even as the same number of rows allows.
//!
//! Every x comes from the shaper — kerning included — and every item on a row
//! shares the row's baseline, so mixed sizes and inline equations line up.

use std::ops::Range;
use std::sync::Arc;

use super::measure::{font_metrics, span_font, word_offsets};
use super::metrics::*;
use super::spans::{math_source, source_col_after_span, span_is_editing};
use super::{MathCache, Measure};
pub(super) use crate::editor::buffer::Affinity;
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
    /// The line inline widgets (checkboxes, equations) centre on: half the
    /// x-height of the row's largest text above the baseline.
    pub axis: f32,
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
    pub font: iced::Font,
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
    map: SourceMap,
    pub font: iced::Font,
    /// Shaped offsets of every character boundary of the word this part
    /// belongs to (see [`word_offsets`]).
    offsets: Arc<[f32]>,
    /// Index into `offsets` of the part's first character.
    base: usize,
}

/// How a span's visible characters correspond to its source columns.
#[derive(Clone, Copy, PartialEq)]
enum SourceMap {
    /// Visible character `i` is source column `span_start + offset + i`: the
    /// visible text is a substring of the source, like a link's label.
    Offset(usize),
    /// Visible text that doesn't appear in the source, like a bullet drawn
    /// for `- `. It is one indivisible unit: the caret stops only before and
    /// after it.
    Atomic,
}

impl SourceMap {
    fn of(source: &str, visible: &str) -> Self {
        if visible == source {
            return SourceMap::Offset(0);
        }
        match source.find(visible) {
            Some(byte) => SourceMap::Offset(source[..byte].chars().count()),
            None => SourceMap::Atomic,
        }
    }
}

impl TextPart {
    /// Source column of the character at `char_idx` of the span's visible
    /// text.
    fn col_of(&self, char_idx: usize) -> usize {
        match self.map {
            SourceMap::Offset(offset) => {
                (self.span_start_col + offset + char_idx).min(self.span_end_col)
            }
            SourceMap::Atomic if char_idx == 0 => self.span_start_col,
            SourceMap::Atomic => self.span_end_col,
        }
    }

    fn is_atomic(&self) -> bool {
        self.map == SourceMap::Atomic
    }

    /// Offset of the part's `k`th character boundary from its start.
    fn offset(&self, k: usize) -> f32 {
        let last = self.offsets.len() - 1;
        self.offsets[(self.base + k).min(last)] - self.offsets[self.base.min(last)]
    }
}

/// Where the caret is drawn for a column.
pub(super) struct CaretSpot {
    pub x: f32,
    pub row: usize,
    pub font_size: f32,
    pub font: iced::Font,
}

impl<'a> Flow<'a> {
    pub fn build<R: Measure>(
        line: &'a StyledLine,
        math_cache: &MathCache,
        available_width: f32,
        block_editing: bool,
        active_col: Option<usize>,
    ) -> Self {
        let width = wrap_width(available_width);
        let build = |max_w| Self::build_at::<R>(line, math_cache, max_w, block_editing, active_col);
        let flow = build(width);

        // Balance a wrapped heading nobody is editing: the narrowest width
        // that keeps the same number of rows spreads the words most evenly.
        // Headings being edited keep greedy breaks, so text doesn't jump
        // between rows as it's typed.
        let rows = flow.rows.len();
        let balance = rows > 1
            && active_col.is_none()
            && !block_editing
            && line.spans.iter().any(|span| span.is_heading)
            && flow.items.iter().all(|item| {
                !matches!(item.kind, ItemKind::Checkbox { .. } | ItemKind::Math { .. })
            });
        if !balance {
            return flow;
        }
        let (mut narrow, mut wide) = (width / rows as f32, width);
        for _ in 0..14 {
            let mid = (narrow + wide) / 2.0;
            if build(mid).rows.len() == rows {
                wide = mid;
            } else {
                narrow = mid;
            }
        }
        build(wide)
    }

    fn build_at<R: Measure>(
        line: &'a StyledLine,
        math_cache: &MathCache,
        max_w: f32,
        block_editing: bool,
        active_col: Option<usize>,
    ) -> Self {
        let mut builder = Builder {
            items: Vec::new(),
            rows: Vec::new(),
            row: RowMetrics::default(),
            row_start: 0,
            x: 0.0,
            indent: 0.0,
            max_w,
            blank: is_blank(line),
        };

        let mut span_start_col = 0;
        for (span_idx, span) in line.spans.iter().enumerate() {
            let span_end_col = source_col_after_span(span, span_start_col);
            let revealed = span_is_editing(line, span_idx, block_editing, active_col);
            let font_size = span.font_size;
            let font = span_font(span, line);
            let atom = |advance, kind| Item {
                span_idx,
                row: 0,
                x: 0.0,
                advance,
                font_size,
                font,
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

            // Wrapped rows of a list item hang past its marker.
            if span_idx == 0 && span.is_list_marker {
                builder.indent = builder.x.min(builder.max_w / 2.0);
            }

            span_start_col = span_end_col;
        }

        builder.finish(line)
    }

    /// Total height of the rows.
    pub fn height(&self) -> f32 {
        self.rows.last().map_or(0.0, |row| row.top + row.height)
    }

    pub fn row_items(&self, row: usize) -> &[Item] {
        &self.items[self.rows[row].items.clone()]
    }

    /// The visible text of a text item.
    pub fn text(&self, part: &TextPart, span_idx: usize) -> &'a str {
        &self.line.spans[span_idx].visible_text(part.revealed)[part.bytes.clone()]
    }

    /// Top of the iced text box that puts text of `font_size` in `font` on
    /// `row`'s baseline.
    pub fn text_top(&self, row: usize, font_size: f32, font: iced::Font) -> f32 {
        self.rows[row].baseline - font_metrics(font).baseline_in_box(font_size)
    }

    /// The line inline widgets (checkboxes, equations) are centred on.
    pub fn axis(&self, row: usize) -> f32 {
        self.rows[row].axis
    }

    /// Where the caret goes for source column `col`.
    pub fn caret<R: Measure>(&self, col: usize, affinity: Affinity) -> CaretSpot {
        let Some(last) = self.items.last() else {
            return CaretSpot {
                x: 0.0,
                row: 0,
                font_size: DEFAULT_FONT_SIZE,
                font: iced::Font::DEFAULT,
            };
        };
        // Downstream, the first item still covering `col`: a column on a
        // boundary belongs to the item that starts there.
        let item = self
            .items
            .iter()
            .find(|item| col < item.end_col)
            .unwrap_or(last);
        let downstream = self.spot_in::<R>(item, col);
        if affinity == Affinity::Downstream {
            return downstream;
        }
        // Upstream, the item ending at `col` on an earlier row, if the column
        // is a row break.
        self.items
            .iter()
            .rev()
            .find(|item| item.row < downstream.row && item.start_col < col && col <= item.end_col)
            .map_or(downstream, |item| self.spot_in::<R>(item, col))
    }

    /// Caret spot for `col` within `item`, or just past it.
    fn spot_in<R: Measure>(&self, item: &Item, col: usize) -> CaretSpot {
        let x = if col >= item.end_col {
            item.x + item.advance
        } else {
            item.x + self.offset_in_item::<R>(item, col)
        };
        CaretSpot {
            x,
            row: item.row,
            font_size: item.font_size,
            font: item.font,
        }
    }

    /// X offset of column `col` from the start of `item`, which must cover it.
    fn offset_in_item<R: Measure>(&self, item: &Item, col: usize) -> f32 {
        let ItemKind::Text(part) = &item.kind else {
            return 0.0;
        };
        if part.is_atomic() {
            return 0.0;
        }
        let chars = self.text(part, item.span_idx).chars().count();
        let k = (0..chars)
            .find(|&k| part.col_of(part.first_char + k) >= col)
            .unwrap_or(chars);
        part.offset(k)
    }

    /// Row at a y offset, clamped to the first and last rows.
    pub fn row_at(&self, y: f32) -> usize {
        self.rows
            .iter()
            .position(|row| y < row.top + row.height)
            .unwrap_or(self.rows.len().saturating_sub(1))
    }

    /// Caret position nearest the point (`x`, `y`): a source column, and the
    /// row it is on when that column is a row break.
    pub fn col_at<R: Measure>(&self, x: f32, y: f32) -> (usize, Affinity) {
        if self.items.is_empty() {
            return (0, Affinity::Downstream);
        }
        let row = self.row_at(y);
        let items = self.row_items(row);

        for item in items {
            match &item.kind {
                ItemKind::Text(part) if part.is_atomic() => {
                    if x < item.x + item.advance / 2.0 {
                        return (item.start_col, Affinity::Downstream);
                    }
                }
                ItemKind::Text(part) => {
                    let chars = self.text(part, item.span_idx).chars().count();
                    for k in 0..chars {
                        let middle = item.x + (part.offset(k) + part.offset(k + 1)) / 2.0;
                        if x < middle {
                            return (part.col_of(part.first_char + k), Affinity::Downstream);
                        }
                    }
                }
                ItemKind::Checkbox { .. } | ItemKind::Math { .. } => {
                    if x < item.x + item.advance / 2.0 {
                        return (item.start_col, Affinity::Downstream);
                    }
                }
                ItemKind::Hidden => {}
            }
        }

        // Past the end of the row: its end column, kept on this row.
        let end = items[items.len() - 1].end_col;
        (end, Affinity::Upstream)
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

/// Whether a line is blank: nothing but whitespace, a paragraph break.
fn is_blank(line: &StyledLine) -> bool {
    !line.is_code_block
        && !line.is_math_block
        && !line.is_table_row
        && line.spans.iter().all(|span| span.text.trim().is_empty())
}

/// What sits on a row, from which its height and baseline follow.
#[derive(Default)]
struct RowMetrics {
    /// Tallest ascent and descent of the row's text.
    text_ascent: f32,
    text_descent: f32,
    /// Size of the row's largest text, and half its x-height.
    axis_size: f32,
    axis_offset: f32,
    /// Tallest thing centred on the axis.
    centered: f32,
    /// The rhythm's row height for the largest text on the row.
    min_height: f32,
    has_math: bool,
}

impl RowMetrics {
    fn add_text(&mut self, font_size: f32, font: iced::Font) {
        let metrics = font_metrics(font);
        self.text_ascent = self.text_ascent.max(metrics.ascent * font_size);
        self.text_descent = self.text_descent.max(metrics.descent * font_size);
        self.min_height = self.min_height.max(row_height(font_size));
        if font_size >= self.axis_size {
            self.axis_size = font_size;
            self.axis_offset = metrics.x_height * font_size / 2.0;
        }
    }

    /// Something `height` tall centred on the row's axis.
    fn add_centered(&mut self, height: f32) {
        self.centered = self.centered.max(height);
    }
}

struct Builder {
    items: Vec<Item>,
    rows: Vec<Row>,
    row: RowMetrics,
    row_start: usize,
    x: f32,
    /// Where rows after the first start: past a list item's marker.
    indent: f32,
    max_w: f32,
    /// A blank line is a single paragraph-gap row.
    blank: bool,
}

impl Builder {
    /// Close the current row and start a new one.
    ///
    /// The row's glyph extents — text, and widgets centred on the axis — are
    /// centred in a height that is at least the rhythm's row height for its
    /// largest text, and lands on the grid.
    fn break_row(&mut self) {
        let metrics = std::mem::take(&mut self.row);
        let top = self.rows.last().map_or(0.0, |row| row.top + row.height);
        let axis_offset = if metrics.axis_size > 0.0 {
            metrics.axis_offset
        } else {
            font_metrics(iced::Font::DEFAULT).x_height * DEFAULT_FONT_SIZE / 2.0
        };
        let half = metrics.centered / 2.0;
        let ascent = metrics.text_ascent.max(half + axis_offset);
        let descent = metrics.text_descent.max(half - axis_offset);
        let content = ascent + descent;
        let height = if self.blank {
            PARAGRAPH_GAP
        } else {
            let padding = if metrics.has_math {
                INLINE_MATH_ROW_PADDING
            } else {
                0.0
            };
            snap_up(metrics.min_height.max(content + padding))
        };
        let baseline = top + (height - content) / 2.0 + ascent;
        self.rows.push(Row {
            top,
            height,
            baseline,
            axis: baseline - axis_offset,
            items: self.row_start..self.items.len(),
        });
        self.row_start = self.items.len();
        self.x = self.indent;
    }

    /// Move to a new row if something `fit_width` wide doesn't fit here.
    /// Zero-width things never break, so hanging whitespace can't strand them
    /// on a row of their own.
    fn fit(&mut self, fit_width: f32) {
        if fit_width > 0.0 && self.x > self.indent && self.x + fit_width > self.max_w {
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
                self.row.min_height = self.row.min_height.max(row_height(item.font_size));
                self.row.add_centered(CHECKBOX_SIZE);
            }
            ItemKind::Math { height, .. } => {
                self.row.add_centered(*height);
                self.row.has_math = true;
            }
            // Concealed text still holds its row open at its own size, so an
            // empty line is as tall and as aligned as a line of text.
            ItemKind::Hidden | ItemKind::Text(_) => self.row.add_text(item.font_size, item.font),
        }
        self.push(item);
    }

    /// Place a span's visible text, one break opportunity at a time.
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
        let map = SourceMap::of(&span.text, text);
        let font = span_font(span, line);
        let font_size = span.font_size;
        let total_chars = text.chars().count();

        let units = if map == SourceMap::Atomic {
            vec![Word {
                bytes: 0..text.len(),
                first_char: 0,
                char_count: total_chars,
            }]
        } else {
            words(text)
        };

        for word in units {
            let segment = &text[word.bytes.clone()];
            let offsets = word_offsets::<R>(segment, font_size, font);
            let n = word.char_count;
            let make = |base: usize, chars: usize, byte_range: Range<usize>| {
                let part = TextPart {
                    revealed,
                    bytes: byte_range,
                    first_char: word.first_char + base,
                    span_start_col,
                    span_end_col,
                    map,
                    font,
                    offsets: offsets.clone(),
                    base,
                };
                let start_col = part.col_of(part.first_char);
                let end_col = if part.first_char + chars == total_chars {
                    span_end_col
                } else {
                    part.col_of(part.first_char + chars)
                };
                Item {
                    span_idx,
                    row: 0,
                    x: 0.0,
                    advance: offsets[base + chars] - offsets[base],
                    font_size,
                    font,
                    start_col,
                    end_col,
                    kind: ItemKind::Text(part),
                }
            };

            // Trailing whitespace hangs, so only the ink has to fit.
            let trailing = segment
                .chars()
                .rev()
                .take_while(|c| c.is_whitespace())
                .count();
            let ink_w = offsets[n - trailing];

            if ink_w <= self.max_w - self.indent || map == SourceMap::Atomic || n <= 1 {
                self.fit(ink_w);
                self.row.add_text(font_size, font);
                self.push(make(0, n, word.bytes.clone()));
                continue;
            }

            // Wider than a row: start it on a fresh row, then break between
            // grapheme clusters.
            self.fit(ink_w);
            let bytes: Vec<usize> = segment
                .char_indices()
                .map(|(b, _)| word.bytes.start + b)
                .chain(std::iter::once(word.bytes.end))
                .collect();
            let is_space: Vec<bool> = segment.chars().map(char::is_whitespace).collect();
            let mut base = 0;
            for k in 1..n {
                let cluster_boundary = offsets[k] > offsets[k - 1];
                let overflows = self.x + (offsets[k + 1] - offsets[base]) > self.max_w;
                if cluster_boundary && overflows && !is_space[k] {
                    self.row.add_text(font_size, font);
                    self.push(make(base, k - base, bytes[base]..bytes[k]));
                    self.break_row();
                    base = k;
                }
            }
            self.row.add_text(font_size, font);
            self.push(make(base, n - base, bytes[base]..bytes[n]));
        }
    }

    fn finish<'a>(mut self, line: &'a StyledLine) -> Flow<'a> {
        if self.items.is_empty() {
            self.row.add_text(DEFAULT_FONT_SIZE, iced::Font::DEFAULT);
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

/// Split `text` at its Unicode line-break opportunities (UAX #14). Each
/// piece carries the whitespace after it.
fn words(text: &str) -> Vec<Word> {
    let mut words = Vec::new();
    let (mut start, mut start_char) = (0, 0);
    for (end, _) in unicode_linebreak::linebreaks(text) {
        if end <= start {
            continue;
        }
        let char_count = text[start..end].chars().count();
        words.push(Word {
            bytes: start..end,
            first_char: start_char,
            char_count,
        });
        start = end;
        start_char += char_count;
    }
    words
}

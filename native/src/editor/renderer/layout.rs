//! Line heights.
//!
//! Every line's height is decided here, and the same numbers feed the
//! widget's size, the height tree used for culling and hit-testing, and
//! [`line_visual_y`], which the app uses to scroll a line into view.
//!
//! A line's total extent is its body plus two extras: a caption band above
//! the first line of a code block or table, and a scrollbar gutter below the
//! last row of a table.

use std::collections::{HashMap, HashSet};

use super::measure::{measure_char_width, measure_width, span_font};
use super::metrics::*;
use super::spans::{is_block_editing_line, math_source, span_is_editing, span_visible_text};
use super::{Editor, ImageCache, MATH_BLOCK_SCALE, MathCache, Measure, State};
use crate::editor::highlight::StyledLine;
use crate::editor::layout_cache::{LineHeightCache, line_hash, resource_hash};
use crate::editor::layout_tree::HeightTree;

/// Height of a line's body, excluding its caption band and gutter.
///
/// `seen_math_blocks` must be shared across a document-order walk: a math
/// block is drawn as one unit, so only its first visible line gets height.
pub(super) fn line_height_for<R: Measure>(
    line: &StyledLine,
    image_cache: &ImageCache,
    math_cache: &MathCache,
    available_width: f32,
    is_editing: bool,
    active_col: Option<usize>,
    seen_math_blocks: &mut HashSet<usize>,
) -> f32 {
    if let Some(span) = line.spans.iter().find(|s| s.is_image) {
        if let Some(path) = &span.image_path
            && let Some((_, w, h)) = image_cache.get(path)
        {
            let max_w = text_column_width(available_width);
            let scale = if *w > max_w { max_w / w } else { 1.0 };
            return (h * scale) + IMAGE_CAPTION_SPACE;
        }
        return IMAGE_PLACEHOLDER_HEIGHT;
    }

    if line.is_math_block {
        return math_block_line_height(line, math_cache, is_editing, seen_math_blocks);
    }

    if line.is_code_block {
        return CODE_LINE_HEIGHT;
    }

    if line.is_table_row {
        if is_editing {
            return measured_inline_height::<R>(
                line,
                math_cache,
                available_width,
                is_editing,
                active_col,
            );
        }
        // Separator rows (`|---|`) have no cells and collapse.
        return if line.table_cells.is_empty() {
            0.0
        } else {
            TABLE_ROW_HEIGHT
        };
    }

    let height =
        measured_inline_height::<R>(line, math_cache, available_width, is_editing, active_col);
    if line.spans.iter().any(|s| s.is_math) {
        height + INLINE_MATH_EXTRA_HEIGHT
    } else {
        height
    }
}

fn math_block_line_height(
    line: &StyledLine,
    math_cache: &MathCache,
    is_editing: bool,
    seen_math_blocks: &mut HashSet<usize>,
) -> f32 {
    if is_editing {
        return BASE_LINE_HEIGHT;
    }

    let has_visible_math = line.spans.iter().any(|span| !math_source(span).is_empty());
    if !has_visible_math || !seen_math_blocks.insert(line.block_id) {
        return 0.0;
    }

    let mut max_h: f32 = MATH_BLOCK_MIN_HEIGHT;
    for span in &line.spans {
        let tex = math_source(span);
        if let Some(m) = math_cache.get(tex) {
            max_h = max_h.max(m.height * MATH_BLOCK_SCALE + MATH_BLOCK_PADDING);
        } else if !tex.is_empty() {
            // Not rendered yet: size for the source, wrapped at a nominal width.
            let visual_lines = tex
                .lines()
                .map(|line| {
                    (line.chars().count() as f32 / MATH_SOURCE_CHARS_PER_ROW)
                        .ceil()
                        .max(1.0)
                })
                .sum::<f32>()
                .max(1.0);
            max_h = max_h.max(visual_lines * BASE_LINE_HEIGHT + MATH_BLOCK_PADDING);
        }
    }
    max_h
}

/// Greedy row filling for measuring a wrapped line. Rows start at x = 0.
struct RowFill {
    x: f32,
    y: f32,
    /// Height of the current row: the tallest step placed on it so far.
    row_step: f32,
    right: f32,
}

impl RowFill {
    /// Start a new row first if `width` does not fit on the current one.
    fn fit(&mut self, width: f32, step: f32) {
        if self.x > 0.0 && self.x + width > self.right {
            self.y += self.row_step;
            self.x = 0.0;
            self.row_step = step;
        }
    }

    /// Place a whitespace-delimited token, breaking inside it only when it is
    /// wider than a whole row.
    fn place_token<R: Measure>(
        &mut self,
        token: &str,
        font_size: f32,
        font: iced::Font,
        step: f32,
    ) {
        if token.is_empty() {
            return;
        }

        let width = measure_width::<R>(token, font_size, font);
        self.fit(width, step);

        if width <= self.right.max(1.0) {
            self.x += width;
        } else {
            for ch in token.chars() {
                let ch_w = measure_char_width::<R>(ch, font_size, font);
                self.fit(ch_w, step);
                self.x += ch_w;
            }
        }
        self.row_step = self.row_step.max(step);
    }
}

/// Height of an inline line after wrapping its spans at the text column.
fn measured_inline_height<R: Measure>(
    line: &StyledLine,
    math_cache: &MathCache,
    available_width: f32,
    is_editing: bool,
    active_col: Option<usize>,
) -> f32 {
    let mut fill = RowFill {
        x: 0.0,
        y: 0.0,
        row_step: BASE_LINE_HEIGHT,
        right: wrap_width(available_width),
    };

    for (span_idx, span) in line.spans.iter().enumerate() {
        let fs = span.font_size;
        let step = visual_line_step(fs);
        fill.row_step = fill.row_step.max(step);
        let span_editing = span_is_editing(line, span_idx, is_editing, active_col);

        if span.is_checkbox && !span_editing {
            fill.fit(CHECKBOX_ADVANCE, step);
            fill.x += CHECKBOX_ADVANCE;
            continue;
        }

        if span.is_math && !span_editing {
            let tex = math_source(span);
            if tex.is_empty() || span.is_syntax {
                continue;
            }
            let (width, height) = math_cache
                .get(tex)
                .map(|m| (m.width, m.height))
                .unwrap_or_else(|| {
                    (
                        measure_width::<R>(tex, fs, span_font(span, line)),
                        BASE_LINE_HEIGHT,
                    )
                });
            let extra_h = (height - BASE_LINE_HEIGHT).max(0.0);
            fill.row_step = fill.row_step.max(BASE_LINE_HEIGHT + extra_h);
            fill.fit(width, step);
            fill.x += width + INLINE_MATH_GAP;
            continue;
        }

        let display = span_visible_text(line, span_idx, is_editing, active_col);
        if display.is_empty() {
            continue;
        }

        let font = span_font(span, line);
        let mut token = String::new();
        for ch in display.chars() {
            token.push(ch);
            if ch.is_whitespace() {
                fill.place_token::<R>(&token, fs, font, step);
                token.clear();
            }
        }
        fill.place_token::<R>(&token, fs, font, step);
    }

    (fill.y + fill.row_step).max(BASE_LINE_HEIGHT)
}

/// Scrollbar gutter reserved below line `line_idx`: only after the last row of
/// a table that is not being edited.
pub(super) fn table_block_gutter_after(
    lines: &[StyledLine],
    line_idx: usize,
    is_editing: bool,
) -> f32 {
    let Some(line) = lines.get(line_idx) else {
        return 0.0;
    };
    if is_editing || !line.is_table_row {
        return 0.0;
    }
    let next_same_table_block = lines
        .get(line_idx + 1)
        .is_some_and(|next| next.is_table_row && next.block_id == line.block_id);
    if next_same_table_block {
        0.0
    } else {
        HORIZONTAL_SCROLLBAR_GUTTER
    }
}

/// Whether line `line_idx` starts its block.
pub(super) fn is_first_block_line(lines: &[StyledLine], line_idx: usize) -> bool {
    let Some(line) = lines.get(line_idx) else {
        return false;
    };
    !lines
        .get(line_idx.saturating_sub(1))
        .is_some_and(|prev| prev.block_id == line.block_id)
}

/// Caption band above a line: present on the first line of a code block or
/// table, unless the block is being edited.
pub(super) fn caption_band(opens_block: bool, is_editing: bool) -> f32 {
    if opens_block && !is_editing {
        BLOCK_CAPTION_HEIGHT
    } else {
        0.0
    }
}

/// The vertical pieces of one line, top to bottom.
pub(super) struct LineExtent {
    pub caption: f32,
    pub body: f32,
    pub gutter: f32,
}

/// A document-order pass over lines, carrying the per-block memory that makes
/// a line's height depend on the lines before it.
pub(super) struct HeightWalk<'a> {
    image_cache: &'a ImageCache,
    math_cache: &'a MathCache,
    width: f32,
    seen_math_blocks: HashSet<usize>,
    seen_code_blocks: HashSet<usize>,
    seen_table_blocks: HashSet<usize>,
}

impl<'a> HeightWalk<'a> {
    pub fn new(image_cache: &'a ImageCache, math_cache: &'a MathCache, width: f32) -> Self {
        Self {
            image_cache,
            math_cache,
            width,
            seen_math_blocks: HashSet::new(),
            seen_code_blocks: HashSet::new(),
            seen_table_blocks: HashSet::new(),
        }
    }

    /// Whether `line` is the first line seen of a code block or table.
    /// Must be called for every line, in order.
    pub fn opens_block(&mut self, line: &StyledLine) -> bool {
        (line.is_code_block && self.seen_code_blocks.insert(line.block_id))
            || (line.is_table_row && self.seen_table_blocks.insert(line.block_id))
    }

    pub fn body_height<R: Measure>(
        &mut self,
        line: &StyledLine,
        is_editing: bool,
        active_col: Option<usize>,
    ) -> f32 {
        line_height_for::<R>(
            line,
            self.image_cache,
            self.math_cache,
            self.width,
            is_editing,
            active_col,
            &mut self.seen_math_blocks,
        )
    }

    /// Account for a line whose height was reused from cache instead of
    /// going through [`Self::body_height`].
    pub fn skip_cached(&mut self, line: &StyledLine) {
        if line.is_math_block {
            self.seen_math_blocks.insert(line.block_id);
        }
    }

    pub fn extent<R: Measure>(
        &mut self,
        lines: &[StyledLine],
        idx: usize,
        is_editing: bool,
        active_col: Option<usize>,
    ) -> LineExtent {
        let line = &lines[idx];
        let caption = caption_band(self.opens_block(line), is_editing);
        let body = self.body_height::<R>(line, is_editing, active_col);
        let gutter = table_block_gutter_after(lines, idx, is_editing);
        LineExtent {
            caption,
            body,
            gutter,
        }
    }
}

/// Total document height in pixels.
pub(super) fn total_height<R: Measure>(
    lines: &[StyledLine],
    image_cache: &ImageCache,
    math_cache: &MathCache,
    width: f32,
    active_block_id: Option<usize>,
    active_cursor: Option<(usize, usize)>,
    focused: bool,
) -> f32 {
    let mut walk = HeightWalk::new(image_cache, math_cache, width);
    let mut h = TOP_PAD;
    for idx in 0..lines.len() {
        let is_editing = is_block_editing_line(&lines[idx], active_block_id, focused);
        let active_col = active_cursor
            .filter(|(line_idx, _)| *line_idx == idx)
            .map(|(_, col)| col);
        let extent = walk.extent::<R>(lines, idx, is_editing, active_col);
        h += extent.caption;
        h += extent.body;
        h += extent.gutter;
    }
    h + BOTTOM_PAD
}

/// Widget-relative y of the top of `target_line`, for a caret at
/// (`active_line`, `active_col`).
#[allow(clippy::too_many_arguments)]
pub fn line_visual_y<R>(
    lines: &[StyledLine],
    image_cache: &ImageCache,
    math_cache: &MathCache,
    available_width: f32,
    active_line: usize,
    active_col: usize,
    target_line: usize,
    focused: bool,
) -> f32
where
    R: iced::advanced::text::Renderer<Font = iced::Font>,
{
    let active_block_id = lines.get(active_line).map(|line| line.block_id);
    let mut walk = HeightWalk::new(image_cache, math_cache, available_width);
    let mut y = TOP_PAD;

    for idx in 0..lines.len().min(target_line) {
        let is_editing = is_block_editing_line(&lines[idx], active_block_id, focused);
        let line_active_col = (focused && idx == active_line).then_some(active_col);
        let extent = walk.extent::<R>(lines, idx, is_editing, line_active_col);
        y += extent.caption;
        y += extent.body;
        y += extent.gutter;
    }

    y
}

/// Extend the recorded line range of `line`'s block to include `idx`.
fn record_block_range(ranges: &mut HashMap<usize, (usize, usize)>, line: &StyledLine, idx: usize) {
    if line.is_code_block || line.is_table_row || line.is_math_block || line.is_blockquote {
        ranges
            .entry(line.block_id)
            .and_modify(|(_, end)| *end = idx)
            .or_insert((idx, idx));
    }
}

impl<Message> Editor<'_, Message> {
    /// Refresh the height tree and block ranges for the current lines, reusing
    /// cached heights where nothing that affects them changed. Returns the
    /// widget height.
    pub(super) fn layout_lines<R: Measure>(&self, state: &mut State, max_width: f32) -> f32 {
        let focused = state.is_focused;
        let n = self.lines.len();
        if state.layout_tree.len() != n {
            state.layout_tree = HeightTree::new(n);
        }
        if state.line_height_cache.len() != n {
            state.line_height_cache = vec![LineHeightCache::default(); n];
        }
        if (state.last_layout_width - max_width).abs() > 0.5 {
            for cache in &mut state.line_height_cache {
                cache.valid = false;
            }
            state.last_layout_width = max_width;
        }

        let mut walk = HeightWalk::new(self.image_cache, self.math_cache, max_width);
        state.block_ranges.clear();

        for (i, line) in self.lines.iter().enumerate() {
            let is_editing = self.is_block_editing(line, focused);
            let active_col = self.active_col(i, focused);
            let caption = caption_band(walk.opens_block(line), is_editing);
            let gutter = table_block_gutter_after(self.lines, i, is_editing);

            let hash = line_hash(line) ^ resource_hash(line, self.image_cache, self.math_cache);
            let cached = state.line_height_cache[i];
            let line_total = if cached.valid
                && cached.hash == hash
                && cached.is_editing == is_editing
                && cached.active_col == active_col
            {
                walk.skip_cached(line);
                cached.height
            } else {
                let height = caption + walk.body_height::<R>(line, is_editing, active_col) + gutter;
                state.line_height_cache[i] = LineHeightCache {
                    hash,
                    is_editing,
                    active_col,
                    height,
                    valid: true,
                };
                height
            };

            state.layout_tree.update_height(i, line_total);
            record_block_range(&mut state.block_ranges, line, i);
        }

        TOP_PAD + state.layout_tree.prefix_sum(n) + BOTTOM_PAD
    }

    /// Rebuild the height tree from scratch, bypassing the height cache. For
    /// callers that may run before the first layout.
    pub(super) fn rebuild_layout_tree<R: Measure>(&self, state: &mut State, available_width: f32) {
        state.layout_tree.resize(self.lines.len());
        state.block_ranges.clear();
        let mut walk = HeightWalk::new(self.image_cache, self.math_cache, available_width);

        for (i, line) in self.lines.iter().enumerate() {
            let is_editing = self.is_block_editing(line, state.is_focused);
            let active_col = self.active_col(i, state.is_focused);
            let extent = walk.extent::<R>(self.lines, i, is_editing, active_col);
            state
                .layout_tree
                .update_height(i, extent.caption + extent.body + extent.gutter);
            record_block_range(&mut state.block_ranges, line, i);
        }
    }

    /// Line under a widget-relative y.
    pub(super) fn line_at_widget_y(&self, y: f32, state: &State) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        let relative_y = (y - TOP_PAD).max(0.0);
        Some(state.layout_tree.find_line_at_y(relative_y))
    }

    /// Widget-relative y of the top of a line.
    pub(super) fn widget_y_for_line(&self, line_idx: usize, state: &State) -> f32 {
        TOP_PAD + state.layout_tree.prefix_sum(line_idx.min(self.lines.len()))
    }
}

/// Widget-relative top and total height of lines `start..=end`.
pub(super) fn lines_extent(state: &State, start: usize, end: usize) -> (f32, f32) {
    let top = state.layout_tree.prefix_sum(start);
    let height = state.layout_tree.prefix_sum(end.saturating_add(1)) - top;
    (TOP_PAD + top, height)
}

#[cfg(test)]
mod tests {
    use super::super::MathRender;
    use super::super::test_support::make_line;
    use super::*;
    use crate::editor::highlight::StyledSpan;

    #[test]
    fn table_scrollbar_gutter_is_reserved_only_after_last_table_row() {
        let mut first = make_line(1, vec![]);
        first.is_table_row = true;
        let mut second = make_line(1, vec![]);
        second.is_table_row = true;
        let plain = make_line(2, vec![StyledSpan::plain("after")]);
        let lines = vec![first, second, plain];

        assert_eq!(table_block_gutter_after(&lines, 0, false), 0.0);
        assert_eq!(
            table_block_gutter_after(&lines, 1, false),
            HORIZONTAL_SCROLLBAR_GUTTER
        );
        assert_eq!(table_block_gutter_after(&lines, 1, true), 0.0);
        assert_eq!(table_block_gutter_after(&lines, 2, false), 0.0);
    }

    #[test]
    fn inactive_plain_line_height_does_not_create_cursorless_blank_gap() {
        let line = make_line(1, vec![StyledSpan::plain("short line")]);
        let image_cache = HashMap::new();
        let math_cache = HashMap::new();
        let mut seen_math_blocks = HashSet::new();

        let h = line_height_for::<iced::Renderer>(
            &line,
            &image_cache,
            &math_cache,
            900.0,
            false,
            None,
            &mut seen_math_blocks,
        );
        assert_eq!(h, BASE_LINE_HEIGHT);
    }

    #[test]
    fn line_visual_y_includes_single_table_scrollbar_gutter() {
        let mut header = make_line(1, vec![]);
        header.is_table_row = true;
        header.table_cells = vec![vec![StyledSpan::plain("A")], vec![StyledSpan::plain("B")]];
        let mut body = make_line(1, vec![]);
        body.is_table_row = true;
        body.table_cells = vec![vec![StyledSpan::plain("1")], vec![StyledSpan::plain("2")]];
        let after = make_line(2, vec![StyledSpan::plain("after")]);
        let lines = vec![header, body, after];
        let image_cache = HashMap::new();
        let math_cache = HashMap::new();

        let y_after_table = line_visual_y::<iced::Renderer>(
            &lines,
            &image_cache,
            &math_cache,
            900.0,
            0,
            0,
            2,
            false,
        );

        assert_eq!(
            y_after_table,
            TOP_PAD
                + BLOCK_CAPTION_HEIGHT
                + TABLE_ROW_HEIGHT
                + TABLE_ROW_HEIGHT
                + HORIZONTAL_SCROLLBAR_GUTTER
        );
    }

    #[test]
    fn test_renderer_line_height_permutations() {
        let mut lines = Vec::new();

        // 1. Plain text line
        lines.push(make_line(1, vec![StyledSpan::plain("Hello world")]));

        // 2. Code block line
        let mut code_line = make_line(
            2,
            vec![StyledSpan {
                text: "let x = 10;".to_string(),
                is_code: true,
                ..StyledSpan::plain("")
            }],
        );
        code_line.is_code_block = true;
        lines.push(code_line);

        // 3. Math block line (not editing)
        let mut math_line = make_line(
            3,
            vec![StyledSpan {
                text: "$$ E = mc^2 $$".to_string(),
                is_math: true,
                ..StyledSpan::plain("")
            }],
        );
        math_line.is_math_block = true;
        lines.push(math_line);

        // 4. Table row
        let mut table_line = make_line(4, vec![]);
        table_line.is_table_row = true;
        table_line.table_cells = vec![
            vec![StyledSpan::plain("Col A")],
            vec![StyledSpan::plain("Col B")],
        ];
        lines.push(table_line);

        // 5. Image line
        let img_line = make_line(
            5,
            vec![StyledSpan {
                text: "![alt](image.png)".to_string(),
                is_image: true,
                image_path: Some("image.png".to_string()),
                ..StyledSpan::plain("")
            }],
        );
        lines.push(img_line);

        // 6. Deep quote line
        let mut quote_line = make_line(6, vec![StyledSpan::plain("A quote")]);
        quote_line.is_blockquote = true;
        lines.push(quote_line);

        let mut image_cache = HashMap::new();
        let mut math_cache = HashMap::new();

        image_cache.insert(
            "image.png".to_string(),
            (
                iced::widget::image::Handle::from_rgba(10, 10, vec![0; 400]),
                400.0,
                300.0,
            ),
        );
        math_cache.insert(
            "E = mc^2".to_string(),
            MathRender {
                inline_handle: iced::widget::image::Handle::from_rgba(10, 10, vec![0; 400]),
                block_handle: iced::widget::image::Handle::from_rgba(10, 10, vec![0; 400]),
                width: 200.0,
                height: 50.0,
            },
        );

        let widths = vec![100.0, 200.0, 400.0, 600.0, 800.0, 1000.0, 1200.0];
        let mut seen_math_blocks = HashSet::new();

        for &width in &widths {
            for &is_editing in &[true, false] {
                for line in &lines {
                    seen_math_blocks.clear();
                    let h = line_height_for::<iced::Renderer>(
                        line,
                        &image_cache,
                        &math_cache,
                        width,
                        is_editing,
                        None,
                        &mut seen_math_blocks,
                    );

                    assert!(h >= 0.0);

                    if line.is_table_row {
                        if is_editing {
                            assert!(h >= BASE_LINE_HEIGHT);
                        } else {
                            assert_eq!(h, TABLE_ROW_HEIGHT);
                        }
                    } else if line.is_math_block && is_editing {
                        assert_eq!(h, BASE_LINE_HEIGHT);
                    } else if line.is_blockquote {
                        assert!(h > 0.0);
                    }
                }
            }
        }
    }

    #[test]
    fn test_renderer_total_height_accumulation() {
        let mut lines = Vec::new();
        for i in 1..=200 {
            lines.push(make_line(
                i,
                vec![StyledSpan::plain("Hello accumulated document")],
            ));
        }

        let image_cache = HashMap::new();
        let math_cache = HashMap::new();
        let height = |lines: &[StyledLine], width: f32| {
            total_height::<iced::Renderer>(
                lines,
                &image_cache,
                &math_cache,
                width,
                None,
                None,
                false,
            )
        };

        // 1. Verify adding lines monotonically increases total height
        let h1 = height(&lines[0..50], 800.0);
        let h2 = height(&lines[0..100], 800.0);
        let h3 = height(&lines[0..200], 800.0);

        assert!(h2 > h1);
        assert!(h3 > h2);

        // 2. Verify width decreases wrapping space and monotonically increases total height
        let h_wide = height(&lines, 1000.0);
        let h_narrow = height(&lines, 200.0);

        assert!(h_narrow >= h_wide);
    }

    #[test]
    fn test_bug_finder_renderer_extreme_dimensions() {
        let line = make_line(
            1,
            vec![StyledSpan::plain(
                "Wrap this extremely long sentence with extreme layout boundary dimensions to find bugs.",
            )],
        );
        let image_cache = HashMap::new();
        let math_cache = HashMap::new();
        let mut seen_math_blocks = HashSet::new();

        // Extreme layout widths (0, negative, infinite, sub-pixel)
        let extreme_widths = vec![
            0.0,
            -100.0,
            -0.0001,
            0.0001,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];
        for &width in &extreme_widths {
            seen_math_blocks.clear();
            let h = line_height_for::<iced::Renderer>(
                &line,
                &image_cache,
                &math_cache,
                width,
                false,
                None,
                &mut seen_math_blocks,
            );
            assert!(h >= 0.0);
        }
    }
}

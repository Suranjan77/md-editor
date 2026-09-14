//! Line heights.
//!
//! Every line's height is decided here, and the same numbers feed the
//! widget's size and the height tree used for culling and hit testing. The
//! uncached model behind `line_visual_y` and `total_height` exists only for
//! tests to check cached layout against.
//!
//! A line's extent, top to bottom: the margin above it, a caption band (first
//! line of a code block or table), its body, and a scrollbar gutter (last row
//! of a table).
//!
//! Margins follow typographic rhythm. Each line asks for space above and
//! below by its kind — headings more above than below, blocks a little on
//! both sides — and adjacent requests collapse: the space between two lines is
//! the larger of the two, never their sum. Blank lines are paragraph gaps
//! that count toward that space. The whole margin is realized above the lower
//! line, so every block's box starts exactly at its content.

use std::collections::{HashMap, HashSet};

use super::caret::code_x_for_col;
use super::flow::Flow;
use super::metrics::*;
#[cfg(test)]
use super::spans::is_block_editing_line;
use super::spans::{math_source, span_is_editing};
use super::{Editor, ImageCache, MATH_BLOCK_SCALE, MathCache, Measure, State};
use crate::editor::highlight::StyledLine;
use crate::editor::layout_cache::{LineHeightCache, line_hash, resource_hash};
use crate::editor::layout_tree::HeightTree;

/// Height of a line's body, excluding its caption band and gutter.
///
/// `seen_math_blocks` must be shared across a document-order walk: a math
/// block is drawn as one unit, so only its first visible line gets height.
#[cfg(test)]
pub(super) fn line_height_for<R: Measure>(
    line: &StyledLine,
    image_cache: &ImageCache,
    math_cache: &MathCache,
    available_width: f32,
    is_editing: bool,
    active_col: Option<usize>,
    seen_math_blocks: &mut HashSet<usize>,
) -> f32 {
    let math_head = claims_math_head(line, is_editing, seen_math_blocks);
    line_body_height::<R>(
        line,
        image_cache,
        math_cache,
        available_width,
        is_editing,
        active_col,
        math_head,
    )
}

/// Whether `line` carries its math block's height: the first line of a math
/// block, not being edited, with visible math. Records the claim in
/// `seen_math_blocks`.
fn claims_math_head(
    line: &StyledLine,
    is_editing: bool,
    seen_math_blocks: &mut HashSet<usize>,
) -> bool {
    line.is_math_block
        && !is_editing
        && line.spans.iter().any(|span| !math_source(span).is_empty())
        && seen_math_blocks.insert(line.block_id)
}

/// Height of a line's body: a pure function of the line and its arguments.
fn line_body_height<R: Measure>(
    line: &StyledLine,
    image_cache: &ImageCache,
    math_cache: &MathCache,
    available_width: f32,
    is_editing: bool,
    active_col: Option<usize>,
    math_head: bool,
) -> f32 {
    let flow_height =
        || Flow::build::<R>(line, math_cache, available_width, is_editing, active_col).height();

    if let Some((idx, span)) = line.spans.iter().enumerate().find(|(_, s)| s.is_image) {
        let image_height = match span.image_path.as_ref().and_then(|p| image_cache.get(p)) {
            Some((_, w, h)) => {
                let max_w = text_column_width(available_width);
                let scale = if *w > max_w && *w > 0.0 {
                    max_w / w
                } else {
                    1.0
                };
                snap_up(h * scale + IMAGE_CAPTION_SPACE)
            }
            None => IMAGE_PLACEHOLDER_HEIGHT,
        };
        // While its source is showing the line keeps the image's height, so
        // moving the caret onto it doesn't make the document jump.
        return if span_is_editing(line, idx, is_editing, active_col) {
            image_height.max(flow_height())
        } else {
            image_height
        };
    }

    if line.is_math_block {
        if is_editing {
            return flow_height();
        }
        return if math_head {
            math_block_height(line, math_cache)
        } else {
            0.0
        };
    }

    if line.is_code_block {
        return row_height(CODE_FONT_SIZE);
    }

    if line.is_table_row && !is_editing {
        // Separator rows (`|---|`) have no cells and collapse.
        return if line.table_cells.is_empty() {
            0.0
        } else {
            TABLE_ROW_HEIGHT
        };
    }

    flow_height()
}

/// Height of a math block that is not being edited, carried by its first line.
fn math_block_height(line: &StyledLine, math_cache: &MathCache) -> f32 {
    let mut max_h: f32 = MATH_BLOCK_MIN_HEIGHT;
    for span in &line.spans {
        let tex = math_source(span);
        if let Some(m) = math_cache.get(tex) {
            max_h = max_h.max(snap_up(m.height * MATH_BLOCK_SCALE + MATH_BLOCK_PADDING));
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
            max_h =
                max_h.max(visual_lines * row_height(MATH_SOURCE_FONT_SIZE) + MATH_BLOCK_PADDING);
        }
    }
    max_h
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

/// Caption band above a line: present on the first line of a code block or
/// table, whether or not it is being edited, so entering a block doesn't
/// move it.
pub(super) fn caption_band(opens_block: bool) -> f32 {
    if opens_block {
        BLOCK_CAPTION_HEIGHT
    } else {
        0.0
    }
}

/// A line's vertical margins as decided by layout.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct LineMargins {
    /// Realized space above the line, after collapsing.
    pub top: f32,
    pub caption: f32,
    pub gutter: f32,
}

/// The vertical pieces of one line, top to bottom.
pub(super) struct LineExtent {
    pub margins: LineMargins,
    pub body: f32,
}

impl LineExtent {
    pub fn total(&self) -> f32 {
        self.margins.top + self.margins.caption + self.body + self.margins.gutter
    }
}

/// Whether a line is blank: a paragraph break.
fn is_blank(line: &StyledLine) -> bool {
    !line.is_code_block
        && !line.is_math_block
        && !line.is_table_row
        && line.spans.iter().all(|span| span.text.trim().is_empty())
}

/// Space a line asks for above and below it, by its kind.
fn requested_margins(lines: &[StyledLine], idx: usize) -> (f32, f32) {
    let line = &lines[idx];
    let prev = idx.checked_sub(1).and_then(|i| lines.get(i));
    let next = lines.get(idx + 1);

    if let Some(level) = line
        .spans
        .iter()
        .find(|span| span.is_heading)
        .map(|span| span.heading_level)
    {
        return HEADING_MARGINS[(level.clamp(1, 6) - 1) as usize];
    }
    if line.is_code_block || line.is_math_block || line.is_table_row {
        let starts = prev.is_none_or(|p| p.block_id != line.block_id);
        let ends = next.is_none_or(|n| n.block_id != line.block_id);
        return (
            if starts { BLOCK_MARGIN } else { 0.0 },
            if ends { BLOCK_MARGIN } else { 0.0 },
        );
    }
    if line.is_blockquote {
        let starts = prev.is_none_or(|p| !p.is_blockquote);
        let ends = next.is_none_or(|n| !n.is_blockquote);
        return (
            if starts { BLOCK_MARGIN } else { 0.0 },
            if ends { BLOCK_MARGIN } else { 0.0 },
        );
    }
    if line.spans.iter().any(|span| span.is_image || span.is_rule) {
        return (BLOCK_MARGIN, BLOCK_MARGIN);
    }
    (0.0, 0.0)
}

/// Collapses margins through a document-order pass.
#[derive(Default)]
struct MarginWalk {
    /// Bottom margin requested by the last non-blank line.
    pending_bottom: f32,
    /// Blank-line space since that line.
    blank_since: f32,
    /// Whether a non-blank line has been seen; the first gets no top margin.
    seen_content: bool,
}

impl MarginWalk {
    /// The realized top margin of line `idx`, whose body is `body` tall.
    fn top_margin(&mut self, lines: &[StyledLine], idx: usize, body: f32) -> f32 {
        if is_blank(&lines[idx]) {
            self.blank_since += body;
            return 0.0;
        }
        let (top, bottom) = requested_margins(lines, idx);
        let realized = if self.seen_content {
            (top.max(self.pending_bottom) - self.blank_since).max(0.0)
        } else {
            0.0
        };
        self.pending_bottom = bottom;
        self.blank_since = 0.0;
        self.seen_content = true;
        realized
    }
}

/// A document-order pass over lines, carrying the memory that makes a line's
/// extent depend on the lines before it.
#[derive(Default)]
pub(super) struct HeightWalk {
    seen_math_blocks: HashSet<usize>,
    seen_code_blocks: HashSet<usize>,
    seen_table_blocks: HashSet<usize>,
    margins: MarginWalk,
}

impl HeightWalk {
    /// Whether `line` is the first line seen of a code block or table.
    /// Must be called for every line, in order.
    pub fn opens_block(&mut self, line: &StyledLine) -> bool {
        (line.is_code_block && self.seen_code_blocks.insert(line.block_id))
            || (line.is_table_row && self.seen_table_blocks.insert(line.block_id))
    }

    /// See [`claims_math_head`]. Must be called for every line, in order.
    pub fn claims_math_head(&mut self, line: &StyledLine, is_editing: bool) -> bool {
        claims_math_head(line, is_editing, &mut self.seen_math_blocks)
    }

    /// Margins of line `idx` given its body height. Must be called for every
    /// line, in order, after [`Self::opens_block`].
    pub fn margins(
        &mut self,
        lines: &[StyledLine],
        idx: usize,
        body: f32,
        opens_block: bool,
        is_editing: bool,
    ) -> LineMargins {
        LineMargins {
            top: self.margins.top_margin(lines, idx, body),
            caption: caption_band(opens_block),
            gutter: table_block_gutter_after(lines, idx, is_editing),
        }
    }

    /// Line `idx`'s extent measured from scratch, with no cache: the model
    /// that cached layout is checked against.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub fn extent<R: Measure>(
        &mut self,
        lines: &[StyledLine],
        idx: usize,
        is_editing: bool,
        active_col: Option<usize>,
        image_cache: &ImageCache,
        math_cache: &MathCache,
        width: f32,
    ) -> LineExtent {
        let line = &lines[idx];
        let opens = self.opens_block(line);
        let math_head = self.claims_math_head(line, is_editing);
        let body = line_body_height::<R>(
            line,
            image_cache,
            math_cache,
            width,
            is_editing,
            active_col,
            math_head,
        );
        let margins = self.margins(lines, idx, body, opens, is_editing);
        LineExtent { margins, body }
    }
}

#[cfg(test)]
thread_local! {
    /// Full layouts run on this thread, to check they are skipped when
    /// nothing changed.
    pub(super) static LAYOUTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Total document height in pixels, measured from scratch.
#[cfg(test)]
pub(super) fn total_height<R: Measure>(
    lines: &[StyledLine],
    image_cache: &ImageCache,
    math_cache: &MathCache,
    width: f32,
    active_block_id: Option<usize>,
    active_cursor: Option<(usize, usize)>,
    focused: bool,
) -> f32 {
    let mut walk = HeightWalk::default();
    let mut h = TOP_PAD;
    for idx in 0..lines.len() {
        let is_editing = is_block_editing_line(&lines[idx], active_block_id, focused);
        let active_col = active_cursor
            .filter(|(line_idx, _)| *line_idx == idx)
            .map(|(_, col)| col);
        h += walk
            .extent::<R>(
                lines,
                idx,
                is_editing,
                active_col,
                image_cache,
                math_cache,
                width,
            )
            .total();
    }
    h + BOTTOM_PAD
}

/// Widget-relative y of the top of `target_line`, for a caret at
/// (`active_line`, `active_col`).
#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(crate) fn line_visual_y<R>(
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
    let mut walk = HeightWalk::default();
    let mut y = TOP_PAD;

    for idx in 0..lines.len().min(target_line) {
        let is_editing = is_block_editing_line(&lines[idx], active_block_id, focused);
        let line_active_col = (focused && idx == active_line).then_some(active_col);
        y += walk
            .extent::<R>(
                lines,
                idx,
                is_editing,
                line_active_col,
                image_cache,
                math_cache,
                available_width,
            )
            .total();
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
        #[cfg(test)]
        LAYOUTS.with(|count| count.set(count.get() + 1));
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

        let mut walk = HeightWalk::default();
        state.block_ranges.clear();
        state.line_margins.clear();

        for (i, line) in self.lines.iter().enumerate() {
            let is_editing = self.is_block_editing(line, focused);
            let active_col = self.active_col(i, focused);
            let opens = walk.opens_block(line);
            let math_head = walk.claims_math_head(line, is_editing);

            // Only the body is cached: it is a pure function of the line and
            // this key. Margins depend on neighbours and are recomputed every
            // time.
            let hash = line_hash(line) ^ resource_hash(line, self.image_cache, self.math_cache);
            let cached = state.line_height_cache[i];
            let body = if cached.valid
                && cached.hash == hash
                && cached.is_editing == is_editing
                && cached.active_col == active_col
                && cached.math_head == math_head
            {
                cached.height
            } else {
                let height = line_body_height::<R>(
                    line,
                    self.image_cache,
                    self.math_cache,
                    max_width,
                    is_editing,
                    active_col,
                    math_head,
                );
                state.line_height_cache[i] = LineHeightCache {
                    hash,
                    is_editing,
                    active_col,
                    math_head,
                    height,
                    valid: true,
                };
                height
            };
            let extent = LineExtent {
                margins: walk.margins(self.lines, i, body, opens, is_editing),
                body,
            };
            state.line_margins.push(extent.margins);
            state.layout_tree.update_height(i, extent.total());
            record_block_range(&mut state.block_ranges, line, i);
        }

        self.follow_caret_in_code::<R>(state, max_width);
        TOP_PAD + state.layout_tree.prefix_sum(n) + BOTTOM_PAD
    }

    /// When the caret has moved within a code block, scroll the block
    /// horizontally by the least amount that keeps the caret in view.
    fn follow_caret_in_code<R: Measure>(&self, state: &mut State, available_width: f32) {
        let caret = (self.buffer.cursor_line, self.buffer.cursor_col);
        if state.last_caret.replace(caret) == Some(caret) || !state.is_focused {
            return;
        }
        let Some(line) = self.lines.get(caret.0).filter(|line| line.is_code_block) else {
            return;
        };
        let Some(extent) = self.scroll_extent::<R>(state, line.block_id, available_width, true)
        else {
            return;
        };
        let x = code_x_for_col::<R>(line, caret.1, self.is_block_editing(line, true));
        let margin = CODE_CARET_MARGIN.min(extent.viewport_w / 4.0);
        let scroll = state.block_scroll(line.block_id, &extent);
        let target = if x < scroll + margin {
            x - margin
        } else if x > scroll + extent.viewport_w - margin {
            x - extent.viewport_w + margin
        } else {
            return;
        };
        state
            .block_scroll_x
            .insert(line.block_id, target.clamp(0.0, extent.max_scroll()));
    }

    /// The inline layout of line `line_idx`.
    pub(super) fn flow<R: Measure>(
        &self,
        line_idx: usize,
        available_width: f32,
        is_editing: bool,
        active_col: Option<usize>,
    ) -> Flow<'_> {
        Flow::build::<R>(
            &self.lines[line_idx],
            self.math_cache,
            available_width,
            is_editing,
            active_col,
        )
    }

    /// Widget-relative y of the top of a line's body, below its margin and
    /// caption band.
    pub(super) fn line_body_top(&self, line_idx: usize, state: &State) -> f32 {
        let above = state
            .line_margins
            .get(line_idx)
            .map_or(0.0, |m| m.top + m.caption);
        self.widget_y_for_line(line_idx, state) + above
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

/// Widget-relative top and height of the box of lines `start..=end`: from the
/// first line's content (below its margin) to the end of the last line.
pub(super) fn lines_extent(state: &State, start: usize, end: usize) -> (f32, f32) {
    let margin = state.line_margins.get(start).map_or(0.0, |m| m.top);
    let top = state.layout_tree.prefix_sum(start) + margin;
    let height = state.layout_tree.prefix_sum(end.saturating_add(1)) - top;
    (TOP_PAD + top, height)
}

#[cfg(test)]
mod tests {
    use super::super::MathRender;
    use super::super::testing::make_line;
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
        assert_eq!(h, body_row());
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
                            assert!(h >= body_row());
                        } else {
                            assert_eq!(h, TABLE_ROW_HEIGHT);
                        }
                    } else if line.is_math_block && is_editing {
                        // Source being edited wraps like any other text.
                        assert!(h >= body_row());
                        if width >= 400.0 {
                            assert_eq!(h, body_row());
                        }
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

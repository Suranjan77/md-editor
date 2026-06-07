use super::*;
use crate::editor::buffer::{DocBuffer, EditorCommand, Movement};
use crate::editor::highlight::{StyledLine, StyledSpan};
use crate::editor::layout_cache::{LineHeightCache, line_hash, resource_hash};
use crate::editor::renderer::geometry::{clip_viewport, normalized_selection};
use crate::{search, theme};
use iced::advanced::graphics::core::event::Event;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::keyboard;
use iced::mouse;
use iced::{Color, Element, Length, Point, Rectangle, Size};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

pub(crate) fn line_height_for<R>(
    line: &StyledLine,
    image_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    math_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    available_width: f32,
    is_editing: bool,
    active_col: Option<usize>,
    seen_math_blocks: &mut std::collections::HashSet<usize>,
) -> f32
where
    R: iced::advanced::text::Renderer<Font = iced::Font>,
{
    if let Some(span) = line.spans.iter().find(|s| s.is_image) {
        if let Some(path) = &span.image_path {
            if let Some((_, w, h)) = image_cache.get(path) {
                let max_w = available_width - TEXT_X_OFFSET - MARGIN_RIGHT;
                let scale = if *w > max_w { max_w / w } else { 1.0 };
                return (h * scale) + 40.0; // Extra padding for caption
            }
        }
        return IMAGE_HEIGHT;
    }
    if line.is_math_block {
        if is_editing {
            return BASE_LINE_HEIGHT;
        } else {
            let has_visible_math = line
                .spans
                .iter()
                .any(|span| !span.visible_text(false).trim_matches('$').trim().is_empty());
            if !has_visible_math {
                return 0.0;
            }

            if seen_math_blocks.insert(line.block_id) {
                let mut max_h: f32 = 72.0;
                for span in &line.spans {
                    let tex = span.visible_text(false).trim_matches('$').trim();
                    if let Some((_, _, h)) = math_cache.get(tex) {
                        max_h = max_h.max(*h * 1.2 + 48.0);
                    } else if !tex.is_empty() {
                        let visual_lines = tex
                            .lines()
                            .map(|line| (line.chars().count() as f32 / 72.0).ceil().max(1.0))
                            .sum::<f32>()
                            .max(1.0);
                        max_h = max_h.max(visual_lines * BASE_LINE_HEIGHT + 48.0);
                    }
                }
                return max_h;
            } else {
                return 0.0;
            }
        }
    }
    if line.is_code_block {
        return 34.0;
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
        } else {
            if line.table_cells.is_empty() {
                return 0.0;
            }
            return 34.0;
        }
    }
    if !line.is_math_block && line.spans.iter().any(|s| s.is_math) {
        return measured_inline_height::<R>(
            line,
            math_cache,
            available_width,
            is_editing,
            active_col,
        ) + 10.0;
    }
    measured_inline_height::<R>(line, math_cache, available_width, is_editing, active_col)
}

pub(crate) fn bounded_block_scan_range(
    start: usize,
    end: usize,
    preferred_start: usize,
    preferred_end: usize,
) -> Option<(usize, usize)> {
    if start > end {
        return None;
    }
    let preferred_start = preferred_start.clamp(start, end);
    let preferred_end = preferred_end.clamp(start, end).max(preferred_start);
    let preferred_len = preferred_end
        .saturating_sub(preferred_start)
        .saturating_add(1);

    if preferred_len >= HOT_PATH_BLOCK_SCAN_LIMIT {
        let scan_end = preferred_start
            .saturating_add(HOT_PATH_BLOCK_SCAN_LIMIT - 1)
            .min(end);
        return Some((preferred_start, scan_end));
    }

    let remaining = HOT_PATH_BLOCK_SCAN_LIMIT - preferred_len;
    let before = remaining / 2;
    let after = remaining - before;
    let scan_start = preferred_start.saturating_sub(before).max(start);
    let scan_end = preferred_end.saturating_add(after).min(end);
    Some((scan_start, scan_end))
}

/// Pick the iced font for a span.
pub(crate) fn span_font(
    span: &crate::editor::highlight::StyledSpan,
    line: &StyledLine,
) -> iced::Font {
    if span.is_code || line.is_code_block || line.is_math_block {
        iced::Font::MONOSPACE
    } else if span.bold {
        iced::Font {
            weight: iced::font::Weight::Bold,
            ..iced::Font::DEFAULT
        }
    } else if span.italic {
        iced::Font {
            style: iced::font::Style::Italic,
            ..iced::Font::DEFAULT
        }
    } else {
        iced::Font::DEFAULT
    }
}

/// Measure the width of a string at a given font size + font.
pub(crate) fn measure_width<R>(content: &str, size: f32, font: iced::Font) -> f32
where
    R: iced::advanced::text::Renderer<Font = iced::Font>,
{
    use iced::advanced::text::Paragraph;
    if content.is_empty() {
        return 0.0;
    }
    let paragraph = R::Paragraph::with_text(iced::advanced::text::Text {
        content,
        bounds: Size::new(f32::INFINITY, f32::INFINITY),
        size: size.into(),
        line_height: iced::advanced::text::LineHeight::default(),
        font,
        align_x: iced::alignment::Horizontal::Left.into(),
        align_y: iced::alignment::Vertical::Top.into(),
        shaping: iced::advanced::text::Shaping::Basic,
        wrapping: iced::advanced::text::Wrapping::None,
    });
    paragraph.min_bounds().width
}

pub(crate) fn measure_char_width<R>(ch: char, size: f32, font: iced::Font) -> f32
where
    R: iced::advanced::text::Renderer<Font = iced::Font>,
{
    static CACHE: OnceLock<Mutex<HashMap<CharCacheKey, f32>>> = OnceLock::new();
    let key = CharCacheKey {
        ch,
        font,
        size_bits: size.to_bits(),
    };
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(cache) = cache.lock() {
        if let Some(width) = cache.get(&key) {
            return *width;
        }
    }

    let width = measure_width::<R>(&ch.to_string(), size, font);
    if let Ok(mut cache) = cache.lock() {
        cache.insert(key, width);
    }
    width
}

pub(crate) fn measured_inline_height<R>(
    line: &StyledLine,
    math_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    available_width: f32,
    is_editing: bool,
    active_col: Option<usize>,
) -> f32
where
    R: iced::advanced::text::Renderer<Font = iced::Font>,
{
    let line_start_x = 0.0_f32;
    let line_right_x = (available_width - TEXT_X_OFFSET - MARGIN_RIGHT).max(80.0);
    let mut x = line_start_x;
    let mut y = 0.0_f32;
    let mut row_step = BASE_LINE_HEIGHT;

    for (span_idx, span) in line.spans.iter().enumerate() {
        let fs = span.font_size;
        let step = visual_line_step(fs);
        row_step = row_step.max(step);
        let span_editing = is_editing
            || active_col.is_some_and(|col| span_is_inline_edit_target(line, span_idx, col));

        if span.is_checkbox && !span_editing {
            let width = 26.0;
            if x > line_start_x && x + width > line_right_x {
                y += row_step;
                x = line_start_x;
                row_step = step;
            }
            x += width;
            continue;
        }

        if span.is_math && !span_editing {
            let tex = span.visible_text(false).trim_matches('$').trim();
            if tex.is_empty() || span.is_syntax {
                continue;
            }
            let (width, height) = math_cache
                .get(tex)
                .map(|(_, w, h)| (*w, *h))
                .unwrap_or_else(|| {
                    (
                        measure_width::<R>(tex, fs, span_font(span, line)),
                        BASE_LINE_HEIGHT,
                    )
                });
            let extra_h = (height - BASE_LINE_HEIGHT).max(0.0);
            row_step = row_step.max(BASE_LINE_HEIGHT + extra_h);
            if x > line_start_x && x + width > line_right_x {
                y += row_step;
                x = line_start_x;
                row_step = step;
            }
            x += width + 4.0;
            continue;
        }

        let display = span_visible_text(line, span_idx, is_editing, active_col);
        if display.is_empty() {
            continue;
        }

        let font = span_font(span, line);
        let mut token = String::new();
        let flush_token = |token: &mut String, x: &mut f32, y: &mut f32, row_step: &mut f32| {
            if token.is_empty() {
                return;
            }

            let width = measure_width::<R>(token, fs, font);
            if *x > line_start_x && *x + width > line_right_x {
                *y += *row_step;
                *x = line_start_x;
                *row_step = step;
            }

            if width <= (line_right_x - line_start_x).max(1.0) {
                *x += width;
            } else {
                for ch in token.chars() {
                    let ch_w = measure_char_width::<R>(ch, fs, font);
                    if *x > line_start_x && *x + ch_w > line_right_x {
                        *y += *row_step;
                        *x = line_start_x;
                        *row_step = step;
                    }
                    *x += ch_w;
                }
            }
            *row_step = (*row_step).max(step);
            token.clear();
        };

        for ch in display.chars() {
            token.push(ch);
            if ch.is_whitespace() {
                flush_token(&mut token, &mut x, &mut y, &mut row_step);
            }
        }
        flush_token(&mut token, &mut x, &mut y, &mut row_step);
    }

    (y + row_step).max(BASE_LINE_HEIGHT)
}

pub(crate) fn visual_line_step(font_size: f32) -> f32 {
    (font_size * 1.45).max(BASE_LINE_HEIGHT)
}

pub(crate) fn table_block_gutter_after(
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

pub(crate) fn is_first_block_line(lines: &[StyledLine], line_idx: usize) -> bool {
    let Some(line) = lines.get(line_idx) else {
        return false;
    };
    !lines
        .get(line_idx.saturating_sub(1))
        .is_some_and(|prev| prev.block_id == line.block_id)
}

/// Total document height in pixels.
pub(crate) fn total_height<R>(
    lines: &[StyledLine],
    image_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    math_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    width: f32,
    active_block_id: Option<usize>,
    active_cursor: Option<(usize, usize)>,
    focused: bool,
) -> f32
where
    R: iced::advanced::text::Renderer<Font = iced::Font>,
{
    let mut h = TOP_PAD;
    let mut seen_math_blocks = std::collections::HashSet::new();
    let mut seen_code_blocks = std::collections::HashSet::new();
    let mut seen_table_blocks = std::collections::HashSet::new();
    for (idx, line) in lines.iter().enumerate() {
        let is_editing = is_block_editing_line(line, active_block_id, focused);
        let active_col = active_cursor
            .filter(|(line_idx, _)| *line_idx == idx)
            .map(|(_, col)| col);

        let is_new_block = (line.is_code_block && seen_code_blocks.insert(line.block_id))
            || (line.is_table_row && seen_table_blocks.insert(line.block_id));

        if is_new_block && !is_editing {
            h += 24.0;
        }

        h += line_height_for::<R>(
            line,
            image_cache,
            math_cache,
            width,
            is_editing,
            active_col,
            &mut seen_math_blocks,
        );
        h += table_block_gutter_after(lines, idx, is_editing);
    }
    h + 80.0 // bottom padding
}

pub fn line_visual_y<R>(
    lines: &[StyledLine],
    image_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    math_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
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
    let mut y = TOP_PAD;
    let mut seen_math_blocks = std::collections::HashSet::new();
    let mut seen_code_blocks = std::collections::HashSet::new();
    let mut seen_table_blocks = std::collections::HashSet::new();

    for (idx, line) in lines.iter().enumerate() {
        if idx >= target_line {
            break;
        }
        let is_editing = is_block_editing_line(line, active_block_id, focused);
        let line_active_col = (focused && idx == active_line).then_some(active_col);

        let is_new_block = (line.is_code_block && seen_code_blocks.insert(line.block_id))
            || (line.is_table_row && seen_table_blocks.insert(line.block_id));

        if is_new_block && !is_editing {
            y += 24.0;
        }

        y += line_height_for::<R>(
            line,
            image_cache,
            math_cache,
            available_width,
            is_editing,
            line_active_col,
            &mut seen_math_blocks,
        );
        y += table_block_gutter_after(lines, idx, is_editing);
    }

    y
}

impl<'a, Message> Editor<'a, Message> {
    pub(crate) fn rebuild_layout_tree<R>(&self, state: &mut State, available_width: f32)
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        state.layout_tree.resize(self.lines.len());
        state.block_ranges.clear();
        let active_block_id = self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
        let mut seen_math_blocks = std::collections::HashSet::new();
        let mut seen_code_blocks = std::collections::HashSet::new();
        let mut seen_table_blocks = std::collections::HashSet::new();

        let selection = normalized_selection(state.selection_anchor, state.selection_focus);

        for (i, line) in self.lines.iter().enumerate() {
            let line_has_selection = selection.is_some_and(|((sl, _), (el, _))| i >= sl && i <= el);
            let is_editing = is_block_editing_line(line, active_block_id, state.is_focused)
                || line_has_selection;
            let active_col = (state.is_focused && i == self.buffer.cursor_line)
                .then_some(self.buffer.cursor_col);

            let is_new_block = (line.is_code_block && seen_code_blocks.insert(line.block_id))
                || (line.is_table_row && seen_table_blocks.insert(line.block_id));
            let spacer = if is_new_block && !is_editing {
                24.0
            } else {
                0.0
            };
            let lh = line_height_for::<R>(
                line,
                self.image_cache,
                self.math_cache,
                available_width,
                is_editing,
                active_col,
                &mut seen_math_blocks,
            );
            let gutter = table_block_gutter_after(self.lines, i, is_editing);
            state.layout_tree.update_height(i, spacer + lh + gutter);

            if line.is_code_block || line.is_table_row || line.is_math_block || line.is_blockquote {
                state
                    .block_ranges
                    .entry(line.block_id)
                    .and_modify(|(_, end)| *end = i)
                    .or_insert((i, i));
            }
        }
    }

    pub(crate) fn line_at_widget_y(&self, y: f32, state: &State) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        let relative_y = (y - TOP_PAD).max(0.0);
        Some(state.layout_tree.find_line_at_y(relative_y))
    }

    pub(crate) fn widget_y_for_line(&self, line_idx: usize, state: &State) -> f32 {
        TOP_PAD + state.layout_tree.prefix_sum(line_idx.min(self.lines.len()))
    }

    pub(crate) fn block_content_width<R>(
        &self,
        block_id: usize,
        available_width: f32,
        focused: bool,
        scan_hint: Option<(usize, usize)>,
        state: &State,
    ) -> f32
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let active_block_id = self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
        let mut max_width = 0.0_f32;
        let mut table_widths: Vec<f32> = Vec::new();
        let Some(&(start, end)) = state.block_ranges.get(&block_id) else {
            return (available_width - TEXT_X_OFFSET - MARGIN_RIGHT).max(80.0);
        };
        let (preferred_start, preferred_end) = scan_hint.unwrap_or((start, end));
        let Some((scan_start, scan_end)) =
            bounded_block_scan_range(start, end, preferred_start, preferred_end)
        else {
            return (available_width - TEXT_X_OFFSET - MARGIN_RIGHT).max(80.0);
        };
        for line in &self.lines[scan_start..=scan_end] {
            let is_editing = is_block_editing_line(line, active_block_id, focused);
            if line.is_code_block {
                let width = line
                    .spans
                    .iter()
                    .map(|span| {
                        measure_width::<R>(
                            span.visible_text(is_editing),
                            15.0,
                            iced::Font::MONOSPACE,
                        )
                    })
                    .sum::<f32>();
                max_width = max_width.max(width + 28.0);
            } else if line.is_table_row && !is_editing {
                for (idx, cell) in line.table_cells.iter().enumerate() {
                    let width = cell
                        .iter()
                        .map(|span| {
                            measure_width::<R>(
                                span.visible_text(false),
                                span.font_size,
                                span_font(span, line),
                            )
                        })
                        .sum::<f32>()
                        + 20.0;
                    if idx >= table_widths.len() {
                        table_widths.push(width);
                    } else {
                        table_widths[idx] = table_widths[idx].max(width);
                    }
                }
            } else if line.is_math_block {
                for span in &line.spans {
                    let tex = span.visible_text(false).trim_matches('$').trim();
                    let width = self
                        .math_cache
                        .get(tex)
                        .map(|(_, w, _)| *w * 1.2 + 72.0)
                        .unwrap_or_else(|| measure_width::<R>(tex, 16.0, iced::Font::MONOSPACE));
                    max_width = max_width.max(width);
                }
            }
        }
        max_width
            .max(table_widths.iter().sum::<f32>() + 12.0)
            .max((available_width - TEXT_X_OFFSET - MARGIN_RIGHT).max(80.0))
    }
}

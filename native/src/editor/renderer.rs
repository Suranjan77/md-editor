use iced::advanced::graphics::core::event::Event;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::keyboard;
use iced::mouse;
use iced::{Color, Element, Length, Point, Rectangle, Size};
use std::collections::HashMap;

use crate::editor::buffer::{DocBuffer, EditorCommand, Movement};
use crate::editor::highlight::StyledLine;
use crate::{search, theme};

const MARGIN_LEFT: f32 = 64.0;
const MARGIN_RIGHT: f32 = 56.0;
const TEXT_X_OFFSET: f32 = MARGIN_LEFT;
const TOP_PAD: f32 = 24.0;
const BASE_LINE_HEIGHT: f32 = 36.0;
const IMAGE_HEIGHT: f32 = 280.0;
const HORIZONTAL_SCROLLBAR_GUTTER: f32 = 16.0;

// ── Widget ───────────────────────────────────────────────────────────

pub struct Editor<'a, Message> {
    buffer: &'a DocBuffer,
    lines: &'a [StyledLine],
    image_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    math_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    search_query: &'a str,
    search_regex: bool,
    search_match_case: bool,
    active_search_match: Option<(usize, usize)>,
    on_command: Box<dyn Fn(EditorCommand) -> Message + 'a>,
    on_link_click: Box<dyn Fn(String) -> Message + 'a>,
    on_checkbox_toggle: Box<dyn Fn(usize) -> Message + 'a>,
}

#[derive(Default)]
pub struct State {
    is_dragging: bool,
    is_focused: bool,
    modifiers: keyboard::Modifiers,
    selection_anchor: Option<(usize, usize)>,
    selection_focus: Option<(usize, usize)>,
    block_scroll_x: HashMap<usize, f32>,
    horizontal_scroll_drag: Option<HorizontalScrollDrag>,
    desired_visual_x: Option<f32>,
}

#[derive(Debug, Clone, Copy)]
struct HorizontalScrollDrag {
    block_id: usize,
    viewport_x: f32,
    viewport_w: f32,
    content_w: f32,
    grab_offset: f32,
}

impl<'a, Message> Editor<'a, Message> {
    pub fn new(
        buffer: &'a DocBuffer,
        lines: &'a [StyledLine],
        image_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
        math_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
        on_command: impl Fn(EditorCommand) -> Message + 'a,
        on_link_click: impl Fn(String) -> Message + 'a,
        on_checkbox_toggle: impl Fn(usize) -> Message + 'a,
    ) -> Self {
        Self {
            buffer,
            lines,
            image_cache,
            math_cache,
            search_query: "",
            search_regex: false,
            search_match_case: false,
            active_search_match: None,
            on_command: Box::new(on_command),
            on_link_click: Box::new(on_link_click),
            on_checkbox_toggle: Box::new(on_checkbox_toggle),
        }
    }

    pub fn search(
        mut self,
        query: &'a str,
        regex: bool,
        match_case: bool,
        active_match: Option<(usize, usize)>,
    ) -> Self {
        self.search_query = query;
        self.search_regex = regex;
        self.search_match_case = match_case;
        self.active_search_match = active_match;
        self
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn line_height_for<R>(
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

/// Pick the iced font for a span.
fn span_font(span: &crate::editor::highlight::StyledSpan, line: &StyledLine) -> iced::Font {
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
fn measure_width<R>(content: &str, size: f32, font: iced::Font) -> f32
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

fn measured_inline_height<R>(
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
                    let ch_text = ch.to_string();
                    let ch_w = measure_width::<R>(&ch_text, fs, font);
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

fn visual_line_step(font_size: f32) -> f32 {
    (font_size * 1.45).max(BASE_LINE_HEIGHT)
}

fn source_col_after_span(span: &crate::editor::highlight::StyledSpan, start_col: usize) -> usize {
    start_col + span.text.chars().count()
}

fn span_source_range(line: &StyledLine, span_idx: usize) -> Option<(usize, usize)> {
    let mut start = 0usize;
    for (idx, span) in line.spans.iter().enumerate() {
        let end = source_col_after_span(span, start);
        if idx == span_idx {
            return Some((start, end));
        }
        start = end;
    }
    None
}

fn col_touches_range(col: usize, start: usize, end: usize) -> bool {
    col >= start && col <= end
}

fn span_is_inline_edit_target(line: &StyledLine, span_idx: usize, active_col: usize) -> bool {
    let Some(span) = line.spans.get(span_idx) else {
        return false;
    };

    let touches = |idx: usize| -> bool {
        span_source_range(line, idx)
            .is_some_and(|(start, end)| col_touches_range(active_col, start, end))
    };

    if touches(span_idx) {
        return true;
    }

    let is_content = |idx: usize| {
        line.spans.get(idx).is_some_and(|s| {
            !s.is_syntax
                && (s.bold
                    || s.italic
                    || s.is_code
                    || s.is_link
                    || s.is_math
                    || s.is_heading
                    || line.is_blockquote)
        })
    };

    let is_syntax = |idx: usize| line.spans.get(idx).is_some_and(|s| s.is_syntax);

    if span.is_syntax {
        let check_side = |content_idx: usize| -> bool {
            if is_content(content_idx) {
                if touches(content_idx) {
                    return true;
                }
                let other_syntax_idx = if content_idx > span_idx {
                    content_idx + 1
                } else {
                    content_idx.saturating_sub(1)
                };
                if is_syntax(other_syntax_idx) && touches(other_syntax_idx) {
                    return true;
                }
            }
            false
        };

        if span_idx > 0 && check_side(span_idx - 1) {
            return true;
        }
        if check_side(span_idx + 1) {
            return true;
        }
    } else if is_content(span_idx) {
        if span_idx > 0 && is_syntax(span_idx - 1) && touches(span_idx - 1) {
            return true;
        }
        if is_syntax(span_idx + 1) && touches(span_idx + 1) {
            return true;
        }
    }

    false
}

fn span_visible_text<'a>(
    line: &'a StyledLine,
    span_idx: usize,
    block_editing: bool,
    active_col: Option<usize>,
) -> &'a str {
    let Some(span) = line.spans.get(span_idx) else {
        return "";
    };
    let span_editing = block_editing
        || active_col.is_some_and(|col| span_is_inline_edit_target(line, span_idx, col));
    span.visible_text(span_editing)
}

fn is_block_editing_line(line: &StyledLine, active_block_id: Option<usize>, focused: bool) -> bool {
    focused
        && Some(line.block_id) == active_block_id
        && (line.is_code_block || line.is_math_block || line.is_table_row)
}

fn table_block_gutter_after(lines: &[StyledLine], line_idx: usize, is_editing: bool) -> f32 {
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

fn draw_text_chunk<R>(
    renderer: &mut R,
    content: &str,
    x: f32,
    y: f32,
    max_width: f32,
    font_size: f32,
    font: iced::Font,
    color: Color,
    viewport: &Rectangle,
) where
    R: renderer::Renderer + iced::advanced::text::Renderer<Font = iced::Font>,
{
    renderer.fill_text(
        iced::advanced::text::Text {
            content: content.to_string(),
            bounds: Size::new(max_width.max(1.0), visual_line_step(font_size)),
            size: font_size.into(),
            line_height: iced::advanced::text::LineHeight::default(),
            font,
            align_x: iced::alignment::Horizontal::Left.into(),
            align_y: iced::alignment::Vertical::Top.into(),
            shaping: iced::advanced::text::Shaping::Basic,
            wrapping: iced::advanced::text::Wrapping::None,
        },
        Point::new(x, y + (BASE_LINE_HEIGHT - font_size) / 2.0),
        color,
        *viewport,
    );
}

fn draw_wrapped_text<R>(
    renderer: &mut R,
    text: &str,
    x: &mut f32,
    y: &mut f32,
    line_start_x: f32,
    line_right_x: f32,
    font_size: f32,
    font: iced::Font,
    color: Color,
    viewport: &Rectangle,
) where
    R: renderer::Renderer + iced::advanced::text::Renderer<Font = iced::Font>,
{
    let step = visual_line_step(font_size);
    let mut token = String::new();
    let mut flush = |token: &mut String, x: &mut f32, y: &mut f32| {
        if token.is_empty() {
            return;
        }
        let width = measure_width::<R>(token, font_size, font);
        if *x > line_start_x && *x + width > line_right_x {
            *y += step;
            *x = line_start_x;
        }
        if width <= (line_right_x - line_start_x).max(1.0) {
            draw_text_chunk(
                renderer,
                token,
                *x,
                *y,
                line_right_x - *x,
                font_size,
                font,
                color,
                viewport,
            );
            *x += width;
        } else {
            for ch in token.chars() {
                let ch_text = ch.to_string();
                let ch_w = measure_width::<R>(&ch_text, font_size, font);
                if *x > line_start_x && *x + ch_w > line_right_x {
                    *y += step;
                    *x = line_start_x;
                }
                draw_text_chunk(
                    renderer,
                    &ch_text,
                    *x,
                    *y,
                    line_right_x - *x,
                    font_size,
                    font,
                    color,
                    viewport,
                );
                *x += ch_w;
            }
        }
        token.clear();
    };

    for ch in text.chars() {
        token.push(ch);
        if ch.is_whitespace() {
            flush(&mut token, x, y);
        }
    }
    flush(&mut token, x, y);
}

fn draw_nowrap_text<R>(
    renderer: &mut R,
    content: &str,
    x: f32,
    y: f32,
    max_width: f32,
    font_size: f32,
    font: iced::Font,
    color: Color,
    viewport: &Rectangle,
) where
    R: renderer::Renderer + iced::advanced::text::Renderer<Font = iced::Font>,
{
    if content.is_empty() {
        return;
    }
    renderer.fill_text(
        iced::advanced::text::Text {
            content: content.to_string(),
            bounds: Size::new(max_width.max(1.0), visual_line_step(font_size)),
            size: font_size.into(),
            line_height: iced::advanced::text::LineHeight::default(),
            font,
            align_x: iced::alignment::Horizontal::Left.into(),
            align_y: iced::alignment::Vertical::Top.into(),
            shaping: iced::advanced::text::Shaping::Basic,
            wrapping: iced::advanced::text::Wrapping::None,
        },
        Point::new(x, y),
        color,
        *viewport,
    );
}

fn draw_horizontal_scrollbar<R>(
    renderer: &mut R,
    block_id: usize,
    state: &State,
    viewport_x: f32,
    viewport_w: f32,
    y: f32,
    content_w: f32,
) where
    R: renderer::Renderer,
{
    if content_w <= viewport_w + 1.0 {
        return;
    }

    let scroll = state
        .block_scroll_x
        .get(&block_id)
        .copied()
        .unwrap_or(0.0)
        .clamp(0.0, (content_w - viewport_w).max(0.0));
    let track_w = viewport_w.max(1.0);
    let thumb_w = (track_w * (viewport_w / content_w)).clamp(32.0, track_w);
    let thumb_x = viewport_x + ((track_w - thumb_w) * (scroll / (content_w - viewport_w)));

    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle {
                x: viewport_x,
                y,
                width: track_w,
                height: 4.0,
            },
            border: iced::Border {
                radius: 2.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        Color::from_rgba(1.0, 1.0, 1.0, 0.06),
    );
    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle {
                x: thumb_x,
                y,
                width: thumb_w,
                height: 4.0,
            },
            border: iced::Border {
                radius: 2.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        theme::ACCENT_DIM,
    );
}

fn normalized_selection(
    anchor: Option<(usize, usize)>,
    focus: Option<(usize, usize)>,
) -> Option<((usize, usize), (usize, usize))> {
    let (a_line, a_col) = anchor?;
    let (f_line, f_col) = focus?;
    if (a_line, a_col) == (f_line, f_col) {
        return None;
    }
    if (a_line, a_col) <= (f_line, f_col) {
        Some(((a_line, a_col), (f_line, f_col)))
    } else {
        Some(((f_line, f_col), (a_line, a_col)))
    }
}

/// Total document height in pixels.
fn total_height<R>(
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
    for (idx, line) in lines.iter().enumerate() {
        let is_editing = is_block_editing_line(line, active_block_id, focused);
        let active_col = active_cursor
            .filter(|(line_idx, _)| *line_idx == idx)
            .map(|(_, col)| col);
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

    for (idx, line) in lines.iter().enumerate() {
        if idx >= target_line {
            break;
        }
        let is_editing = is_block_editing_line(line, active_block_id, focused);
        let line_active_col = (focused && idx == active_line).then_some(active_col);
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

// ── Widget impl ──────────────────────────────────────────────────────

impl<'a, Message, Theme, R> Widget<Message, Theme, R> for Editor<'a, Message>
where
    R: renderer::Renderer
        + iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::image::Renderer<Handle = iced::widget::image::Handle>,
{
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fixed(total_height::<R>(
                self.lines,
                self.image_cache,
                self.math_cache,
                800.0,
                None,
                None,
                false,
            )),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &R,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = _tree.state.downcast_ref::<State>();
        let focused = state.is_focused;
        let active_block_id = self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
        let max_width = limits.max().width;
        let h = total_height::<R>(
            self.lines,
            self.image_cache,
            self.math_cache,
            max_width,
            active_block_id,
            focused.then_some((self.buffer.cursor_line, self.buffer.cursor_col)),
            focused,
        );
        layout::Node::new(limits.resolve(Length::Fill, Length::Fixed(h), Size::new(0.0, 0.0)))
    }

    // ── draw ──────────────────────────────────────────────────────────

    fn draw(
        &self,
        _state: &widget::Tree,
        renderer: &mut R,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = _state.state.downcast_ref::<State>();
        let focused = state.is_focused;

        // Background
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: iced::Border::default(),
                ..Default::default()
            },
            theme::BG_PRIMARY,
        );

        let active_block_id = self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
        let mut image_counter = 0;
        let mut equation_counter = 0;

        // ── Pre-calculate and draw block backgrounds ────────────────────────
        struct BlockMeta {
            y: f32,
            height: f32,
            is_code: bool,
            is_math: bool,
            is_quote: bool,
            is_table: bool,
            is_editing: bool,
            col_widths: Vec<f32>,
            content_width: f32,
        }
        let mut blocks: std::collections::HashMap<usize, BlockMeta> =
            std::collections::HashMap::new();
        let mut temp_y = bounds.y + TOP_PAD;
        let mut seen_math_blocks_layout = std::collections::HashSet::new();
        for (i, line) in self.lines.iter().enumerate() {
            let is_editing = is_block_editing_line(line, active_block_id, focused);
            let active_col =
                (focused && i == self.buffer.cursor_line).then_some(self.buffer.cursor_col);
            let lh = line_height_for::<R>(
                line,
                self.image_cache,
                self.math_cache,
                bounds.width,
                is_editing,
                active_col,
                &mut seen_math_blocks_layout,
            );
            if line.is_code_block || line.is_math_block || line.is_blockquote || line.is_table_row {
                if lh <= 0.0 {
                    temp_y += lh;
                    continue;
                }
                let entry = blocks.entry(line.block_id).or_insert(BlockMeta {
                    y: temp_y,
                    height: 0.0,
                    is_code: line.is_code_block,
                    is_math: line.is_math_block,
                    is_quote: line.is_blockquote,
                    is_table: line.is_table_row,
                    is_editing: is_block_editing_line(line, active_block_id, focused),
                    col_widths: Vec::new(),
                    content_width: 0.0,
                });
                entry.height += lh;

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
                    entry.content_width = entry.content_width.max(width + 28.0);
                } else if line.is_math_block {
                    let width = line
                        .spans
                        .iter()
                        .map(|span| {
                            let tex = span.visible_text(false).trim_matches('$').trim();
                            self.math_cache
                                .get(tex)
                                .map(|(_, w, _)| *w * 1.2 + 72.0)
                                .unwrap_or_else(|| {
                                    measure_width::<R>(tex, 16.0, iced::Font::MONOSPACE) + 72.0
                                })
                        })
                        .fold(0.0_f32, f32::max);
                    entry.content_width = entry.content_width.max(width);
                } else if line.is_table_row && !entry.is_editing {
                    for (c_idx, cell) in line.table_cells.iter().enumerate() {
                        let mut w = 0.0;
                        for span in cell {
                            let text = span.visible_text(false);
                            w += measure_width::<R>(text, span.font_size, span_font(span, line));
                        }
                        if c_idx >= entry.col_widths.len() {
                            entry.col_widths.push(w + 20.0); // 20.0 padding
                        } else if w + 20.0 > entry.col_widths[c_idx] {
                            entry.col_widths[c_idx] = w + 20.0;
                        }
                    }
                    entry.content_width = entry.col_widths.iter().sum::<f32>() + 12.0;
                }
            }
            let gutter = table_block_gutter_after(self.lines, i, is_editing);
            if let Some(entry) = blocks.get_mut(&line.block_id) {
                if line.is_table_row && !is_editing {
                    entry.height += gutter;
                }
            }
            temp_y += lh + gutter;
        }

        for meta in blocks.values() {
            if meta.y + meta.height < viewport.y || meta.y > viewport.y + viewport.height {
                continue;
            }

            if meta.is_quote {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: bounds.x + TEXT_X_OFFSET - 16.0,
                            y: meta.y,
                            width: 4.0,
                            height: meta.height,
                        },
                        border: iced::Border {
                            radius: 2.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    theme::ACCENT_DIM,
                );
            } else if meta.is_table && !meta.is_editing {
                let available_w = bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT;
                let table_width = available_w;
                let table_x = bounds.x + TEXT_X_OFFSET;
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: table_x - 6.0,
                            y: meta.y,
                            width: table_width + 12.0,
                            height: meta.height,
                        },
                        border: iced::Border {
                            color: theme::BORDER_SUBTLE,
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    },
                    theme::BG_SECONDARY,
                );
            } else {
                let bg = if meta.is_editing && meta.is_code {
                    theme::BG_SECONDARY
                } else if meta.is_editing && meta.is_math {
                    theme::BG_SECONDARY
                } else if meta.is_code {
                    theme::BG_SECONDARY
                } else {
                    Color::TRANSPARENT
                };

                if bg != Color::TRANSPARENT || meta.is_code || meta.is_math {
                    let block_x = bounds.x + TEXT_X_OFFSET - 16.0;
                    let block_w = bounds.width - TEXT_X_OFFSET;
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle {
                                x: block_x,
                                y: meta.y,
                                width: block_w,
                                height: meta.height,
                            },
                            border: iced::Border {
                                color: theme::BORDER_SUBTLE,
                                width: 1.0,
                                radius: 8.0.into(),
                            },
                            ..Default::default()
                        },
                        bg,
                    );
                }
            }
        }

        let mut y = bounds.y + TOP_PAD;
        let mut last_table_block = None;

        let mut seen_math_blocks_draw = std::collections::HashSet::new();
        for (i, line) in self.lines.iter().enumerate() {
            let is_editing = is_block_editing_line(line, active_block_id, focused);
            let active_col =
                (focused && i == self.buffer.cursor_line).then_some(self.buffer.cursor_col);
            let lh = line_height_for::<R>(
                line,
                self.image_cache,
                self.math_cache,
                bounds.width,
                is_editing,
                active_col,
                &mut seen_math_blocks_draw,
            );

            // Viewport culling
            let gutter = table_block_gutter_after(self.lines, i, is_editing);
            if y + lh + gutter < viewport.y {
                y += lh + gutter;
                continue;
            }
            if y > viewport.y + viewport.height {
                break;
            }

            if line.is_math_block && !is_editing && lh == 0.0 {
                continue;
            }

            // (Active line highlight removed)

            let selection = normalized_selection(state.selection_anchor, state.selection_focus)
                .or_else(|| {
                    self.buffer.selection.map(|(sl, sc, el, ec)| {
                        if (sl, sc) <= (el, ec) {
                            ((sl, sc), (el, ec))
                        } else {
                            ((el, ec), (sl, sc))
                        }
                    })
                });

            if let Some(((start_line, start_col), (end_line, end_col))) = selection {
                if i >= start_line && i <= end_line {
                    let line_len = self.buffer.line_text(i).chars().count();
                    let from_col = if i == start_line {
                        start_col.min(line_len)
                    } else {
                        0
                    };
                    let to_col = if i == end_line {
                        end_col.min(line_len)
                    } else {
                        line_len
                    };

                    if from_col < to_col {
                        let (from_x, from_y) = self.position_for_col::<R>(
                            i,
                            from_col,
                            bounds.width,
                            is_editing,
                            active_col,
                        );
                        let (to_x, to_y) = self.position_for_col::<R>(
                            i,
                            to_col,
                            bounds.width,
                            is_editing,
                            active_col,
                        );
                        let select_x = bounds.x + TEXT_X_OFFSET + from_x;
                        let select_w = if (to_y - from_y).abs() < 1.0 {
                            (to_x - from_x).max(3.0)
                        } else {
                            (bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT - from_x).max(3.0)
                        };
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: select_x,
                                    y: y + from_y + 4.0,
                                    width: select_w,
                                    height: (BASE_LINE_HEIGHT - 8.0).max(16.0),
                                },
                                border: iced::Border {
                                    radius: 3.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                            Color::from_rgba(0.69, 0.80, 0.78, 0.24),
                        );
                    }
                }
            }

            if !self.search_query.is_empty() {
                let line_text = self.buffer.line_text(i);
                for line_match in search::line_matches(
                    &line_text,
                    self.search_query,
                    self.search_regex,
                    self.search_match_case,
                ) {
                    let from_col = line_match.start_col;
                    let to_col = line_match.end_col;
                    let (from_x, from_y) = self.position_for_col::<R>(
                        i,
                        from_col,
                        bounds.width,
                        is_editing,
                        active_col,
                    );
                    let (to_x, to_y) =
                        self.position_for_col::<R>(i, to_col, bounds.width, is_editing, active_col);
                    let same_visual_line = (to_y - from_y).abs() < 1.0;
                    let highlight_w = if same_visual_line {
                        (to_x - from_x).max(4.0)
                    } else {
                        (bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT - from_x).max(4.0)
                    };
                    let active = self.active_search_match == Some((i, from_col));
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle {
                                x: bounds.x + TEXT_X_OFFSET + from_x,
                                y: y + from_y + 5.0,
                                width: highlight_w,
                                height: (BASE_LINE_HEIGHT - 10.0).max(16.0),
                            },
                            border: iced::Border {
                                radius: 3.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                        if active {
                            Color::from_rgba(0.92, 0.70, 0.30, 0.45)
                        } else {
                            Color::from_rgba(0.92, 0.70, 0.30, 0.24)
                        },
                    );
                }
            }

            // ── horizontal rule ──────────────────────────────────
            if line.spans.iter().any(|s| s.is_rule) {
                let rule_y = y + lh / 2.0;
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: bounds.x + TEXT_X_OFFSET,
                            y: rule_y,
                            width: bounds.width - TEXT_X_OFFSET - 20.0,
                            height: 2.0,
                        },
                        border: iced::Border {
                            radius: 1.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    theme::ACCENT_GLOW, // using a visible accent color for HR
                );
                y += lh;
                continue;
            }

            if !is_editing
                && !line.is_code_block
                && !line.is_math_block
                && !line.is_table_row
                && !line.is_blockquote
                && line.spans.iter().all(|span| {
                    !span.bold
                        && !span.italic
                        && !span.is_code
                        && !span.is_link
                        && !span.is_syntax
                        && span.display_text.is_none()
                })
                && !line
                    .spans
                    .iter()
                    .any(|s| s.is_image || s.is_math || s.is_checkbox)
            {
                let content = line
                    .spans
                    .iter()
                    .map(|span| span.visible_text(false))
                    .collect::<String>();
                if !content.trim().is_empty() {
                    let max_font = line
                        .spans
                        .iter()
                        .map(|s| s.font_size)
                        .fold(17.0_f32, f32::max);
                    let color = line
                        .spans
                        .iter()
                        .find(|span| !span.visible_text(false).is_empty())
                        .map(|span| span.color)
                        .unwrap_or(theme::TEXT_PRIMARY);
                    let font = if line.spans.iter().any(|span| span.bold) {
                        iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..iced::Font::DEFAULT
                        }
                    } else {
                        iced::Font::DEFAULT
                    };
                    renderer.fill_text(
                        iced::advanced::text::Text {
                            content,
                            bounds: Size::new(bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT, lh),
                            size: max_font.into(),
                            line_height: iced::advanced::text::LineHeight::default(),
                            font,
                            align_x: iced::alignment::Horizontal::Left.into(),
                            align_y: iced::alignment::Vertical::Top.into(),
                            shaping: iced::advanced::text::Shaping::Basic,
                            wrapping: iced::advanced::text::Wrapping::WordOrGlyph,
                        },
                        Point::new(bounds.x + TEXT_X_OFFSET, y + 2.0),
                        color,
                        *viewport,
                    );
                }
                y += lh;
                continue;
            }

            // (Block backgrounds removed from per-line loop)

            if line.is_code_block && !line.is_math_block {
                let viewport_w = (bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT - 24.0).max(80.0);
                let content_w = blocks
                    .get(&line.block_id)
                    .map(|meta| meta.content_width)
                    .unwrap_or(viewport_w);
                let scroll_x = state
                    .block_scroll_x
                    .get(&line.block_id)
                    .copied()
                    .unwrap_or(0.0)
                    .clamp(0.0, (content_w - viewport_w).max(0.0));
                let mut code_x = bounds.x + TEXT_X_OFFSET - scroll_x;
                let code_left = bounds.x + TEXT_X_OFFSET;
                let code_right = code_left + viewport_w;

                for span in &line.spans {
                    let text = span.visible_text(is_editing);
                    if text.is_empty() {
                        continue;
                    }
                    let width = measure_width::<R>(text, 15.0, iced::Font::MONOSPACE);
                    if code_x + width >= code_left && code_x <= code_right {
                        draw_nowrap_text::<R>(
                            renderer,
                            text,
                            code_x,
                            y + 10.0,
                            width,
                            15.0,
                            iced::Font::MONOSPACE,
                            span.color,
                            viewport,
                        );
                    }
                    code_x += width;
                }

                if focused && i == self.buffer.cursor_line {
                    let (cx, _) = self.cursor_position::<R>(i, bounds.width);
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle {
                                x: bounds.x + TEXT_X_OFFSET + cx - scroll_x,
                                y: y + 12.0,
                                width: 2.0,
                                height: 22.0,
                            },
                            border: iced::Border {
                                radius: 1.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                        theme::ACCENT_SECONDARY,
                    );
                }

                if let Some(meta) = blocks.get(&line.block_id) {
                    draw_horizontal_scrollbar::<R>(
                        renderer,
                        line.block_id,
                        state,
                        code_left,
                        viewport_w,
                        meta.y + meta.height - 7.0,
                        content_w,
                    );
                }

                y += lh;
                continue;
            }

            // ── table rendering ──────────────────────────────────
            if line.is_table_row && !is_editing {
                if last_table_block != Some(line.block_id) {
                    last_table_block = Some(line.block_id);
                }

                if let Some(meta) = blocks.get(&line.block_id) {
                    let available_w = bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT;
                    let raw_table_width: f32 = meta.col_widths.iter().sum();
                    let table_width = available_w;
                    let scroll_content_width = raw_table_width.max(table_width);
                    let scroll_x = state
                        .block_scroll_x
                        .get(&line.block_id)
                        .copied()
                        .unwrap_or(0.0)
                        .clamp(0.0, (scroll_content_width - table_width).max(0.0));
                    let table_x = bounds.x + TEXT_X_OFFSET;
                    let row_y = y;
                    let row_h = lh;
                    let mut cx = table_x - scroll_x;
                    let is_header = meta.y == row_y;

                    // Is this a separator row? We can check if it has spans with only `-` or `|` or just check table_cells
                    if line.table_cells.is_empty() {
                        // Separator row: draw a horizontal line
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: table_x - 8.0,
                                    y: row_y + row_h / 2.0,
                                    width: table_width + 16.0,
                                    height: 1.0,
                                },
                                ..Default::default()
                            },
                            theme::BORDER_SUBTLE,
                        );
                        y += lh + gutter;
                        continue;
                    }

                    let row_bg = if is_header {
                        Some(theme::BG_TERTIARY)
                    } else if ((row_y - meta.y) / row_h).round() as usize % 2 == 1 {
                        Some(Color::from_rgba(1.0, 1.0, 1.0, 0.025))
                    } else {
                        None
                    };
                    if let Some(bg) = row_bg {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: table_x - 6.0,
                                    y: row_y,
                                    width: table_width + 12.0,
                                    height: row_h,
                                },
                                ..Default::default()
                            },
                            bg,
                        );
                    }

                    for (c_idx, cell) in line.table_cells.iter().enumerate() {
                        if c_idx >= meta.col_widths.len() {
                            break;
                        }
                        let cw = meta.col_widths[c_idx].max(42.0);

                        // Draw Vertical Separator
                        if c_idx > 0 && cx >= table_x && cx <= table_x + table_width {
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: Rectangle {
                                        x: cx - 3.0,
                                        y: row_y,
                                        width: 1.0,
                                        height: row_h,
                                    },
                                    ..Default::default()
                                },
                                theme::BORDER_SUBTLE,
                            );
                        }

                        // Draw Cell Spans
                        let mut px = cx + 7.0;
                        for span in cell {
                            let text = span.visible_text(false);
                            if text.is_empty() {
                                continue;
                            }

                            let font = span_font(span, line);
                            let fs = span.font_size;
                            let ty = row_y + (row_h - fs) / 2.0;
                            let width = measure_width::<R>(text, fs, font);
                            if px + width < table_x || px > table_x + table_width {
                                px += width;
                                continue;
                            }

                            renderer.fill_text(
                                iced::advanced::text::Text {
                                    content: text.to_string(),
                                    bounds: Size::new(
                                        width.min((table_x + table_width - px).max(1.0)).max(1.0),
                                        row_h,
                                    ),
                                    size: fs.into(),
                                    line_height: iced::advanced::text::LineHeight::default(),
                                    font,
                                    align_x: iced::alignment::Horizontal::Left.into(),
                                    align_y: iced::alignment::Vertical::Top.into(),
                                    shaping: iced::advanced::text::Shaping::Basic,
                                    wrapping: iced::advanced::text::Wrapping::None,
                                },
                                Point::new(px, ty),
                                if is_header {
                                    theme::TEXT_PRIMARY
                                } else {
                                    span.color
                                },
                                Rectangle {
                                    x: table_x,
                                    y: row_y,
                                    width: table_width,
                                    height: row_h,
                                },
                            );
                            px += width;
                        }
                        cx += cw;
                    }
                    draw_horizontal_scrollbar::<R>(
                        renderer,
                        line.block_id,
                        state,
                        table_x,
                        table_width,
                        meta.y + meta.height - HORIZONTAL_SCROLLBAR_GUTTER + 5.0,
                        scroll_content_width,
                    );
                }

                y += lh + gutter;
                continue;
            }

            // ── spans ────────────────────────────────────────────
            let mut x = bounds.x + TEXT_X_OFFSET;
            let mut line_draw_y = y;

            for (span_idx, span) in line.spans.iter().enumerate() {
                let font = span_font(span, line);
                let is_math = span.is_math || line.is_math_block;
                let span_editing = is_editing
                    || active_col
                        .is_some_and(|col| span_is_inline_edit_target(line, span_idx, col));

                // ── image ────────────────────────────────────────
                if span.is_image && !span_editing {
                    image_counter += 1;
                    if let Some(path) = &span.image_path {
                        if let Some((handle, w, h)) = self.image_cache.get(path) {
                            let available_w = bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT;
                            let scale = if *w > available_w {
                                available_w / w
                            } else {
                                1.0
                            };
                            let draw_w = w * scale;
                            let draw_h = h * scale;
                            let draw_x = bounds.x + TEXT_X_OFFSET + (available_w - draw_w) / 2.0;

                            renderer.draw_image(
                                iced::advanced::image::Image::new(handle.clone()),
                                Rectangle {
                                    x: draw_x,
                                    y: y + 5.0,
                                    width: draw_w,
                                    height: draw_h,
                                },
                                *viewport,
                            );

                            // Draw caption
                            let caption = format!(
                                "Figure {}: {}",
                                image_counter,
                                span.image_alt.as_deref().unwrap_or("")
                            );
                            let caption_w =
                                measure_width::<R>(&caption, 13.0, iced::Font::DEFAULT).min(draw_w);
                            renderer.fill_text(
                                iced::advanced::text::Text {
                                    content: caption,
                                    bounds: Size::new(caption_w, 20.0),
                                    size: 13.0.into(),
                                    line_height: iced::advanced::text::LineHeight::default(),
                                    font: iced::Font::DEFAULT,
                                    align_x: iced::alignment::Horizontal::Left.into(),
                                    align_y: iced::alignment::Vertical::Top.into(),
                                    shaping: iced::advanced::text::Shaping::Basic,
                                    wrapping: iced::advanced::text::Wrapping::WordOrGlyph,
                                },
                                Point::new(draw_x + (draw_w - caption_w) / 2.0, y + draw_h + 12.0),
                                theme::TEXT_MUTED,
                                *viewport,
                            );

                            x += draw_w + 10.0;
                            continue;
                        }
                    }
                }

                // ── math (rendered to image) ─────────────────────
                if is_math {
                    if line.is_block_fence
                        && !span_editing
                        && span.visible_text(false).trim().is_empty()
                    {
                        continue; // Hide fences in preview
                    }
                    if span.is_syntax && !span_editing {
                        continue; // Hide inline $ in preview
                    }

                    let tex = span.visible_text(false).trim_matches('$').trim();
                    let scale: f32 = if line.is_math_block { 1.2 } else { 1.0 };
                    let mut drawn_w = 0.0;
                    let mut image_rendered = false;

                    if !tex.is_empty() {
                        if let Some((handle, w, h)) = self.math_cache.get(tex) {
                            let available_w = bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT;
                            let block_max_w = (available_w - 88.0).max(80.0);
                            let fit_scale = if line.is_math_block { scale } else { scale };
                            let draw_w = w * fit_scale;
                            let draw_h = h * fit_scale;
                            drawn_w = draw_w;

                            // While editing math, show the source text only. Drawing the rendered
                            // image behind/above the source makes the edit target unreadable.
                            if span_editing {
                                // Skip drawing image, will draw text
                            } else {
                                let line_start_x = bounds.x + TEXT_X_OFFSET;
                                let line_right_x = bounds.x + bounds.width - MARGIN_RIGHT;
                                let mut draw_x = x;
                                if line.is_math_block {
                                    equation_counter += 1;
                                    let max_scroll = (draw_w - block_max_w).max(0.0);
                                    let scroll_x = state
                                        .block_scroll_x
                                        .get(&line.block_id)
                                        .copied()
                                        .unwrap_or(0.0)
                                        .clamp(0.0, max_scroll);
                                    draw_x = bounds.x
                                        + TEXT_X_OFFSET
                                        + if draw_w <= block_max_w {
                                            (block_max_w - draw_w) / 2.0
                                        } else {
                                            -scroll_x
                                        };
                                } else if draw_x > line_start_x && draw_x + draw_w > line_right_x {
                                    line_draw_y += BASE_LINE_HEIGHT;
                                    draw_x = line_start_x;
                                    x = line_start_x;
                                }

                                if line.is_math_block {
                                    // Equation number right aligned
                                    let eq_num = format!("({})", equation_counter);
                                    let eq_w =
                                        measure_width::<R>(&eq_num, 14.0, iced::Font::DEFAULT);
                                    let eq_y = line_draw_y + (lh - draw_h) / 2.0; // center with the equation
                                    renderer.fill_text(
                                        iced::advanced::text::Text {
                                            content: eq_num,
                                            bounds: Size::new(eq_w, draw_h),
                                            size: 14.0.into(),
                                            line_height: iced::advanced::text::LineHeight::default(
                                            ),
                                            font: iced::Font::DEFAULT,
                                            align_x: iced::alignment::Horizontal::Left.into(),
                                            align_y: iced::alignment::Vertical::Center.into(),
                                            shaping: iced::advanced::text::Shaping::Basic,
                                            wrapping: iced::advanced::text::Wrapping::None,
                                        },
                                        Point::new(
                                            bounds.x + TEXT_X_OFFSET + available_w - eq_w,
                                            eq_y,
                                        ),
                                        theme::TEXT_MUTED,
                                        *viewport,
                                    );
                                }

                                renderer.draw_image(
                                    iced::advanced::image::Image::new(handle.clone()),
                                    Rectangle {
                                        x: draw_x,
                                        y: if line.is_math_block {
                                            line_draw_y + (lh - draw_h) / 2.0
                                        } else {
                                            let margin_top =
                                                (BASE_LINE_HEIGHT - draw_h).max(0.0) / 2.0;
                                            line_draw_y + margin_top
                                        },
                                        width: draw_w,
                                        height: draw_h,
                                    },
                                    *viewport,
                                );
                                if line.is_math_block {
                                    draw_horizontal_scrollbar::<R>(
                                        renderer,
                                        line.block_id,
                                        state,
                                        bounds.x + TEXT_X_OFFSET,
                                        block_max_w,
                                        y + lh - 7.0,
                                        draw_w,
                                    );
                                }
                                image_rendered = true;
                            }
                        }
                    }

                    if image_rendered
                        && (line.is_math_block || (!line.is_math_block && !span_editing))
                    {
                        x += drawn_w + 4.0;
                        continue;
                    }

                    if line.is_math_block && !is_editing && !tex.is_empty() {
                        equation_counter += 1;
                        let available_w = bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT;
                        let viewport_w = (available_w - 72.0).max(80.0);
                        let content_w = tex
                            .lines()
                            .map(|raw_math_line| {
                                measure_width::<R>(raw_math_line, 16.0, iced::Font::MONOSPACE)
                            })
                            .fold(0.0_f32, f32::max);
                        let scroll_x = state
                            .block_scroll_x
                            .get(&line.block_id)
                            .copied()
                            .unwrap_or(0.0)
                            .clamp(0.0, (content_w - viewport_w).max(0.0));
                        let mut text_y = line_draw_y + 18.0;
                        for raw_math_line in tex.lines() {
                            renderer.fill_text(
                                iced::advanced::text::Text {
                                    content: raw_math_line.to_string(),
                                    bounds: Size::new(content_w.max(1.0), BASE_LINE_HEIGHT),
                                    size: 16.0.into(),
                                    line_height: iced::advanced::text::LineHeight::default(),
                                    font: iced::Font::MONOSPACE,
                                    align_x: iced::alignment::Horizontal::Left.into(),
                                    align_y: iced::alignment::Vertical::Top.into(),
                                    shaping: iced::advanced::text::Shaping::Basic,
                                    wrapping: iced::advanced::text::Wrapping::None,
                                },
                                Point::new(bounds.x + TEXT_X_OFFSET - scroll_x, text_y),
                                theme::TEXT_SECONDARY,
                                *viewport,
                            );
                            text_y += BASE_LINE_HEIGHT;
                        }
                        draw_horizontal_scrollbar::<R>(
                            renderer,
                            line.block_id,
                            state,
                            bounds.x + TEXT_X_OFFSET,
                            viewport_w,
                            y + lh - 7.0,
                            content_w,
                        );
                        let eq_num = format!("({})", equation_counter);
                        let eq_w = measure_width::<R>(&eq_num, 14.0, iced::Font::DEFAULT);
                        renderer.fill_text(
                            iced::advanced::text::Text {
                                content: eq_num,
                                bounds: Size::new(eq_w, BASE_LINE_HEIGHT),
                                size: 14.0.into(),
                                line_height: iced::advanced::text::LineHeight::default(),
                                font: iced::Font::DEFAULT,
                                align_x: iced::alignment::Horizontal::Left.into(),
                                align_y: iced::alignment::Vertical::Center.into(),
                                shaping: iced::advanced::text::Shaping::Basic,
                                wrapping: iced::advanced::text::Wrapping::None,
                            },
                            Point::new(
                                bounds.x + TEXT_X_OFFSET + available_w - eq_w,
                                y + (lh - BASE_LINE_HEIGHT) / 2.0,
                            ),
                            theme::TEXT_MUTED,
                            *viewport,
                        );
                        continue;
                    }
                }

                // ── text span ────────────────────────────────────
                let fs = span.font_size;
                let display_text = span_visible_text(line, span_idx, is_editing, active_col);
                if display_text.is_empty() {
                    continue;
                }

                if span.is_checkbox && !span_editing {
                    // Draw a premium custom checkbox quad!
                    let box_size = 18.0;
                    let box_y = line_draw_y + (BASE_LINE_HEIGHT - box_size) / 2.0;
                    let box_rect = Rectangle {
                        x,
                        y: box_y,
                        width: box_size,
                        height: box_size,
                    };

                    if span.is_checked {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: box_rect,
                                border: iced::Border {
                                    radius: 4.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                            theme::ACCENT,
                        );

                        let inner_size = 8.0;
                        let inner_rect = Rectangle {
                            x: x + (box_size - inner_size) / 2.0,
                            y: box_y + (box_size - inner_size) / 2.0,
                            width: inner_size,
                            height: inner_size,
                        };
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: inner_rect,
                                border: iced::Border {
                                    radius: 2.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                            theme::BG_PRIMARY,
                        );
                        renderer.fill_text(
                            iced::advanced::text::Text {
                                content: "✓".to_string(),
                                bounds: Size::new(box_size, box_size),
                                size: 13.0.into(),
                                line_height: iced::advanced::text::LineHeight::default(),
                                font: iced::Font {
                                    weight: iced::font::Weight::Bold,
                                    ..iced::Font::DEFAULT
                                },
                                align_x: iced::alignment::Horizontal::Center.into(),
                                align_y: iced::alignment::Vertical::Center.into(),
                                shaping: iced::advanced::text::Shaping::Basic,
                                wrapping: iced::advanced::text::Wrapping::None,
                            },
                            Point::new(x, box_y),
                            theme::BG_PRIMARY,
                            *viewport,
                        );
                    } else {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: box_rect,
                                border: iced::Border {
                                    color: theme::BORDER,
                                    width: 1.5,
                                    radius: 4.0.into(),
                                },
                                ..Default::default()
                            },
                            Color::TRANSPARENT,
                        );
                    }

                    x += box_size + 8.0;
                    continue;
                }

                draw_wrapped_text::<R>(
                    renderer,
                    display_text,
                    &mut x,
                    &mut line_draw_y,
                    bounds.x + TEXT_X_OFFSET,
                    bounds.x + bounds.width - MARGIN_RIGHT,
                    fs,
                    font,
                    span.color,
                    viewport,
                );
            }

            // ── cursor ───────────────────────────────────────────
            if focused && i == self.buffer.cursor_line {
                let (cx, cy) = self.cursor_position::<R>(i, bounds.width);
                let cursor_h = lh.min(20.0);
                let cursor_x = bounds.x + TEXT_X_OFFSET + cx;
                let cursor_y = y + cy + (BASE_LINE_HEIGHT - cursor_h) / 2.0;

                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: cursor_x,
                            y: cursor_y,
                            width: 2.0,
                            height: cursor_h,
                        },
                        border: iced::Border {
                            radius: 1.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    theme::ACCENT,
                );
            }

            y += lh;
        }
    }

    // ── update (event handling) ───────────────────────────────────────

    fn update(
        &mut self,
        _tree: &mut widget::Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &R,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = _tree.state.downcast_mut::<State>();

        match event {
            // ── mouse click ──────────────────────────────────────
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = _cursor.position_in(_layout.bounds()) {
                    if let Some(drag) =
                        self.horizontal_scrollbar_hit::<R>(pos, _layout.bounds().width, state)
                    {
                        state.horizontal_scroll_drag = Some(drag);
                        state.is_dragging = false;
                        shell.capture_event();
                        return;
                    }

                    let active_block_id =
                        self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
                    let (line_idx, col) = self.hit_test::<R>(
                        pos,
                        _layout.bounds().width,
                        active_block_id,
                        state.is_focused,
                    );
                    state.is_focused = true;
                    state.selection_anchor = Some((line_idx, col));
                    state.selection_focus = Some((line_idx, col));
                    state.desired_visual_x = None;
                    shell.publish((self.on_command)(EditorCommand::SetCursor {
                        line: line_idx,
                        col,
                    }));
                    state.is_dragging = true;

                    // Check for checkbox / link clicks
                    if let Some(line) = self.lines.get(line_idx) {
                        let active_block_id =
                            self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
                        let is_editing =
                            is_block_editing_line(line, active_block_id, state.is_focused);
                        let mut x_acc = 0.0_f32;
                        let active_col =
                            (line_idx == self.buffer.cursor_line).then_some(self.buffer.cursor_col);
                        for (span_idx, span) in line.spans.iter().enumerate() {
                            let font = span_font(span, line);
                            let w = if span.is_checkbox && !is_editing {
                                26.0
                            } else {
                                measure_width::<R>(
                                    span_visible_text(line, span_idx, is_editing, active_col),
                                    span.font_size,
                                    font,
                                )
                            };
                            let click_x = pos.x - TEXT_X_OFFSET;
                            if click_x >= x_acc && click_x < x_acc + w {
                                if span.is_checkbox {
                                    shell.publish((self.on_checkbox_toggle)(line_idx));
                                    return;
                                }
                                if span.is_link {
                                    if let Some(target) = &span.link_target {
                                        shell.publish((self.on_link_click)(target.clone()));
                                        return;
                                    }
                                }
                            }
                            x_acc += w;
                        }
                    }
                } else {
                    state.is_focused = false;
                    state.selection_anchor = None;
                    state.selection_focus = None;
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.is_dragging => {
                if let Some(pos) = _cursor.position_in(_layout.bounds()) {
                    let active_block_id =
                        self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
                    let (line_idx, col) = self.hit_test::<R>(
                        pos,
                        _layout.bounds().width,
                        active_block_id,
                        state.is_focused,
                    );
                    state.selection_focus = Some((line_idx, col));
                    if let Some((anchor_line, anchor_col)) = state.selection_anchor {
                        shell.publish((self.on_command)(EditorCommand::SetSelection {
                            anchor_line,
                            anchor_col,
                            focus_line: line_idx,
                            focus_col: col,
                        }));
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
                if state.horizontal_scroll_drag.is_some() =>
            {
                if let (Some(pos), Some(drag)) = (
                    _cursor.position_in(_layout.bounds()),
                    state.horizontal_scroll_drag,
                ) {
                    let track_w = drag.viewport_w.max(1.0);
                    let thumb_w =
                        (track_w * (drag.viewport_w / drag.content_w)).clamp(32.0, track_w);
                    let max_scroll = (drag.content_w - drag.viewport_w).max(0.0);
                    let track_range = (track_w - thumb_w).max(1.0);
                    let thumb_x =
                        (pos.x - drag.viewport_x - drag.grab_offset).clamp(0.0, track_range);
                    state
                        .block_scroll_x
                        .insert(drag.block_id, (thumb_x / track_range) * max_scroll);
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.is_dragging = false;
                state.horizontal_scroll_drag = None;
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let Some(pos) = _cursor.position_in(_layout.bounds()) else {
                    return;
                };
                let Some(block_id) = self.block_at_y::<R>(
                    pos.y,
                    _layout.bounds().width,
                    self.lines.get(self.buffer.cursor_line).map(|l| l.block_id),
                    state.is_focused,
                ) else {
                    return;
                };
                let viewport_w =
                    (_layout.bounds().width - TEXT_X_OFFSET - MARGIN_RIGHT - 24.0).max(80.0);
                let content_w = self.block_content_width::<R>(
                    block_id,
                    _layout.bounds().width,
                    state.is_focused,
                );
                let max_scroll = (content_w - viewport_w).max(0.0);
                if max_scroll <= 0.0 {
                    return;
                }

                let (dx, dy) = match delta {
                    mouse::ScrollDelta::Lines { x, y } => (*x * 48.0, *y * 48.0),
                    mouse::ScrollDelta::Pixels { x, y } => (*x, *y),
                };
                let horizontal_delta = if dx.abs() > 0.0 {
                    dx
                } else if state.modifiers.shift() {
                    -dy
                } else {
                    0.0
                };
                if horizontal_delta.abs() > 0.0 {
                    let entry = state.block_scroll_x.entry(block_id).or_insert(0.0);
                    *entry = (*entry + horizontal_delta).clamp(0.0, max_scroll);
                }
            }

            // ── keyboard ─────────────────────────────────────────
            Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                state.modifiers = *m;
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) if state.is_focused => {
                state.modifiers = *modifiers;

                if !matches!(
                    key.as_ref(),
                    keyboard::Key::Named(keyboard::key::Named::ArrowUp)
                        | keyboard::Key::Named(keyboard::key::Named::ArrowDown)
                ) {
                    state.desired_visual_x = None;
                }

                // Named keys first — they must never fall through to char input
                match key.as_ref() {
                    keyboard::Key::Named(keyboard::key::Named::Backspace) => {
                        shell.publish((self.on_command)(EditorCommand::DeleteBackward));
                        state.selection_anchor = None;
                        state.selection_focus = None;
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::Delete) => {
                        shell.publish((self.on_command)(EditorCommand::DeleteForward));
                        state.selection_anchor = None;
                        state.selection_focus = None;
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::Enter) => {
                        shell.publish((self.on_command)(EditorCommand::InsertText(
                            "\n".to_string(),
                        )));
                        state.selection_anchor = None;
                        state.selection_focus = None;
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
                        shell.publish((self.on_command)(EditorCommand::MoveCursor {
                            movement: Movement::Left,
                            extend: modifiers.shift(),
                        }));
                        state.selection_anchor = None;
                        state.selection_focus = None;
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
                        shell.publish((self.on_command)(EditorCommand::MoveCursor {
                            movement: Movement::Right,
                            extend: modifiers.shift(),
                        }));
                        state.selection_anchor = None;
                        state.selection_focus = None;
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                        let (new_line, new_col) =
                            self.move_visual::<R>(state, -1.0, _layout.bounds().width);
                        if modifiers.shift() {
                            let (a_l, a_c) = state
                                .selection_anchor
                                .or_else(|| self.buffer.selection.map(|(sl, sc, _, _)| (sl, sc)))
                                .unwrap_or((self.buffer.cursor_line, self.buffer.cursor_col));
                            state.selection_anchor = Some((a_l, a_c));
                            state.selection_focus = Some((new_line, new_col));
                            shell.publish((self.on_command)(EditorCommand::SetSelection {
                                anchor_line: a_l,
                                anchor_col: a_c,
                                focus_line: new_line,
                                focus_col: new_col,
                            }));
                        } else {
                            state.selection_anchor = None;
                            state.selection_focus = None;
                            shell.publish((self.on_command)(EditorCommand::SetCursor {
                                line: new_line,
                                col: new_col,
                            }));
                        }
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                        let (new_line, new_col) =
                            self.move_visual::<R>(state, 1.0, _layout.bounds().width);
                        if modifiers.shift() {
                            let (a_l, a_c) = state
                                .selection_anchor
                                .or_else(|| self.buffer.selection.map(|(sl, sc, _, _)| (sl, sc)))
                                .unwrap_or((self.buffer.cursor_line, self.buffer.cursor_col));
                            state.selection_anchor = Some((a_l, a_c));
                            state.selection_focus = Some((new_line, new_col));
                            shell.publish((self.on_command)(EditorCommand::SetSelection {
                                anchor_line: a_l,
                                anchor_col: a_c,
                                focus_line: new_line,
                                focus_col: new_col,
                            }));
                        } else {
                            state.selection_anchor = None;
                            state.selection_focus = None;
                            shell.publish((self.on_command)(EditorCommand::SetCursor {
                                line: new_line,
                                col: new_col,
                            }));
                        }
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::Home) => {
                        shell.publish((self.on_command)(EditorCommand::MoveCursor {
                            movement: Movement::Home,
                            extend: modifiers.shift(),
                        }));
                        state.selection_anchor = None;
                        state.selection_focus = None;
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::End) => {
                        shell.publish((self.on_command)(EditorCommand::MoveCursor {
                            movement: Movement::End,
                            extend: modifiers.shift(),
                        }));
                        state.selection_anchor = None;
                        state.selection_focus = None;
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::Tab) => {
                        shell.publish((self.on_command)(EditorCommand::InsertText(
                            "    ".to_string(),
                        )));
                        state.selection_anchor = None;
                        state.selection_focus = None;
                        return;
                    }
                    _ => {}
                }

                // Ctrl / Cmd shortcuts
                if modifiers.command() || modifiers.control() {
                    match key.as_ref() {
                        keyboard::Key::Character(c) if c == "z" => {
                            shell.publish((self.on_command)(EditorCommand::Undo));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "y" => {
                            shell.publish((self.on_command)(EditorCommand::Redo));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "a" => {
                            shell.publish((self.on_command)(EditorCommand::SelectAll));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "b" => {
                            shell.publish((self.on_command)(EditorCommand::FormatBold));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "i" => {
                            shell.publish((self.on_command)(EditorCommand::FormatItalic));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "e" => {
                            shell.publish((self.on_command)(EditorCommand::FormatInlineCode));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "k" => {
                            shell.publish((self.on_command)(EditorCommand::InsertLink));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "c" => {
                            if let Some(selected) = self
                                .buffer
                                .selected_text()
                                .or_else(|| self.selected_text(state))
                            {
                                _clipboard
                                    .write(iced::advanced::clipboard::Kind::Standard, selected);
                            }
                        }
                        keyboard::Key::Character(c) if c == "x" => {
                            if let Some(selected) = self
                                .buffer
                                .selected_text()
                                .or_else(|| self.selected_text(state))
                            {
                                _clipboard
                                    .write(iced::advanced::clipboard::Kind::Standard, selected);
                                shell.publish((self.on_command)(EditorCommand::DeleteSelection));
                                state.selection_anchor = None;
                                state.selection_focus = None;
                            }
                        }
                        keyboard::Key::Character(c) if c == "v" => {
                            if let Some(text) =
                                _clipboard.read(iced::advanced::clipboard::Kind::Standard)
                            {
                                shell.publish((self.on_command)(EditorCommand::InsertText(text)));
                                state.selection_anchor = None;
                                state.selection_focus = None;
                            }
                        }
                        _ => {}
                    }
                    return;
                }

                // Printable character input
                if let Some(t) = text {
                    if let Some(c) = t.chars().next() {
                        if !c.is_control() {
                            shell.publish((self.on_command)(EditorCommand::InsertText(
                                t.to_string(),
                            )));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

// ── Private helpers on Editor ────────────────────────────────────────

impl<'a, Message> Editor<'a, Message> {
    fn position_for_col<R>(
        &self,
        line_idx: usize,
        col: usize,
        available_width: f32,
        is_editing: bool,
        active_col: Option<usize>,
    ) -> (f32, f32)
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let Some(line) = self.lines.get(line_idx) else {
            return (0.0, 0.0);
        };
        if line.is_code_block {
            let mut x = 0.0_f32;
            let mut source_col = 0usize;
            for span in &line.spans {
                let display = span.visible_text(is_editing);
                for ch in display.chars() {
                    if source_col >= col {
                        return (x, 0.0);
                    }
                    x += measure_width::<R>(&ch.to_string(), 15.0, iced::Font::MONOSPACE);
                    source_col += 1;
                }
            }
            return (x, 0.0);
        }
        let max_w = (available_width - TEXT_X_OFFSET - MARGIN_RIGHT).max(80.0);
        let mut x = 0.0_f32;
        let mut y = 0.0_f32;
        let mut source_col = 0usize;

        for (span_idx, span) in line.spans.iter().enumerate() {
            let font = span_font(span, line);
            let span_editing = is_editing
                || active_col.is_some_and(|col| span_is_inline_edit_target(line, span_idx, col));
            let display = span_visible_text(line, span_idx, is_editing, active_col);
            let span_start_col = source_col;
            let span_end_col = source_col_after_span(span, span_start_col);
            if display.is_empty() {
                if col <= span_end_col {
                    return (x, y);
                }
                source_col = span_end_col;
                continue;
            }
            let step = visual_line_step(span.font_size);
            let mut token = Vec::new();
            let flush_token =
                |token: &mut Vec<(char, usize)>, x: &mut f32, y: &mut f32| -> Option<(f32, f32)> {
                    if token.is_empty() {
                        return None;
                    }

                    let token_width = token
                        .iter()
                        .map(|(ch, _)| measure_width::<R>(&ch.to_string(), span.font_size, font))
                        .sum::<f32>();

                    if *x > 0.0 && *x + token_width > max_w {
                        *y += step;
                        *x = 0.0;
                    }

                    if token_width <= max_w {
                        for (ch, ch_col) in token.iter() {
                            if *ch_col >= col {
                                return Some((*x, *y));
                            }
                            *x += measure_width::<R>(&ch.to_string(), span.font_size, font);
                        }
                    } else {
                        for (ch, ch_col) in token.iter() {
                            let ch_w = measure_width::<R>(&ch.to_string(), span.font_size, font);
                            if *x > 0.0 && *x + ch_w > max_w {
                                *y += step;
                                *x = 0.0;
                            }
                            if *ch_col >= col {
                                return Some((*x, *y));
                            }
                            *x += ch_w;
                        }
                    }

                    token.clear();
                    None
                };

            for ch in display.chars() {
                if span.is_checkbox && !is_editing {
                    if source_col >= col {
                        return (x, y);
                    }
                    x += 26.0;
                    source_col += 1;
                    continue;
                }

                if span.is_math && !span_editing {
                    let tex = span.visible_text(false).trim_matches('$').trim();
                    if !tex.is_empty() && !span.is_syntax {
                        let (width, _) = self
                            .math_cache
                            .get(tex)
                            .map(|(_, w, h)| (*w, *h))
                            .unwrap_or_else(|| {
                                (
                                    measure_width::<R>(tex, span.font_size, font),
                                    BASE_LINE_HEIGHT,
                                )
                            });

                        if x > 0.0 && x + width > max_w {
                            y += step;
                            x = 0.0;
                        }

                        if source_col >= col {
                            return (x, y);
                        }

                        x += width + 4.0;
                        // Since this is a single block, if col is within it, return x, y
                        if col <= span_end_col {
                            return (x, y);
                        }
                        break; // Move to next span
                    }
                }
                token.push((ch, source_col));
                source_col += 1;
                if ch.is_whitespace() {
                    if let Some(pos) = flush_token(&mut token, &mut x, &mut y) {
                        return pos;
                    }
                }
            }
            if let Some(pos) = flush_token(&mut token, &mut x, &mut y) {
                return pos;
            }
            if col <= span_end_col {
                return (x, y);
            }
            source_col = span_end_col;
        }
        (x, y)
    }

    fn col_for_visual_point<R>(
        &self,
        line: &StyledLine,
        click_x: f32,
        line_y: f32,
        available_width: f32,
        is_editing: bool,
        active_col: Option<usize>,
    ) -> usize
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        if click_x <= 0.0 {
            return 0;
        }

        let max_w = (available_width - TEXT_X_OFFSET - MARGIN_RIGHT).max(80.0);
        let mut x_acc = 0.0_f32;
        let mut row_y = 0.0_f32;
        let mut source_col = 0usize;
        let mut row_start_col = 0usize;
        let mut row_end_col = 0usize;
        let mut row_step = BASE_LINE_HEIGHT;

        for (span_idx, span) in line.spans.iter().enumerate() {
            let font = span_font(span, line);
            let span_editing = is_editing
                || active_col.is_some_and(|col| span_is_inline_edit_target(line, span_idx, col));
            let display = span_visible_text(line, span_idx, is_editing, active_col);
            let span_start_col = source_col;
            let span_end_col = source_col_after_span(span, span_start_col);

            if display.is_empty() {
                source_col = span_end_col;
                row_end_col = source_col;
                continue;
            }

            let step = visual_line_step(span.font_size);
            row_step = row_step.max(step);
            let mut token = Vec::new();
            let flush_token = |token: &mut Vec<(char, usize)>,
                               x_acc: &mut f32,
                               row_y: &mut f32,
                               row_start_col: &mut usize,
                               row_end_col: &mut usize,
                               row_step: &mut f32|
             -> Option<usize> {
                if token.is_empty() {
                    return None;
                }

                let token_width = token
                    .iter()
                    .map(|(ch, _)| measure_width::<R>(&ch.to_string(), span.font_size, font))
                    .sum::<f32>();

                if *x_acc > 0.0 && *x_acc + token_width > max_w {
                    if line_y < *row_y + *row_step {
                        return Some(*row_end_col);
                    }
                    *row_y += *row_step;
                    *x_acc = 0.0;
                    *row_start_col = token.first().map(|(_, col)| *col).unwrap_or(*row_end_col);
                    *row_end_col = *row_start_col;
                    *row_step = step;
                }

                if token_width <= max_w {
                    if line_y < *row_y + *row_step {
                        for (ch, ch_col) in token.iter() {
                            let cw = measure_width::<R>(&ch.to_string(), span.font_size, font);
                            if click_x < *x_acc + cw * 0.6 {
                                return Some(*ch_col);
                            }
                            *row_end_col = *ch_col + 1;
                            *x_acc += cw;
                        }
                    } else {
                        *x_acc += token_width;
                        if let Some((_, last_col)) = token.last() {
                            *row_end_col = *last_col + 1;
                        }
                    }
                } else {
                    for (ch, ch_col) in token.iter() {
                        let cw = measure_width::<R>(&ch.to_string(), span.font_size, font);
                        if *x_acc > 0.0 && *x_acc + cw > max_w {
                            if line_y < *row_y + *row_step {
                                return Some(*row_end_col);
                            }
                            *row_y += *row_step;
                            *x_acc = 0.0;
                            *row_start_col = *ch_col;
                            *row_end_col = *ch_col;
                            *row_step = step;
                        }

                        if line_y < *row_y + *row_step {
                            if click_x < *x_acc + cw * 0.6 {
                                return Some(*ch_col);
                            }
                            *row_end_col = *ch_col + 1;
                        }
                        *x_acc += cw;
                    }
                }

                token.clear();
                None
            };

            for ch in display.chars() {
                if span.is_checkbox && !is_editing {
                    let cw = 26.0;
                    if x_acc > 0.0 && x_acc + cw > max_w {
                        if line_y < row_y + row_step {
                            return row_end_col;
                        }
                        row_y += row_step;
                        x_acc = 0.0;
                        row_start_col = source_col;
                        row_end_col = source_col;
                        row_step = step;
                    }

                    if line_y < row_y + row_step {
                        if click_x < x_acc + cw * 0.6 {
                            return source_col;
                        }
                        row_end_col = source_col + 1;
                    }

                    x_acc += cw;
                    source_col += 1;
                    continue;
                }

                if span.is_math && !span_editing {
                    let tex = span.visible_text(false).trim_matches('$').trim();
                    if !tex.is_empty() && !span.is_syntax {
                        let (width, height) = self
                            .math_cache
                            .get(tex)
                            .map(|(_, w, h)| (*w, *h))
                            .unwrap_or_else(|| {
                                (
                                    measure_width::<R>(tex, span.font_size, font),
                                    BASE_LINE_HEIGHT,
                                )
                            });

                        let extra_h = (height - BASE_LINE_HEIGHT).max(0.0);
                        row_step = row_step.max(BASE_LINE_HEIGHT + extra_h);

                        if x_acc > 0.0 && x_acc + width > max_w {
                            if line_y < row_y + row_step {
                                return row_end_col;
                            }
                            row_y += row_step;
                            x_acc = 0.0;
                            row_start_col = source_col;
                            row_end_col = source_col;
                            row_step = step;
                        }

                        if line_y < row_y + row_step {
                            if click_x < x_acc + width {
                                return source_col;
                            }
                            row_end_col = span_end_col;
                        }

                        x_acc += width + 4.0;
                        break; // Skip token loop entirely for this span
                    }
                }
                token.push((ch, source_col));
                source_col += 1;
                if ch.is_whitespace() {
                    if let Some(col) = flush_token(
                        &mut token,
                        &mut x_acc,
                        &mut row_y,
                        &mut row_start_col,
                        &mut row_end_col,
                        &mut row_step,
                    ) {
                        return col;
                    }
                }
            }
            if let Some(col) = flush_token(
                &mut token,
                &mut x_acc,
                &mut row_y,
                &mut row_start_col,
                &mut row_end_col,
                &mut row_step,
            ) {
                return col;
            }

            source_col = span_end_col;
            if line_y < row_y + row_step {
                row_end_col = source_col;
            }
        }

        if line_y < row_y + row_step {
            row_end_col.max(row_start_col)
        } else {
            source_col
        }
    }

    fn selected_text(&self, state: &State) -> Option<String> {
        let ((start_line, start_col), (end_line, end_col)) =
            normalized_selection(state.selection_anchor, state.selection_focus)?;

        let mut out = String::new();
        for line_idx in start_line..=end_line {
            let line = self.buffer.line_text(line_idx);
            let line_len = line.chars().count();
            let from = if line_idx == start_line {
                start_col.min(line_len)
            } else {
                0
            };
            let to = if line_idx == end_line {
                end_col.min(line_len)
            } else {
                line_len
            };
            if from < to {
                out.push_str(&line.chars().skip(from).take(to - from).collect::<String>());
            }
            if line_idx != end_line {
                out.push('\n');
            }
        }

        if out.is_empty() { None } else { Some(out) }
    }

    fn cursor_position<R>(&self, line_idx: usize, available_width: f32) -> (f32, f32)
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let Some(line) = self.lines.get(line_idx) else {
            return (0.0, 0.0);
        };
        let is_editing = is_block_editing_line(
            line,
            self.lines.get(self.buffer.cursor_line).map(|l| l.block_id),
            true,
        );
        self.position_for_col::<R>(
            line_idx,
            self.buffer.cursor_col,
            available_width,
            is_editing,
            (line_idx == self.buffer.cursor_line).then_some(self.buffer.cursor_col),
        )
    }

    fn block_at_y<R>(
        &self,
        pos_y: f32,
        available_width: f32,
        active_block_id: Option<usize>,
        focused: bool,
    ) -> Option<usize>
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let mut y_acc = TOP_PAD;
        let mut seen_math_blocks = std::collections::HashSet::new();
        for (i, line) in self.lines.iter().enumerate() {
            let is_editing = is_block_editing_line(line, active_block_id, focused);
            let active_col =
                (focused && i == self.buffer.cursor_line).then_some(self.buffer.cursor_col);
            let lh = line_height_for::<R>(
                line,
                self.image_cache,
                self.math_cache,
                available_width,
                is_editing,
                active_col,
                &mut seen_math_blocks,
            );
            if pos_y >= y_acc && pos_y < y_acc + lh {
                if line.is_code_block || line.is_table_row || line.is_math_block {
                    return Some(line.block_id);
                }
                return None;
            }
            let gutter = table_block_gutter_after(self.lines, i, is_editing);
            if pos_y >= y_acc + lh && pos_y < y_acc + lh + gutter {
                return Some(line.block_id);
            }
            y_acc += lh + gutter;
        }
        None
    }

    fn block_content_width<R>(&self, block_id: usize, available_width: f32, focused: bool) -> f32
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let active_block_id = self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
        let mut max_width = 0.0_f32;
        let mut table_widths: Vec<f32> = Vec::new();
        for line in self.lines.iter().filter(|line| line.block_id == block_id) {
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

    fn horizontal_scrollbar_hit<R>(
        &self,
        pos: Point,
        available_width: f32,
        state: &State,
    ) -> Option<HorizontalScrollDrag>
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let active_block_id = self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
        let focused = state.is_focused;
        let viewport_w = (available_width - TEXT_X_OFFSET - MARGIN_RIGHT - 24.0).max(80.0);
        let viewport_x = TEXT_X_OFFSET;
        let mut y_acc = TOP_PAD;
        let mut seen_math_blocks = std::collections::HashSet::new();
        let mut block_start_y: Option<(usize, f32)> = None;
        let mut block_height = 0.0_f32;

        for (i, line) in self.lines.iter().enumerate() {
            let is_editing = is_block_editing_line(line, active_block_id, focused);
            let active_col =
                (focused && i == self.buffer.cursor_line).then_some(self.buffer.cursor_col);
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
            let scrollable_block = line.is_code_block || line.is_table_row || line.is_math_block;

            if scrollable_block {
                if block_start_y
                    .map(|(block_id, _)| block_id != line.block_id)
                    .unwrap_or(false)
                {
                    if let Some(hit) = self.scrollbar_hit_for_block::<R>(
                        pos,
                        block_start_y.unwrap().0,
                        block_start_y.unwrap().1,
                        block_height,
                        viewport_x,
                        viewport_w,
                        available_width,
                        focused,
                        state,
                    ) {
                        return Some(hit);
                    }
                    block_start_y = Some((line.block_id, y_acc));
                    block_height = 0.0;
                } else if block_start_y.is_none() {
                    block_start_y = Some((line.block_id, y_acc));
                }
                block_height += lh + gutter;
            } else if let Some((block_id, y)) = block_start_y.take() {
                if let Some(hit) = self.scrollbar_hit_for_block::<R>(
                    pos,
                    block_id,
                    y,
                    block_height,
                    viewport_x,
                    viewport_w,
                    available_width,
                    focused,
                    state,
                ) {
                    return Some(hit);
                }
                block_height = 0.0;
            }

            y_acc += lh + gutter;
        }

        if let Some((block_id, y)) = block_start_y {
            return self.scrollbar_hit_for_block::<R>(
                pos,
                block_id,
                y,
                block_height,
                viewport_x,
                viewport_w,
                available_width,
                focused,
                state,
            );
        }

        None
    }

    #[allow(clippy::too_many_arguments)]
    fn scrollbar_hit_for_block<R>(
        &self,
        pos: Point,
        block_id: usize,
        block_y: f32,
        block_h: f32,
        viewport_x: f32,
        viewport_w: f32,
        available_width: f32,
        focused: bool,
        state: &State,
    ) -> Option<HorizontalScrollDrag>
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let content_w = self.block_content_width::<R>(block_id, available_width, focused);
        if content_w <= viewport_w + 1.0 {
            return None;
        }

        let scrollbar_y = block_y + block_h - 7.0;
        if pos.x < viewport_x
            || pos.x > viewport_x + viewport_w
            || pos.y < scrollbar_y - 8.0
            || pos.y > scrollbar_y + 10.0
        {
            return None;
        }

        let scroll = state
            .block_scroll_x
            .get(&block_id)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, (content_w - viewport_w).max(0.0));
        let track_w = viewport_w.max(1.0);
        let thumb_w = (track_w * (viewport_w / content_w)).clamp(32.0, track_w);
        let thumb_x = viewport_x + ((track_w - thumb_w) * (scroll / (content_w - viewport_w)));
        let grab_offset = if pos.x >= thumb_x && pos.x <= thumb_x + thumb_w {
            pos.x - thumb_x
        } else {
            thumb_w / 2.0
        };

        Some(HorizontalScrollDrag {
            block_id,
            viewport_x,
            viewport_w,
            content_w,
            grab_offset,
        })
    }

    /// Convert a click position (relative to widget bounds) into (line, col).
    fn hit_test<R>(
        &self,
        pos: Point,
        available_width: f32,
        active_block_id: Option<usize>,
        focused: bool,
    ) -> (usize, usize)
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let mut y_acc = TOP_PAD;
        let mut line_idx = 0;
        let mut seen_math_blocks = std::collections::HashSet::new();

        for (i, line) in self.lines.iter().enumerate() {
            let is_editing = is_block_editing_line(line, active_block_id, focused);
            let active_col =
                (focused && i == self.buffer.cursor_line).then_some(self.buffer.cursor_col);
            let lh = line_height_for::<R>(
                line,
                self.image_cache,
                self.math_cache,
                available_width,
                is_editing,
                active_col,
                &mut seen_math_blocks,
            );
            if pos.y < y_acc + lh {
                line_idx = i;
                break;
            }
            let gutter = table_block_gutter_after(self.lines, i, is_editing);
            if pos.y < y_acc + lh + gutter {
                line_idx = i;
                break;
            }
            y_acc += lh + gutter;
            line_idx = i; // clamp to last
        }

        // Horizontal: walk spans character by character
        let Some(line) = self.lines.get(line_idx) else {
            return (line_idx, 0);
        };
        let click_x = pos.x - TEXT_X_OFFSET;
        if click_x <= 0.0 {
            return (line_idx, 0);
        }

        let is_editing = is_block_editing_line(line, active_block_id, focused);
        let col = self.col_for_visual_point::<R>(
            line,
            click_x,
            pos.y - y_acc,
            available_width,
            is_editing,
            (focused && line_idx == self.buffer.cursor_line).then_some(self.buffer.cursor_col),
        );
        (line_idx, col)
    }

    fn move_visual<R>(
        &self,
        state: &mut State,
        delta_lines: f32,
        available_width: f32,
    ) -> (usize, usize)
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let (cur_x, cur_y_in_line) =
            self.cursor_position::<R>(self.buffer.cursor_line, available_width);
        let cur_y_base = line_visual_y::<R>(
            self.lines,
            self.image_cache,
            self.math_cache,
            available_width,
            self.buffer.cursor_line,
            self.buffer.cursor_col,
            self.buffer.cursor_line,
            state.is_focused,
        );

        let visual_x = state.desired_visual_x.get_or_insert(cur_x);
        let line = &self.lines[self.buffer.cursor_line];
        let max_font = line
            .spans
            .iter()
            .map(|s| s.font_size)
            .fold(17.0_f32, f32::max);
        let step = visual_line_step(max_font);

        let target_y = cur_y_base + cur_y_in_line + delta_lines * step + step / 2.0;

        let mut target = self.hit_test::<R>(
            Point::new(*visual_x + TEXT_X_OFFSET, target_y),
            available_width,
            self.lines.get(self.buffer.cursor_line).map(|l| l.block_id),
            state.is_focused,
        );

        let current = (self.buffer.cursor_line, self.buffer.cursor_col);
        if delta_lines > 0.0 && target <= current {
            target = self.fallback_visual_line_move::<R>(*visual_x, 1, available_width);
        } else if delta_lines < 0.0 && target >= current {
            target = self.fallback_visual_line_move::<R>(*visual_x, -1, available_width);
        }

        target
    }

    fn fallback_visual_line_move<R>(
        &self,
        visual_x: f32,
        delta_lines: isize,
        available_width: f32,
    ) -> (usize, usize)
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let current_line = self.buffer.cursor_line;
        let target_line = if delta_lines < 0 {
            current_line.saturating_sub(1)
        } else {
            (current_line + 1).min(self.lines.len().saturating_sub(1))
        };

        if target_line == current_line {
            return (self.buffer.cursor_line, self.buffer.cursor_col);
        }

        let Some(line) = self.lines.get(target_line) else {
            return (self.buffer.cursor_line, self.buffer.cursor_col);
        };
        let active_block_id = self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
        let is_editing = is_block_editing_line(line, active_block_id, true);
        let col = self.col_for_visual_point::<R>(
            line,
            visual_x,
            BASE_LINE_HEIGHT / 2.0,
            available_width,
            is_editing,
            None,
        );
        (target_line, col)
    }
}

// ── Into<Element> ────────────────────────────────────────────────────

impl<'a, Message, Theme, R> From<Editor<'a, Message>> for Element<'a, Message, Theme, R>
where
    R: renderer::Renderer
        + iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::image::Renderer<Handle = iced::widget::image::Handle>,
    Message: 'a,
{
    fn from(editor: Editor<'a, Message>) -> Self {
        Self::new(editor)
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::buffer::{DocBuffer, EditorCommand};
    use crate::editor::highlight::{StyledLine, StyledSpan, highlight_markdown};
    use std::collections::HashMap;

    fn make_line(block_id: usize, spans: Vec<StyledSpan>) -> StyledLine {
        let mut line = StyledLine::new();
        line.block_id = block_id;
        line.spans = spans;
        line
    }

    fn editor_for<'a>(
        buffer: &'a DocBuffer,
        lines: &'a [StyledLine],
        image_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
        math_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    ) -> Editor<'a, ()> {
        Editor::new(
            buffer,
            lines,
            image_cache,
            math_cache,
            |_| (),
            |_| (),
            |_| (),
        )
    }

    fn test_state() -> State {
        State {
            is_dragging: false,
            is_focused: true,
            modifiers: keyboard::Modifiers::default(),
            selection_anchor: None,
            selection_focus: None,
            block_scroll_x: HashMap::new(),
            horizontal_scroll_drag: None,
            desired_visual_x: None,
        }
    }

    #[test]
    fn test_normalized_selection_combinatorics() {
        // Run thousands of combinations of boundary cases for selections
        for anchor_line in 0..15 {
            for anchor_col in 0..10 {
                for focus_line in 0..15 {
                    for focus_col in 0..10 {
                        let norm = normalized_selection(
                            Some((anchor_line, anchor_col)),
                            Some((focus_line, focus_col)),
                        );
                        if (anchor_line, anchor_col) == (focus_line, focus_col) {
                            assert!(norm.is_none());
                        } else {
                            let (start, end) = norm.unwrap();
                            assert!(start <= end);
                            if anchor_line < focus_line {
                                assert_eq!(start, (anchor_line, anchor_col));
                                assert_eq!(end, (focus_line, focus_col));
                            } else if anchor_line > focus_line {
                                assert_eq!(start, (focus_line, focus_col));
                                assert_eq!(end, (anchor_line, anchor_col));
                            } else {
                                assert_eq!(start, (anchor_line, anchor_col.min(focus_col)));
                                assert_eq!(end, (anchor_line, anchor_col.max(focus_col)));
                            }
                        }
                    }
                }
            }
        }

        assert!(normalized_selection(None, None).is_none());
        assert!(normalized_selection(Some((1, 1)), None).is_none());
        assert!(normalized_selection(None, Some((2, 2))).is_none());
    }

    #[test]
    fn test_editor_selected_text_extraction() {
        let buffer = DocBuffer::from_text("line one\nline two\nline three\nline four");
        let lines: Vec<StyledLine> = vec![
            make_line(1, vec![StyledSpan::plain("line one")]),
            make_line(2, vec![StyledSpan::plain("line two")]),
            make_line(3, vec![StyledSpan::plain("line three")]),
            make_line(4, vec![StyledSpan::plain("line four")]),
        ];

        let image_cache = HashMap::new();
        let math_cache = HashMap::new();

        let editor = Editor::new(
            &buffer,
            &lines,
            &image_cache,
            &math_cache,
            |_| (),
            |_| (),
            |_| (),
        );

        // Perform combinatorial selections over the entire document
        for start_line in 0..4 {
            for start_col in 0..10 {
                for end_line in 0..4 {
                    for end_col in 0..10 {
                        let state = State {
                            is_dragging: false,
                            is_focused: true,
                            modifiers: keyboard::Modifiers::default(),
                            selection_anchor: Some((start_line, start_col)),
                            selection_focus: Some((end_line, end_col)),
                            block_scroll_x: HashMap::new(),
                            horizontal_scroll_drag: None,
                            desired_visual_x: None,
                        };

                        let sel = editor.selected_text(&state);
                        if let Some(((s_l, s_c), (e_l, e_c))) = normalized_selection(
                            Some((start_line, start_col)),
                            Some((end_line, end_col)),
                        ) {
                            let mut manual = String::new();
                            for l in s_l..=e_l {
                                let content = buffer.line_text(l);
                                let from = if l == s_l {
                                    s_c.min(content.chars().count())
                                } else {
                                    0
                                };
                                let to = if l == e_l {
                                    e_c.min(content.chars().count())
                                } else {
                                    content.chars().count()
                                };
                                if from < to {
                                    manual.push_str(
                                        &content
                                            .chars()
                                            .skip(from)
                                            .take(to - from)
                                            .collect::<String>(),
                                    );
                                }
                                if l != e_l {
                                    manual.push('\n');
                                }
                            }
                            if manual.is_empty() {
                                assert!(sel.is_none());
                            } else {
                                assert_eq!(sel.unwrap(), manual);
                            }
                        } else {
                            assert!(sel.is_none());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn typora_inline_editing_reveals_only_active_span_and_markers() {
        let lines = highlight_markdown("alpha **bold** omega");
        let line = &lines[0];

        let rendered = line
            .spans
            .iter()
            .enumerate()
            .map(|(idx, _)| span_visible_text(line, idx, false, None))
            .collect::<Vec<_>>();
        assert_eq!(rendered, vec!["alpha ", "", "bold", "", " omega"]);

        let active_inside_bold = "alpha **bo".chars().count();
        let editing = line
            .spans
            .iter()
            .enumerate()
            .map(|(idx, _)| span_visible_text(line, idx, false, Some(active_inside_bold)))
            .collect::<Vec<_>>();
        assert_eq!(editing, vec!["alpha ", "**", "bold", "**", " omega"]);

        let active_inside_plain = "al".chars().count();
        let editing_plain = line
            .spans
            .iter()
            .enumerate()
            .map(|(idx, _)| span_visible_text(line, idx, false, Some(active_inside_plain)))
            .collect::<Vec<_>>();
        assert_eq!(editing_plain, vec!["alpha ", "", "bold", "", " omega"]);
    }

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
        let mut seen_math_blocks = std::collections::HashSet::new();

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
    fn visual_down_movement_is_monotonic_through_wrapped_markdown_lines() {
        let text = concat!(
            "alpha **bold text with enough words to wrap around the editor width** omega\n",
            "second line with `inline code` and more words to move through\n",
            "third line ends here"
        );
        let mut buffer = DocBuffer::from_text(text);
        buffer.execute(EditorCommand::SetCursor { line: 0, col: 0 });
        let image_cache = HashMap::new();
        let math_cache = HashMap::new();
        let mut previous = (buffer.cursor_line, buffer.cursor_col);

        for _ in 0..12 {
            let lines = highlight_markdown(&buffer.text());
            let editor = editor_for(&buffer, &lines, &image_cache, &math_cache);
            let mut state = test_state();
            let next = editor.move_visual::<iced::Renderer>(&mut state, 1.0, 260.0);

            if next == previous {
                assert_eq!(next.0, lines.len().saturating_sub(1));
                break;
            }
            assert!(
                next > previous,
                "visual down must move forward, previous={previous:?}, next={next:?}"
            );
            drop(editor);
            buffer.execute(EditorCommand::SetCursor {
                line: next.0,
                col: next.1,
            });
            previous = next;
        }

        assert_eq!(previous.0, 2);
    }

    #[test]
    fn visual_down_moves_through_empty_lines_without_vanishing() {
        let text = "first\n\nthird\n\nfifth";
        let mut buffer = DocBuffer::from_text(text);
        buffer.execute(EditorCommand::SetCursor { line: 0, col: 2 });
        let image_cache = HashMap::new();
        let math_cache = HashMap::new();

        let mut visited = Vec::new();
        for _ in 0..8 {
            let lines = highlight_markdown(&buffer.text());
            let editor = editor_for(&buffer, &lines, &image_cache, &math_cache);
            let mut state = test_state();
            let next = editor.move_visual::<iced::Renderer>(&mut state, 1.0, 900.0);
            visited.push(next);
            drop(editor);
            buffer.execute(EditorCommand::SetCursor {
                line: next.0,
                col: next.1,
            });
            if next.0 == lines.len().saturating_sub(1) {
                break;
            }
        }

        assert!(
            visited.iter().any(|(line, col)| *line == 1 && *col == 0),
            "down should visit first empty line, visited={visited:?}"
        );
        assert!(
            visited.iter().any(|(line, col)| *line == 3 && *col == 0),
            "down should visit second empty line, visited={visited:?}"
        );
        assert_eq!(buffer.cursor_line, 4);
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
            TOP_PAD + 34.0 + 34.0 + HORIZONTAL_SCROLLBAR_GUTTER
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
            (
                iced::widget::image::Handle::from_rgba(10, 10, vec![0; 400]),
                200.0,
                50.0,
            ),
        );

        let widths = vec![100.0, 200.0, 400.0, 600.0, 800.0, 1000.0, 1200.0];
        let mut seen_math_blocks = std::collections::HashSet::new();

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
                            assert_eq!(h, 34.0);
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

        // 1. Verify adding lines monotonically increases total height
        let h1 = total_height::<iced::Renderer>(
            &lines[0..50],
            &image_cache,
            &math_cache,
            800.0,
            None,
            None,
            false,
        );
        let h2 = total_height::<iced::Renderer>(
            &lines[0..100],
            &image_cache,
            &math_cache,
            800.0,
            None,
            None,
            false,
        );
        let h3 = total_height::<iced::Renderer>(
            &lines[0..200],
            &image_cache,
            &math_cache,
            800.0,
            None,
            None,
            false,
        );

        assert!(h2 > h1);
        assert!(h3 > h2);

        // 2. Verify width decreases wrapping space and monotonically increases total height
        let h_wide = total_height::<iced::Renderer>(
            &lines,
            &image_cache,
            &math_cache,
            1000.0,
            None,
            None,
            false,
        );
        let h_narrow = total_height::<iced::Renderer>(
            &lines,
            &image_cache,
            &math_cache,
            200.0,
            None,
            None,
            false,
        );

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
        let mut seen_math_blocks = std::collections::HashSet::new();

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

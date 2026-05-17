use iced::advanced::graphics::core::event::Event;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::keyboard;
use iced::mouse;
use iced::{Color, Element, Length, Point, Rectangle, Size};
use std::collections::HashMap;

use crate::editor::buffer::DocBuffer;
use crate::editor::highlight::StyledLine;
use crate::messages::EditorAction;
use crate::theme;

const MARGIN_LEFT: f32 = 64.0;
const MARGIN_RIGHT: f32 = 56.0;
const TEXT_X_OFFSET: f32 = MARGIN_LEFT;
const TOP_PAD: f32 = 24.0;
const BASE_LINE_HEIGHT: f32 = 36.0;
const IMAGE_HEIGHT: f32 = 280.0;

// ── Widget ───────────────────────────────────────────────────────────

pub struct Editor<'a, Message> {
    buffer: &'a DocBuffer,
    lines: &'a [StyledLine],
    image_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    math_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    on_change: Box<dyn Fn(String) -> Message + 'a>,
    on_cursor_move: Box<dyn Fn(usize, usize) -> Message + 'a>,
    on_action: Box<dyn Fn(EditorAction) -> Message + 'a>,
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
}

impl<'a, Message> Editor<'a, Message> {
    pub fn new(
        buffer: &'a DocBuffer,
        lines: &'a [StyledLine],
        image_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
        math_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
        on_change: impl Fn(String) -> Message + 'a,
        on_cursor_move: impl Fn(usize, usize) -> Message + 'a,
        on_action: impl Fn(EditorAction) -> Message + 'a,
        on_link_click: impl Fn(String) -> Message + 'a,
        on_checkbox_toggle: impl Fn(usize) -> Message + 'a,
    ) -> Self {
        Self {
            buffer,
            lines,
            image_cache,
            math_cache,
            on_change: Box::new(on_change),
            on_cursor_move: Box::new(on_cursor_move),
            on_action: Box::new(on_action),
            on_link_click: Box::new(on_link_click),
            on_checkbox_toggle: Box::new(on_checkbox_toggle),
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn line_height_for(
    line: &StyledLine,
    image_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    math_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    available_width: f32,
    is_editing: bool,
    seen_math_blocks: &mut std::collections::HashSet<usize>,
) -> f32 {
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
                let mut max_h: f32 = 50.0;
                for span in &line.spans {
                    let tex = span.visible_text(false).trim_matches('$').trim();
                    if let Some((_, _, h)) = math_cache.get(tex) {
                        max_h = max_h.max(*h * 1.2 + 32.0); // extra padding for equation spacing
                    } else if !tex.is_empty() {
                        let visual_lines = tex
                            .lines()
                            .map(|line| (line.chars().count() as f32 / 90.0).ceil().max(1.0))
                            .sum::<f32>()
                            .max(1.0);
                        max_h = max_h.max(visual_lines * BASE_LINE_HEIGHT + 28.0);
                    }
                }
                return max_h;
            } else {
                return 0.0;
            }
        }
    }
    if line.is_table_row {
        return BASE_LINE_HEIGHT + 12.0;
    }
    if !line.is_code_block
        && !line.is_table_row
        && !line.is_blockquote
        && !line.spans.iter().any(|s| s.is_image || s.is_math || s.is_checkbox)
    {
        let max_font = line
            .spans
            .iter()
            .map(|s| s.font_size)
            .fold(17.0_f32, f32::max);
        let char_count = line
            .spans
            .iter()
            .map(|s| s.visible_text(false).chars().count())
            .sum::<usize>() as f32;
        let available_chars = ((available_width - TEXT_X_OFFSET - MARGIN_RIGHT).max(120.0) / (max_font * 0.55)).max(12.0);
        let visual_lines = (char_count / available_chars).ceil().max(1.0);
        return if max_font > 18.0 {
            visual_lines * (max_font * 1.35).max(BASE_LINE_HEIGHT)
        } else {
            visual_lines * BASE_LINE_HEIGHT
        };
    }
    // For headings, use 1.6× the font size so larger headings get more space
    let max_font = line
        .spans
        .iter()
        .map(|s| s.font_size)
        .fold(14.0_f32, f32::max);
    if max_font > 15.0 {
        (max_font * 1.6).max(BASE_LINE_HEIGHT)
    } else {
        BASE_LINE_HEIGHT
    }
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
fn total_height(
    lines: &[StyledLine],
    image_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    math_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    width: f32,
    active_block_id: Option<usize>,
    focused: bool,
) -> f32 {
    let mut h = TOP_PAD;
    let mut seen_math_blocks = std::collections::HashSet::new();
    for line in lines {
        let is_editing = focused && Some(line.block_id) == active_block_id;
        h += line_height_for(
            line,
            image_cache,
            math_cache,
            width,
            is_editing,
            &mut seen_math_blocks,
        );
    }
    h + 80.0 // bottom padding
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
            height: Length::Fixed(total_height(
                self.lines,
                self.image_cache,
                self.math_cache,
                800.0,
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
        let h = total_height(
            self.lines,
            self.image_cache,
            self.math_cache,
            max_width,
            active_block_id,
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
        }
        let mut blocks: std::collections::HashMap<usize, BlockMeta> =
            std::collections::HashMap::new();
        let mut temp_y = bounds.y + TOP_PAD;
        let mut seen_math_blocks_layout = std::collections::HashSet::new();
        for line in self.lines.iter() {
            let is_editing = focused && Some(line.block_id) == active_block_id;
            let lh = line_height_for(
                line,
                self.image_cache,
                self.math_cache,
                bounds.width,
                is_editing,
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
                    is_editing: Some(line.block_id) == active_block_id,
                    col_widths: Vec::new(),
                });
                entry.height += lh;

                if line.is_table_row && !entry.is_editing {
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
                }
            }
            temp_y += lh;
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
                let table_width = meta.col_widths.iter().sum::<f32>();
                let table_x = bounds.x
                    + TEXT_X_OFFSET
                    + ((bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT - table_width).max(0.0) / 2.0);
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: table_x - 8.0,
                            y: meta.y,
                            width: table_width + 16.0,
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
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle {
                                x: bounds.x + TEXT_X_OFFSET - 16.0,
                                y: meta.y,
                                width: bounds.width - TEXT_X_OFFSET,
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
        let mut table_counter = 0;
        let mut last_table_block = None;

        let mut seen_math_blocks_draw = std::collections::HashSet::new();
        for (i, line) in self.lines.iter().enumerate() {
            let is_editing = focused && Some(line.block_id) == active_block_id;
            let lh = line_height_for(
                line,
                self.image_cache,
                self.math_cache,
                bounds.width,
                is_editing,
                &mut seen_math_blocks_draw,
            );

            // Viewport culling
            if y + lh < viewport.y {
                y += lh;
                continue;
            }
            if y > viewport.y + viewport.height {
                break;
            }

            if line.is_math_block && !is_editing && lh == 0.0 {
                continue;
            }

            // ── active line highlight ────────────────────────────
            if is_editing && i == self.buffer.cursor_line {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: bounds.x,
                            y,
                            width: bounds.width,
                            height: lh,
                        },
                        ..Default::default()
                    },
                    Color::from_rgba(1.0, 1.0, 1.0, 0.03),
                );
            }

            if let Some(((start_line, start_col), (end_line, end_col))) =
                normalized_selection(state.selection_anchor, state.selection_focus)
            {
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
                        let select_x =
                            bounds.x + TEXT_X_OFFSET + self.x_for_col::<R>(i, from_col, is_editing);
                        let select_w = (self.x_for_col::<R>(i, to_col, is_editing)
                            - self.x_for_col::<R>(i, from_col, is_editing))
                        .max(3.0);
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: select_x,
                                    y: y + 4.0,
                                    width: select_w,
                                    height: (lh - 8.0).max(16.0),
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
                && !line.spans.iter().any(|s| s.is_image || s.is_math || s.is_checkbox)
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

            // ── table rendering ──────────────────────────────────
            if line.is_table_row && !is_editing {
                if last_table_block != Some(line.block_id) {
                    table_counter += 1;
                    last_table_block = Some(line.block_id);
                    let caption = format!("Table {}", table_counter);
                    let caption_w = measure_width::<R>(&caption, 13.0, iced::Font::DEFAULT);
                    let available_w = bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT;
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
                            wrapping: iced::advanced::text::Wrapping::None,
                        },
                        Point::new(bounds.x + TEXT_X_OFFSET + (available_w - caption_w) / 2.0, y + 4.0),
                        theme::TEXT_MUTED,
                        *viewport,
                    );
                }

                if let Some(meta) = blocks.get(&line.block_id) {
                    let table_width: f32 = meta.col_widths.iter().sum();
                    let table_x = bounds.x
                        + TEXT_X_OFFSET
                        + ((bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT - table_width).max(0.0) / 2.0);
                    let is_first_table_row = last_table_block == Some(line.block_id)
                        && table_counter > 0
                        && (y - meta.y).abs() < 1.0;
                    let row_y = if is_first_table_row { y + 24.0 } else { y };
                    let row_h = if is_first_table_row { (lh - 24.0).max(BASE_LINE_HEIGHT) } else { lh };
                    let mut cx = table_x;

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
                            theme::BORDER,
                        );
                        y += lh;
                        continue;
                    }

                    for (c_idx, cell) in line.table_cells.iter().enumerate() {
                        if c_idx >= meta.col_widths.len() {
                            break;
                        }
                        let cw = meta.col_widths[c_idx];

                        // Draw Vertical Separator
                        if c_idx > 0 {
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: Rectangle {
                                        x: cx - 4.0,
                                        y: row_y,
                                        width: 1.0,
                                        height: row_h,
                                    },
                                    ..Default::default()
                                },
                                theme::BORDER,
                            );
                        }

                        // Draw Cell Spans
                        let mut px = cx + 8.0;
                        for span in cell {
                            let text = span.visible_text(false);
                            if text.is_empty() {
                                continue;
                            }

                            let font = span_font(span, line);
                            let fs = span.font_size;
                            let ty = row_y + (row_h - fs) / 2.0;

                            renderer.fill_text(
                                iced::advanced::text::Text {
                                    content: text.to_string(),
                                    bounds: Size::new(cw - 16.0, row_h),
                                    size: fs.into(),
                                    line_height: iced::advanced::text::LineHeight::default(),
                                    font,
                                    align_x: iced::alignment::Horizontal::Left.into(),
                                    align_y: iced::alignment::Vertical::Top.into(),
                                    shaping: iced::advanced::text::Shaping::Basic,
                                    wrapping: iced::advanced::text::Wrapping::None,
                                },
                                Point::new(px, ty),
                                span.color,
                                *viewport,
                            );
                            px += measure_width::<R>(text, fs, font);
                        }
                        cx += cw;
                    }
                }

                y += lh;
                continue;
            }

            // ── spans ────────────────────────────────────────────
            let mut x = bounds.x + TEXT_X_OFFSET;

            for span in &line.spans {
                let font = span_font(span, line);
                let is_math = span.is_math || line.is_math_block;

                // ── image ────────────────────────────────────────
                if span.is_image {
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
                            let caption_w = measure_width::<R>(&caption, 13.0, iced::Font::DEFAULT)
                                .min(draw_w);
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
                        && !is_editing
                        && span.visible_text(false).trim().is_empty()
                    {
                        continue; // Hide fences in preview
                    }
                    if span.is_syntax && !is_editing {
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
                            let fit_scale = if line.is_math_block {
                                scale.min(block_max_w / *w)
                            } else {
                                scale
                            };
                            let draw_w = w * fit_scale;
                            let draw_h = h * fit_scale;
                            drawn_w = draw_w;

                            // While editing math, show the source text only. Drawing the rendered
                            // image behind/above the source makes the edit target unreadable.
                            if is_editing {
                                // Skip drawing image, will draw text
                            } else {
                                let mut draw_x = x;
                                if line.is_math_block {
                                    equation_counter += 1;
                                    draw_x =
                                        bounds.x + TEXT_X_OFFSET + (available_w - draw_w) / 2.0;

                                    // Equation number right aligned
                                    let eq_num = format!("({})", equation_counter);
                                    let eq_w = measure_width::<R>(&eq_num, 14.0, iced::Font::DEFAULT);
                                    let eq_y = y + (lh - draw_h) / 2.0; // center with the equation
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
                                        y: y + (lh - draw_h) / 2.0,
                                        width: draw_w,
                                        height: draw_h,
                                    },
                                    *viewport,
                                );
                                image_rendered = true;
                            }
                        }
                    }

                    if image_rendered
                        && (line.is_math_block || (!line.is_math_block && !is_editing))
                    {
                        x += drawn_w + 4.0;
                        continue;
                    }

                    if line.is_math_block && !is_editing && !tex.is_empty() {
                        equation_counter += 1;
                        let available_w = bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT;
                        let mut text_y = y + 14.0;
                        for raw_math_line in tex.lines() {
                            let line_w =
                                measure_width::<R>(raw_math_line, 16.0, iced::Font::MONOSPACE)
                                    .min(available_w - 72.0);
                            renderer.fill_text(
                                iced::advanced::text::Text {
                                    content: raw_math_line.to_string(),
                                    bounds: Size::new(available_w - 72.0, BASE_LINE_HEIGHT),
                                    size: 16.0.into(),
                                    line_height: iced::advanced::text::LineHeight::default(),
                                    font: iced::Font::MONOSPACE,
                                    align_x: iced::alignment::Horizontal::Left.into(),
                                    align_y: iced::alignment::Vertical::Top.into(),
                                    shaping: iced::advanced::text::Shaping::Basic,
                                    wrapping: iced::advanced::text::Wrapping::WordOrGlyph,
                                },
                                Point::new(
                                    bounds.x + TEXT_X_OFFSET + (available_w - 72.0 - line_w) / 2.0,
                                    text_y,
                                ),
                                theme::TEXT_SECONDARY,
                                *viewport,
                            );
                            text_y += BASE_LINE_HEIGHT;
                        }
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
                let ty = y + (lh - fs) / 2.0;
                let display_text = span.visible_text(is_editing);
                if display_text.is_empty() {
                    continue;
                }

                if span.is_checkbox && !is_editing {
                    // Draw a premium custom checkbox quad!
                    let box_size = 18.0;
                    let box_y = y + (lh - box_size) / 2.0;
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

                renderer.fill_text(
                    iced::advanced::text::Text {
                        content: display_text.to_string(),
                        bounds: Size::new(f32::INFINITY, lh),
                        size: fs.into(),
                        line_height: iced::advanced::text::LineHeight::default(),
                        font,
                        align_x: iced::alignment::Horizontal::Left.into(),
                        align_y: iced::alignment::Vertical::Top.into(),
                        shaping: iced::advanced::text::Shaping::Basic,
                        wrapping: iced::advanced::text::Wrapping::None,
                    },
                    Point::new(x, ty),
                    span.color,
                    *viewport,
                );

                x += measure_width::<R>(display_text, fs, font);
            }

            // ── cursor ───────────────────────────────────────────
            if focused && i == self.buffer.cursor_line {
                let cx = self.cursor_x_offset::<R>(i);
                let cursor_h = lh.min(20.0);
                let cursor_x = bounds.x + TEXT_X_OFFSET + cx;
                let cursor_y = y + (lh - cursor_h) / 2.0;

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
                    shell.publish((self.on_cursor_move)(line_idx, col));
                    state.is_dragging = true;

                    // Check for checkbox / link clicks
                    if let Some(line) = self.lines.get(line_idx) {
                        let active_block_id =
                            self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
                        let is_editing = state.is_focused && Some(line.block_id) == active_block_id;
                        let mut x_acc = 0.0_f32;
                        for span in &line.spans {
                            let font = span_font(span, line);
                            let w = if span.is_checkbox && !is_editing {
                                26.0
                            } else {
                                measure_width::<R>(&span.text, span.font_size, font)
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
                    shell.publish((self.on_cursor_move)(line_idx, col));
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.is_dragging = false;
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

                // Named keys first — they must never fall through to char input
                match key.as_ref() {
                    keyboard::Key::Named(keyboard::key::Named::Backspace) => {
                        shell.publish((self.on_action)(EditorAction::Backspace));
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::Delete) => {
                        shell.publish((self.on_action)(EditorAction::Delete));
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::Enter) => {
                        shell.publish((self.on_change)("\n".to_string()));
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
                        shell.publish((self.on_action)(EditorAction::MoveLeft));
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
                        shell.publish((self.on_action)(EditorAction::MoveRight));
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                        shell.publish((self.on_action)(EditorAction::MoveUp));
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                        shell.publish((self.on_action)(EditorAction::MoveDown));
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::Home) => {
                        shell.publish((self.on_action)(EditorAction::MoveHome));
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::End) => {
                        shell.publish((self.on_action)(EditorAction::MoveEnd));
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::Tab) => {
                        shell.publish((self.on_change)("    ".to_string()));
                        return;
                    }
                    _ => {}
                }

                // Ctrl / Cmd shortcuts
                if modifiers.command() || modifiers.control() {
                    match key.as_ref() {
                        keyboard::Key::Character(c) if c == "z" => {
                            shell.publish((self.on_action)(EditorAction::Undo));
                        }
                        keyboard::Key::Character(c) if c == "y" => {
                            shell.publish((self.on_action)(EditorAction::Redo));
                        }
                        keyboard::Key::Character(c) if c == "c" => {
                            if let Some(selected) = self.selected_text(state) {
                                _clipboard
                                    .write(iced::advanced::clipboard::Kind::Standard, selected);
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
                            shell.publish((self.on_change)(t.to_string()));
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
    fn x_for_col<R>(&self, line_idx: usize, col: usize, _is_editing: bool) -> f32
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let text = self.buffer.line_text(line_idx);
        let partial: String = text.chars().take(col).collect();
        measure_width::<R>(&partial, 17.0, iced::Font::DEFAULT)
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

        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    /// Compute the x‑offset of the cursor in pixels (relative to TEXT_X_OFFSET).
    fn cursor_x_offset<R>(&self, line_idx: usize) -> f32
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let Some(line) = self.lines.get(line_idx) else {
            return 0.0;
        };
        let mut chars_left = self.buffer.cursor_col;
        let mut x = 0.0_f32;
        let is_editing =
            Some(line.block_id) == self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);

        for span in &line.spans {
            let font = span_font(span, line);
            let display_text = span.visible_text(is_editing);
            if display_text.is_empty() {
                continue;
            }
            let span_chars: Vec<char> = display_text.chars().collect();
            let source_chars: Vec<char> = span.text.chars().collect();

            // This is a rough approximation since display chars might not match source chars 1:1
            // A perfect implementation would map source offsets to display offsets.
            // For now, if we're in preview mode and characters are hidden, the cursor offset will just be at the end of the span
            if chars_left <= source_chars.len() {
                let display_idx = chars_left.min(span_chars.len());
                let partial: String = span_chars[..display_idx].iter().collect();
                x += measure_width::<R>(&partial, span.font_size, font);
                break;
            }
            x += measure_width::<R>(display_text, span.font_size, font);
            chars_left -= source_chars.len();
        }
        x
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
            let is_editing = focused && Some(line.block_id) == active_block_id;
            let lh = line_height_for(
                line,
                self.image_cache,
                self.math_cache,
                available_width,
                is_editing,
                &mut seen_math_blocks,
            );
            if pos.y < y_acc + lh {
                line_idx = i;
                break;
            }
            y_acc += lh;
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

        let mut x_acc = 0.0_f32;
        let mut col = 0;
        let is_editing = focused && Some(line.block_id) == active_block_id;
        for span in &line.spans {
            let font = span_font(span, line);
            let chars: Vec<char> = span.text.chars().collect();

            if span.is_checkbox && !is_editing {
                let cw = 26.0;
                if click_x < x_acc + cw * 0.6 {
                    return (line_idx, col);
                }
                x_acc += cw;
                col += chars.len();
                continue;
            }

            for j in 0..chars.len() {
                let cw = measure_width::<R>(&chars[j].to_string(), span.font_size, font);
                if click_x < x_acc + cw * 0.6 {
                    return (line_idx, col);
                }
                x_acc += cw;
                col += 1;
            }
        }
        (line_idx, col)
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

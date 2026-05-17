use iced::advanced::graphics::core::event::Event;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::mouse;
use iced::keyboard;
use iced::{Element, Length, Rectangle, Size, Color, Point};
use std::collections::HashMap;

use crate::editor::buffer::DocBuffer;
use crate::editor::highlight::StyledLine;
use crate::messages::EditorAction;
use crate::theme;

const MARGIN_LEFT: f32 = 60.0;
const MARGIN_RIGHT: f32 = 60.0;
const TEXT_X_OFFSET: f32 = MARGIN_LEFT;
const TOP_PAD: f32 = 8.0;
const BASE_LINE_HEIGHT: f32 = 28.0;
const IMAGE_HEIGHT: f32 = 220.0;

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

/// Compute the pixel height of a single styled line.
fn line_height_for(
    line: &StyledLine,
    image_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    math_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    available_width: f32,
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
        let mut max_h: f32 = 50.0;
        for span in &line.spans {
            let tex = span.text.trim_matches('$').trim();
            if let Some((_, _, h)) = math_cache.get(tex) {
                max_h = max_h.max(*h * 1.2 + 20.0);
            }
        }
        return max_h;
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

/// Total document height in pixels.
fn total_height(
    lines: &[StyledLine],
    image_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    math_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    width: f32,
) -> f32 {
    let mut h = TOP_PAD;
    for line in lines {
        h += line_height_for(line, image_cache, math_cache, width);
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
            height: Length::Fixed(total_height(self.lines, self.image_cache, self.math_cache, 800.0)),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &R,
        limits: &layout::Limits,
    ) -> layout::Node {
        let max_width = limits.max().width;
        let h = total_height(self.lines, self.image_cache, self.math_cache, max_width);
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
        let mut blocks: std::collections::HashMap<usize, BlockMeta> = std::collections::HashMap::new();
        let mut temp_y = bounds.y + TOP_PAD;
        for line in self.lines.iter() {
            let lh = line_height_for(line, self.image_cache, self.math_cache, bounds.width);
            if line.is_code_block || line.is_math_block || line.is_blockquote || line.is_table_row {
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
                        border: iced::Border { radius: 2.0.into(), ..Default::default() },
                        ..Default::default()
                    },
                    theme::ACCENT_DIM,
                );
            } else if meta.is_table && !meta.is_editing {
                // Table is drawn completely in the main loop, here we only draw the table background
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: bounds.x + TEXT_X_OFFSET - 8.0,
                            y: meta.y,
                            width: meta.col_widths.iter().sum::<f32>() + 16.0,
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

        for (i, line) in self.lines.iter().enumerate() {
            let lh = line_height_for(line, self.image_cache, self.math_cache, bounds.width);
            let is_editing = focused && Some(line.block_id) == active_block_id;

            // Viewport culling
            if y + lh < viewport.y {
                y += lh;
                continue;
            }
            if y > viewport.y + viewport.height {
                break;
            }

            // ── active line highlight ────────────────────────────
            if is_editing && i == self.buffer.cursor_line {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle { x: bounds.x, y, width: bounds.width, height: lh },
                        ..Default::default()
                    },
                    Color::from_rgba(1.0, 1.0, 1.0, 0.03),
                );
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

            // (Block backgrounds removed from per-line loop)

            // ── table rendering ──────────────────────────────────
            if line.is_table_row && !is_editing {
                if last_table_block != Some(line.block_id) {
                    table_counter += 1;
                    last_table_block = Some(line.block_id);
                    // Draw Table Caption Above
                    let caption = format!("Table {}", table_counter);
                    renderer.fill_text(
                        iced::advanced::text::Text {
                            content: caption,
                            bounds: Size::new(bounds.width - TEXT_X_OFFSET, 20.0),
                            size: 13.0.into(),
                            line_height: iced::advanced::text::LineHeight::default(),
                            font: iced::Font::DEFAULT,
                            align_x: iced::alignment::Horizontal::Center.into(),
                            align_y: iced::alignment::Vertical::Top.into(),
                            shaping: iced::advanced::text::Shaping::Basic,
                            wrapping: iced::advanced::text::Wrapping::None,
                        },
                        Point::new(bounds.x + TEXT_X_OFFSET, y - 24.0),
                        theme::TEXT_MUTED,
                        *viewport,
                    );
                }

                if let Some(meta) = blocks.get(&line.block_id) {
                    let mut cx = bounds.x + TEXT_X_OFFSET;
                    
                    // Is this a separator row? We can check if it has spans with only `-` or `|` or just check table_cells
                    if line.table_cells.is_empty() {
                        // Separator row: draw a horizontal line
                        let table_width: f32 = meta.col_widths.iter().sum();
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: bounds.x + TEXT_X_OFFSET - 8.0,
                                    y: y + lh / 2.0,
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
                        if c_idx >= meta.col_widths.len() { break; }
                        let cw = meta.col_widths[c_idx];
                        
                        // Draw Vertical Separator
                        if c_idx > 0 {
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: Rectangle {
                                        x: cx - 4.0,
                                        y,
                                        width: 1.0,
                                        height: lh,
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
                            if text.is_empty() { continue; }
                            
                            let font = span_font(span, line);
                            let fs = span.font_size;
                            let ty = y + (lh - fs) / 2.0;

                            renderer.fill_text(
                                iced::advanced::text::Text {
                                    content: text.to_string(),
                                    bounds: Size::new(cw - 16.0, lh),
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
                            let scale = if *w > available_w { available_w / w } else { 1.0 };
                            let draw_w = w * scale;
                            let draw_h = h * scale;
                            let draw_x = bounds.x + TEXT_X_OFFSET + (available_w - draw_w) / 2.0;

                            renderer.draw_image(
                                iced::advanced::image::Image::new(handle.clone()),
                                Rectangle { x: draw_x, y: y + 5.0, width: draw_w, height: draw_h },
                                *viewport,
                            );

                            // Draw caption
                            let caption = format!("Figure {}: {}", image_counter, span.image_alt.as_deref().unwrap_or(""));
                            renderer.fill_text(
                                iced::advanced::text::Text {
                                    content: caption,
                                    bounds: Size::new(available_w, 20.0),
                                    size: 13.0.into(),
                                    line_height: iced::advanced::text::LineHeight::default(),
                                    font: iced::Font::DEFAULT,
                                    align_x: iced::alignment::Horizontal::Center.into(),
                                    align_y: iced::alignment::Vertical::Top.into(),
                                    shaping: iced::advanced::text::Shaping::Basic,
                                    wrapping: iced::advanced::text::Wrapping::None,
                                },
                                Point::new(bounds.x + TEXT_X_OFFSET, y + draw_h + 10.0),
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
                    if line.is_block_fence && !is_editing {
                        continue; // Hide fences in preview
                    }
                    if span.is_syntax && !is_editing {
                        continue; // Hide inline $ in preview
                    }
                    
                    let tex = span.text.trim_matches('$').trim();
                    let scale = if line.is_math_block { 1.2 } else { 1.0 };
                    let mut drawn_w = 0.0;
                    let mut image_rendered = false;
                    
                    if !tex.is_empty() {
                        if let Some((handle, w, h)) = self.math_cache.get(tex) {
                            let draw_w = w * scale;
                            let draw_h = h * scale;
                            drawn_w = draw_w;

                            // For inline math, we hide the rendered image if editing, but for block math we keep it
                            if !line.is_math_block && is_editing {
                                // Skip drawing image, will draw text
                            } else {
                                let mut draw_x = x;
                                if line.is_math_block {
                                    equation_counter += 1;
                                    let available_w = bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT;
                                    draw_x = bounds.x + TEXT_X_OFFSET + (available_w - draw_w) / 2.0;

                                    // Equation number right aligned
                                    let eq_num = format!("({})", equation_counter);
                                    let eq_y = y + (lh - draw_h) / 2.0; // center with the equation
                                    renderer.fill_text(
                                        iced::advanced::text::Text {
                                            content: eq_num,
                                            bounds: Size::new(bounds.width - MARGIN_RIGHT - (bounds.x + TEXT_X_OFFSET), draw_h),
                                            size: 14.0.into(),
                                            line_height: iced::advanced::text::LineHeight::default(),
                                            font: iced::Font::DEFAULT,
                                            align_x: iced::alignment::Horizontal::Right.into(),
                                            align_y: iced::alignment::Vertical::Center.into(),
                                            shaping: iced::advanced::text::Shaping::Basic,
                                            wrapping: iced::advanced::text::Wrapping::None,
                                        },
                                        Point::new(bounds.x + TEXT_X_OFFSET, eq_y),
                                        theme::TEXT_MUTED,
                                        *viewport,
                                    );
                                }

                                renderer.draw_image(
                                    iced::advanced::image::Image::new(handle.clone()),
                                    Rectangle { x: draw_x, y: y + (lh - draw_h) / 2.0, width: draw_w, height: draw_h },
                                    *viewport,
                                );
                                image_rendered = true;
                            }
                        }
                    }
                    
                    if image_rendered && (line.is_math_block || (!line.is_math_block && !is_editing)) {
                        x += drawn_w + 4.0;
                        continue;
                    }
                }

                // ── text span ────────────────────────────────────
                let fs = span.font_size;
                let ty = y + (lh - fs) / 2.0;
                let display_text = span.visible_text(is_editing);
                if display_text.is_empty() { continue; }

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
                        border: iced::Border { radius: 1.0.into(), ..Default::default() },
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
                    state.is_focused = true;
                    let (line_idx, col) = self.hit_test::<R>(pos, _layout.bounds().width);
                    shell.publish((self.on_cursor_move)(line_idx, col));
                    state.is_dragging = true;

                    // Check for checkbox / link clicks
                    if let Some(line) = self.lines.get(line_idx) {
                        let mut x_acc = 0.0_f32;
                        for span in &line.spans {
                            let font = span_font(span, line);
                            let w = measure_width::<R>(&span.text, span.font_size, font);
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
                key, modifiers, text, ..
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
        let is_editing = Some(line.block_id) == self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
        
        for span in &line.spans {
            let font = span_font(span, line);
            let display_text = span.visible_text(is_editing);
            if display_text.is_empty() { continue; }
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
    fn hit_test<R>(&self, pos: Point, available_width: f32) -> (usize, usize)
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let mut y_acc = TOP_PAD;
        let mut line_idx = 0;

        for (i, line) in self.lines.iter().enumerate() {
            let lh = line_height_for(line, self.image_cache, self.math_cache, available_width);
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
        for span in &line.spans {
            let font = span_font(span, line);
            let chars: Vec<char> = span.text.chars().collect();
            for j in 0..chars.len() {
                let cw = measure_width::<R>(
                    &chars[j].to_string(),
                    span.font_size,
                    font,
                );
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

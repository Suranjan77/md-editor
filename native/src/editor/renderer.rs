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

// ── Constants ────────────────────────────────────────────────────────
const LEFT_GUTTER: f32 = 50.0;  // line‑number column width
const LEFT_PAD: f32 = 12.0;     // gap between gutter and text
const TEXT_X_OFFSET: f32 = LEFT_GUTTER + LEFT_PAD;
const TOP_PAD: f32 = 8.0;
const BASE_LINE_HEIGHT: f32 = 24.0;
const IMAGE_HEIGHT: f32 = 220.0;

// ── Widget ───────────────────────────────────────────────────────────

pub struct Editor<'a, Message> {
    buffer: &'a DocBuffer,
    lines: &'a [StyledLine],
    image_cache: &'a HashMap<String, iced::widget::image::Handle>,
    math_cache: &'a HashMap<String, iced::widget::image::Handle>,
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
        image_cache: &'a HashMap<String, iced::widget::image::Handle>,
        math_cache: &'a HashMap<String, iced::widget::image::Handle>,
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
fn line_height_for(line: &StyledLine) -> f32 {
    if line.spans.iter().any(|s| s.is_image) {
        return IMAGE_HEIGHT;
    }
    if line.is_math_block {
        return 50.0;
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
fn total_height(lines: &[StyledLine]) -> f32 {
    let mut h = TOP_PAD;
    for line in lines {
        h += line_height_for(line);
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
            height: Length::Fixed(total_height(self.lines)),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &R,
        limits: &layout::Limits,
    ) -> layout::Node {
        let h = total_height(self.lines);
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

        let mut y = bounds.y + TOP_PAD;

        for (i, line) in self.lines.iter().enumerate() {
            let lh = line_height_for(line);

            // Viewport culling
            if y + lh < viewport.y {
                y += lh;
                continue;
            }
            if y > viewport.y + viewport.height {
                break;
            }

            // ── active line highlight ────────────────────────────
            if focused && i == self.buffer.cursor_line {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle { x: bounds.x, y, width: bounds.width, height: lh },
                        ..Default::default()
                    },
                    Color::from_rgba(1.0, 1.0, 1.0, 0.03),
                );
            }

            // ── line number ──────────────────────────────────────
            let ln = format!("{}", i + 1);
            renderer.fill_text(
                iced::advanced::text::Text {
                    content: ln,
                    bounds: Size::new(LEFT_GUTTER - 8.0, lh),
                    size: 12.0.into(),
                    line_height: iced::advanced::text::LineHeight::default(),
                    font: iced::Font::MONOSPACE,
                    align_x: iced::alignment::Horizontal::Right.into(),
                    align_y: iced::alignment::Vertical::Center.into(),
                    shaping: iced::advanced::text::Shaping::Basic,
                    wrapping: iced::advanced::text::Wrapping::None,
                },
                Point::new(bounds.x + 4.0, y),
                theme::TEXT_MUTED,
                *viewport,
            );

            // ── horizontal rule ──────────────────────────────────
            if line.spans.iter().any(|s| s.is_rule) {
                let rule_y = y + lh / 2.0;
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: bounds.x + TEXT_X_OFFSET,
                            y: rule_y,
                            width: bounds.width - TEXT_X_OFFSET - 20.0,
                            height: 1.0,
                        },
                        ..Default::default()
                    },
                    Color::from_rgba(1.0, 1.0, 1.0, 0.12),
                );
                y += lh;
                continue;
            }

            // ── code block background ────────────────────────────
            if line.is_code_block {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: bounds.x + TEXT_X_OFFSET - 4.0,
                            y,
                            width: bounds.width - TEXT_X_OFFSET - 16.0,
                            height: lh,
                        },
                        ..Default::default()
                    },
                    Color::from_rgba(0.0, 0.0, 0.0, 0.25),
                );
            }

            // ── math block background ────────────────────────────
            if line.is_math_block {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: bounds.x + TEXT_X_OFFSET - 4.0,
                            y,
                            width: bounds.width - TEXT_X_OFFSET - 16.0,
                            height: lh,
                        },
                        border: iced::Border {
                            color: Color::from_rgba(0.4, 0.75, 0.4, 0.2),
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    },
                    Color::from_rgba(0.2, 0.35, 0.2, 0.15),
                );
            }

            // ── spans ────────────────────────────────────────────
            let mut x = bounds.x + TEXT_X_OFFSET;

            for span in &line.spans {
                let font = span_font(span, line);
                let is_math = span.is_math || line.is_math_block;

                // ── image ────────────────────────────────────────
                if span.is_image {
                    if let Some(path) = &span.image_path {
                        if let Some(handle) = self.image_cache.get(path) {
                            renderer.draw_image(
                                iced::advanced::image::Image::new(handle.clone()),
                                Rectangle { x, y: y + 5.0, width: 400.0, height: IMAGE_HEIGHT - 20.0 },
                                *viewport,
                            );
                            x += 410.0;
                            continue;
                        }
                    }
                    // Fallback: render the alt text
                }

                // ── math (rendered to image) ─────────────────────
                if is_math {
                    let tex = span.text.trim_matches('$').trim();
                    if !tex.is_empty() {
                        if let Some(handle) = self.math_cache.get(tex) {
                            // TODO: query actual image dimensions
                            let img_w = 120.0_f32;
                            let img_h = (lh - 4.0).min(40.0);
                            renderer.draw_image(
                                iced::advanced::image::Image::new(handle.clone()),
                                Rectangle { x, y: y + (lh - img_h) / 2.0, width: img_w, height: img_h },
                                *viewport,
                            );
                            x += img_w + 4.0;
                            continue;
                        }
                    }
                    // Fallback: render the raw LaTeX as green monospace
                }

                // ── text span ────────────────────────────────────
                let fs = span.font_size;
                let ty = y + (lh - fs) / 2.0; // vertically center within lh

                renderer.fill_text(
                    iced::advanced::text::Text {
                        content: span.text.clone(),
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

                x += measure_width::<R>(&span.text, fs, font);
            }

            // ── cursor ───────────────────────────────────────────
            if focused && i == self.buffer.cursor_line {
                let cx = self.cursor_x_offset::<R>(i);
                let cursor_h = lh.min(20.0);
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: bounds.x + TEXT_X_OFFSET + cx,
                            y: y + (lh - cursor_h) / 2.0,
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
                    let (line_idx, col) = self.hit_test::<R>(pos);
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
        for span in &line.spans {
            let font = span_font(span, line);
            let span_chars: Vec<char> = span.text.chars().collect();
            if chars_left <= span_chars.len() {
                let partial: String = span_chars[..chars_left].iter().collect();
                x += measure_width::<R>(&partial, span.font_size, font);
                break;
            }
            x += measure_width::<R>(&span.text, span.font_size, font);
            chars_left -= span_chars.len();
        }
        x
    }

    /// Convert a click position (relative to widget bounds) into (line, col).
    fn hit_test<R>(&self, pos: Point) -> (usize, usize)
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let mut y_acc = TOP_PAD;
        let mut line_idx = 0;

        for (i, line) in self.lines.iter().enumerate() {
            let lh = line_height_for(line);
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

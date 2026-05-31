use iced::advanced::graphics::core::event::Event;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::mouse;
use iced::{Color, Element, Length, Rectangle, Size};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdfSelection {
    pub page_index: u16,
    pub anchor_idx: usize,
    pub focus_idx: usize,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct State {
    pub modifiers: iced::keyboard::Modifiers,
    pub drag_start: Option<iced::Point>,
    pub is_dragging: bool,
}

pub struct InteractivePdf<'a, Message> {
    handle: iced::widget::image::Handle,
    width: f32,
    height: f32,
    page_width: f32,
    page_height: f32,
    page_index: u16,
    page_text: Option<&'a md_editor_core::pdf::PdfPageText>,
    highlights: &'a [md_editor_core::pdf::PdfAnnotation],
    search_highlights: Vec<md_editor_core::pdf::PdfRect>,
    active_search_highlights: Vec<md_editor_core::pdf::PdfRect>,
    active_selection: Option<PdfSelection>,
    focused_annotation_id: Option<&'a str>,
    links: &'a [md_editor_core::pdf::LinkInfo],
    rotation: u16,
    on_left_click: Box<dyn Fn(f32, f32, iced::keyboard::Modifiers) -> Message + 'a>,
    on_right_click: Box<dyn Fn(f32, f32, iced::Point) -> Message + 'a>,
    on_selection_changed: Box<dyn Fn(u16, usize, usize) -> Message + 'a>,
    on_selection_finished: Box<dyn Fn(u16, usize, usize) -> Message + 'a>,
    on_selection_cleared: Box<dyn Fn() -> Message + 'a>,
    on_copy_selection: Box<dyn Fn() -> Message + 'a>,
}

impl<'a, Message> InteractivePdf<'a, Message> {
    pub fn new(
        handle: iced::widget::image::Handle,
        width: f32,
        height: f32,
        page_width: f32,
        page_height: f32,
        page_index: u16,
        page_text: Option<&'a md_editor_core::pdf::PdfPageText>,
        highlights: &'a [md_editor_core::pdf::PdfAnnotation],
        search_highlights: Vec<md_editor_core::pdf::PdfRect>,
        active_search_highlights: Vec<md_editor_core::pdf::PdfRect>,
        active_selection: Option<PdfSelection>,
        focused_annotation_id: Option<&'a str>,
        links: &'a [md_editor_core::pdf::LinkInfo],
        rotation: u16,
        on_left_click: impl Fn(f32, f32, iced::keyboard::Modifiers) -> Message + 'a,
        on_right_click: impl Fn(f32, f32, iced::Point) -> Message + 'a,
        on_selection_changed: impl Fn(u16, usize, usize) -> Message + 'a,
        on_selection_finished: impl Fn(u16, usize, usize) -> Message + 'a,
        on_selection_cleared: impl Fn() -> Message + 'a,
        on_copy_selection: impl Fn() -> Message + 'a,
    ) -> Self {
        Self {
            handle,
            width,
            height,
            page_width,
            page_height,
            page_index,
            page_text,
            highlights,
            search_highlights,
            active_search_highlights,
            active_selection,
            focused_annotation_id,
            links,
            rotation,
            on_left_click: Box::new(on_left_click),
            on_right_click: Box::new(on_right_click),
            on_selection_changed: Box::new(on_selection_changed),
            on_selection_finished: Box::new(on_selection_finished),
            on_selection_cleared: Box::new(on_selection_cleared),
            on_copy_selection: Box::new(on_copy_selection),
        }
    }

    fn get_zoom(&self, page_width: f32, page_height: f32) -> f32 {
        let is_rotated = self.rotation == 90 || self.rotation == 270;
        if is_rotated {
            self.width / page_height.max(1.0)
        } else {
            self.width / page_width.max(1.0)
        }
    }
}

pub(crate) fn search_rect_to_view_rect(
    rect: &md_editor_core::pdf::PdfRect,
    page_width: f32,
    page_height: f32,
    zoom: f32,
    rotation: u16,
) -> md_editor_core::pdf::PdfRect {
    match rotation {
        90 => md_editor_core::pdf::PdfRect {
            x: rect.y * zoom,
            y: rect.x * zoom,
            width: rect.height * zoom,
            height: rect.width * zoom,
        },
        180 => md_editor_core::pdf::PdfRect {
            x: (page_width - rect.x - rect.width) * zoom,
            y: rect.y * zoom,
            width: rect.width * zoom,
            height: rect.height * zoom,
        },
        270 => md_editor_core::pdf::PdfRect {
            x: (page_height - rect.y - rect.height) * zoom,
            y: (page_width - rect.x - rect.width) * zoom,
            width: rect.height * zoom,
            height: rect.width * zoom,
        },
        _ => md_editor_core::pdf::PdfRect {
            x: rect.x * zoom,
            y: (page_height - rect.y - rect.height) * zoom,
            width: rect.width * zoom,
            height: rect.height * zoom,
        },
    }
}

fn to_screen_rect(
    pdf_rect: &md_editor_core::pdf::PdfRect,
    page_width: f32,
    page_height: f32,
    zoom: f32,
    rotation: u16,
    bounds: Rectangle,
) -> Rectangle {
    let r = search_rect_to_view_rect(pdf_rect, page_width, page_height, zoom, rotation);
    Rectangle {
        x: bounds.x + r.x,
        y: bounds.y + r.y,
        width: r.width,
        height: r.height,
    }
}

fn get_annotation_color(color: md_editor_core::pdf::PdfAnnotationColor) -> Color {
    match color {
        md_editor_core::pdf::PdfAnnotationColor::Yellow => Color::from_rgba(1.0, 0.92, 0.23, 0.35),
        md_editor_core::pdf::PdfAnnotationColor::Green => Color::from_rgba(0.3, 0.85, 0.3, 0.35),
        md_editor_core::pdf::PdfAnnotationColor::Blue => Color::from_rgba(0.12, 0.53, 0.9, 0.35),
        md_editor_core::pdf::PdfAnnotationColor::Pink => Color::from_rgba(0.95, 0.3, 0.6, 0.35),
        md_editor_core::pdf::PdfAnnotationColor::Orange => Color::from_rgba(1.0, 0.6, 0.1, 0.35),
    }
}

fn hit_test(
    page_text: &md_editor_core::pdf::PdfPageText,
    point: iced::Point,
    zoom: f32,
    rotation: u16,
) -> Option<usize> {
    let x = point.x;
    let y = point.y;

    // Find the line with the minimum vertical distance to `y`
    let mut best_line: Option<usize> = None;
    let mut min_line_dist = f32::MAX;

    for (line_idx, line) in page_text.lines.iter().enumerate() {
        let line_view = search_rect_to_view_rect(&line.bbox, page_text.page_width, page_text.page_height, zoom, rotation);
        let line_y_min = line_view.y;
        let line_y_max = line_view.y + line_view.height;

        let dist = if y >= line_y_min && y <= line_y_max {
            0.0
        } else if y < line_y_min {
            line_y_min - y
        } else {
            y - line_y_max
        };

        if dist < min_line_dist {
            min_line_dist = dist;
            best_line = Some(line_idx);
        }
    }

    let best_line_idx = best_line?;
    let line = &page_text.lines[best_line_idx];

    // Filter characters that belong to this line
    let line_chars: Vec<&md_editor_core::pdf::PdfTextChar> = page_text
        .chars
        .iter()
        .filter(|c| {
            c.text_index >= line.start_text_index
                && c.text_index < line.end_text_index
                && c.bbox.width > 0.0
                && c.bbox.height > 0.0
        })
        .collect();

    if line_chars.is_empty() {
        return None;
    }

    // Find the character closest to x
    let mut best_char_idx = None;
    let mut min_char_dist = f32::MAX;

    for ch in line_chars {
        let char_view = search_rect_to_view_rect(&ch.bbox, page_text.page_width, page_text.page_height, zoom, rotation);
        let char_x_min = char_view.x;
        let char_x_max = char_view.x + char_view.width;

        let dist = if x >= char_x_min && x <= char_x_max {
            0.0
        } else if x < char_x_min {
            char_x_min - x
        } else {
            x - char_x_max
        };

        if dist < min_char_dist {
            min_char_dist = dist;
            best_char_idx = Some(ch.text_index);
        }
    }

    best_char_idx
}

impl<'a, Message, Theme, R> Widget<Message, Theme, R> for InteractivePdf<'a, Message>
where
    R: renderer::Renderer + iced::advanced::image::Renderer<Handle = iced::widget::image::Handle>,
{
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.width),
            height: Length::Fixed(self.height),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &R,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.resolve(
            Length::Fixed(self.width),
            Length::Fixed(self.height),
            Size::new(self.width, self.height),
        ))
    }

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

        renderer.draw_image(
            iced::advanced::image::Image::new(self.handle.clone()),
            bounds,
            *viewport,
        );
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::new(State::default())
    }

    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<State>()
    }

    fn update(
        &mut self,
        _state: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &R,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = _state.state.downcast_mut::<State>();
        let bounds = layout.bounds();

        match event {
            Event::Keyboard(iced::keyboard::Event::ModifiersChanged(m)) => {
                state.modifiers = *m;
            }
            Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                if (modifiers.command() || modifiers.control())
                    && matches!(key, iced::keyboard::Key::Character(c) if c == "c")
                    && self.active_selection.is_some()
                {
                    shell.publish((self.on_copy_selection)());
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(position) = cursor.position_in(bounds) {
                    state.drag_start = Some(position);
                    state.is_dragging = false;
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some(start_pos) = state.drag_start {
                    if let Some(current_pos) = cursor.position() {
                        let current_rel =
                            iced::Point::new(current_pos.x - bounds.x, current_pos.y - bounds.y);
                        let dx = current_rel.x - start_pos.x;
                        let dy = current_rel.y - start_pos.y;
                        let dist_sq = dx * dx + dy * dy;
                        if dist_sq > 4.0 {
                            state.is_dragging = true;
                            if let Some(page_text) = self.page_text {
                                let zoom = self.get_zoom(self.page_width, page_text.page_height);
                                let start_rel = start_pos;
                                if let (Some(anchor), Some(focus)) = (
                                    hit_test(page_text, start_rel, zoom, self.rotation),
                                    hit_test(page_text, current_rel, zoom, self.rotation),
                                ) {
                                    shell.publish((self.on_selection_changed)(
                                        self.page_index,
                                        anchor,
                                        focus,
                                    ));
                                }
                            }
                            shell.capture_event();
                        }
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(start_pos) = state.drag_start {
                    if state.is_dragging {
                        if let Some(page_text) = self.page_text {
                            if let Some(current_pos) = cursor.position() {
                                let current_rel = iced::Point::new(
                                    current_pos.x - bounds.x,
                                    current_pos.y - bounds.y,
                                );
                                let zoom = self.get_zoom(self.page_width, page_text.page_height);
                                let start_rel = start_pos;
                                if let (Some(anchor), Some(focus)) = (
                                    hit_test(page_text, start_rel, zoom, self.rotation),
                                    hit_test(page_text, current_rel, zoom, self.rotation),
                                ) {
                                    shell.publish((self.on_selection_finished)(
                                        self.page_index,
                                        anchor,
                                        focus,
                                    ));
                                }
                            }
                        }
                    } else {
                        shell.publish((self.on_selection_cleared)());
                        if let Some(position) = cursor.position_in(bounds) {
                            let x = position.x / self.width;
                            let y = position.y / self.height;
                            shell.publish((self.on_left_click)(x, y, state.modifiers));
                        }
                    }
                    shell.capture_event();
                }
                state.drag_start = None;
                state.is_dragging = false;
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                if let Some(position) = cursor.position_in(bounds) {
                    let x = position.x / self.width;
                    let y = position.y / self.height;
                    let absolute_pos = cursor.position().unwrap_or_default();
                    shell.publish((self.on_right_click)(x, y, absolute_pos));
                    shell.capture_event();
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        _state: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &R,
    ) -> mouse::Interaction {
        if !cursor.is_over(layout.bounds()) {
            return mouse::Interaction::Idle;
        }
        let bounds = layout.bounds();
        let point = cursor.position_in(bounds);

        // Hit-test links against cursor position
        if self.hover_link(point, bounds) {
            return mouse::Interaction::Pointer;
        }

        // Hit-test text against cursor position
        if self.hover_text(point, bounds) {
            return mouse::Interaction::Text;
        }

        mouse::Interaction::Grab
    }
}

impl<'a, Message> InteractivePdf<'a, Message> {
    /// Convert screen-space point to page-space PDF coords, check if over any link.
    fn hover_link(&self, point: Option<iced::Point>, _bounds: Rectangle) -> bool {
        let point = match point {
            Some(p) => p,
            None => return false,
        };
        let screen_x = point.x;
        let screen_y = point.y;

        let page_width = self.page_width;
        let page_height = self
            .page_text
            .map(|p| p.page_height)
            .unwrap_or(self.page_height);
        let zoom = self.get_zoom(page_width, page_height);

        for link in self.links {
            let r = search_rect_to_view_rect(&link.bbox, page_width, page_height, zoom, self.rotation);
            if screen_x >= r.x
                && screen_x <= r.x + r.width
                && screen_y >= r.y
                && screen_y <= r.y + r.height
            {
                return true;
            }
        }
        false
    }

    /// Check if screen-space point is hovering over any text character or line.
    fn hover_text(&self, point: Option<iced::Point>, _bounds: Rectangle) -> bool {
        let point = match point {
            Some(p) => p,
            None => return false,
        };
        let screen_x = point.x;
        let screen_y = point.y;

        let page_width = self.page_width;
        let page_height = self
            .page_text
            .map(|p| p.page_height)
            .unwrap_or(self.page_height);
        let zoom = self.get_zoom(page_width, page_height);

        let page_text = match self.page_text {
            Some(p) => p,
            None => return false,
        };

        for line in &page_text.lines {
            let r = search_rect_to_view_rect(&line.bbox, page_width, page_height, zoom, self.rotation);
            if screen_x >= r.x
                && screen_x <= r.x + r.width
                && screen_y >= r.y
                && screen_y <= r.y + r.height
            {
                return true;
            }
        }
        false
    }
}

impl<'a, Message, Theme, R> From<InteractivePdf<'a, Message>> for Element<'a, Message, Theme, R>
where
    R: renderer::Renderer + iced::advanced::image::Renderer<Handle = iced::widget::image::Handle>,
    Message: 'a,
{
    fn from(widget: InteractivePdf<'a, Message>) -> Self {
        Self::new(widget)
    }
}

#[derive(Clone, Debug)]
pub struct HighlightRect {
    pub rect: Rectangle,
    pub color: Color,
    pub border: iced::Border,
}

pub struct PdfHighlights {
    width: f32,
    height: f32,
    rects: Vec<HighlightRect>,
}

impl PdfHighlights {
    pub fn new(width: f32, height: f32, rects: Vec<HighlightRect>) -> Self {
        Self { width, height, rects }
    }
}

impl<'a, Message, Theme, R> Widget<Message, Theme, R> for PdfHighlights
where
    R: renderer::Renderer,
{
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.width),
            height: Length::Fixed(self.height),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &R,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.resolve(
            Length::Fixed(self.width),
            Length::Fixed(self.height),
            Size::new(self.width, self.height),
        ))
    }

    fn draw(
        &self,
        _state: &widget::Tree,
        renderer: &mut R,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        for item in &self.rects {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x + item.rect.x,
                        y: bounds.y + item.rect.y,
                        width: item.rect.width,
                        height: item.rect.height,
                    },
                    border: item.border,
                    ..Default::default()
                },
                item.color,
            );
        }
    }
}

impl<'a, Message, Theme, R> From<PdfHighlights> for Element<'a, Message, Theme, R>
where
    R: renderer::Renderer,
    Message: 'a,
{
    fn from(widget: PdfHighlights) -> Self {
        Self::new(widget)
    }
}


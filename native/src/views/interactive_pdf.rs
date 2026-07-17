use iced::advanced::graphics::core::event::Event;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::mouse;
use iced::{Element, Length, Rectangle, Size};

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
    page_index: u16,
    page_text: Option<&'a md_editor_core::pdf::PdfPageText>,
    active_selection: Option<PdfSelection>,
    on_left_click: Box<dyn Fn(f32, f32, iced::keyboard::Modifiers) -> Message + 'a>,
    on_right_click: Box<dyn Fn(f32, f32) -> Message + 'a>,
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
        page_index: u16,
        page_text: Option<&'a md_editor_core::pdf::PdfPageText>,
        active_selection: Option<PdfSelection>,
        on_left_click: impl Fn(f32, f32, iced::keyboard::Modifiers) -> Message + 'a,
        on_right_click: impl Fn(f32, f32) -> Message + 'a,
        on_selection_changed: impl Fn(u16, usize, usize) -> Message + 'a,
        on_selection_finished: impl Fn(u16, usize, usize) -> Message + 'a,
        on_selection_cleared: impl Fn() -> Message + 'a,
        on_copy_selection: impl Fn() -> Message + 'a,
    ) -> Self {
        Self {
            handle,
            width,
            height,
            page_index,
            page_text,
            active_selection,
            on_left_click: Box::new(on_left_click),
            on_right_click: Box::new(on_right_click),
            on_selection_changed: Box::new(on_selection_changed),
            on_selection_finished: Box::new(on_selection_finished),
            on_selection_cleared: Box::new(on_selection_cleared),
            on_copy_selection: Box::new(on_copy_selection),
        }
    }
}

fn search_rect_to_view_rect(
    rect: &md_editor_core::pdf::PdfRect,
    page_height: f32,
    zoom: f32,
) -> md_editor_core::pdf::PdfRect {
    md_editor_core::pdf::PdfRect {
        x: rect.x * zoom,
        y: (page_height - rect.y - rect.height) * zoom,
        width: rect.width * zoom,
        height: rect.height * zoom,
    }
}

fn hit_test(
    page_text: &md_editor_core::pdf::PdfPageText,
    point: iced::Point,
    zoom: f32,
) -> Option<usize> {
    let x = point.x;
    let y = point.y;

    // Find the line with the minimum vertical distance to `y`
    let mut best_line: Option<usize> = None;
    let mut min_line_dist = f32::MAX;

    for (line_idx, line) in page_text.lines.iter().enumerate() {
        let line_view = search_rect_to_view_rect(&line.bbox, page_text.page_height, zoom);
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
        let char_view = search_rect_to_view_rect(&ch.bbox, page_text.page_height, zoom);
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
            // Linear filtering keeps supersampled pages crisp when iced scales
            // the bitmap to the layout box; set explicitly so a future change
            // to the iced default can't silently regress sharpness.
            iced::advanced::image::Image::new(self.handle.clone())
                .filter_method(iced::widget::image::FilterMethod::Linear),
            bounds,
            *viewport,
        );

        // The stacked `PdfOverlay` paints annotations, search matches, and
        // selection exactly once above this page bitmap.
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
                                let zoom = self.width / page_text.page_width;
                                let start_rel = start_pos;
                                if let (Some(anchor), Some(focus)) = (
                                    hit_test(page_text, start_rel, zoom),
                                    hit_test(page_text, current_rel, zoom),
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
                                let zoom = self.width / page_text.page_width;
                                let start_rel = start_pos;
                                if let (Some(anchor), Some(focus)) = (
                                    hit_test(page_text, start_rel, zoom),
                                    hit_test(page_text, current_rel, zoom),
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
                    shell.publish((self.on_right_click)(x, y));
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
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::Idle
        }
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

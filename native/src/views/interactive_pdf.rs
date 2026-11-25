//! One PDF page as a widget: the bitmap plus hit testing, text selection and clicks.

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
    /// When and where the last plain click landed, and how many came in quick
    /// succession: two select a word, three a line.
    pub last_click: Option<(std::time::Instant, iced::Point)>,
    pub click_count: u8,
}

use crate::design::metrics::MULTI_CLICK_SLOP;

pub struct InteractivePdf<'a, Message> {
    handle: iced::widget::image::Handle,
    width: f32,
    height: f32,
    page_index: u16,
    page_text: Option<&'a md_editor_core::pdf::PdfPageText>,
    on_left_click: Box<dyn Fn(f32, f32, iced::keyboard::Modifiers) -> Message + 'a>,
    on_right_click: Box<dyn Fn(f32, f32) -> Message + 'a>,
    on_selection_changed: Box<dyn Fn(u16, usize, usize) -> Message + 'a>,
    on_selection_finished: Box<dyn Fn(u16, usize, usize) -> Message + 'a>,
    on_selection_cleared: Box<dyn Fn() -> Message + 'a>,
}

/// What a page publishes for pointer and selection events.
pub struct PageEvents<'a, Message> {
    pub on_left_click: Box<dyn Fn(f32, f32, iced::keyboard::Modifiers) -> Message + 'a>,
    pub on_right_click: Box<dyn Fn(f32, f32) -> Message + 'a>,
    pub on_selection_changed: Box<dyn Fn(u16, usize, usize) -> Message + 'a>,
    pub on_selection_finished: Box<dyn Fn(u16, usize, usize) -> Message + 'a>,
    pub on_selection_cleared: Box<dyn Fn() -> Message + 'a>,
}

impl<'a, Message> InteractivePdf<'a, Message> {
    pub fn new(
        handle: iced::widget::image::Handle,
        width: f32,
        height: f32,
        page_index: u16,
        page_text: Option<&'a md_editor_core::pdf::PdfPageText>,
        events: PageEvents<'a, Message>,
    ) -> Self {
        let PageEvents {
            on_left_click,
            on_right_click,
            on_selection_changed,
            on_selection_finished,
            on_selection_cleared,
        } = events;
        Self {
            handle,
            width,
            height,
            page_index,
            page_text,
            on_left_click,
            on_right_click,
            on_selection_changed,
            on_selection_finished,
            on_selection_cleared,
        }
    }
}

/// Points → this page's pixels. `x` and `y` scale separately: when the page
/// is laid out narrower than asked (a clamp only touches the width), the
/// bitmap is squeezed horizontally, and hit testing must follow what is drawn.
#[derive(Debug, Clone, Copy)]
struct Scale {
    x: f32,
    y: f32,
}

impl Scale {
    fn of(bounds: Rectangle, page_text: &md_editor_core::pdf::PdfPageText) -> Self {
        Self {
            x: bounds.width / page_text.page_width.max(1.0),
            y: bounds.height / page_text.page_height.max(1.0),
        }
    }
}

fn search_rect_to_view_rect(
    rect: &md_editor_core::pdf::PdfRect,
    page_height: f32,
    scale: Scale,
) -> md_editor_core::pdf::PdfRect {
    md_editor_core::pdf::PdfRect {
        x: rect.x * scale.x,
        y: (page_height - rect.y - rect.height) * scale.y,
        width: rect.width * scale.x,
        height: rect.height * scale.y,
    }
}

fn hit_test(
    page_text: &md_editor_core::pdf::PdfPageText,
    point: iced::Point,
    zoom: Scale,
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

/// The word containing `index`, as an inclusive `(start, end)` text-index
/// range; a lone symbol or space selects just itself.
pub(crate) fn word_around(
    page_text: &md_editor_core::pdf::PdfPageText,
    index: usize,
) -> Option<(usize, usize)> {
    let chars: Vec<char> = page_text.text.chars().collect();
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '\'' || c == '\u{2019}';
    let &at = chars.get(index)?;
    if !is_word(at) {
        return Some((index, index));
    }
    let mut start = index;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = index;
    while end + 1 < chars.len() && is_word(chars[end + 1]) {
        end += 1;
    }
    // Apostrophes join a word but do not end or start one ("'quoted'").
    while start < end && !chars[start].is_alphanumeric() {
        start += 1;
    }
    while end > start && !chars[end].is_alphanumeric() {
        end -= 1;
    }
    Some((start, end))
}

/// The text line containing `index`, as an inclusive text-index range.
pub(crate) fn line_around(
    page_text: &md_editor_core::pdf::PdfPageText,
    index: usize,
) -> Option<(usize, usize)> {
    page_text
        .lines
        .iter()
        .find(|line| index >= line.start_text_index && index < line.end_text_index)
        .map(|line| (line.start_text_index, line.end_text_index.saturating_sub(1)))
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
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(position) = cursor.position_in(bounds) {
                    state.drag_start = Some(position);
                    state.is_dragging = false;
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some(start_pos) = state.drag_start
                    && let Some(current_pos) = cursor.position()
                {
                    let current_rel =
                        iced::Point::new(current_pos.x - bounds.x, current_pos.y - bounds.y);
                    let dx = current_rel.x - start_pos.x;
                    let dy = current_rel.y - start_pos.y;
                    let dist_sq = dx * dx + dy * dy;
                    if dist_sq > 4.0 {
                        state.is_dragging = true;
                        if let Some(page_text) = self.page_text {
                            let zoom = Scale::of(bounds, page_text);
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
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(start_pos) = state.drag_start {
                    if state.is_dragging {
                        if let Some(page_text) = self.page_text
                            && let Some(current_pos) = cursor.position()
                        {
                            let current_rel = iced::Point::new(
                                current_pos.x - bounds.x,
                                current_pos.y - bounds.y,
                            );
                            let zoom = Scale::of(bounds, page_text);
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
                    } else {
                        let now = std::time::Instant::now();
                        let repeated = state.last_click.is_some_and(|(at, point)| {
                            now.duration_since(at) <= crate::design::motion::MULTI_CLICK
                                && (point.x - start_pos.x).abs() <= MULTI_CLICK_SLOP
                                && (point.y - start_pos.y).abs() <= MULTI_CLICK_SLOP
                        });
                        state.click_count = if repeated {
                            state.click_count.saturating_add(1).min(3)
                        } else {
                            1
                        };
                        state.last_click = Some((now, start_pos));

                        let multi = match (state.click_count, self.page_text) {
                            (count @ 2..=3, Some(page_text)) => {
                                let zoom = Scale::of(bounds, page_text);
                                hit_test(page_text, start_pos, zoom).and_then(|index| {
                                    if count == 2 {
                                        word_around(page_text, index)
                                    } else {
                                        line_around(page_text, index)
                                    }
                                })
                            }
                            _ => None,
                        };
                        if let Some((anchor, focus)) = multi {
                            shell.publish((self.on_selection_finished)(
                                self.page_index,
                                anchor,
                                focus,
                            ));
                        } else {
                            shell.publish((self.on_selection_cleared)());
                            if let Some(position) = cursor.position_in(bounds) {
                                let x = position.x / bounds.width.max(1.0);
                                let y = position.y / bounds.height.max(1.0);
                                shell.publish((self.on_left_click)(x, y, state.modifiers));
                            }
                        }
                    }
                    shell.capture_event();
                }
                state.drag_start = None;
                state.is_dragging = false;
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                if let Some(position) = cursor.position_in(bounds) {
                    let x = position.x / bounds.width.max(1.0);
                    let y = position.y / bounds.height.max(1.0);
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

#[cfg(test)]
mod tests;

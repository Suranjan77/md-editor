use iced::advanced::graphics::core::event::Event;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::mouse;
use iced::{Color, Element, Length, Rectangle, Size};

use md_editor_core::pdf::PdfRect;

pub struct InteractivePdf<Message> {
    handle: iced::widget::image::Handle,
    width: f32,
    height: f32,
    highlights: Vec<PdfRect>,
    active_highlights: Vec<PdfRect>,
    on_left_click: Box<dyn Fn(f32, f32) -> Message>,
    on_right_click: Box<dyn Fn(f32, f32) -> Message>,
}

impl<Message> InteractivePdf<Message> {
    pub fn new(
        handle: iced::widget::image::Handle,
        width: f32,
        height: f32,
        on_left_click: impl Fn(f32, f32) -> Message + 'static,
        on_right_click: impl Fn(f32, f32) -> Message + 'static,
    ) -> Self {
        Self {
            handle,
            width,
            height,
            highlights: Vec::new(),
            active_highlights: Vec::new(),
            on_left_click: Box::new(on_left_click),
            on_right_click: Box::new(on_right_click),
        }
    }

    pub fn highlights(mut self, highlights: Vec<PdfRect>, active_highlights: Vec<PdfRect>) -> Self {
        self.highlights = highlights;
        self.active_highlights = active_highlights;
        self
    }
}

impl<Message, Theme, R> Widget<Message, Theme, R> for InteractivePdf<Message>
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
        renderer.draw_image(
            iced::advanced::image::Image::new(self.handle.clone()),
            layout.bounds(),
            *viewport,
        );

        let bounds = layout.bounds();
        for rect in &self.highlights {
            draw_highlight(
                renderer,
                bounds,
                rect,
                Color::from_rgba(1.0, 0.78, 0.18, 0.38),
            );
        }
        for rect in &self.active_highlights {
            draw_highlight(
                renderer,
                bounds,
                rect,
                Color::from_rgba(1.0, 0.62, 0.0, 0.68),
            );
        }
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
        if let Event::Mouse(mouse::Event::ButtonPressed(button)) = event {
            if matches!(button, mouse::Button::Left | mouse::Button::Right) {
                if let Some(position) = cursor.position_in(layout.bounds()) {
                    let x = position.x / self.width;
                    let y = position.y / self.height;
                    match button {
                        mouse::Button::Left => shell.publish((self.on_left_click)(x, y)),
                        mouse::Button::Right => shell.publish((self.on_right_click)(x, y)),
                        _ => {}
                    }
                }
            }
        }
    }
}

fn draw_highlight<R>(renderer: &mut R, page_bounds: Rectangle, rect: &PdfRect, color: Color)
where
    R: renderer::Renderer,
{
    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle {
                x: page_bounds.x + rect.x,
                y: page_bounds.y + rect.y,
                width: rect.width.max(3.0),
                height: rect.height.max(8.0),
            },
            border: iced::Border {
                radius: 2.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        color,
    );
}

impl<'a, Message, Theme, R> From<InteractivePdf<Message>> for Element<'a, Message, Theme, R>
where
    R: renderer::Renderer + iced::advanced::image::Renderer<Handle = iced::widget::image::Handle>,
    Message: 'a,
{
    fn from(widget: InteractivePdf<Message>) -> Self {
        Self::new(widget)
    }
}

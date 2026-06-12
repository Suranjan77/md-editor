use iced::mouse;
use iced::widget::canvas;
use iced::{Element, Length, Point, Rectangle, Renderer, Theme};
use md3_kernel::pane::{SplitAxis, SplitPath};

use super::Message;

const DRAG_SCALE_PX: f32 = 800.0;

#[derive(Debug, Default)]
struct State {
    start: Option<(Point, f32)>,
}

#[derive(Debug, Clone)]
struct Divider {
    path: SplitPath,
    axis: SplitAxis,
    initial_ratio: f32,
}

pub fn divider(path: SplitPath, axis: SplitAxis, ratio: f32) -> Element<'static, Message> {
    let canvas = canvas(Divider {
        path,
        axis,
        initial_ratio: ratio,
    });
    match axis {
        SplitAxis::Horizontal => canvas.width(6).height(Length::Fill).into(),
        SplitAxis::Vertical => canvas.width(Length::Fill).height(6).into(),
    }
}

impl canvas::Program<Message> for Divider {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(bounds) =>
            {
                state.start = cursor
                    .position()
                    .map(|position| (position, self.initial_ratio));
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let (start, initial_ratio) = state.start?;
                let delta = match self.axis {
                    SplitAxis::Horizontal => position.x - start.x,
                    SplitAxis::Vertical => position.y - start.y,
                };
                Some(
                    canvas::Action::publish(Message::SplitRatioDragged {
                        path: self.path.clone(),
                        ratio: initial_ratio + delta / DRAG_SCALE_PX,
                    })
                    .and_capture(),
                )
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.start.take().is_some() =>
            {
                Some(canvas::Action::publish(Message::SplitRatioDragFinished).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), super::tokens::dark().border);
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if !cursor.is_over(bounds) {
            return mouse::Interaction::default();
        }
        match self.axis {
            SplitAxis::Horizontal => mouse::Interaction::ResizingHorizontally,
            SplitAxis::Vertical => mouse::Interaction::ResizingVertically,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKind {
    Toc,
    Annotations,
    Outline,
}

pub struct PanelResizer {
    kind: PanelKind,
    initial_width: f32,
}

#[derive(Debug, Default)]
pub struct PanelResizerState {
    start: Option<(Point, f32)>,
}

pub fn panel_resizer(kind: PanelKind, initial_width: f32) -> Element<'static, Message> {
    canvas(PanelResizer {
        kind,
        initial_width,
    })
    .width(6)
    .height(Length::Fill)
    .into()
}

impl canvas::Program<Message> for PanelResizer {
    type State = PanelResizerState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(bounds) =>
            {
                state.start = cursor.position().map(|pos| (pos, self.initial_width));
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let (start, initial_width) = state.start?;
                let delta = position.x - start.x;
                Some(
                    canvas::Action::publish(Message::PanelResized {
                        kind: self.kind,
                        width: initial_width - delta,
                    })
                    .and_capture(),
                )
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.start.take().is_some() =>
            {
                Some(
                    canvas::Action::publish(Message::PanelResizeFinished { kind: self.kind })
                        .and_capture(),
                )
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), super::tokens::dark().border);
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if !cursor.is_over(bounds) {
            return mouse::Interaction::default();
        }
        mouse::Interaction::ResizingHorizontally
    }
}

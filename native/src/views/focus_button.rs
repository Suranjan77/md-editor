use iced::Border;
use iced::advanced::Clipboard;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::Widget;
use iced::advanced::widget::operation::{self, Operation};
use iced::advanced::widget::tree::{self, Tree};
use iced::event::Event;
use iced::keyboard;
use iced::mouse;
use iced::{Element, Length, Rectangle, Size};

pub fn focus_button<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> FocusButton<'a, Message, Theme, Renderer> {
    FocusButton::new(content)
}

pub struct FocusButton<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    on_press: Option<Message>,
    id: Option<iced::advanced::widget::Id>,
    border_radius: f32,
    padding: f32,
    active: bool,
    subtle: bool, // For buttons that shouldn't have a background unless hovered/active
    width: Length,
    height: Length,
}

impl<'a, Message, Theme, Renderer> FocusButton<'a, Message, Theme, Renderer> {
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            on_press: None,
            id: None,
            border_radius: 4.0,
            padding: 4.0,
            active: false,
            subtle: false,
            width: Length::Shrink,
            height: Length::Shrink,
        }
    }

    pub fn on_press(mut self, msg: Message) -> Self {
        self.on_press = Some(msg);
        self
    }

    pub fn id(mut self, id: iced::advanced::widget::Id) -> Self {
        self.id = Some(id);
        self
    }

    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = padding;
        self
    }

    pub fn border_radius(mut self, radius: f32) -> Self {
        self.border_radius = radius;
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn subtle(mut self, subtle: bool) -> Self {
        self.subtle = subtle;
        self
    }

    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }
}

#[derive(Default)]
pub struct State {
    is_focused: bool,
    is_pressed: bool,
}

impl operation::Focusable for State {
    fn is_focused(&self) -> bool {
        self.is_focused
    }

    fn focus(&mut self) {
        self.is_focused = true;
    }

    fn unfocus(&mut self) {
        self.is_focused = false;
    }
}

impl<'a, Message: Clone, Theme, Renderer: renderer::Renderer> Widget<Message, Theme, Renderer>
    for FocusButton<'a, Message, Theme, Renderer>
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits
            .width(self.width)
            .height(self.height)
            .shrink(Size::new(self.padding * 2.0, self.padding * 2.0));

        let mut child_node =
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, &limits);

        let size = limits.resolve(
            self.width,
            self.height,
            child_node
                .size()
                .expand(Size::new(self.padding * 2.0, self.padding * 2.0)),
        );

        child_node = child_node.move_to(iced::Point::new(self.padding, self.padding));
        layout::Node::with_children(size, vec![child_node])
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let state = tree.state.downcast_mut::<State>();
        operation.focusable(self.id.as_ref(), layout.bounds(), state);

        if let Some(child_layout) = layout.children().next() {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                child_layout,
                renderer,
                operation,
            );
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut iced::advanced::Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();

        match event {
            Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => {
                if state.is_focused {
                    if *key == keyboard::Key::Named(keyboard::key::Named::Enter)
                        || *key == keyboard::Key::Named(keyboard::key::Named::Space)
                    {
                        if let Some(msg) = &self.on_press {
                            shell.publish(msg.clone());
                            shell.capture_event();
                        }
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if cursor.is_over(bounds) {
                    state.is_pressed = true;
                    if self.on_press.is_some() {
                        use iced::advanced::widget::operation::Focusable;
                        state.focus();
                    }
                    shell.capture_event();
                } else if state.is_focused {
                    use iced::advanced::widget::operation::Focusable;
                    state.unfocus();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.is_pressed => {
                state.is_pressed = false;
                if cursor.is_over(bounds) {
                    if let Some(msg) = &self.on_press {
                        shell.publish(msg.clone());
                    }
                }
                shell.capture_event();
            }
            _ => {}
        }

        if let Some(child_layout) = layout.children().next() {
            self.content.as_widget_mut().update(
                &mut tree.children[0],
                event,
                child_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.on_press.is_some() && cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else if let Some(child_layout) = layout.children().next() {
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                child_layout,
                cursor,
                viewport,
                renderer,
            )
        } else {
            mouse::Interaction::None
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let is_hovered = cursor.is_over(bounds);

        let bg_color = if self.active {
            Some(crate::theme::bg_tertiary())
        } else if is_hovered && self.on_press.is_some() {
            Some(crate::theme::bg_secondary())
        } else if !self.subtle {
            Some(crate::theme::bg_secondary())
        } else {
            None
        };

        if let Some(color) = bg_color {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: Border {
                        color: iced::Color::TRANSPARENT,
                        width: 0.0,
                        radius: self.border_radius.into(),
                    },
                    ..Default::default()
                },
                color,
            );
        }

        if let Some(child_layout) = layout.children().next() {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                child_layout,
                cursor,
                viewport,
            );
        }

        if state.is_focused {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: Border {
                        color: crate::theme::accent(),
                        width: crate::theme::FOCUS_RING_WIDTH,
                        radius: self.border_radius.into(),
                    },
                    ..Default::default()
                },
                iced::Color::TRANSPARENT,
            );
        }
    }
}

impl<'a, Message, Theme: 'a, Renderer> From<FocusButton<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(button: FocusButton<'a, Message, Theme, Renderer>) -> Self {
        Self::new(button)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum TestMessage {
        Pressed,
    }

    #[test]
    fn focused_button_activates_with_enter() {
        let button: FocusButton<'_, TestMessage, iced::Theme, iced::Renderer> =
            focus_button(text("Focusable row")).on_press(TestMessage::Pressed);
        let mut ui = iced_test::simulator(button);

        ui.click("Focusable row")
            .expect("mouse click should focus and activate row");
        ui.tap_key(keyboard::Key::Named(keyboard::key::Named::Enter));

        assert_eq!(
            ui.into_messages().collect::<Vec<_>>(),
            vec![TestMessage::Pressed, TestMessage::Pressed]
        );
    }
}

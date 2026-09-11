//! Test support: a recording renderer and a driver that exercises [`Editor`]
//! through its `Widget` API, the way iced does.

use iced::advanced::graphics::core::event::Event;
use iced::advanced::layout::{Layout, Limits, Node};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Clipboard, Shell, clipboard, image, text};
use iced::keyboard::{self, Key, key};
use iced::{Background, Color, Point, Rectangle, Size, mouse};

use super::{Editor, ImageCache, MathCache, MathRender, State};
use crate::editor::buffer::{DocBuffer, EditorCommand};
use crate::editor::highlight::{StyledLine, StyledSpan, highlight_markdown};

type Real = iced::Renderer;

/// One primitive the widget drew.
#[derive(Debug, Clone)]
pub(crate) enum Call {
    Quad {
        bounds: Rectangle,
        color: Color,
        raw: String,
    },
    Text {
        content: String,
        size: f32,
        font: iced::Font,
        at: Point,
        raw: String,
    },
    Image {
        bounds: Rectangle,
        raw: String,
    },
    Other(String),
}

impl Call {
    /// Stable one-line description, used for transcripts.
    pub fn describe(&self) -> &str {
        match self {
            Call::Quad { raw, .. } | Call::Text { raw, .. } | Call::Image { raw, .. } => raw,
            Call::Other(raw) => raw,
        }
    }
}

/// Renders nothing; records every primitive. Measurement types are borrowed
/// from the real renderer so text metrics match production exactly.
#[derive(Default)]
pub(crate) struct Recorder {
    pub calls: Vec<Call>,
}

impl renderer::Renderer for Recorder {
    fn start_layer(&mut self, bounds: Rectangle) {
        self.calls.push(Call::Other(format!("layer {:?}", bounds)));
    }
    fn end_layer(&mut self) {
        self.calls.push(Call::Other("end_layer".into()));
    }
    fn start_transformation(&mut self, _t: iced::Transformation) {
        self.calls.push(Call::Other("transform".into()));
    }
    fn end_transformation(&mut self) {
        self.calls.push(Call::Other("end_transform".into()));
    }
    fn fill_quad(&mut self, quad: Quad, background: impl Into<Background>) {
        let bg: Background = background.into();
        let raw = format!(
            "quad {:?} border={:?} shadow={:?} snap={} bg={:?}",
            quad.bounds, quad.border, quad.shadow, quad.snap, bg
        );
        let color = match bg {
            Background::Color(color) => color,
            Background::Gradient(_) => Color::TRANSPARENT,
        };
        self.calls.push(Call::Quad {
            bounds: quad.bounds,
            color,
            raw,
        });
    }
    fn reset(&mut self, _new_bounds: Rectangle) {}
    fn allocate_image(
        &mut self,
        _handle: &image::Handle,
        _callback: impl FnOnce(Result<image::Allocation, image::Error>) + Send + 'static,
    ) {
    }
}

impl text::Renderer for Recorder {
    type Font = iced::Font;
    type Paragraph = <Real as text::Renderer>::Paragraph;
    type Editor = <Real as text::Renderer>::Editor;

    const ICON_FONT: Self::Font = <Real as text::Renderer>::ICON_FONT;
    const CHECKMARK_ICON: char = <Real as text::Renderer>::CHECKMARK_ICON;
    const ARROW_DOWN_ICON: char = <Real as text::Renderer>::ARROW_DOWN_ICON;
    const SCROLL_UP_ICON: char = <Real as text::Renderer>::SCROLL_UP_ICON;
    const SCROLL_DOWN_ICON: char = <Real as text::Renderer>::SCROLL_DOWN_ICON;
    const SCROLL_LEFT_ICON: char = <Real as text::Renderer>::SCROLL_LEFT_ICON;
    const SCROLL_RIGHT_ICON: char = <Real as text::Renderer>::SCROLL_RIGHT_ICON;
    const ICED_LOGO: char = <Real as text::Renderer>::ICED_LOGO;

    fn default_font(&self) -> Self::Font {
        iced::Font::DEFAULT
    }
    fn default_size(&self) -> iced::Pixels {
        16.0.into()
    }
    fn fill_paragraph(&mut self, _t: &Self::Paragraph, _p: Point, _c: Color, _clip: Rectangle) {
        self.calls.push(Call::Other("paragraph".into()));
    }
    fn fill_editor(&mut self, _e: &Self::Editor, _p: Point, _c: Color, _clip: Rectangle) {
        self.calls.push(Call::Other("editor".into()));
    }
    fn fill_text(
        &mut self,
        t: text::Text<String, Self::Font>,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
    ) {
        let raw = format!(
            "text {:?} bounds={:?} size={:?} lh={:?} font={:?} ax={:?} ay={:?} shaping={:?} wrap={:?} at={:?} color={:?} clip={:?}",
            t.content,
            t.bounds,
            t.size,
            t.line_height,
            t.font,
            t.align_x,
            t.align_y,
            t.shaping,
            t.wrapping,
            position,
            color,
            clip_bounds
        );
        self.calls.push(Call::Text {
            content: t.content,
            size: t.size.0,
            font: t.font,
            at: position,
            raw,
        });
    }
}

impl image::Renderer for Recorder {
    type Handle = image::Handle;
    fn load_image(&self, _h: &Self::Handle) -> Result<image::Allocation, image::Error> {
        Err(image::Error::Unsupported)
    }
    fn measure_image(&self, _h: &Self::Handle) -> Option<Size<u32>> {
        None
    }
    fn draw_image(&mut self, img: image::Image<Self::Handle>, bounds: Rectangle, clip: Rectangle) {
        let raw = format!(
            "image {:?} filter={:?} rot={:?} opacity={} snap={} bounds={:?} clip={:?}",
            img.handle.id(),
            img.filter_method,
            img.rotation,
            img.opacity,
            img.snap,
            bounds,
            clip
        );
        self.calls.push(Call::Image { bounds, raw });
    }
}

#[derive(Default)]
pub(crate) struct RecordingClipboard {
    pub writes: Vec<String>,
}

impl Clipboard for RecordingClipboard {
    fn read(&self, _kind: clipboard::Kind) -> Option<String> {
        Some("PASTED".into())
    }
    fn write(&mut self, _kind: clipboard::Kind, contents: String) {
        self.writes.push(contents);
    }
}

/// What one delivered event produced.
pub(crate) struct EventOutcome {
    pub messages: Vec<String>,
    pub captured: bool,
    pub clipboard_writes: Vec<String>,
}

/// An editor widget with its own buffer, caches and widget tree, driven the
/// way iced drives it. Messages are the debug form of what was published:
/// `cmd …`, `ptr …`, `link …`, `check …`.
pub(crate) struct View {
    pub buffer: DocBuffer,
    pub lines: Vec<StyledLine>,
    pub images: ImageCache,
    pub math: MathCache,
    pub tree: Tree,
    pub width: f32,
    pub search: (&'static str, bool, bool, Option<(usize, usize)>),
}

type Wd<'a> = Editor<'a, String>;

impl View {
    pub fn new(doc: &str, width: f32) -> Self {
        let buffer = DocBuffer::from_text(doc);
        let lines = highlight_markdown(doc);
        let mut view = Self {
            buffer,
            lines,
            images: ImageCache::new(),
            math: MathCache::new(),
            tree: Tree::empty(),
            width,
            search: ("", false, false, None),
        };
        view.reset_tree();
        view
    }

    pub fn editor(&self) -> Editor<'_, String> {
        Editor::new(
            &self.buffer,
            &self.lines,
            &self.images,
            &self.math,
            |c| format!("cmd {c:?}"),
            |c| format!("ptr {c:?}"),
            |t| format!("link {t}"),
            |l| format!("check {l}"),
        )
        .search(self.search.0, self.search.1, self.search.2, self.search.3)
        .scale_factor(1.5)
    }

    pub fn reset_tree(&mut self) {
        let (tag, state) = {
            let editor = self.editor();
            (
                <Wd<'_> as Widget<String, iced::Theme, Recorder>>::tag(&editor),
                <Wd<'_> as Widget<String, iced::Theme, Recorder>>::state(&editor),
            )
        };
        self.tree = Tree {
            tag,
            state,
            children: Vec::new(),
        };
    }

    pub fn add_math(&mut self, tex: &str, width: f32, height: f32) {
        let handle = image::Handle::from_rgba(2, 2, vec![7; 16]);
        self.math.insert(
            tex.to_string(),
            MathRender {
                inline_handle: handle.clone(),
                block_handle: handle,
                width,
                height,
            },
        );
    }

    pub fn layout(&mut self) -> Node {
        let width = self.width;
        let mut tree = std::mem::replace(&mut self.tree, Tree::empty());
        let node = {
            let mut editor = self.editor();
            <Wd<'_> as Widget<String, iced::Theme, Recorder>>::layout(
                &mut editor,
                &mut tree,
                &Recorder::default(),
                &Limits::new(Size::ZERO, Size::new(width, f32::INFINITY)),
            )
        };
        self.tree = tree;
        node
    }

    /// Lay out and draw with the whole widget in view.
    pub fn draw(&mut self) -> Vec<Call> {
        self.draw_in(None)
    }

    pub fn draw_in(&mut self, viewport: Option<Rectangle>) -> Vec<Call> {
        let node = self.layout();
        let viewport = viewport.unwrap_or(node.bounds());
        let mut rec = Recorder::default();
        let editor = self.editor();
        <Wd<'_> as Widget<String, iced::Theme, Recorder>>::draw(
            &editor,
            &self.tree,
            &mut rec,
            &iced::Theme::Dark,
            &renderer::Style::default(),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &viewport,
        );
        rec.calls
    }

    pub fn event(&mut self, event: Event, cursor: Option<Point>) -> EventOutcome {
        let node = self.layout();
        self.event_on(event, cursor, node)
    }

    /// Deliver an event against `node` without running widget layout first.
    pub fn event_on(&mut self, event: Event, cursor: Option<Point>, node: Node) -> EventOutcome {
        let mut messages = Vec::new();
        let mut clip = RecordingClipboard::default();
        let captured;
        let mut tree = std::mem::replace(&mut self.tree, Tree::empty());
        {
            let mut editor = self.editor();
            let mut shell = Shell::new(&mut messages);
            let cursor = cursor.map_or(mouse::Cursor::Unavailable, mouse::Cursor::Available);
            <Wd<'_> as Widget<String, iced::Theme, Recorder>>::update(
                &mut editor,
                &mut tree,
                &event,
                Layout::new(&node),
                cursor,
                &Recorder::default(),
                &mut clip,
                &mut shell,
                &node.bounds(),
            );
            captured = shell.is_event_captured();
        }
        self.tree = tree;
        EventOutcome {
            messages,
            captured,
            clipboard_writes: clip.writes,
        }
    }

    pub fn interaction(&mut self, p: Point) -> mouse::Interaction {
        let node = self.layout();
        let editor = self.editor();
        <Wd<'_> as Widget<String, iced::Theme, Recorder>>::mouse_interaction(
            &editor,
            &self.tree,
            Layout::new(&node),
            mouse::Cursor::Available(p),
            &node.bounds(),
            &Recorder::default(),
        )
    }

    pub fn set_cursor(&mut self, line: usize, col: usize) {
        self.buffer.execute(EditorCommand::SetCursor { line, col });
    }

    pub fn press(&mut self, p: Point) -> EventOutcome {
        self.event(
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Some(p),
        )
    }

    pub fn release(&mut self, p: Point) -> EventOutcome {
        self.event(
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            Some(p),
        )
    }

    pub fn move_to(&mut self, p: Point) -> EventOutcome {
        self.event(
            Event::Mouse(mouse::Event::CursorMoved { position: p }),
            Some(p),
        )
    }

    /// Press and release; returns what the press published.
    pub fn click(&mut self, p: Point) -> EventOutcome {
        let outcome = self.press(p);
        self.release(p);
        outcome
    }

    pub fn wheel(&mut self, p: Point, delta: mouse::ScrollDelta) -> EventOutcome {
        self.event(Event::Mouse(mouse::Event::WheelScrolled { delta }), Some(p))
    }

    pub fn key(
        &mut self,
        k: Key,
        modifiers: keyboard::Modifiers,
        text: Option<&str>,
    ) -> EventOutcome {
        self.event(
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: k.clone(),
                modified_key: k,
                physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
                location: keyboard::Location::Standard,
                modifiers,
                text: text.map(Into::into),
                repeat: false,
            }),
            None,
        )
    }

    pub fn modifiers(&mut self, m: keyboard::Modifiers) -> EventOutcome {
        self.event(Event::Keyboard(keyboard::Event::ModifiersChanged(m)), None)
    }

    /// Focus the editor without moving the buffer's caret.
    pub fn focus(&mut self) {
        let (line, col) = (self.buffer.cursor_line, self.buffer.cursor_col);
        self.click(Point::new(self.width.min(880.0) - 1.0, 1.0));
        self.set_cursor(line, col);
    }
}

pub fn make_line(block_id: usize, spans: Vec<StyledSpan>) -> StyledLine {
    let mut line = StyledLine::new();
    line.block_id = block_id;
    line.spans = spans;
    line
}

pub fn editor_for<'a>(
    buffer: &'a DocBuffer,
    lines: &'a [StyledLine],
    image_cache: &'a ImageCache,
    math_cache: &'a MathCache,
) -> Editor<'a, ()> {
    Editor::new(
        buffer,
        lines,
        image_cache,
        math_cache,
        |_| (),
        |_| (),
        |_| (),
        |_| (),
    )
}

pub fn focused_state() -> State {
    State {
        is_focused: true,
        ..Default::default()
    }
}

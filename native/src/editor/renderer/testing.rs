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
use crate::editor::buffer::{Affinity, DocBuffer, EditorCommand};
use crate::editor::highlight::{StyledLine, StyledSpan, highlight_markdown};

type Real = iced::Renderer;

/// One primitive the widget drew.
#[derive(Debug, Clone)]
pub(crate) enum Call {
    Quad {
        bounds: Rectangle,
        color: Color,
        /// The layer it was drawn into; see [`Call::paint_order`].
        layer: usize,
        raw: String,
    },
    Text {
        content: String,
        size: f32,
        font: iced::Font,
        at: Point,
        align_y: iced::alignment::Vertical,
        /// What every renderer clips it to. Only a layer clips; drawing text
        /// with a clip rectangle inside its layer turns tiny-skia's clip off.
        clip: Rectangle,
        layer: usize,
        raw: String,
    },
    Image {
        bounds: Rectangle,
        /// What a renderer clips it to: its layers only. Renderers ignore the
        /// clip rectangle drawn with an image.
        clip: Rectangle,
        layer: usize,
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

    /// When a renderer actually paints the call recorded at `index`: layers
    /// in the order they were opened, and within a layer every quad, then
    /// every image, then all text — whatever order they were drawn in. Later
    /// sorts on top.
    pub fn paint_order(&self, index: usize) -> (usize, u8, usize) {
        match self {
            Call::Quad { layer, .. } => (*layer, 0, index),
            Call::Image { layer, .. } => (*layer, 1, index),
            Call::Text { layer, .. } => (*layer, 2, index),
            Call::Other(_) => (0, 0, index),
        }
    }
}

/// Renders nothing; records every primitive. Measurement types are borrowed
/// from the real renderer so text metrics match production exactly.
#[derive(Default)]
pub(crate) struct Recorder {
    pub calls: Vec<Call>,
    /// The clip of each open layer, innermost last.
    layers: Vec<Rectangle>,
    /// The layer being drawn into, the layers it was opened from, and how
    /// many have been opened — tracked the way iced's layer stack does: a new
    /// layer paints after every earlier one, and closing it returns to its
    /// parent.
    current: usize,
    parents: Vec<usize>,
    opened: usize,
}

/// Stands for no clip at all.
const UNCLIPPED: Rectangle = Rectangle {
    x: -1e9,
    y: -1e9,
    width: 2e9,
    height: 2e9,
};

impl Recorder {
    /// The clip a renderer applies to a primitive: the innermost layer's,
    /// narrowed by the clip drawn with it where the renderer honours one.
    fn clip(&self, drawn_with: Option<Rectangle>) -> Rectangle {
        let layer = self.layers.last().copied().unwrap_or(UNCLIPPED);
        drawn_with.map_or(layer, |clip| {
            layer.intersection(&clip).unwrap_or(Rectangle {
                width: 0.0,
                height: 0.0,
                ..clip
            })
        })
    }
}

impl renderer::Renderer for Recorder {
    fn start_layer(&mut self, bounds: Rectangle) {
        self.calls.push(Call::Other(format!("layer {:?}", bounds)));
        let clip = self.clip(Some(bounds));
        self.layers.push(clip);
        self.parents.push(self.current);
        self.opened += 1;
        self.current = self.opened;
    }
    fn end_layer(&mut self) {
        self.calls.push(Call::Other("end_layer".into()));
        self.layers.pop();
        self.current = self.parents.pop().unwrap_or(0);
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
            layer: self.current,
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
        // wgpu clips text to its layer and `clip_bounds`. tiny-skia culls text
        // whose `clip_bounds` miss the layer, and masks the rest to the layer
        // only when `clip_bounds` reach past it: clip bounds inside the layer
        // leave text unclipped. What holds in both is the weaker.
        let layer = self.clip(None);
        let clip = if !layer.intersects(&clip_bounds) {
            Rectangle {
                width: 0.0,
                height: 0.0,
                ..clip_bounds
            }
        } else if clip_bounds.is_within(&layer) {
            UNCLIPPED
        } else {
            layer
        };
        self.calls.push(Call::Text {
            content: t.content,
            size: t.size.0,
            font: t.font,
            at: position,
            align_y: t.align_y,
            clip,
            layer: self.current,
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
        let clip = self.clip(None);
        self.calls.push(Call::Image {
            bounds,
            clip,
            layer: self.current,
            raw,
        });
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
    /// The reveal request the widget is shown, as the app bumps it.
    pub reveal_request: u64,
    /// The layout revision the widget is given, if any.
    pub layout_revision: Option<u64>,
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
            reveal_request: 0,
            layout_revision: None,
        };
        view.reset_tree();
        view
    }

    pub fn editor(&self) -> Editor<'_, String> {
        let editor = Editor::new(
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
        .reveal_caret(self.reveal_request, |view| format!("caret {view:?}"));
        match self.layout_revision {
            Some(revision) => editor.layout_revision(revision),
            None => editor,
        }
    }

    pub fn state(&self) -> &State {
        self.tree.state.downcast_ref::<State>()
    }

    /// Re-highlight after editing `buffer` directly.
    pub fn rehighlight(&mut self) {
        self.lines = highlight_markdown(&self.buffer.text());
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
        let viewport = node.bounds();
        self.deliver(event, cursor, node, viewport)
    }

    /// A frame about to be drawn with `viewport` — widget coordinates, as a
    /// scrollable passes them — in view.
    pub fn redraw(&mut self, viewport: Rectangle) -> EventOutcome {
        self.redraw_at(viewport, std::time::Instant::now())
    }

    /// [`View::redraw`] for a frame requested at `at`.
    pub fn redraw_at(&mut self, viewport: Rectangle, at: std::time::Instant) -> EventOutcome {
        let node = self.layout();
        let event = Event::Window(iced::window::Event::RedrawRequested(at));
        self.deliver(event, None, node, viewport)
    }

    fn deliver(
        &mut self,
        event: Event,
        cursor: Option<Point>,
        node: Node,
        viewport: Rectangle,
    ) -> EventOutcome {
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
                &viewport,
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
        self.place_caret(line, col, Affinity::Downstream);
    }

    pub fn place_caret(&mut self, line: usize, col: usize, affinity: Affinity) {
        self.buffer.execute(EditorCommand::SetCursor {
            line,
            col,
            affinity,
        });
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

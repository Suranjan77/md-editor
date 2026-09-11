//! The markdown editor widget.
//!
//! [`Editor`] is a custom iced widget that lays out, paints and handles input
//! for a [`DocBuffer`] shown through its highlighted [`StyledLine`]s. The work
//! is split by concern:
//!
//! - `metrics`: layout constants that layout, painting and hit-testing must
//!   agree on
//! - `measure`: memoized text shaping
//! - `spans`: which spans reveal their markdown source while being edited
//! - `layout`: per-line heights and the height tree
//! - `caret`: mapping between source columns and visual positions
//! - `selection`: selection normalization and extraction
//! - `scroll`: horizontal scrolling of wide blocks (code, tables, math)
//! - `events`: mouse and keyboard handling
//! - `draw`: painting

use std::collections::HashMap;

use iced::advanced::graphics::core::event::Event;
use iced::advanced::layout::{Layout, Limits, Node};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell, renderer};
use iced::{Element, Length, Rectangle, Size, keyboard, mouse};

use crate::editor::buffer::{DocBuffer, EditorCommand};
use crate::editor::highlight::StyledLine;
use crate::editor::layout_cache::LineHeightCache;
use crate::editor::layout_tree::HeightTree;

mod caret;
mod draw;
mod events;
mod layout;
mod measure;
mod metrics;
mod scroll;
mod selection;
mod spans;

pub use layout::line_visual_y;

/// Display-size boost applied to block (`$$`) math relative to inline math.
/// The rasterizer bakes this into the bitmap's device-pixel ratio (see
/// `render_latex_task`) so block math is displayed at exactly 1:1 device
/// pixels instead of being upscaled from a smaller bitmap.
pub const MATH_BLOCK_SCALE: f32 = 1.2;

/// A rendered equation. Two bitmaps are rasterized per TeX string — one at
/// inline display scale and one at block display scale (`MATH_BLOCK_SCALE`
/// larger) — so each context draws its bitmap 1:1 on device pixels instead of
/// resampling one bitmap at a fractional ratio (which blurs thin strokes).
/// `width`/`height` are the logical layout size shared by both; block layout
/// multiplies by `MATH_BLOCK_SCALE`.
#[derive(Debug, Clone)]
pub struct MathRender {
    pub inline_handle: iced::widget::image::Handle,
    pub block_handle: iced::widget::image::Handle,
    pub width: f32,
    pub height: f32,
}

/// Decoded images keyed by path: handle plus natural width and height.
pub type ImageCache = HashMap<String, (iced::widget::image::Handle, f32, f32)>;

/// Rasterized equations keyed by their trimmed TeX source.
pub type MathCache = HashMap<String, MathRender>;

/// Renderer capability needed to measure text.
trait Measure: iced::advanced::text::Renderer<Font = iced::Font> {}
impl<T: iced::advanced::text::Renderer<Font = iced::Font>> Measure for T {}

/// Renderer capability needed to paint the editor.
trait Paint: Measure + iced::advanced::image::Renderer<Handle = iced::widget::image::Handle> {}
impl<T> Paint for T where
    T: Measure + iced::advanced::image::Renderer<Handle = iced::widget::image::Handle>
{
}

pub struct Editor<'a, Message> {
    buffer: &'a DocBuffer,
    lines: &'a [StyledLine],
    image_cache: &'a ImageCache,
    math_cache: &'a MathCache,
    search_query: &'a str,
    search_regex: bool,
    search_match_case: bool,
    active_search_match: Option<(usize, usize)>,
    /// Device pixels per logical unit; used to snap rendered math bitmaps to
    /// the device-pixel grid so 1-px glyph strokes don't straddle two pixels.
    scale_factor: f32,
    on_command: Box<dyn Fn(EditorCommand) -> Message + 'a>,
    on_pointer_command: Box<dyn Fn(EditorCommand) -> Message + 'a>,
    on_link_click: Box<dyn Fn(String) -> Message + 'a>,
    on_checkbox_toggle: Box<dyn Fn(usize) -> Message + 'a>,
}

/// Widget state persisted across frames in the iced tree.
#[derive(Default)]
pub struct State {
    /// A left-button drag is extending the pointer selection.
    is_dragging: bool,
    is_focused: bool,
    modifiers: keyboard::Modifiers,
    /// Pointer-driven selection, in (line, col). Takes precedence over the
    /// buffer's own selection when painting.
    selection_anchor: Option<(usize, usize)>,
    selection_focus: Option<(usize, usize)>,
    /// Horizontal scroll offset of each wide block, keyed by block id.
    block_scroll_x: HashMap<usize, f32>,
    horizontal_scroll_drag: Option<scroll::HorizontalScrollDrag>,
    /// Column-preserving x for repeated up/down movement.
    desired_visual_x: Option<f32>,
    /// Prefix sums of line heights, for O(log n) y ⇄ line lookups.
    layout_tree: HeightTree,
    line_height_cache: Vec<LineHeightCache>,
    last_layout_width: f32,
    /// First and last line index of every code, table, math and quote block.
    block_ranges: HashMap<usize, (usize, usize)>,
}

impl<'a, Message> Editor<'a, Message> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        buffer: &'a DocBuffer,
        lines: &'a [StyledLine],
        image_cache: &'a ImageCache,
        math_cache: &'a MathCache,
        on_command: impl Fn(EditorCommand) -> Message + 'a,
        on_pointer_command: impl Fn(EditorCommand) -> Message + 'a,
        on_link_click: impl Fn(String) -> Message + 'a,
        on_checkbox_toggle: impl Fn(usize) -> Message + 'a,
    ) -> Self {
        Self {
            buffer,
            lines,
            image_cache,
            math_cache,
            search_query: "",
            search_regex: false,
            search_match_case: false,
            active_search_match: None,
            scale_factor: 1.0,
            on_command: Box::new(on_command),
            on_pointer_command: Box::new(on_pointer_command),
            on_link_click: Box::new(on_link_click),
            on_checkbox_toggle: Box::new(on_checkbox_toggle),
        }
    }

    pub fn search(
        mut self,
        query: &'a str,
        regex: bool,
        match_case: bool,
        active_match: Option<(usize, usize)>,
    ) -> Self {
        self.search_query = query;
        self.search_regex = regex;
        self.search_match_case = match_case;
        self.active_search_match = active_match;
        self
    }

    pub fn scale_factor(mut self, factor: f32) -> Self {
        self.scale_factor = factor.max(1.0);
        self
    }

    /// Block id of the line holding the caret.
    fn active_block_id(&self) -> Option<usize> {
        self.lines.get(self.buffer.cursor_line).map(|l| l.block_id)
    }

    /// The caret column if the caret is on `line_idx` and `focused` holds.
    fn active_col(&self, line_idx: usize, focused: bool) -> Option<usize> {
        (focused && line_idx == self.buffer.cursor_line).then_some(self.buffer.cursor_col)
    }

    /// Whether `line` belongs to the code, math or table block holding the
    /// caret, in which case the whole block shows its markdown source.
    fn is_block_editing(&self, line: &StyledLine, focused: bool) -> bool {
        spans::is_block_editing_line(line, self.active_block_id(), focused)
    }
}

impl<'a, Message, Theme, R> Widget<Message, Theme, R> for Editor<'a, Message>
where
    R: renderer::Renderer
        + iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::image::Renderer<Handle = iced::widget::image::Handle>,
{
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fixed(layout::total_height::<R>(
                self.lines,
                self.image_cache,
                self.math_cache,
                800.0,
                None,
                None,
                false,
            )),
        }
    }

    fn layout(&mut self, tree: &mut widget::Tree, _renderer: &R, limits: &Limits) -> Node {
        let state = tree.state.downcast_mut::<State>();
        let max_width = limits.max().width.min(metrics::MAX_CONTENT_WIDTH);
        let height = self.layout_lines::<R>(state, max_width);
        Node::new(limits.resolve(Length::Fill, Length::Fixed(height), Size::ZERO))
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut R,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        self.draw_document(state, renderer, layout.bounds(), *viewport);
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &R,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = metrics::content_bounds(layout.bounds());
        self.on_event::<R>(state, event, bounds, cursor, clipboard, shell);
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &R,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        let bounds = metrics::content_bounds(layout.bounds());
        self.interaction::<R>(state, bounds, cursor)
    }
}

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

#[cfg(test)]
mod test_support {
    use super::*;
    use crate::editor::highlight::StyledSpan;

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
}

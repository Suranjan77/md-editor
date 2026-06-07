use super::*;
use crate::editor::buffer::{DocBuffer, EditorCommand, Movement};
use crate::editor::highlight::{StyledLine, StyledSpan};
use crate::editor::layout_cache::{LineHeightCache, line_hash, resource_hash};
use crate::editor::renderer::geometry::{clip_viewport, normalized_selection};
use crate::{search, theme};
use iced::advanced::graphics::core::event::Event;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::keyboard;
use iced::mouse;
use iced::{Color, Element, Length, Point, Rectangle, Size};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
pub struct CharCacheKey {
    pub ch: char,
    pub font: iced::Font,
    pub size_bits: u32,
}

#[derive(Default)]
pub struct State {
    pub(crate) is_dragging: bool,
    pub(crate) is_focused: bool,
    pub(crate) modifiers: keyboard::Modifiers,
    pub(crate) selection_anchor: Option<(usize, usize)>,
    pub(crate) selection_focus: Option<(usize, usize)>,
    pub(crate) block_scroll_x: HashMap<usize, f32>,
    pub(crate) horizontal_scroll_drag: Option<HorizontalScrollDrag>,
    pub(crate) desired_visual_x: Option<f32>,
    pub(crate) layout_tree: crate::editor::layout_tree::HeightTree,
    pub(crate) line_height_cache: Vec<LineHeightCache>,
    pub(crate) last_layout_width: f32,
    pub(crate) block_ranges: HashMap<usize, (usize, usize)>,
}

#[derive(Debug, Clone, Copy)]
pub struct HorizontalScrollDrag {
    pub(crate) block_id: usize,
    pub(crate) viewport_x: f32,
    pub(crate) viewport_w: f32,
    pub(crate) content_w: f32,
    pub(crate) grab_offset: f32,
}

pub(crate) const MARGIN_LEFT: f32 = 64.0;
pub(crate) const MARGIN_RIGHT: f32 = 56.0;
pub(crate) const TEXT_X_OFFSET: f32 = MARGIN_LEFT;
pub(crate) const TOP_PAD: f32 = 24.0;
pub(crate) const BASE_LINE_HEIGHT: f32 = 36.0;
pub(crate) const IMAGE_HEIGHT: f32 = 280.0;
pub(crate) const HORIZONTAL_SCROLLBAR_GUTTER: f32 = 16.0;
pub(crate) const HOT_PATH_BLOCK_SCAN_LIMIT: usize = 256;

pub(crate) mod draw;
pub(crate) mod geometry;
pub(crate) mod hit_test;
pub(crate) mod measure;
pub(crate) mod movement;
pub(crate) mod scrollbar;
pub(crate) mod state;
pub(crate) mod widget;

pub(crate) use state::{CharCacheKey, HorizontalScrollDrag, State};
pub(crate) use widget::Editor;

pub(crate) use draw::*;
pub(crate) use hit_test::*;
pub(crate) use measure::*;
pub(crate) use scrollbar::*;

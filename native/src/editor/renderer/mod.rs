pub(crate) const MARGIN_LEFT: f32 = 64.0;
pub(crate) const MARGIN_RIGHT: f32 = 56.0;
pub(crate) const TEXT_X_OFFSET: f32 = MARGIN_LEFT;
pub(crate) const TOP_PAD: f32 = 24.0;
pub(crate) const BASE_LINE_HEIGHT: f32 = 36.0;
pub(crate) const IMAGE_HEIGHT: f32 = 280.0;
pub(crate) const HORIZONTAL_SCROLLBAR_GUTTER: f32 = 16.0;
pub(crate) const HOT_PATH_BLOCK_SCAN_LIMIT: usize = 256;

pub mod draw;
pub mod geometry;
pub mod hit_test;
pub mod measure;
pub mod movement;
pub mod scrollbar;
pub mod state;
pub mod widget;

pub use state::{CharCacheKey, HorizontalScrollDrag, State};
pub use widget::Editor;

pub use draw::*;
pub use hit_test::*;
pub use measure::*;
pub use scrollbar::*;

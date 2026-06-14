use super::{CHAR_WIDTH, MAX_READING_WIDTH, MIN_PAGE_MARGIN};

pub(crate) fn content_width(viewport_width: f32) -> f32 {
    (viewport_width - MIN_PAGE_MARGIN * 2.0).clamp(CHAR_WIDTH, MAX_READING_WIDTH)
}

pub(crate) fn content_left(viewport_width: f32) -> f32 {
    ((viewport_width - content_width(viewport_width)) / 2.0).max(0.0)
}

pub(crate) fn wrap_columns(wrap_width: f32) -> usize {
    (wrap_width / CHAR_WIDTH).floor().max(1.0) as usize
}

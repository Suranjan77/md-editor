//! Layout constants and geometry.
//!
//! A value lives here when more than one pass — layout, painting,
//! hit-testing — has to agree on it; getting one of these out of sync is what
//! puts the caret somewhere other than the text. Purely cosmetic values stay
//! next to the painter that uses them.

use iced::Rectangle;

// ── Page ─────────────────────────────────────────────────────────────

pub(super) const MARGIN_LEFT: f32 = 64.0;
pub(super) const MARGIN_RIGHT: f32 = 56.0;
/// X of the text column, relative to the content bounds.
pub(super) const TEXT_X_OFFSET: f32 = MARGIN_LEFT;
/// Space above the first line.
pub(super) const TOP_PAD: f32 = 24.0;
/// Space below the last line.
pub(super) const BOTTOM_PAD: f32 = 80.0;

/// Maximum width of the editor's content column. On wider viewports the
/// content is centered with automatic side margins, like a web reader,
/// instead of stretching edge to edge.
pub(super) const MAX_CONTENT_WIDTH: f32 = 880.0;

// ── Text rows ────────────────────────────────────────────────────────

/// Height of one row of body text, and the floor for every row.
pub(super) const BASE_LINE_HEIGHT: f32 = 36.0;
/// Rows grow with their font size by this factor.
const LINE_STEP_FACTOR: f32 = 1.45;
/// Height of a line of text relative to its font size (iced's default).
pub(super) const LINE_BOX_FACTOR: f32 = 1.3;
/// Distance from the top of a line of text to its baseline, relative to its
/// font size. Common UI fonts put the baseline within a few percent of this.
pub(super) const BASELINE_FACTOR: f32 = 1.0;
/// Font size assumed when a line gives no better answer.
pub(super) const DEFAULT_FONT_SIZE: f32 = 17.0;
/// Narrowest width text is allowed to wrap at.
pub(super) const MIN_TEXT_WIDTH: f32 = 80.0;

// ── Blocks ───────────────────────────────────────────────────────────

/// Band above a code block or table that holds its caption.
pub(super) const BLOCK_CAPTION_HEIGHT: f32 = 24.0;
/// Space reserved under the last row of a table for its scrollbar.
pub(super) const HORIZONTAL_SCROLLBAR_GUTTER: f32 = 16.0;
/// Distance from a code or math block's bottom edge to its scrollbar.
pub(super) const SCROLLBAR_BOTTOM_OFFSET: f32 = 7.0;

pub(super) const CODE_LINE_HEIGHT: f32 = 34.0;
pub(super) const CODE_FONT_SIZE: f32 = 15.0;
/// Added to the widest code line to get the block's scrollable width.
pub(super) const CODE_CONTENT_PADDING: f32 = 28.0;
/// How much narrower a code block's viewport is than the text column.
pub(super) const CODE_VIEWPORT_INSET: f32 = 24.0;

pub(super) const TABLE_ROW_HEIGHT: f32 = 34.0;
/// Added to a column's widest cell.
pub(super) const TABLE_CELL_PADDING: f32 = 20.0;
/// Added to the sum of column widths: the card bleeds 6px past each side.
pub(super) const TABLE_EDGE_PADDING: f32 = 12.0;

// ── Math ─────────────────────────────────────────────────────────────

/// Size of TeX source shown for block math that has not rendered.
pub(super) const MATH_SOURCE_FONT_SIZE: f32 = 16.0;
pub(super) const MATH_BLOCK_MIN_HEIGHT: f32 = 72.0;
/// Vertical padding around block math, and how much narrower its viewport is
/// than the text column.
pub(super) const MATH_BLOCK_PADDING: f32 = 48.0;
/// Characters per row assumed when estimating unrendered block math height.
pub(super) const MATH_SOURCE_CHARS_PER_ROW: f32 = 72.0;
/// Extra height given to a row holding a rendered inline equation.
pub(super) const INLINE_MATH_ROW_PADDING: f32 = 10.0;
/// Height above the baseline that inline equations and checkboxes are centred
/// on, roughly where a minus sign sits.
pub(super) const MATH_AXIS_HEIGHT: f32 = 5.0;
/// Gap after an inline equation.
pub(super) const INLINE_MATH_GAP: f32 = 4.0;

// ── Inline widgets ───────────────────────────────────────────────────

/// Horizontal advance of a rendered checkbox: the box plus its gap.
pub(super) const CHECKBOX_ADVANCE: f32 = 26.0;
pub(super) const CHECKBOX_SIZE: f32 = 18.0;

/// Height reserved for an image that has not loaded yet.
pub(super) const IMAGE_PLACEHOLDER_HEIGHT: f32 = 280.0;
/// Space under a loaded image for its caption.
pub(super) const IMAGE_CAPTION_SPACE: f32 = 40.0;

// ── Geometry ─────────────────────────────────────────────────────────

/// Returns the centered content rectangle for a raw widget bounds. When the
/// widget is wider than [`MAX_CONTENT_WIDTH`] the content column is capped and
/// horizontally centered, leaving equal auto margins on each side. All
/// horizontal layout/hit-testing derives from this rectangle, so callers can
/// treat it as the editor's effective bounds.
pub(super) fn content_bounds(raw: Rectangle) -> Rectangle {
    let width = raw.width.min(MAX_CONTENT_WIDTH);
    let inset = ((raw.width - width) / 2.0).max(0.0);
    Rectangle {
        x: raw.x + inset,
        width,
        ..raw
    }
}

/// Height of a visual row of text at `font_size`.
pub(super) fn visual_line_step(font_size: f32) -> f32 {
    (font_size * LINE_STEP_FACTOR).max(BASE_LINE_HEIGHT)
}

/// Width of the text column for a content width.
pub(super) fn text_column_width(content_width: f32) -> f32 {
    content_width - TEXT_X_OFFSET - MARGIN_RIGHT
}

/// Width text wraps at for a content width.
pub(super) fn wrap_width(content_width: f32) -> f32 {
    text_column_width(content_width).max(MIN_TEXT_WIDTH)
}

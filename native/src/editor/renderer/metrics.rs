//! Layout constants and geometry.
//!
//! A value lives here when more than one pass — layout, painting,
//! hit-testing — has to agree on it; getting one of these out of sync is what
//! puts the caret somewhere other than the text. Purely cosmetic values stay
//! next to the painter that uses them.

use iced::Rectangle;

// ── Page ─────────────────────────────────────────────────────────────

/// Page margins at full width. Narrower content shrinks them in proportion
/// (see [`page_scale`]).
const MARGIN_LEFT: f32 = 64.0;
const MARGIN_RIGHT: f32 = 56.0;
/// Content widths at or above this get full margins.
const FULL_MARGIN_WIDTH: f32 = 640.0;
/// The smallest fraction of the full margins a page is given.
const MIN_PAGE_SCALE: f32 = 0.125;
/// Space above the first line.
pub(super) const TOP_PAD: f32 = 24.0;
/// Space below the last line.
pub(super) const BOTTOM_PAD: f32 = 80.0;

/// Maximum width of the editor's content column. On wider viewports the
/// content is centered with automatic side margins, like a web reader,
/// instead of stretching edge to edge.
pub(super) const MAX_CONTENT_WIDTH: f32 = 880.0;

// ── Vertical rhythm ──────────────────────────────────────────────────

/// Every vertical measure — row heights, margins, gaps — is a multiple of
/// this, so baselines of text in different blocks share one grid.
pub(super) const GRID: f32 = 4.0;
/// Body text size, and the size assumed when a line gives no better answer.
pub(super) const DEFAULT_FONT_SIZE: f32 = 17.0;
/// Height of iced's text line box relative to font size; glyphs are centred
/// in it. Painters position text through this.
pub(super) const LINE_BOX_FACTOR: f32 = 1.3;
/// Line height relative to font size for body text, easing down to
/// [`DISPLAY_LINE_HEIGHT`] by [`DISPLAY_SIZE`]: large type needs less leading.
const BODY_LINE_HEIGHT: f32 = 1.6;
const DISPLAY_LINE_HEIGHT: f32 = 1.2;
const DISPLAY_SIZE: f32 = 34.0;

/// The nearest multiple of [`GRID`], at least one.
pub(super) fn snap(value: f32) -> f32 {
    ((value / GRID).round() * GRID).max(GRID)
}

/// The smallest multiple of [`GRID`] at or above `value`.
pub(super) fn snap_up(value: f32) -> f32 {
    ((value / GRID - 1e-4).ceil() * GRID).max(GRID)
}

/// Height of a row of text at `font_size`, on the grid.
pub(super) fn row_height(font_size: f32) -> f32 {
    let t = ((font_size - DEFAULT_FONT_SIZE) / (DISPLAY_SIZE - DEFAULT_FONT_SIZE)).clamp(0.0, 1.0);
    let factor = BODY_LINE_HEIGHT + (DISPLAY_LINE_HEIGHT - BODY_LINE_HEIGHT) * t;
    snap(font_size * factor)
}

/// A row of body text: 28px.
pub(super) fn body_row() -> f32 {
    row_height(DEFAULT_FONT_SIZE)
}

/// A blank line: the gap between paragraphs. Tall enough to hold the caret.
pub(super) const PARAGRAPH_GAP: f32 = 20.0;

/// Space required above and below a heading of each level (1–6).
pub(super) const HEADING_MARGINS: [(f32, f32); 6] = [
    (48.0, 16.0),
    (40.0, 12.0),
    (32.0, 8.0),
    (24.0, 8.0),
    (24.0, 8.0),
    (24.0, 8.0),
];
/// Space required above and below code blocks, tables, block math, images,
/// rules and runs of quoted lines.
pub(super) const BLOCK_MARGIN: f32 = 16.0;

// ── Blocks ───────────────────────────────────────────────────────────

/// Band above a code block or table that holds its caption.
pub(super) const BLOCK_CAPTION_HEIGHT: f32 = 24.0;
/// Space reserved under the last row of a table for its scrollbar.
pub(super) const HORIZONTAL_SCROLLBAR_GUTTER: f32 = 16.0;
/// Distance from a code or math block's bottom edge to its scrollbar.
pub(super) const SCROLLBAR_BOTTOM_OFFSET: f32 = 7.0;

pub(super) const CODE_FONT_SIZE: f32 = 15.0;
/// Space kept between the caret and the edge of a code block's viewport when
/// the block scrolls to follow it.
pub(super) const CODE_CARET_MARGIN: f32 = 24.0;
/// Added to the widest code line to get the block's scrollable width.
pub(super) const CODE_CONTENT_PADDING: f32 = 28.0;
/// How much narrower a code block's viewport is than the text column, at
/// full width.
const CODE_VIEWPORT_INSET: f32 = 24.0;

/// A table row: a body row plus padding.
pub(super) const TABLE_ROW_HEIGHT: f32 = 36.0;
/// Added to a column's widest cell.
pub(super) const TABLE_CELL_PADDING: f32 = 20.0;
/// Added to the sum of column widths: the card bleeds 6px past each side.
pub(super) const TABLE_EDGE_PADDING: f32 = 12.0;

// ── Math ─────────────────────────────────────────────────────────────

/// Size of TeX source shown for block math that has not rendered.
pub(super) const MATH_SOURCE_FONT_SIZE: f32 = 16.0;
pub(super) const MATH_BLOCK_MIN_HEIGHT: f32 = 72.0;
/// Vertical padding around block math, and — at full width — how much
/// narrower its viewport is than the text column.
pub(super) const MATH_BLOCK_PADDING: f32 = 48.0;
/// Characters per row assumed when estimating unrendered block math height.
pub(super) const MATH_SOURCE_CHARS_PER_ROW: f32 = 72.0;
/// Extra height given to a row holding a rendered inline equation.
pub(super) const INLINE_MATH_ROW_PADDING: f32 = 8.0;
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

/// Fraction of the full page margins a content width gets: 1 from
/// [`FULL_MARGIN_WIDTH`] up, shrinking linearly below it, so a narrow window
/// keeps a usable text column instead of margins that overflow it.
pub(super) fn page_scale(content_width: f32) -> f32 {
    if content_width.is_finite() {
        (content_width / FULL_MARGIN_WIDTH).clamp(MIN_PAGE_SCALE, 1.0)
    } else {
        1.0
    }
}

/// X of the text column, relative to the content bounds.
pub(super) fn text_left(content_width: f32) -> f32 {
    MARGIN_LEFT * page_scale(content_width)
}

/// Width of the text column for a content width.
pub(super) fn text_column_width(content_width: f32) -> f32 {
    let scale = page_scale(content_width);
    (content_width - (MARGIN_LEFT + MARGIN_RIGHT) * scale).max(0.0)
}

/// Width text wraps at for a content width.
pub(super) fn wrap_width(content_width: f32) -> f32 {
    text_column_width(content_width).max(1.0)
}

/// Width of a code block's scrollable viewport.
pub(super) fn code_viewport_width(content_width: f32) -> f32 {
    let inset = CODE_VIEWPORT_INSET * page_scale(content_width);
    (text_column_width(content_width) - inset).max(1.0)
}

/// Width of a block equation's scrollable viewport.
pub(super) fn math_viewport_width(content_width: f32) -> f32 {
    let inset = MATH_BLOCK_PADDING * page_scale(content_width);
    (text_column_width(content_width) - inset).max(1.0)
}

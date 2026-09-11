//! Numbered captions: "Table 1", "Listing 2", "Figure 3: …", "(4)".
//!
//! Numbers come from the ids the highlighter assigns (`table-1`, `code-2`,
//! `figure-3`, `equation-4`), so they follow document order even when only
//! part of the document is painted.

use iced::alignment::{Horizontal, Vertical};
use iced::{Point, Rectangle, Size};

use super::super::Measure;
use super::super::measure::{bold_font, measure_width};
use super::super::metrics::BLOCK_CAPTION_HEIGHT;
use super::primitives::{TextRun, fill, rounded};
use crate::editor::highlight::{StyledLine, StyledSpan};
use crate::theme;

const CAPTION_SIZE: f32 = 11.0;
const EQUATION_NUMBER_SIZE: f32 = 14.0;
const BADGE_HEIGHT: f32 = 18.0;
/// Horizontal padding inside a badge, both sides together.
const BADGE_PADDING: f32 = 12.0;
/// Distance of a badge from its block's right edge.
const BADGE_MARGIN_RIGHT: f32 = 12.0;
const BADGE_MARGIN_TOP: f32 = 8.0;

/// Number from the id on the first span of `block_id`'s first line, when that
/// id is `<prefix><n>`.
pub(super) fn block_ordinal(lines: &[StyledLine], block_id: usize, prefix: &str) -> Option<usize> {
    let first_line = lines.iter().find(|l| l.block_id == block_id)?;
    let id = first_line.spans.first()?.id.as_deref()?;
    id.strip_prefix(prefix)?.parse().ok()
}

/// Figure number of an image span.
pub(super) fn figure_number(span: &StyledSpan) -> Option<usize> {
    span.id.as_deref()?.strip_prefix("figure-")?.parse().ok()
}

/// Centered bold caption in the band above a block spanning `x..x + width`.
pub(super) fn paint_block_caption<R: Measure>(
    renderer: &mut R,
    label: String,
    x: f32,
    width: f32,
    top: f32,
    viewport: Rectangle,
) {
    TextRun::new(label, CAPTION_SIZE, Size::new(width, BLOCK_CAPTION_HEIGHT))
        .font(bold_font())
        .align(Horizontal::Center, Vertical::Center)
        .draw(
            renderer,
            Point::new(x + width / 2.0, top + BLOCK_CAPTION_HEIGHT / 2.0),
            theme::TEXT_MUTED,
            viewport,
        );
}

/// Bounds of a language badge in the top-right corner of a block.
pub(super) fn badge_rect(block_x: f32, block_w: f32, block_top: f32, text_w: f32) -> Rectangle {
    let badge_w = text_w + BADGE_PADDING;
    Rectangle {
        x: block_x + block_w - badge_w - BADGE_MARGIN_RIGHT,
        y: block_top + BADGE_MARGIN_TOP,
        width: badge_w,
        height: BADGE_HEIGHT,
    }
}

/// A code block's language badge.
pub(super) fn paint_language_badge<R: Measure>(
    renderer: &mut R,
    lang: &str,
    block_x: f32,
    block_w: f32,
    block_top: f32,
    viewport: Rectangle,
) {
    let badge_text = lang.to_uppercase();
    if badge_text.is_empty() {
        return;
    }
    let text_w = measure_width::<R>(&badge_text, CAPTION_SIZE, bold_font());
    let rect = badge_rect(block_x, block_w, block_top, text_w);
    fill(renderer, rect, rounded(4.0), theme::BG_TERTIARY);
    TextRun::new(badge_text, CAPTION_SIZE, rect.size())
        .font(bold_font())
        .align(Horizontal::Center, Vertical::Center)
        .draw(
            renderer,
            Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0),
            theme::ACCENT,
            viewport,
        );
}

/// "(n)" right-aligned to `right`, vertically centered on `center_y`.
pub(super) fn paint_equation_number<R: Measure>(
    renderer: &mut R,
    number: usize,
    right: f32,
    center_y: f32,
    height: f32,
    viewport: Rectangle,
) {
    let label = format!("({})", number);
    let width = measure_width::<R>(&label, EQUATION_NUMBER_SIZE, iced::Font::DEFAULT);
    TextRun::new(label, EQUATION_NUMBER_SIZE, Size::new(width, height))
        .align(Horizontal::Left, Vertical::Center)
        .draw(
            renderer,
            Point::new(right - width, center_y),
            theme::TEXT_MUTED,
            viewport,
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::highlight::highlight_markdown;

    #[test]
    fn badge_sits_inside_the_top_right_of_its_block() {
        let lines = highlight_markdown("```rust\nfn main() {}\n```");
        assert!(lines[0].is_code_block);
        assert_eq!(lines[0].code_block_lang.as_deref(), Some("rust"));

        let (block_x, block_w, top) = (10.0, 500.0, 100.0);
        let rect = badge_rect(block_x, block_w, top, 32.0);

        assert_eq!(rect.width, 32.0 + BADGE_PADDING);
        assert_eq!(rect.height, BADGE_HEIGHT);
        assert!(rect.x > block_x);
        assert!(rect.x + rect.width < block_x + block_w);
        assert!(rect.y > top);
    }

    #[test]
    fn test_get_equation_and_image_number() {
        let md = "Here is an image:\n![Alt](image.png)\nAnd a math block:\n$$\nE = mc^2\n$$\nAnother image:\n![Alt2](pic.png)";
        let lines = highlight_markdown(md);

        let math_block_id = lines[3].block_id;
        assert_eq!(block_ordinal(&lines, math_block_id, "equation-"), Some(1));

        let img1_span = &lines[1].spans[0];
        let img2_span = &lines[7].spans[0];
        assert_eq!(figure_number(img1_span), Some(1));
        assert_eq!(figure_number(img2_span), Some(2));
    }

    #[test]
    fn test_get_table_and_code_number() {
        let md = "Here is a code block:\n```rust\nfn main() {}\n```\nAnd a table:\n| a | b |\n|---|---|\n| 1 | 2 |\nAnother code:\n```\nhello\n```";
        let lines = highlight_markdown(md);

        let first_code_block_id = lines[1].block_id;
        let table_block_id = lines[5].block_id;
        let second_code_block_id = lines[10].block_id;

        assert_eq!(block_ordinal(&lines, first_code_block_id, "code-"), Some(1));
        assert_eq!(block_ordinal(&lines, table_block_id, "table-"), Some(1));
        assert_eq!(
            block_ordinal(&lines, second_code_block_id, "code-"),
            Some(2)
        );
    }
}

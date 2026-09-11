//! Behavioral tests of the editor widget, driven through its `Widget` API.
//!
//! Each test paints or feeds events to a real [`View`] and checks what came
//! out — draw calls and published messages — rather than internal state.

use iced::keyboard;
use iced::{Color, Point, Rectangle, mouse};

use super::measure::measure_width;
use super::testing::{Call, View};
use crate::editor::buffer::EditorCommand;
use crate::theme;

/// iced's default line height, relative to font size.
const LINE_BOX: f32 = 1.3;

struct Run {
    content: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Run {
    fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

fn runs(calls: &[Call]) -> Vec<Run> {
    calls
        .iter()
        .filter_map(|call| match call {
            Call::Text {
                content,
                size,
                font,
                at,
                ..
            } if !content.trim().is_empty() => Some(Run {
                content: content.clone(),
                x: at.x,
                y: at.y,
                width: measure_width::<iced::Renderer>(content, *size, *font),
                height: size * LINE_BOX,
            }),
            _ => None,
        })
        .collect()
}

fn quads(calls: &[Call], color: Color) -> Vec<Rectangle> {
    calls
        .iter()
        .filter_map(|call| match call {
            Call::Quad {
                bounds, color: c, ..
            } if *c == color => Some(*bounds),
            _ => None,
        })
        .collect()
}

fn caret(calls: &[Call]) -> Option<Rectangle> {
    let carets: Vec<_> = quads(calls, theme::ACCENT)
        .into_iter()
        .filter(|q| q.width == 2.0)
        .collect();
    assert!(carets.len() <= 1, "more than one caret: {carets:?}");
    carets.first().copied()
}

fn run_containing<'a>(runs: &'a [Run], needle: &str) -> &'a Run {
    runs.iter()
        .find(|r| r.content.contains(needle))
        .unwrap_or_else(|| panic!("no painted run contains {needle:?}"))
}

fn run_exact<'a>(runs: &'a [Run], content: &str) -> &'a Run {
    runs.iter()
        .find(|r| r.content.trim() == content)
        .unwrap_or_else(|| panic!("no painted run is {content:?}"))
}

fn contains(rect: Rectangle, p: Point) -> bool {
    p.x >= rect.x && p.x <= rect.x + rect.width && p.y >= rect.y && p.y <= rect.y + rect.height
}

/// Column published by the first `SetCursor` message.
fn published_col(messages: &[String]) -> usize {
    let msg = messages
        .iter()
        .find(|m| m.contains("SetCursor"))
        .unwrap_or_else(|| panic!("no SetCursor in {messages:?}"));
    let col = msg.split("col: ").nth(1).expect("col field");
    col.trim_end_matches([' ', '}'])
        .parse()
        .expect("col number")
}

const TRACK: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.06);
const SELECTION: Color = Color::from_rgba(0.69, 0.80, 0.78, 0.24);

const WIDTHS: [f32; 5] = [260.0, 300.0, 390.0, 480.0, 700.0];

const INLINE_MATH_LINE: &str =
    "Inline $a^2$ math sits on the baseline, and more words follow it across rows $a^2$ again.";

const WRAPPING_LINES: &[&str] = &[
    "A plain paragraph that is long enough to wrap across several rows of the narrow editor column without any styling at all.",
    "Styled **bold words** and *italic ones* with `code` and [a link](http://x.y) that also wraps across several rows here.",
    "- [ ] a checkbox item that has enough trailing words to wrap onto a second row",
    "# A heading that is long enough to wrap in a narrow column",
];

#[test]
fn caret_sits_on_the_painted_text_of_wrapped_lines() {
    for doc in WRAPPING_LINES {
        for width in WIDTHS {
            let mut view = View::new(doc, width);
            view.focus();
            let len = doc.chars().count();
            for col in 0..=len {
                view.set_cursor(0, col);
                let calls = view.draw();
                let caret = caret(&calls).expect("caret painted");
                let center = Point::new(caret.x, caret.y + caret.height / 2.0);
                let runs = runs(&calls);
                assert!(
                    runs.iter().any(|r| {
                        center.x >= r.x - 0.5
                            && center.x <= r.x + r.width + 0.5
                            && center.y >= r.y
                            && center.y <= r.y + r.height
                    }),
                    "caret at col {col} ({center:?}) is off the painted text, width {width}, doc {doc:?}"
                );
            }
        }
    }
}

#[test]
fn clicking_where_the_caret_is_drawn_puts_it_back_there() {
    let docs = WRAPPING_LINES.iter().copied().chain([INLINE_MATH_LINE]);
    for doc in docs {
        for width in WIDTHS {
            let mut view = View::new(doc, width);
            view.add_math("a^2", 26.0, 30.0);
            view.focus();
            let len = doc.chars().count();
            for col in 0..=len {
                view.set_cursor(0, col);
                let before = caret(&view.draw()).expect("caret painted");
                let click = Point::new(before.x + 0.25, before.y + before.height / 2.0);
                let hit = published_col(&view.click(click).messages);

                view.set_cursor(0, hit);
                let after = caret(&view.draw()).expect("caret painted");
                assert!(
                    (after.x - before.x).abs() < 0.5 && (after.y - before.y).abs() < 0.5,
                    "click at col {col}'s caret {before:?} moved it to col {hit} at {after:?}, width {width}, doc {doc:?}"
                );
            }
        }
    }
}

#[test]
fn links_on_wrapped_rows_are_clickable() {
    let doc = "Enough leading words here to push the following link onto a later row: [the target](dest.md) done";
    let mut view = View::new(doc, 300.0);
    let calls = view.draw();
    let runs = runs(&calls);
    let first_row_y = runs.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
    let link = run_containing(&runs, "target");
    assert!(link.y > first_row_y, "test needs the link on a later row");

    view.modifiers(keyboard::Modifiers::CTRL);
    assert_eq!(view.interaction(link.center()), mouse::Interaction::Pointer);
    let messages = view.click(link.center()).messages;
    assert!(
        messages.iter().any(|m| m == "link dest.md"),
        "ctrl+click on the link published {messages:?}"
    );
}

#[test]
fn table_header_row_is_filled_and_body_rows_striped() {
    let doc = "| Name | Value |\n|---|---|\n| a | 1 |\n| b | 2 |\n| c | 3 |";
    let mut view = View::new(doc, 700.0);
    let calls = view.draw();
    let runs = runs(&calls);

    let header = run_containing(&runs, "Name");
    assert!(
        quads(&calls, theme::BG_TERTIARY)
            .iter()
            .any(|q| contains(*q, header.center())),
        "header row has no header fill"
    );

    let stripes = quads(&calls, Color::from_rgba(1.0, 1.0, 1.0, 0.025));
    assert_eq!(stripes.len(), 1, "one striped body row expected");
    assert!(contains(stripes[0], run_exact(&runs, "b").center()));
}

#[test]
fn selection_covers_every_row_of_a_wrapped_line() {
    let doc = WRAPPING_LINES[0];
    let mut view = View::new(doc, 300.0);
    view.focus();
    view.buffer.execute(EditorCommand::SetSelection {
        anchor_line: 0,
        anchor_col: 0,
        focus_line: 0,
        focus_col: doc.chars().count(),
    });
    let calls = view.draw();

    let mut rows: Vec<f32> = runs(&calls).iter().map(|r| r.y).collect();
    rows.dedup_by(|a, b| (*a - *b).abs() < 1.0);
    let highlights = quads(&calls, SELECTION);
    assert!(rows.len() > 1, "test needs a wrapped line");
    assert_eq!(highlights.len(), rows.len(), "one highlight per row");
    for run in runs(&calls) {
        assert!(
            highlights.iter().any(|h| contains(*h, run.center())),
            "{:?} is not highlighted",
            run.content
        );
    }
}

fn shift_wheel(view: &mut View, at: Point, lines: f32) {
    view.modifiers(keyboard::Modifiers::SHIFT);
    view.wheel(at, mouse::ScrollDelta::Lines { x: 0.0, y: -lines });
    view.modifiers(keyboard::Modifiers::empty());
}

fn only_image(calls: &[Call]) -> Rectangle {
    let images: Vec<_> = calls
        .iter()
        .filter_map(|c| match c {
            Call::Image { bounds, .. } => Some(*bounds),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 1);
    images[0]
}

#[test]
fn block_math_wheel_scrolling_has_no_dead_zone() {
    let mut view = View::new("$$\n\\wide\n$$", 700.0);
    view.add_math("\\wide", 900.0, 40.0);
    let image = only_image(&view.draw());
    let at = Point::new(300.0, image.y + image.height / 2.0);

    for _ in 0..40 {
        shift_wheel(&mut view, at, 1.0);
    }
    let right = only_image(&view.draw());
    shift_wheel(&mut view, at, -1.0);
    let back = only_image(&view.draw());

    assert!(
        (back.x - right.x - 48.0).abs() < 1.0,
        "one wheel step back moved the equation by {}",
        back.x - right.x
    );
}

#[test]
fn dragging_a_scrollbar_thumb_moves_it_with_the_pointer() {
    let docs = [
        ("$$\n\\wide\n$$", true),
        (
            "```\nlet value = \"a very long string literal that definitely overflows the code viewport horizontally, twice over\";\n```",
            false,
        ),
    ];
    for (doc, math) in docs {
        let mut view = View::new(doc, 700.0);
        if math {
            view.add_math("\\wide", 900.0, 40.0);
        }
        let calls = view.draw();
        let thumbs = quads(&calls, theme::ACCENT_DIM);
        assert_eq!(thumbs.len(), 1, "one thumb expected in {doc:?}");
        let thumb = thumbs[0];

        let grab = Point::new(thumb.x + thumb.width / 2.0, thumb.y + thumb.height / 2.0);
        assert!(view.press(grab).captured, "press on thumb not captured");
        view.move_to(Point::new(grab.x + 30.0, grab.y));
        view.release(Point::new(grab.x + 30.0, grab.y));

        let moved = quads(&view.draw(), theme::ACCENT_DIM)[0];
        assert!(
            (moved.x - thumb.x - 30.0).abs() < 0.5,
            "thumb moved {} for a 30px drag in {doc:?}",
            moved.x - thumb.x
        );
    }
}

#[test]
fn code_block_scrollbar_is_painted_once() {
    let long = "x".repeat(200);
    let doc = format!("```\n{long}\nb\nc\nd\n```");
    let mut view = View::new(&doc, 600.0);
    assert_eq!(quads(&view.draw(), TRACK).len(), 1);
}

#[test]
fn narrow_widths_do_not_panic() {
    let columns = (0..12).map(|i| format!(" column{i} |")).collect::<String>();
    let separator = "---|".repeat(12);
    let doc = format!(
        "|{columns}\n|{separator}\n|{columns}\n```\n{}\n```\n$$\n\\wide\n$$",
        "y".repeat(120)
    );
    for width in 40..=260 {
        let mut view = View::new(&doc, width as f32);
        view.add_math("\\wide", 900.0, 40.0);
        let calls = view.draw();
        for track in quads(&calls, TRACK) {
            let p = Point::new(track.x + 1.0, track.y + 1.0);
            shift_wheel(&mut view, p, 1.0);
            view.press(p);
            view.move_to(Point::new(p.x + 10.0, p.y));
            view.release(p);
        }
        view.draw();
    }
}

#[test]
fn clicks_in_a_scrolled_code_block_hit_the_character_under_the_pointer() {
    let code = "let value = \"a very long string literal that definitely overflows the code viewport horizontally\";";
    let doc = format!("```\n{code}\n```");
    let mut view = View::new(&doc, 500.0);
    let runs_before = runs(&view.draw());
    let at = Point::new(200.0, run_containing(&runs_before, "overflows").center().y);
    shift_wheel(&mut view, at, 5.0);

    let runs = runs(&view.draw());
    let run = run_containing(&runs, "overflows");
    let offset = run.content.find("overflows").unwrap();
    let prefix = &run.content[..offset];
    let size = super::metrics::CODE_FONT_SIZE;
    let x = run.x + measure_width::<iced::Renderer>(prefix, size, iced::Font::MONOSPACE) + 1.0;

    let col = published_col(&view.click(Point::new(x, run.center().y)).messages);
    assert_eq!(col, code.find("overflows").unwrap());
}

#[test]
fn tables_with_narrow_columns_scroll_to_their_last_column() {
    // Single letters are narrower than the minimum column width.
    let cells = |first: u8| {
        (0..24u8)
            .map(|i| format!(" {} |", (first + i) as char))
            .collect::<String>()
    };
    let doc = format!("|{}\n|{}\n|{}", cells(b'A'), "---|".repeat(24), cells(b'a'));
    let mut view = View::new(&doc, 700.0);
    let row = run_exact(&runs(&view.draw()), "a").center();
    for _ in 0..60 {
        shift_wheel(&mut view, row, 1.0);
    }

    let calls = view.draw();
    let runs = runs(&calls);
    let last = run_exact(&runs, "x");
    let card = quads(&calls, theme::BG_SECONDARY)
        .into_iter()
        .find(|q| contains(*q, last.center()))
        .expect("table card");
    assert!(
        last.x + last.width <= card.x + card.width,
        "last column ends at {} beyond the card edge {}",
        last.x + last.width,
        card.x + card.width
    );
}

#[test]
fn plain_clicks_on_links_do_not_follow_them() {
    let mut view = View::new("[a link](dest.md)", 700.0);
    let run = runs(&view.draw()).remove(0);
    let messages = view.click(run.center()).messages;
    assert!(!messages.iter().any(|m| m.starts_with("link")));
}

#[test]
fn rows_of_one_line_never_overlap_and_stay_inside_it() {
    let doc = "# Heading with enough words to wrap in a column\nStyled **bold** and `code` with $a^2$ inline and many more words to wrap\nafter";
    for width in WIDTHS {
        let mut view = View::new(doc, width);
        view.add_math("a^2", 26.0, 30.0);
        let runs = runs(&view.draw());
        let next_line_top = run_exact(&runs, "after").y;
        let mut boxes: Vec<(f32, f32)> = runs
            .iter()
            .filter(|r| r.content.trim() != "after")
            .map(|r| (r.y, r.y + r.height))
            .collect();
        boxes.sort_by(|a, b| a.0.total_cmp(&b.0));
        for pair in boxes.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(
                b.0 >= a.1 - 0.5 || (b.0 - a.0).abs() < 12.0,
                "rows overlap at width {width}: {a:?} {b:?}"
            );
        }
        let bottom = boxes.iter().map(|b| b.1).fold(0.0, f32::max);
        assert!(
            bottom <= next_line_top,
            "text spills into the next line at width {width}"
        );
    }
}

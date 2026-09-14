//! Behavioral tests of the editor widget, driven through its `Widget` API.
//!
//! Each test paints or feeds events to a real [`View`] and checks what came
//! out — draw calls and published messages — rather than internal state.

use iced::keyboard;
use iced::{Color, Point, Rectangle, mouse};

use super::flow::Affinity;
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

/// Column and affinity published by the first `SetCursor` message.
fn published_caret(messages: &[String]) -> (usize, Affinity) {
    let msg = messages
        .iter()
        .find(|m| m.contains("SetCursor"))
        .unwrap_or_else(|| panic!("no SetCursor in {messages:?}"));
    let col = msg.split("col: ").nth(1).expect("col field");
    let col = col
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("col number");
    let affinity = if msg.contains("Upstream") {
        Affinity::Upstream
    } else {
        Affinity::Downstream
    };
    (col, affinity)
}

fn published_col(messages: &[String]) -> usize {
    published_caret(messages).0
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
                let (hit, affinity) = published_caret(&view.click(click).messages);

                view.place_caret(0, hit, affinity);
                let after = caret(&view.draw()).expect("caret painted");
                assert!(
                    (after.x - before.x).abs() < 0.5 && (after.y - before.y).abs() < 0.5,
                    "click at col {col}'s caret {before:?} moved it to col {hit} at {after:?}, width {width}, doc {doc:?}"
                );
            }
        }
    }
}

/// The column where a line wraps is both the end of one row and the start of
/// the next; a click past a row's end must leave the caret on that row.
#[test]
fn clicking_past_a_rows_end_keeps_the_caret_on_that_row() {
    let doc = WRAPPING_LINES[0];
    for width in WIDTHS {
        let mut view = View::new(doc, width);
        view.focus();
        let calls = view.draw();
        let runs = runs(&calls);
        let first_row_y = runs.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
        let first_row: Vec<_> = runs.iter().filter(|r| r.y == first_row_y).collect();
        let row_end = first_row
            .iter()
            .map(|r| r.x + r.width)
            .fold(f32::NEG_INFINITY, f32::max);
        let row_middle = first_row_y + first_row[0].height / 2.0;

        let (hit, affinity) =
            published_caret(&view.click(Point::new(width - 1.0, row_middle)).messages);
        assert_eq!(affinity, Affinity::Upstream);
        view.place_caret(0, hit, affinity);
        let caret = caret(&view.draw()).expect("caret painted");
        assert!(
            (caret.y + caret.height / 2.0 - row_middle).abs() < 1.0 && caret.x >= row_end - 0.5,
            "click past the first row's end put the caret at {caret:?}, row ends at {row_end}, width {width}"
        );
    }
}

/// A reveal request is answered exactly once, on the next frame, with the
/// caret's drawn place relative to the viewport that frame shows.
#[test]
fn a_reveal_request_is_answered_once_with_where_the_caret_is_drawn() {
    let doc = (0..80)
        .map(|i| format!("Line {i} of a long document"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut view = View::new(&doc, 700.0);
    view.focus();
    view.set_cursor(60, 3);
    let viewport = Rectangle {
        x: 0.0,
        y: 400.0,
        width: 700.0,
        height: 500.0,
    };
    assert!(
        view.redraw(viewport).messages.is_empty(),
        "nothing was asked"
    );

    view.reveal_request = 1;
    let answer = view.redraw(viewport).messages;
    assert_eq!(answer.len(), 1, "{answer:?}");
    let field = |name: &str| -> f32 {
        answer[0]
            .split(&format!("{name}: "))
            .nth(1)
            .and_then(|rest| rest.split([',', ' ', '}']).next())
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| panic!("no {name} in {answer:?}"))
    };
    let caret = caret(&view.draw()).expect("caret painted");
    assert_eq!(field("request"), 1.0);
    assert!(
        (field("caret_top") - caret.y).abs() < 1.0
            && (field("caret_bottom") - (caret.y + caret.height)).abs() < 1.0,
        "reported {answer:?}, drawn {caret:?}"
    );
    assert_eq!(field("viewport_top"), 400.0);
    assert_eq!(field("viewport_height"), 500.0);

    assert!(view.redraw(viewport).messages.is_empty(), "answered twice");
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
        affinity: Affinity::Downstream,
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

/// A selection across lines is one shape: its pieces never overlap, and a
/// piece that shares horizontal extent with the next meets it without a gap.
#[test]
fn a_multi_line_selection_is_one_continuous_shape() {
    let doc = "First paragraph with a few words in it\n\n## A heading\nA last line that is long enough to wrap across rows of a narrow column";
    for width in WIDTHS {
        let mut view = View::new(doc, width);
        view.focus();
        view.buffer.execute(EditorCommand::SetSelection {
            anchor_line: 0,
            anchor_col: 0,
            focus_line: 3,
            focus_col: 40,
            affinity: Affinity::Downstream,
        });
        let mut shape = quads(&view.draw(), SELECTION);
        shape.sort_by(|a, b| a.y.total_cmp(&b.y));
        assert!(shape.len() >= 4, "width {width}: {shape:?}");

        let right = |r: &Rectangle| r.x + r.width;
        let bottom = |r: &Rectangle| r.y + r.height;
        for (i, a) in shape.iter().enumerate() {
            for b in &shape[i + 1..] {
                let w = right(a).min(right(b)) - a.x.max(b.x);
                let h = bottom(a).min(bottom(b)) - a.y.max(b.y);
                assert!(
                    w <= 1e-3 || h <= 1e-3,
                    "width {width}: {a:?} overlaps {b:?}"
                );
            }
        }
        for pair in shape.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            if right(&a).min(right(&b)) > a.x.max(b.x) {
                assert!(
                    (bottom(&a) - b.y).abs() < 1e-3,
                    "width {width}: gap between {a:?} and {b:?}"
                );
            }
        }
    }
}

/// A short caret move glides: the frame it happens in still paints the caret
/// where it was, and a moment later it sits exactly where layout puts it. A
/// move across the page cuts straight there.
#[test]
fn the_caret_glides_across_short_moves_and_cuts_across_long_ones() {
    let doc = (0..60)
        .map(|i| format!("Line {i} with a handful of words"))
        .collect::<Vec<_>>()
        .join("\n");
    let resting = |line, col| {
        let mut view = View::new(&doc, 700.0);
        view.focus();
        view.set_cursor(line, col);
        caret(&view.draw()).expect("caret painted")
    };
    let viewport = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 700.0,
        height: 4000.0,
    };
    let start = std::time::Instant::now();
    let at = |millis: u64| start + std::time::Duration::from_millis(millis);
    let same = |a: Rectangle, b: Rectangle| (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3;

    let mut view = View::new(&doc, 700.0);
    view.focus();
    view.set_cursor(1, 2);
    view.redraw_at(viewport, at(0));
    view.set_cursor(1, 12);
    view.redraw_at(viewport, at(16));
    let gliding = caret(&view.draw()).expect("caret painted");
    let from = resting(1, 2);
    assert!(
        same(gliding, from),
        "first frame of the move: {gliding:?}, was {from:?}"
    );

    view.redraw_at(viewport, at(300));
    let arrived = caret(&view.draw()).expect("caret painted");
    let to = resting(1, 12);
    assert!(
        same(arrived, to),
        "settled at {arrived:?}, belongs at {to:?}"
    );

    view.set_cursor(50, 4);
    view.redraw_at(viewport, at(320));
    let cut = caret(&view.draw()).expect("caret painted");
    let far = resting(50, 4);
    assert!(
        same(cut, far),
        "a long move painted at {cut:?}, belongs at {far:?}"
    );
}

/// With a layout revision, layout runs again exactly when something it
/// depends on changes, and still gives what a fresh widget gives.
#[test]
fn layout_is_skipped_exactly_while_its_inputs_are_unchanged() {
    fn layouts() -> usize {
        super::layout::LAYOUTS.with(|count| count.get())
    }
    fn relaid(view: &mut View) -> bool {
        let before = layouts();
        view.layout();
        layouts() != before
    }
    let doc = "# Title\n\nSome text that wraps across a narrow column of the editor.\n\n```rust\nfn main() {}\n```";
    let mut view = View::new(doc, 420.0);
    view.layout_revision = Some(1);

    assert!(relaid(&mut view), "first layout");
    assert!(!relaid(&mut view), "nothing changed");
    view.set_cursor(2, 5);
    assert!(relaid(&mut view), "caret moved");
    assert!(!relaid(&mut view), "nothing changed since");
    view.width = 300.0;
    assert!(relaid(&mut view), "width changed");
    // Focusing clicks, and the click's own events already lay out anew.
    let before = layouts();
    view.focus();
    assert!(layouts() > before, "focus changed");
    assert!(!relaid(&mut view), "nothing changed since");

    view.buffer
        .execute(EditorCommand::InsertText(" more words".into()));
    view.rehighlight();
    view.layout_revision = Some(2);
    assert!(relaid(&mut view), "lines changed");
    assert!(!relaid(&mut view), "nothing changed since");

    let warm = view.layout().bounds().height;
    let mut cold = View::new(&view.buffer.text(), view.width);
    cold.focus();
    cold.set_cursor(view.buffer.cursor_line, view.buffer.cursor_col);
    assert_eq!(warm, cold.layout().bounds().height);
}

/// What frames cost on a long document. Only meaningful in a release build:
/// `cargo test --release layout_timing -- --ignored --nocapture`.
#[test]
#[ignore = "timing report; run explicitly in a release build"]
fn layout_timing() {
    use std::time::{Duration, Instant};

    let sample = "# Heading\n\nA paragraph with **bold** and *italic* words that wraps across the column of the editor.\n\n- [ ] a task item\n- a bullet item\n\n```rust\nfn main() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n> a quote\n";
    let doc = sample.repeat(37_000 / sample.lines().count() + 1);
    let mut view = View::new(&doc, 880.0);
    view.focus();
    let lines = view.lines.len();

    fn per_layout(view: &mut View, runs: u32) -> Duration {
        let start = Instant::now();
        for _ in 0..runs {
            view.layout();
        }
        start.elapsed() / runs
    }
    view.layout_revision = Some(1);
    let cold = per_layout(&mut view, 1);
    let unchanged = per_layout(&mut view, 50);
    view.layout_revision = None;
    let walked = per_layout(&mut view, 20);
    view.layout_revision = Some(1);
    view.set_cursor(lines / 2, 3);
    let caret_moved = per_layout(&mut view, 1);

    let viewport = Rectangle {
        x: 0.0,
        y: 400_000.0,
        width: 880.0,
        height: 1000.0,
    };
    let start = Instant::now();
    for _ in 0..20 {
        view.draw_in(Some(viewport));
    }
    let draw = start.elapsed() / 20;

    println!(
        "{lines} lines: cold layout {cold:?}, unchanged frame {unchanged:?}, \
         full walk with warm caches {walked:?}, after a caret move {caret_moved:?}, \
         one screen drawn {draw:?}"
    );
}

const RICH_BLOCKS: &str = "Before the blocks\n![alt](img/i.png)\n| a | b |\n|---|---|\n| 1 | 2 |\n$$\n\\wide\n$$\nInline $a^2$ after";

fn rich_view(width: f32) -> View {
    let mut view = View::new(RICH_BLOCKS, width);
    view.add_math("\\wide", 500.0, 40.0);
    view.add_math("a^2", 26.0, 30.0);
    let handle = iced::widget::image::Handle::from_rgba(2, 2, vec![9; 16]);
    view.images
        .insert("img/i.png".into(), (handle, 600.0, 200.0));
    view
}

/// Rendered images, tables and equations are selected whole, and the
/// selection stays one shape across them.
#[test]
fn a_selection_covers_images_tables_and_equations_whole() {
    use super::metrics::{text_column_width, text_left};

    for width in WIDTHS {
        let mut view = rich_view(width);
        let last = view.buffer.line_count() - 1;
        let end = view.buffer.line_text(last).chars().count();
        view.buffer.execute(EditorCommand::SetSelection {
            anchor_line: 0,
            anchor_col: 0,
            focus_line: last,
            focus_col: end,
            affinity: Affinity::Downstream,
        });
        let calls = view.draw();
        let shape = quads(&calls, SELECTION);
        let covered = |p: Point| shape.iter().any(|q| contains(*q, p));

        // Every painted image, clipped to the text column, is inside the shape.
        let (left, right) = (
            text_left(width),
            text_left(width) + text_column_width(width),
        );
        for call in &calls {
            if let Call::Image { bounds, .. } = call {
                let (x0, x1) = (
                    bounds.x.max(left) + 1.0,
                    (bounds.x + bounds.width).min(right) - 1.0,
                );
                let (y0, y1) = (bounds.y + 1.0, bounds.y + bounds.height - 1.0);
                for p in [
                    Point::new(x0, y0),
                    Point::new(x1, y0),
                    Point::new(x0, y1),
                    Point::new(x1, y1),
                    Point::new((x0 + x1) / 2.0, (y0 + y1) / 2.0),
                ] {
                    assert!(
                        covered(p),
                        "width {width}: image {bounds:?} not selected at {p:?}"
                    );
                }
            }
        }
        for run in runs(&calls) {
            assert!(
                covered(run.center()),
                "width {width}: {:?} is not selected",
                run.content
            );
        }

        let right_of = |r: &Rectangle| r.x + r.width;
        let bottom = |r: &Rectangle| r.y + r.height;
        for (i, a) in shape.iter().enumerate() {
            for b in &shape[i + 1..] {
                let w = right_of(a).min(right_of(b)) - a.x.max(b.x);
                let h = bottom(a).min(bottom(b)) - a.y.max(b.y);
                assert!(
                    w <= 1e-3 || h <= 1e-3,
                    "width {width}: {a:?} overlaps {b:?}"
                );
            }
        }
    }
}

/// An equation that renders after layout reflows the document as soon as the
/// layout revision names it, to exactly what a fresh widget lays out.
#[test]
fn equations_rendering_late_reflow_the_layout() {
    let doc = "Text above\n$$\nE = mc^2\n$$\nText below";
    let mut view = View::new(doc, 600.0);
    view.layout_revision = Some(1);
    let unrendered = view.layout().bounds().height;

    view.add_math("E = mc^2", 200.0, 120.0);
    view.layout_revision = Some(2);
    let rendered = view.layout().bounds().height;
    assert!(rendered > unrendered, "{unrendered} → {rendered}");

    let mut cold = View::new(doc, 600.0);
    cold.add_math("E = mc^2", 200.0, 120.0);
    assert_eq!(rendered, cold.layout().bounds().height);
}

/// The selection tints blocks drawn whole from above — an opaque table header
/// or picture would hide it from below — and sits under text, which a
/// translucent band on top would dim.
#[test]
fn selection_goes_over_rendered_blocks_and_under_text() {
    let mut view = rich_view(700.0);
    let last = view.buffer.line_count() - 1;
    let end = view.buffer.line_text(last).chars().count();
    view.buffer.execute(EditorCommand::SetSelection {
        anchor_line: 0,
        anchor_col: 0,
        focus_line: last,
        focus_col: end,
        affinity: Affinity::Downstream,
    });
    let calls = view.draw();
    // When the selection pieces covering `p` are actually painted.
    let covering = |p: Point| -> Vec<(usize, u8, usize)> {
        calls
            .iter()
            .enumerate()
            .filter_map(|(i, call)| match call {
                Call::Quad { bounds, color, .. } if *color == SELECTION && contains(*bounds, p) => {
                    Some(call.paint_order(i))
                }
                _ => None,
            })
            .collect()
    };
    let center = |r: &Rectangle| Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0);

    let mut checked = (0, 0, 0);
    for (i, call) in calls.iter().enumerate() {
        let order = call.paint_order(i);
        match call {
            // The figure and the display equation, not inline math on a text row.
            Call::Image { bounds, .. } if bounds.width >= 100.0 => {
                let over = covering(center(bounds));
                assert!(
                    !over.is_empty() && over.iter().all(|&j| j > order),
                    "image {bounds:?} painted at {order:?}, selection at {over:?}"
                );
                checked.0 += 1;
            }
            Call::Quad { bounds, color, .. } if *color == theme::BG_TERTIARY => {
                let over = covering(center(bounds));
                assert!(
                    !over.is_empty() && over.iter().all(|&j| j > order),
                    "header fill {bounds:?} painted at {order:?}, selection at {over:?}"
                );
                checked.1 += 1;
            }
            Call::Text { content, .. } if content.trim() == "Before" => {
                let run = runs(std::slice::from_ref(call)).remove(0);
                let under = covering(run.center());
                assert!(
                    !under.is_empty() && under.iter().all(|&j| j < order),
                    "text {content:?} painted at {order:?}, selection at {under:?}"
                );
                checked.2 += 1;
            }
            _ => {}
        }
    }
    assert_eq!(checked, (2, 1, 1), "images, header fills and text checked");
}

/// A code block, table or equation wider than its viewport shows nothing past
/// that viewport at any scroll position — judged by what a renderer actually
/// clips, which for images is only their layer.
#[test]
fn wide_blocks_show_nothing_outside_their_viewports() {
    use super::State;
    use super::metrics::{code_viewport_width, math_viewport_width, text_left, wrap_width};

    let long = "wide".repeat(60);
    type ViewportWidth = fn(f32) -> f32;
    let cases: [(&str, String, ViewportWidth); 4] = [
        (
            "code",
            format!("```\nlet {long} = 1;\n```"),
            code_viewport_width,
        ),
        (
            "table",
            format!("| {long} | b |\n|---|---|\n| 1 | {long} |"),
            wrap_width,
        ),
        ("math", "$$\n\\wide\n$$".to_string(), math_viewport_width),
        // Not rendered: its TeX source shows instead.
        (
            "math source",
            format!("$$\n{long}\n$$"),
            math_viewport_width,
        ),
    ];
    for (kind, doc, viewport_width) in cases {
        for width in [300.0, 560.0, 880.0] {
            for scroll in [0.0, 100.0, f32::MAX] {
                let mut view = View::new(&doc, width);
                view.add_math("\\wide", 900.0, 40.0);
                view.layout();
                let state = view.tree.state.downcast_mut::<State>();
                for line in &view.lines {
                    state.block_scroll_x.insert(line.block_id, scroll);
                }

                let (left, right) = (text_left(width), text_left(width) + viewport_width(width));
                for call in view.draw() {
                    let (painted, clip, what) = match &call {
                        Call::Text {
                            content,
                            size,
                            font,
                            at,
                            clip,
                            ..
                        } => {
                            let caption = content.starts_with("Listing")
                                || content.starts_with("Table")
                                || content.as_str() == "(1)";
                            if content.trim().is_empty() || caption {
                                continue;
                            }
                            let bounds = Rectangle {
                                x: at.x,
                                y: at.y,
                                width: measure_width::<iced::Renderer>(content, *size, *font),
                                height: size * LINE_BOX,
                            };
                            (bounds, *clip, content.clone())
                        }
                        Call::Image { bounds, clip, .. } => (*bounds, *clip, "equation".into()),
                        _ => continue,
                    };
                    if let Some(visible) = painted.intersection(&clip) {
                        assert!(
                            visible.x >= left - 0.5 && visible.x + visible.width <= right + 0.5,
                            "{kind} at width {width}, scroll {scroll}: {what:?} shows at \
                             {visible:?}, outside its viewport {left}..{right}"
                        );
                    }
                }
            }
        }
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

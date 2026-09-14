//! Property-based invariants of the editor renderer.
//!
//! Documents, widths, carets and edit scripts come from a seeded generator, so
//! every failure is reproducible from its seed. A failing document is shrunk
//! line by line to a minimal counterexample before it is reported.
//!
//! - `RENDER_PROPERTY_CASES=N` runs `N` cases per property.
//! - `RENDER_PROPERTY_SEED=S` replays a single seed.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::panic::{self, AssertUnwindSafe};

use iced::keyboard::{self, Key, key};
use iced::{Point, Rectangle, mouse};

use super::flow::{Affinity, Flow, ItemKind};
use super::measure::measure_width;
use super::metrics::*;
use super::spans::is_block_editing_line;
use super::testing::{Call, View};
use super::{MathCache, line_visual_y};
use crate::editor::buffer::EditorCommand;
use crate::editor::highlight::StyledLine;
use crate::editor::test_docs::{Rng, block, document, words};
use crate::theme;

type R = iced::Renderer;

// ── Generation ───────────────────────────────────────────────────────

const WIDTHS: &[f32] = &[
    40.0, 80.0, 150.0, 200.0, 260.0, 300.0, 420.0, 600.0, 760.0, 880.0, 1400.0,
];

#[derive(Clone)]
struct Case {
    seed: u64,
    doc: String,
    width: f32,
}

impl Case {
    fn generate(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let doc = document(&mut rng);
        let width = if rng.chance(0.6) {
            *rng.pick(WIDTHS)
        } else {
            rng.f32_in(40.0, 1400.0).round()
        };
        Self { seed, doc, width }
    }

    /// A random stream for the property's own choices, independent of the
    /// document so shrinking doesn't perturb it more than necessary.
    fn rng(&self, stream: u64) -> Rng {
        Rng::new(self.seed ^ stream.wrapping_mul(0xA24B_AED4_963E_E407))
    }

    fn view(&self) -> View {
        let mut view = View::new(&self.doc, self.width);
        let mut rng = self.rng(99);
        if rng.chance(0.7) {
            view.add_math("a^2", 26.0, 30.0);
            view.add_math("x", 10.0, 12.0);
            view.add_math("E = mc^2", 140.0, 30.0);
            view.add_math("x^2", 30.0, 24.0);
            view.add_math("\\wide", 900.0, 40.0);
        }
        if rng.chance(0.5) {
            let handle = iced::widget::image::Handle::from_rgba(2, 2, vec![9; 16]);
            view.images
                .insert("img/i.png".into(), (handle, 1200.0, 500.0));
        }
        view
    }
}

// ── Runner ───────────────────────────────────────────────────────────

fn case_count(default: usize) -> usize {
    std::env::var("RENDER_PROPERTY_CASES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(default)
}

fn run(prop: &dyn Fn(&Case) -> Result<(), String>, case: &Case) -> Result<(), String> {
    match panic::catch_unwind(AssertUnwindSafe(|| prop(case))) {
        Ok(result) => result,
        Err(payload) => Err(payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
            .map_or_else(|| "panicked".into(), |msg| format!("panicked: {msg}"))),
    }
}

/// Drop lines while the property still fails.
fn shrink(prop: &dyn Fn(&Case) -> Result<(), String>, mut case: Case) -> (Case, String) {
    let mut message = run(prop, &case).err().unwrap_or_default();
    loop {
        let lines: Vec<&str> = case.doc.split('\n').collect();
        let mut shrunk = None;
        for skip in 0..lines.len() {
            let doc = lines
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != skip)
                .map(|(_, l)| *l)
                .collect::<Vec<_>>()
                .join("\n");
            let candidate = Case {
                doc,
                ..case.clone()
            };
            if let Err(msg) = run(prop, &candidate) {
                shrunk = Some((candidate, msg));
                break;
            }
        }
        match shrunk {
            Some((smaller, msg)) => {
                case = smaller;
                message = msg;
            }
            None => return (case, message),
        }
    }
}

fn check(name: &str, default_cases: usize, prop: impl Fn(&Case) -> Result<(), String>) {
    let seeds: Vec<u64> = match std::env::var("RENDER_PROPERTY_SEED") {
        Ok(seed) => vec![seed.parse().expect("RENDER_PROPERTY_SEED is a u64")],
        Err(_) => (0..case_count(default_cases) as u64)
            .map(|i| {
                let mut h = DefaultHasher::new();
                (name, i).hash(&mut h);
                h.finish()
            })
            .collect(),
    };
    for seed in seeds {
        let case = Case::generate(seed);
        if run(&prop, &case).is_err() {
            let (minimal, message) = shrink(&prop, case);
            panic!(
                "property `{name}` failed\n  seed: {seed}\n  width: {}\n  error: {message}\n--- minimal document ---\n{}\n------------------------",
                minimal.width, minimal.doc
            );
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn ensure(condition: bool, message: impl FnOnce() -> String) -> Result<(), String> {
    if condition { Ok(()) } else { Err(message()) }
}

/// Content width the widget lays out at for a window width.
fn content_width(width: f32) -> f32 {
    width.min(MAX_CONTENT_WIDTH)
}

struct TextBox {
    content: String,
    rect: Rectangle,
}

/// Boxes of the painted text runs; whitespace-only runs are included only
/// when asked for.
fn text_boxes(calls: &[Call], whitespace: bool) -> Vec<TextBox> {
    calls
        .iter()
        .filter_map(|call| match call {
            Call::Text {
                content,
                size,
                font,
                at,
                align_y,
                ..
            } if whitespace || !content.trim().is_empty() => {
                let height = size * 1.3;
                let top = match align_y {
                    iced::alignment::Vertical::Top => at.y,
                    iced::alignment::Vertical::Center => at.y - height / 2.0,
                    iced::alignment::Vertical::Bottom => at.y - height,
                };
                Some(TextBox {
                    content: content.clone(),
                    rect: Rectangle {
                        x: at.x,
                        y: top,
                        width: measure_width::<R>(content, *size, *font),
                        height,
                    },
                })
            }
            _ => None,
        })
        .collect()
}

fn caret_rect(calls: &[Call]) -> Option<Rectangle> {
    calls.iter().find_map(|call| match call {
        Call::Quad { bounds, color, .. }
            if bounds.width == 2.0
                && (*color == theme::ACCENT || *color == theme::ACCENT_SECONDARY) =>
        {
            Some(*bounds)
        }
        _ => None,
    })
}

/// (line, col) of the first published `SetCursor`.
fn published_cursor(messages: &[String]) -> Option<(usize, usize)> {
    let msg = messages.iter().find(|m| m.contains("SetCursor"))?;
    let number = |key: &str| -> Option<usize> {
        let rest = msg.split(key).nth(1)?;
        rest.chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .ok()
    };
    Some((number("line: ")?, number("col: ")?))
}

fn is_flow_line(line: &StyledLine, is_editing: bool) -> bool {
    !line.is_code_block
        && !(line.is_table_row && !is_editing)
        && !(line.is_math_block && !is_editing)
}

fn random_flow<'a>(
    lines: &'a [StyledLine],
    math: &MathCache,
    width: f32,
    rng: &mut Rng,
) -> Option<(usize, Flow<'a>)> {
    let idx = rng.below(lines.len());
    let line = &lines[idx];
    let is_editing =
        rng.chance(0.3) && (line.is_code_block || line.is_math_block || line.is_table_row);
    if !is_flow_line(line, is_editing) {
        return None;
    }
    let len: usize = line.spans.iter().map(|s| s.text.chars().count()).sum();
    let active_col = rng.chance(0.6).then(|| rng.below(len + 1));
    Some((
        idx,
        Flow::build::<R>(line, math, width, is_editing, active_col),
    ))
}

fn source_len(line: &StyledLine) -> usize {
    line.spans.iter().map(|s| s.text.chars().count()).sum()
}

// ── Invariants ───────────────────────────────────────────────────────

/// I1: the caret is drawn on painted text of its own row.
#[test]
fn caret_lies_on_painted_glyphs() {
    check("caret_lies_on_painted_glyphs", 40, |case| {
        let mut view = case.view();
        view.focus();
        let mut rng = case.rng(1);
        let code_viewport = code_viewport_width(content_width(case.width));
        for _ in 0..6 {
            let line = rng.below(view.lines.len());
            let text = view.buffer.line_text(line);
            if text.trim().is_empty() {
                continue;
            }
            let col = rng.below(text.chars().count() + 1);
            let affinity = if rng.chance(0.5) {
                Affinity::Upstream
            } else {
                Affinity::Downstream
            };
            view.place_caret(line, col, affinity);
            let calls = view.draw();
            let caret = caret_rect(&calls).ok_or_else(|| format!("no caret at {line}:{col}"))?;
            let left = (case.width - content_width(case.width)).max(0.0) / 2.0
                + text_left(content_width(case.width));
            if view.lines[line].is_code_block {
                ensure(
                    caret.x >= left - 0.5 && caret.x <= left + code_viewport + 0.5,
                    || {
                        format!(
                            "code caret at {line}:{col} ({}) is outside the code viewport {left}..{}",
                            caret.x,
                            left + code_viewport
                        )
                    },
                )?;
            }
            let center = Point::new(caret.x, caret.y + caret.height / 2.0);
            ensure(
                // A caret after a hanging space sits on the space's box.
                text_boxes(&calls, true).iter().any(|b| {
                    center.x >= b.rect.x - 0.75
                        && center.x <= b.rect.x + b.rect.width + 0.75
                        && center.y >= b.rect.y
                        && center.y <= b.rect.y + b.rect.height
                }),
                || {
                    format!(
                        "caret at {line}:{col} {affinity:?} ({center:?}) is off the painted text"
                    )
                },
            )?;
        }
        Ok(())
    });
}

/// I2: hitting where the caret is drawn yields a position drawn at exactly the
/// same place, on either side of a row break.
#[test]
fn hit_testing_inverts_caret_placement() {
    check("hit_testing_inverts_caret_placement", 60, |case| {
        let view = case.view();
        let mut rng = case.rng(2);
        for _ in 0..4 {
            let Some((idx, flow)) =
                random_flow(&view.lines, &view.math, content_width(case.width), &mut rng)
            else {
                continue;
            };
            for col in 0..=source_len(&view.lines[idx]) {
                for affinity in [Affinity::Downstream, Affinity::Upstream] {
                    let spot = flow.caret::<R>(col, affinity);
                    let row = &flow.rows[spot.row];
                    let (hit, hit_affinity) =
                        flow.col_at::<R>(spot.x + 0.25, row.top + row.height / 2.0);
                    let back = flow.caret::<R>(hit, hit_affinity);
                    ensure(back.row == spot.row && back.x == spot.x, || {
                        format!(
                            "line {idx}: col {col} {affinity:?} at row {} x {} hit col {hit} \
                             {hit_affinity:?} at row {} x {}",
                            spot.row, spot.x, back.row, back.x
                        )
                    })?;
                }
            }
        }
        Ok(())
    });
}

/// I3: caret positions advance in reading order as the column advances.
#[test]
fn caret_order_follows_logical_order() {
    check("caret_order_follows_logical_order", 60, |case| {
        let view = case.view();
        let mut rng = case.rng(3);
        for _ in 0..4 {
            let Some((idx, flow)) =
                random_flow(&view.lines, &view.math, content_width(case.width), &mut rng)
            else {
                continue;
            };
            let mut previous = (0usize, f32::NEG_INFINITY);
            for col in 0..=source_len(&view.lines[idx]) {
                let spot = flow.caret::<R>(col, Affinity::Downstream);
                let here = (spot.row, spot.x);
                ensure(
                    here.0 > previous.0 || (here.0 == previous.0 && here.1 >= previous.1 - 1e-3),
                    || format!("line {idx}: col {col} at {here:?} precedes {previous:?}"),
                )?;
                previous = here;
            }
        }
        Ok(())
    });
}

/// I5a: rows tile a line, and everything on a row sits inside it.
#[test]
fn rows_tile_their_line_and_contain_their_items() {
    check("rows_tile_their_line_and_contain_their_items", 80, |case| {
        let view = case.view();
        let mut rng = case.rng(4);
        let max_w = wrap_width(content_width(case.width));
        for _ in 0..4 {
            let Some((idx, flow)) =
                random_flow(&view.lines, &view.math, content_width(case.width), &mut rng)
            else {
                continue;
            };
            let mut top = 0.0;
            for (r, row) in flow.rows.iter().enumerate() {
                ensure((row.top - top).abs() < 1e-3, || {
                    format!("line {idx} row {r} starts at {} not {top}", row.top)
                })?;
                ensure(
                    row.height >= GRID && (row.height / GRID).fract() == 0.0,
                    || {
                        format!(
                            "line {idx} row {r} height {} is off the {GRID}px grid",
                            row.height
                        )
                    },
                )?;
                top = row.top + row.height;
            }
            ensure((flow.height() - top).abs() < 1e-3, || {
                format!("line {idx} height {} != rows {top}", flow.height())
            })?;

            for item in &flow.items {
                let row = &flow.rows[item.row];
                let (item_top, item_bottom) = match &item.kind {
                    // Items that draw nothing have no extent to contain.
                    ItemKind::Hidden => continue,
                    ItemKind::Text(part) if flow.text(part, item.span_idx).trim().is_empty() => {
                        continue;
                    }
                    // Glyph extents, as the row's baseline places them.
                    ItemKind::Text(_) => {
                        let m = super::measure::font_metrics(item.font);
                        (
                            row.baseline - m.ascent * item.font_size,
                            row.baseline + m.descent * item.font_size,
                        )
                    }
                    ItemKind::Checkbox { .. } => {
                        let a = flow.axis(item.row);
                        (a - CHECKBOX_SIZE / 2.0, a + CHECKBOX_SIZE / 2.0)
                    }
                    ItemKind::Math { height, .. } => {
                        let a = flow.axis(item.row);
                        (a - height / 2.0, a + height / 2.0)
                    }
                };
                ensure(
                    item_top >= row.top - 0.01 && item_bottom <= row.top + row.height + 0.01,
                    || {
                        format!(
                            "line {idx}: item spanning {item_top}..{item_bottom} escapes row {}..{}",
                            row.top,
                            row.top + row.height
                        )
                    },
                )?;
                if let ItemKind::Text(part) = &item.kind {
                    let ink = flow.text(part, item.span_idx).trim_end();
                    let ink_w = measure_width::<R>(ink, item.font_size, part.font);
                    ensure(
                        item.x == 0.0 || item.x + ink_w <= max_w + 0.5 || ink.chars().count() <= 1,
                        || {
                            format!(
                                "line {idx}: {ink:?} at x {} overflows the {max_w}px column",
                                item.x
                            )
                        },
                    )?;
                }
            }
        }
        Ok(())
    });
}

/// Line boxes of the laid-out document, in window coordinates.
fn line_boxes(view: &View) -> Vec<(f32, f32)> {
    let tree = &view.state().layout_tree;
    (0..view.lines.len())
        .map(|i| {
            let top = TOP_PAD + tree.prefix_sum(i);
            (top, TOP_PAD + tree.prefix_sum(i + 1))
        })
        .collect()
}

/// I5b: all painted text and images sit inside their line's box.
#[test]
fn painted_content_stays_inside_line_boxes() {
    check("painted_content_stays_inside_line_boxes", 60, |case| {
        let mut view = case.view();
        let mut rng = case.rng(5);
        if rng.chance(0.5) {
            view.focus();
            let line = rng.below(view.lines.len());
            let len = view.buffer.line_text(line).chars().count();
            view.set_cursor(line, rng.below(len + 1));
        }
        let calls = view.draw();
        let boxes = line_boxes(&view);
        let inside = |rect: Rectangle| {
            boxes
                .iter()
                .any(|(top, bottom)| rect.y >= top - 0.5 && rect.y + rect.height <= bottom + 0.5)
        };
        for text in text_boxes(&calls, false) {
            ensure(inside(text.rect), || {
                format!(
                    "text {:?} at {:?} crosses a line boundary",
                    text.content, text.rect
                )
            })?;
        }
        for call in &calls {
            if let Call::Image { bounds, .. } = call {
                ensure(inside(*bounds), || {
                    format!("image at {bounds:?} crosses a line boundary")
                })?;
            }
        }
        Ok(())
    });
}

fn apply_random_edit(view: &mut View, rng: &mut Rng) {
    let line = rng.below(view.lines.len().max(1));
    let len = view.buffer.line_text(line).chars().count();
    view.set_cursor(line, rng.below(len + 1));
    let command = match rng.below(8) {
        0 => EditorCommand::InsertText("\n".into()),
        1 => EditorCommand::DeleteBackward,
        2 => EditorCommand::DeleteForward,
        3 => {
            let mut lines = Vec::new();
            block(rng, &mut lines);
            EditorCommand::InsertText(format!("\n{}", lines.join("\n")))
        }
        4 => EditorCommand::InsertText(" | x |".into()),
        _ => EditorCommand::InsertText(words(rng, 3)),
    };
    view.buffer.execute(command);
    view.rehighlight();
}

/// I6: a layout reusing cached heights through edits equals a cold layout.
#[test]
fn cached_layout_equals_cold_layout() {
    check("cached_layout_equals_cold_layout", 40, |case| {
        let mut view = case.view();
        view.focus();
        let mut rng = case.rng(6);
        view.layout_revision = Some(0);
        view.layout();
        for step in 0..6 {
            // Either an edit, which names new lines, or only a caret move,
            // which a layout that skips unchanged inputs must still notice.
            if rng.chance(0.6) {
                apply_random_edit(&mut view, &mut rng);
                view.layout_revision = Some(step as u64 + 1);
            } else {
                let line = rng.below(view.lines.len().max(1));
                let len = view.buffer.line_text(line).chars().count();
                view.set_cursor(line, rng.below(len + 1));
            }
            if rng.chance(0.2) {
                view.width = *rng.pick(WIDTHS);
            }
            let warm = view.layout().bounds().height;

            let mut cold = View::new(&view.buffer.text(), view.width);
            cold.images = view.images.clone();
            cold.math = view.math.clone();
            cold.focus();
            cold.set_cursor(view.buffer.cursor_line, view.buffer.cursor_col);
            let cold_height = cold.layout().bounds().height;

            let (a, b) = (&view.state().layout_tree, &cold.state().layout_tree);
            for i in 0..view.lines.len() {
                ensure(a.get_height(i) == b.get_height(i), || {
                    format!(
                        "step {step}: line {i} cached height {} != cold {}\n{}",
                        a.get_height(i),
                        b.get_height(i),
                        view.buffer.text()
                    )
                })?;
            }
            ensure(warm == cold_height, || {
                format!("step {step}: height {warm} != cold {cold_height}")
            })?;
            ensure(
                view.state().block_ranges == cold.state().block_ranges,
                || format!("step {step}: block ranges differ"),
            )?;
        }
        Ok(())
    });
}

/// I8: the height tree, the widget height and `line_visual_y` agree.
#[test]
fn height_tree_agrees_with_line_visual_y() {
    check("height_tree_agrees_with_line_visual_y", 60, |case| {
        let mut view = case.view();
        let mut rng = case.rng(8);
        let focused = rng.chance(0.5);
        if focused {
            view.focus();
            let line = rng.below(view.lines.len());
            let len = view.buffer.line_text(line).chars().count();
            view.set_cursor(line, rng.below(len + 1));
        }
        let height = view.layout().bounds().height;
        let width = content_width(case.width);
        let (line, col) = (view.buffer.cursor_line, view.buffer.cursor_col);
        let y = |target| {
            line_visual_y::<R>(
                &view.lines,
                &view.images,
                &view.math,
                width,
                line,
                col,
                target,
                focused,
            )
        };
        let n = view.lines.len();
        ensure((y(n) + BOTTOM_PAD - height).abs() < 0.01, || {
            format!(
                "widget height {height} != line_visual_y {}",
                y(n) + BOTTOM_PAD
            )
        })?;
        let tree = &view.state().layout_tree;
        for i in 0..n {
            let expected = TOP_PAD + tree.prefix_sum(i);
            ensure((y(i) - expected).abs() < 0.01, || {
                format!("line {i}: line_visual_y {} != tree {expected}", y(i))
            })?;
        }
        Ok(())
    });
}

/// I9: up and down move exactly one visual row, never skipping or repeating
/// one.
#[test]
fn vertical_moves_step_one_visual_row() {
    check("vertical_moves_step_one_visual_row", 60, |case| {
        let mut view = case.view();
        view.focus();
        let mut rng = case.rng(9);
        let width = content_width(case.width);
        for _ in 0..4 {
            let line = rng.below(view.lines.len());
            let len = view.buffer.line_text(line).chars().count();
            let col = rng.below(len + 1);
            view.set_cursor(line, col);
            view.layout();
            let down = rng.chance(0.5);
            let arrow = if down {
                key::Named::ArrowDown
            } else {
                key::Named::ArrowUp
            };
            // A fresh goal column for each probe.
            view.key(
                Key::Named(key::Named::ArrowLeft),
                keyboard::Modifiers::empty(),
                None,
            );
            view.set_cursor(line, col);
            let outcome = view.key(Key::Named(arrow), keyboard::Modifiers::empty(), None);
            let Some((to_line, to_col)) = published_cursor(&outcome.messages) else {
                return Err(format!(
                    "no cursor published for {arrow:?} from {line}:{col}"
                ));
            };

            // The side of a row break the widget put the caret on.
            let landed = if outcome
                .messages
                .iter()
                .any(|m| m.contains("SetCursor") && m.contains("Upstream"))
            {
                Affinity::Upstream
            } else {
                Affinity::Downstream
            };

            let lines = &view.lines;
            let active_block = lines.get(line).map(|l| l.block_id);
            let row_of =
                |idx: usize, c: usize, active: Option<usize>, affinity| -> (usize, usize) {
                    let l = &lines[idx];
                    if l.is_code_block {
                        return (0, 1);
                    }
                    let editing = is_block_editing_line(l, active_block, true);
                    let flow = Flow::build::<R>(l, &view.math, width, editing, active);
                    (flow.caret::<R>(c, affinity).row, flow.rows.len())
                };
            let (row, rows) = row_of(line, col, Some(col), Affinity::Downstream);

            let expected_line = if down && row + 1 < rows || !down && row > 0 {
                line
            } else if down {
                (line + 1).min(lines.len() - 1)
            } else {
                line.saturating_sub(1)
            };
            ensure(to_line == expected_line, || {
                format!(
                    "{arrow:?} from {line}:{col} (row {row}/{rows}) went to line {to_line}, expected {expected_line}"
                )
            })?;

            if to_line == line
                && expected_line == line
                && (down && row + 1 < rows || !down && row > 0)
            {
                let (to_row, _) = row_of(line, to_col, Some(col), landed);
                let expected_row = if down { row + 1 } else { row - 1 };
                ensure(to_row == expected_row, || {
                    format!("{arrow:?} from {line}:{col} row {row} landed on row {to_row}")
                })?;
            } else if to_line != line {
                let (to_row, to_rows) = row_of(to_line, to_col, None, landed);
                let expected_row = if down { 0 } else { to_rows - 1 };
                ensure(to_row == expected_row, || {
                    format!(
                        "{arrow:?} from {line}:{col} landed on row {to_row} of line {to_line}, expected {expected_row}"
                    )
                })?;
            }
        }
        Ok(())
    });
}

/// I10: nothing panics, whatever the width, input or document.
#[test]
fn arbitrary_input_never_panics() {
    check("arbitrary_input_never_panics", 30, |case| {
        let mut view = case.view();
        let mut rng = case.rng(10);
        if rng.chance(0.3) {
            view.width = *rng.pick(&[0.0, 1.0, 12.0, 33.0, 100_000.0]);
        }
        for _ in 0..25 {
            let height = view.layout().bounds().height.max(1.0);
            let p = Point::new(
                rng.f32_in(-50.0, view.width + 50.0),
                rng.f32_in(-50.0, height + 50.0),
            );
            match rng.below(10) {
                0 => {
                    view.click(p);
                }
                1 => {
                    view.press(p);
                    view.move_to(Point::new(
                        p.x + rng.f32_in(-300.0, 300.0),
                        p.y + rng.f32_in(-100.0, 100.0),
                    ));
                    view.release(p);
                }
                2 => {
                    view.modifiers(if rng.chance(0.5) {
                        keyboard::Modifiers::SHIFT
                    } else {
                        keyboard::Modifiers::empty()
                    });
                    let delta = if rng.chance(0.5) {
                        mouse::ScrollDelta::Lines {
                            x: rng.f32_in(-3.0, 3.0),
                            y: rng.f32_in(-3.0, 3.0),
                        }
                    } else {
                        mouse::ScrollDelta::Pixels {
                            x: rng.f32_in(-400.0, 400.0),
                            y: rng.f32_in(-400.0, 400.0),
                        }
                    };
                    view.wheel(p, delta);
                }
                3 => {
                    let named = *rng.pick(&[
                        key::Named::ArrowUp,
                        key::Named::ArrowDown,
                        key::Named::ArrowLeft,
                        key::Named::ArrowRight,
                        key::Named::Home,
                        key::Named::End,
                        key::Named::Backspace,
                        key::Named::Enter,
                        key::Named::Tab,
                    ]);
                    let modifiers =
                        *rng.pick(&[keyboard::Modifiers::empty(), keyboard::Modifiers::SHIFT]);
                    view.key(Key::Named(named), modifiers, None);
                }
                4 => {
                    view.key(
                        Key::Character("z".into()),
                        keyboard::Modifiers::empty(),
                        Some("z"),
                    );
                }
                5 => apply_random_edit(&mut view, &mut rng),
                6 => {
                    let line = rng.below(view.lines.len() + 3);
                    view.set_cursor(line, rng.below(500));
                }
                7 => {
                    let y = rng.f32_in(-100.0, height + 100.0);
                    view.draw_in(Some(Rectangle {
                        x: 0.0,
                        y,
                        width: view.width,
                        height: rng.f32_in(0.0, 900.0),
                    }));
                }
                8 => {
                    view.interaction(p);
                }
                _ => {
                    view.draw();
                }
            }
        }
        Ok(())
    });
}

/// I11: scrollbar thumbs stay inside their tracks, and a dragged thumb follows
/// the pointer.
#[test]
fn scrollbars_stay_in_bounds_and_follow_drags() {
    check("scrollbars_stay_in_bounds_and_follow_drags", 40, |case| {
        let mut view = case.view();
        let mut rng = case.rng(11);
        let track_color = iced::Color::from_rgba(1.0, 1.0, 1.0, 0.06);
        for _ in 0..6 {
            let calls = view.draw();
            let quads = |color| -> Vec<Rectangle> {
                calls
                    .iter()
                    .filter_map(|c| match c {
                        Call::Quad {
                            bounds, color: c, ..
                        } if *c == color => Some(*bounds),
                        _ => None,
                    })
                    .collect()
            };
            let (tracks, thumbs) = (quads(track_color), quads(theme::ACCENT_DIM));
            ensure(tracks.len() == thumbs.len(), || {
                format!("{} tracks but {} thumbs", tracks.len(), thumbs.len())
            })?;
            for (track, thumb) in tracks.iter().zip(&thumbs) {
                ensure(
                    thumb.x >= track.x - 0.01
                        && thumb.x + thumb.width <= track.x + track.width + 0.01,
                    || format!("thumb {thumb:?} escapes track {track:?}"),
                )?;
            }
            let Some(&thumb) = thumbs.get(rng.below(thumbs.len().max(1))) else {
                return Ok(());
            };
            let track = tracks
                .iter()
                .find(|t| (t.y - thumb.y).abs() < 0.01)
                .copied()
                .unwrap_or(thumb);

            if rng.chance(0.5) {
                let grab = Point::new(thumb.x + thumb.width / 2.0, thumb.y + 2.0);
                let dx = rng.f32_in(-200.0, 200.0);
                view.press(grab);
                view.move_to(Point::new(grab.x + dx, grab.y));
                view.release(Point::new(grab.x + dx, grab.y));
                let moved = view
                    .draw()
                    .iter()
                    .filter_map(|c| match c {
                        Call::Quad { bounds, color, .. }
                            if *color == theme::ACCENT_DIM && (bounds.y - thumb.y).abs() < 0.01 =>
                        {
                            Some(*bounds)
                        }
                        _ => None,
                    })
                    .next();
                if let Some(moved) = moved {
                    let expected =
                        (thumb.x + dx).clamp(track.x, track.x + track.width - thumb.width);
                    ensure((moved.x - expected).abs() < 0.75, || {
                        format!(
                            "dragged thumb by {dx} from {} to {}, expected {expected}",
                            thumb.x, moved.x
                        )
                    })?;
                }
            } else {
                view.modifiers(keyboard::Modifiers::SHIFT);
                view.wheel(
                    Point::new(thumb.x + 1.0, thumb.y - 10.0),
                    mouse::ScrollDelta::Lines {
                        x: 0.0,
                        y: rng.f32_in(-8.0, 8.0),
                    },
                );
                view.modifiers(keyboard::Modifiers::empty());
            }
        }
        Ok(())
    });
}

/// I12: drawing visits only lines that intersect the viewport.
#[test]
fn drawing_visits_only_visible_lines() {
    check("drawing_visits_only_visible_lines", 60, |case| {
        let mut view = case.view();
        let mut rng = case.rng(12);
        let height = view.layout().bounds().height;
        let viewport = Rectangle {
            x: 0.0,
            y: rng.f32_in(0.0, height),
            width: view.width,
            height: rng.f32_in(1.0, 600.0),
        };
        view.draw_in(Some(viewport));
        let visited = super::draw::painted_lines();
        let boxes = line_boxes(&view);
        for idx in visited {
            let (top, bottom) = boxes[idx];
            ensure(
                bottom >= viewport.y - 0.5 && top <= viewport.y + viewport.height + 0.5,
                || format!("painted line {idx} ({top}..{bottom}) outside viewport {viewport:?}"),
            )?;
        }
        Ok(())
    });
}

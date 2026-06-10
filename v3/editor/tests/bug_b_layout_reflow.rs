//! BUG-B regression suite (M1 gate, plan §5).
//!
//! v2 symptom: clicking a line revealed concealed markers, the line wrapped
//! taller, but subsequent line offsets were stale — the overflow row painted
//! over the next line.
//!
//! v3 contract, tested two ways:
//! 1. Even with a *naive* (layout-unstable) styler, a height change updates
//!    every subsequent offset immediately — overdraw is impossible because
//!    offsets are computed from the height sum-tree, never cached.
//! 2. The production strategy is layout-stable conceal (reserved width):
//!    caret motion changes no heights at all, and damage is confined to the
//!    two affected lines ("draw-diff ≤ 2 lines" golden gate).

use md3_editor::height_tree::OutOfBounds;
use md3_editor::{
    BlockState, ConcealMode, Damage, LayoutEngine, LineMeasure, Measurer, StyledLine, Styler,
};

const LINE_HEIGHT: f64 = 16.0;
const WRAP_COLS: f64 = 20.0;

/// Reproduces the v2 behavior: concealed form strips the `**` markers, so
/// revealing makes the display longer (possibly wrapping to more rows).
struct NaiveStyler;

impl Styler for NaiveStyler {
    fn style(&self, text: &str, _block: &BlockState, conceal: ConcealMode) -> StyledLine {
        let display = match conceal {
            ConcealMode::Concealed => text.replace("**", ""),
            ConcealMode::Revealed => text.to_string(),
        };
        StyledLine::plain(display, conceal)
    }

    fn layout_stable(&self) -> bool {
        false
    }
}

/// The v3 production strategy: markers are always part of the measured box
/// (reserved width); conceal only changes paint attributes.
struct ReservedWidthStyler;

impl Styler for ReservedWidthStyler {
    fn style(&self, text: &str, _block: &BlockState, conceal: ConcealMode) -> StyledLine {
        StyledLine::plain(text, conceal)
    }

    fn layout_stable(&self) -> bool {
        true
    }
}

/// Monospace-grid measurer: rows = ceil(chars / wrap columns).
struct CharGridMeasurer;

impl Measurer for CharGridMeasurer {
    fn measure(&self, line: &StyledLine, wrap_width: f64) -> LineMeasure {
        let cols = wrap_width.max(1.0) as usize;
        let chars = line.display.chars().count().max(1);
        let rows = chars.div_ceil(cols) as u32;
        LineMeasure {
            height: rows as f64 * LINE_HEIGHT,
            rows,
        }
    }
}

fn ok<T>(r: Result<T, OutOfBounds>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{e}"),
    }
}

// 18 chars concealed (1 row at 20 cols), 22 revealed (2 rows).
const BOLD_LINE: &str = "**a study of margins**";

fn doc<S: Styler>(styler: S) -> LayoutEngine<S, CharGridMeasurer> {
    let mut engine = LayoutEngine::new(styler, CharGridMeasurer, WRAP_COLS);
    engine.set_text(
        ["# Title", BOLD_LINE, "after one", "after two"].map(|t| (t, BlockState::Normal)),
    );
    engine
}

#[test]
fn reveal_grows_line_and_all_subsequent_offsets_update_immediately() {
    // The exact v2 failure, against the layout-unstable styler.
    let mut engine = doc(NaiveStyler);
    assert_eq!(
        ok(engine.offset_of(2)),
        2.0 * LINE_HEIGHT,
        "concealed: 1 row each"
    );

    // Click line 1 → caret enters → reveal.
    let damage = ok(engine.caret_moved(None, Some(1)));

    assert_eq!(
        engine.height_of(1),
        Some(2.0 * LINE_HEIGHT),
        "revealed line wraps to 2 rows"
    );
    // THE bug assertion: the lines below moved down — no stale offsets,
    // no overdraw.
    assert_eq!(ok(engine.offset_of(2)), 3.0 * LINE_HEIGHT);
    assert_eq!(ok(engine.offset_of(3)), 4.0 * LINE_HEIGHT);
    assert_eq!(engine.total_height(), 5.0 * LINE_HEIGHT);
    // And the paint phase was told geometry shifted from line 2 down.
    assert_eq!(damage.shifted_from, Some(2));
}

#[test]
fn lines_never_overlap_through_conceal_reveal_cycles() {
    let mut engine = doc(NaiveStyler);
    let mut caret: Option<usize> = None;
    for target in [1usize, 2, 1, 0, 3, 1] {
        ok(engine.caret_moved(caret, Some(target)));
        caret = Some(target);
        // Invariant: every line starts exactly where the previous one ends.
        let mut expected = 0.0;
        for i in 0..engine.line_count() {
            assert_eq!(
                ok(engine.offset_of(i)),
                expected,
                "line {i} overlaps after caret→{target}"
            );
            expected += engine.height_of(i).unwrap_or(0.0);
        }
    }
}

#[test]
fn layout_stable_conceal_caret_motion_damages_at_most_two_lines() {
    // M1 golden gate: "caret enter/leave line ⇒ draw-diff ≤ 2 lines".
    let mut engine = doc(ReservedWidthStyler);
    let before_total = engine.total_height();

    ok(engine.caret_moved(None, Some(1)));
    let damage = ok(engine.caret_moved(Some(1), Some(2)));

    assert_eq!(
        damage.repaint,
        1..3,
        "exactly the two affected lines repaint"
    );
    assert_eq!(
        damage.shifted_from, None,
        "no geometry shift — reveal is paint-only"
    );
    assert_eq!(
        engine.total_height(),
        before_total,
        "reserved width: heights never change"
    );
}

#[test]
fn edits_report_shift_only_when_height_actually_changes() {
    let mut engine = doc(ReservedWidthStyler);

    let same_rows = ok(engine.replace_line(2, "after 1!!", BlockState::Normal));
    assert_eq!(same_rows.shifted_from, None, "same height → pure repaint");
    assert_eq!(same_rows.repaint, 2..3);

    let grew = ok(engine.replace_line(
        2,
        "this line is now much longer than one row",
        BlockState::Normal,
    ));
    assert_eq!(grew.shifted_from, Some(3));
    // Lines above: "# Title" (1 row) + BOLD_LINE (22 chars reserved → 2 rows)
    // + the new 41-char line (3 rows) = 6 rows.
    assert_eq!(ok(engine.offset_of(3)), 6.0 * LINE_HEIGHT);
}

#[test]
fn insert_and_remove_shift_subsequent_lines() {
    let mut engine = doc(ReservedWidthStyler);
    let ins = ok(engine.insert_line(1, "inserted", BlockState::Normal));
    assert_eq!(ins.shifted_from, Some(2));
    assert_eq!(engine.line_count(), 5);
    assert_eq!(ok(engine.offset_of(2)), 2.0 * LINE_HEIGHT);

    let rem = ok(engine.remove_line(1));
    assert_eq!(rem.shifted_from, Some(1));
    assert_eq!(engine.line_count(), 4);
    assert_eq!(ok(engine.offset_of(1)), LINE_HEIGHT);
}

#[test]
fn paint_phase_is_viewport_bounded() {
    let mut engine = LayoutEngine::new(ReservedWidthStyler, CharGridMeasurer, WRAP_COLS);
    engine.set_text((0..1000).map(|i| (format!("line {i}"), BlockState::Normal)));
    let visible = engine.visible_lines(50.0 * LINE_HEIGHT, 10.0 * LINE_HEIGHT);
    assert_eq!(visible, 50..61, "paint touches only the viewport slice");
    assert!(matches!(
        engine.caret_moved(None, Some(999)),
        Ok(Damage { .. })
    ));
}

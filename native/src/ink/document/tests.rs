//! Tests for `document`.

use super::*;

const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

fn line(y: f32) -> InkStroke {
    let points: Vec<InkPoint> = (0..10)
        .map(|i| InkPoint::new(i as f32 * 5.0, y, 0.8))
        .collect();
    InkStroke::from_points(&points, 3.0, BLACK, StrokeKind::Pen)
}

#[test]
fn push_then_undo_removes_the_stroke() {
    let mut doc = InkDocument::new();
    doc.push(line(10.0));
    assert_eq!(doc.strokes().len(), 1);

    assert!(doc.undo());
    assert!(doc.is_empty());
    assert!(!doc.can_undo());
    assert!(doc.can_redo());
}

#[test]
fn redo_restores_an_undone_stroke() {
    let mut doc = InkDocument::new();
    doc.push(line(10.0));
    doc.undo();

    assert!(doc.redo());
    assert_eq!(doc.strokes().len(), 1);
    assert!(!doc.can_redo());
}

#[test]
fn new_edit_clears_the_redo_branch() {
    let mut doc = InkDocument::new();
    doc.push(line(10.0));
    doc.undo();
    assert!(doc.can_redo());

    doc.push(line(20.0));
    assert!(
        !doc.can_redo(),
        "a new stroke should discard the redo branch"
    );
}

#[test]
fn erase_removes_only_strokes_under_the_probe() {
    let mut doc = InkDocument::new();
    doc.push(line(10.0));
    doc.push(line(80.0));

    assert!(doc.erase_at(Vec2::new(20.0, 10.5), 4.0));
    assert_eq!(
        doc.strokes().len(),
        1,
        "only the stroke under the eraser should go"
    );
    assert!(
        !doc.erase_at(Vec2::new(500.0, 500.0), 4.0),
        "empty space erases nothing"
    );
}

#[test]
fn undoing_an_erase_restores_z_order() {
    let mut doc = InkDocument::new();
    let bottom = line(10.0);
    let middle = line(10.2);
    let top = line(80.0);
    doc.push(bottom.clone());
    doc.push(middle.clone());
    doc.push(top.clone());

    // Erases the two overlapping strokes, leaving the far one.
    assert!(doc.erase_at(Vec2::new(20.0, 10.1), 4.0));
    assert_eq!(doc.strokes().len(), 1);

    assert!(doc.undo());
    assert_eq!(
        doc.strokes(),
        &[bottom, middle, top][..],
        "z-order must survive undo"
    );
}

#[test]
fn clear_is_a_single_undoable_step() {
    let mut doc = InkDocument::new();
    doc.push(line(10.0));
    doc.push(line(20.0));
    doc.push(line(30.0));

    assert!(doc.clear());
    assert!(doc.is_empty());

    assert!(doc.undo());
    assert_eq!(
        doc.strokes().len(),
        3,
        "one undo should bring back the whole page"
    );
}

#[test]
fn clearing_an_empty_document_is_not_an_edit() {
    let mut doc = InkDocument::new();
    assert!(!doc.clear());
    assert!(!doc.can_undo());
}

#[test]
fn json_round_trip_preserves_strokes() {
    let mut doc = InkDocument::new();
    doc.push(line(10.0));
    doc.push(line(20.0));

    let json = doc.to_json().unwrap();
    let restored = InkDocument::from_json(&json).unwrap();

    assert_eq!(restored.strokes(), doc.strokes());
    assert!(!restored.can_undo(), "history is not persisted");
}

#[test]
fn rejects_a_document_from_a_newer_build() {
    let raw = format!(r#"{{"version":{},"strokes":[]}}"#, FORMAT_VERSION + 1);
    assert!(InkDocument::from_json(&raw).is_err());
}

#[test]
fn hit_test_measures_distance_to_the_path_not_the_endpoints() {
    let stroke = line(0.0); // spans x 0..45 at y = 0
    // Directly above the middle of the stroke, just outside the radius.
    assert!(stroke.hit_by(Vec2::new(22.0, 2.0), 3.0));
    assert!(!stroke.hit_by(Vec2::new(22.0, 9.0), 3.0));
}

/// Square lasso covering x,y in 0..50.
fn square() -> Vec<Vec2> {
    vec![
        Vec2::new(-5.0, -5.0),
        Vec2::new(50.0, -5.0),
        Vec2::new(50.0, 50.0),
        Vec2::new(-5.0, 50.0),
    ]
}

#[test]
fn point_in_polygon_distinguishes_inside_from_outside() {
    let poly = square();
    assert!(point_in_polygon(Vec2::new(20.0, 20.0), &poly));
    assert!(!point_in_polygon(Vec2::new(200.0, 20.0), &poly));
    assert!(!point_in_polygon(Vec2::new(20.0, -80.0), &poly));
}

#[test]
fn a_degenerate_polygon_selects_nothing() {
    assert!(!point_in_polygon(
        Vec2::new(0.0, 0.0),
        &[Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)]
    ));
}

#[test]
fn lasso_selects_enclosed_strokes_only() {
    let mut doc = InkDocument::new();
    doc.push(line(10.0)); // inside: spans x 0..45 at y = 10
    doc.push(line(300.0)); // far outside

    let selected = doc.select_in_polygon(&square());
    assert_eq!(selected, vec![0]);
}

#[test]
fn lasso_ignores_a_stroke_it_merely_clips() {
    let mut doc = InkDocument::new();
    // Long stroke mostly outside the lasso, with only its tail inside.
    let points: Vec<InkPoint> = (0..40)
        .map(|i| InkPoint::new(i as f32 * 20.0, 10.0, 0.8))
        .collect();
    doc.push(InkStroke::from_points(&points, 3.0, BLACK, StrokeKind::Pen));

    assert!(
        doc.select_in_polygon(&square()).is_empty(),
        "a stroke only clipped by the lasso should not be selected"
    );
}

#[test]
fn a_tracked_drag_is_undone_as_one_step() {
    // Mirrors how a drag actually runs: the strokes are walked into place
    // sample by sample, then the total is recorded once.
    let mut doc = InkDocument::new();
    doc.push(line(10.0));
    let before = doc.strokes()[0].clone();

    doc.translate_without_history(&[0], Vec2::new(10.0, -2.0));
    doc.translate_without_history(&[0], Vec2::new(5.0, -2.0));
    assert!(doc.record_move(&[0], Vec2::new(15.0, -4.0)));

    assert!((doc.strokes()[0].points[0][0] - (before.points[0][0] + 15.0)).abs() < 1e-3);
    assert!((doc.strokes()[0].points[0][1] - (before.points[0][1] - 4.0)).abs() < 1e-3);

    assert!(doc.undo());
    assert!(
        (doc.strokes()[0].points[0][0] - before.points[0][0]).abs() < 1e-3,
        "one undo should reverse the whole drag"
    );

    assert!(doc.redo());
    assert!((doc.strokes()[0].points[0][0] - (before.points[0][0] + 15.0)).abs() < 1e-3);
}

#[test]
fn a_zero_distance_move_is_not_recorded() {
    let mut doc = InkDocument::new();
    doc.push(line(10.0));
    assert!(!doc.record_move(&[0], Vec2::new(0.0, 0.0)));
    assert!(!doc.record_move(&[], Vec2::new(5.0, 5.0)));
    assert!(doc.can_undo(), "only the push should be on the stack");
    doc.undo();
    assert!(!doc.can_undo());
}

#[test]
fn removing_a_selection_restores_z_order_on_undo() {
    let mut doc = InkDocument::new();
    let a = line(10.0);
    let b = line(20.0);
    let c = line(30.0);
    doc.push(a.clone());
    doc.push(b.clone());
    doc.push(c.clone());

    assert!(doc.remove_strokes(&[0, 2]));
    assert_eq!(doc.strokes(), &[b.clone()][..]);

    assert!(doc.undo());
    assert_eq!(doc.strokes(), &[a, b, c][..]);
}

#[test]
fn removing_nothing_is_not_an_edit() {
    let mut doc = InkDocument::new();
    doc.push(line(10.0));
    assert!(!doc.remove_strokes(&[]));
    assert!(
        !doc.remove_strokes(&[99]),
        "out-of-range indices change nothing"
    );
}

#[test]
fn a_partial_erase_splits_a_stroke_in_two() {
    let mut doc = InkDocument::new();
    doc.push(line(0.0)); // spans x 0..45 at y = 0

    // Rub out the middle.
    assert!(doc.erase_partial_at(Vec2::new(22.5, 0.0), 6.0));
    assert_eq!(doc.strokes().len(), 2, "the stroke should be cut in two");

    let left = doc.strokes()[0].path_bounds().unwrap();
    let right = doc.strokes()[1].path_bounds().unwrap();
    assert!(left.1.x < right.0.x, "the fragments must not overlap");
}

#[test]
fn a_partial_erase_at_the_end_leaves_one_fragment() {
    let mut doc = InkDocument::new();
    doc.push(line(0.0));

    assert!(doc.erase_partial_at(Vec2::new(45.0, 0.0), 8.0));
    assert_eq!(doc.strokes().len(), 1, "clipping the tail leaves one piece");
}

#[test]
fn a_partial_erase_that_misses_changes_nothing() {
    let mut doc = InkDocument::new();
    doc.push(line(0.0));
    assert!(!doc.erase_partial_at(Vec2::new(500.0, 500.0), 6.0));
    assert_eq!(doc.strokes().len(), 1);
}

#[test]
fn a_partial_erase_is_undoable_back_to_the_whole_stroke() {
    let mut doc = InkDocument::new();
    doc.push(line(0.0));
    let original = doc.strokes()[0].clone();

    doc.erase_partial_at(Vec2::new(22.5, 0.0), 6.0);
    assert_eq!(doc.strokes().len(), 2);

    assert!(doc.undo());
    assert_eq!(
        doc.strokes(),
        &[original.clone()][..],
        "undo restores the whole line"
    );

    assert!(doc.redo());
    assert_eq!(doc.strokes().len(), 2, "redo re-cuts it");
}

#[test]
fn an_eraser_drag_undoes_as_a_single_step() {
    let mut doc = InkDocument::new();
    doc.push(line(0.0));
    doc.push(line(30.0));
    doc.push(line(60.0));

    // One sweep touching all three, as a drag would.
    doc.begin_batch();
    doc.erase_at(Vec2::new(20.0, 0.0), 5.0);
    doc.erase_at(Vec2::new(20.0, 30.0), 5.0);
    doc.erase_at(Vec2::new(20.0, 60.0), 5.0);
    doc.end_batch();

    assert!(doc.is_empty());
    assert!(doc.undo(), "one undo should bring the sweep back");
    assert_eq!(doc.strokes().len(), 3, "the whole drag reverses at once");
}

#[test]
fn a_batch_that_erased_nothing_leaves_no_history() {
    let mut doc = InkDocument::new();
    doc.push(line(0.0));
    doc.undo();
    assert!(!doc.can_undo());

    doc.begin_batch();
    doc.erase_at(Vec2::new(900.0, 900.0), 5.0);
    doc.end_batch();

    assert!(!doc.can_undo(), "a sweep over empty space is not an edit");
}

#[test]
fn a_batched_sweep_redoes_in_order() {
    let mut doc = InkDocument::new();
    doc.push(line(0.0));
    doc.push(line(30.0));

    doc.begin_batch();
    doc.erase_at(Vec2::new(20.0, 0.0), 5.0);
    doc.erase_at(Vec2::new(20.0, 30.0), 5.0);
    doc.end_batch();

    doc.undo();
    assert_eq!(doc.strokes().len(), 2);
    assert!(doc.redo());
    assert!(doc.is_empty(), "redo should reapply the whole sweep");
}

#[test]
fn highlighter_strokes_survive_a_round_trip() {
    let points: Vec<InkPoint> = (0..6).map(|i| InkPoint::new(i as f32, 0.0, 0.5)).collect();
    let mut doc = InkDocument::new();
    doc.paper = PaperStyle::Grid;
    doc.push(InkStroke::from_points(
        &points,
        20.0,
        [1.0, 0.8, 0.2, 0.32],
        StrokeKind::Highlighter,
    ));

    let restored = InkDocument::from_json(&doc.to_json().unwrap()).unwrap();
    assert_eq!(restored.strokes()[0].kind, StrokeKind::Highlighter);
    assert_eq!(
        restored.paper,
        PaperStyle::Grid,
        "paper travels with the page"
    );
}

#[test]
fn pages_written_before_kinds_and_paper_existed_still_load() {
    // No `kind` on the stroke and no `paper` on the file.
    let legacy = r#"{"version":1,"strokes":[{"size":3.0,"color":[1,1,1,1],"points":[[0,0,0.5],[1,1,0.5]]}]}"#;
    let doc = InkDocument::from_json(legacy).expect("legacy pages must still open");
    assert_eq!(doc.strokes()[0].kind, StrokeKind::Pen);
    assert_eq!(doc.paper, PaperStyle::default());
}

#[test]
fn distance_to_degenerate_segment_is_point_distance() {
    let a = Vec2::new(4.0, 4.0);
    assert!((distance_to_segment(Vec2::new(4.0, 7.0), a, a) - 3.0).abs() < 1e-5);
}

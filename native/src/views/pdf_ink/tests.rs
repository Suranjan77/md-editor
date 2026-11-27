//! Unit tests for PDF margin ink hit-testing and coordinate clamping.

use super::*;

#[test]
fn clamp_fraction_points_clamps_coordinates_and_drops_non_finite() {
    let raw = [
        [-0.2, 1.4, 0.0],
        [0.5, 0.5, 0.8],
        [f32::NAN, 0.2, 0.5],
        [0.9, 1.2, 2.0],
    ];
    let clamped = clamp_fraction_points(&raw);
    assert_eq!(clamped.len(), 3);
    assert_eq!(clamped[0], [0.0, 1.0, 0.05]);
    assert_eq!(clamped[1], [0.5, 0.5, 0.8]);
    assert_eq!(clamped[2], [0.9, 1.0, 1.0]);
}

#[test]
fn hit_test_ink_stroke_detects_nearby_segments_and_ignores_distant_clicks() {
    let stroke = PdfInkStroke {
        size: DEFAULT_INK_WIDTH_FRAC,
        points: vec![[0.2, 0.2, 0.5], [0.8, 0.2, 0.5]],
    };
    assert!(hit_test_ink_stroke(
        &stroke,
        0.5,
        0.21,
        ERASE_HIT_RADIUS_FRAC
    ));
    assert!(hit_test_ink_stroke(
        &stroke,
        0.2,
        0.2,
        ERASE_HIT_RADIUS_FRAC
    ));
    assert!(!hit_test_ink_stroke(
        &stroke,
        0.5,
        0.5,
        ERASE_HIT_RADIUS_FRAC
    ));
    assert!(!hit_test_ink_stroke(
        &stroke,
        f32::NAN,
        0.2,
        ERASE_HIT_RADIUS_FRAC
    ));
}

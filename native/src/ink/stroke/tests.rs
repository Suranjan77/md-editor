//! Tests for `stroke`.

use super::*;

/// Straight horizontal run of samples at a constant pressure.
fn horizontal(n: usize, pressure: f32) -> Vec<InkPoint> {
    (0..n)
        .map(|i| InkPoint::new(i as f32 * 4.0, 50.0, pressure))
        .collect()
}

#[test]
fn empty_input_produces_no_outline() {
    assert!(outline(&[], &StrokeOptions::default()).is_empty());
}

#[test]
fn single_sample_becomes_a_dot() {
    let poly = outline(&[InkPoint::new(10.0, 10.0, 1.0)], &StrokeOptions::default());
    assert!(!poly.is_empty(), "a tap should still leave a mark");

    let (min, max) = bounds(&poly).unwrap();
    // Bounded by the stroke size, and centred on the sample.
    let opts = StrokeOptions::default();
    assert!(max.x - min.x <= opts.size + 0.01);
    assert!(((min.x + max.x) * 0.5 - 10.0).abs() < 0.01);
    assert!(((min.y + max.y) * 0.5 - 10.0).abs() < 0.01);
}

#[test]
fn stationary_samples_collapse_to_a_dot() {
    // A pen held still still delivers samples; they must not produce a
    // degenerate zero-area polygon.
    let held: Vec<InkPoint> = (0..20).map(|_| InkPoint::new(5.0, 5.0, 0.8)).collect();
    let poly = outline(&held, &StrokeOptions::default());
    let (min, max) = bounds(&poly).unwrap();
    assert!(max.x - min.x > 0.1, "held pen should still mark the page");
}

#[test]
fn outline_width_tracks_pressure() {
    let opts = StrokeOptions {
        // Taper would dominate on a short test stroke.
        taper_start: 0.0,
        taper_end: 0.0,
        ..StrokeOptions::default()
    };

    let light = outline(&horizontal(24, 0.1), &opts);
    let heavy = outline(&horizontal(24, 1.0), &opts);

    let (lmin, lmax) = bounds(&light).unwrap();
    let (hmin, hmax) = bounds(&heavy).unwrap();

    let light_width = lmax.y - lmin.y;
    let heavy_width = hmax.y - hmin.y;

    assert!(
        heavy_width > light_width * 1.5,
        "pressure should visibly change width: light={light_width}, heavy={heavy_width}"
    );
    assert!(
        (heavy_width - opts.size).abs() < 0.2,
        "full pressure should reach the nominal size, got {heavy_width}"
    );
}

#[test]
fn light_pressure_still_leaves_a_visible_line() {
    let opts = StrokeOptions {
        taper_start: 0.0,
        taper_end: 0.0,
        ..StrokeOptions::default()
    };
    let poly = outline(&horizontal(24, 0.0), &opts);
    let (min, max) = bounds(&poly).unwrap();
    assert!(
        max.y - min.y >= opts.min_radius * 2.0 - 0.01,
        "zero pressure must not produce an invisible stroke"
    );
}

#[test]
fn streamline_damps_jitter() {
    // Alternating vertical noise on an otherwise straight path.
    let jittery: Vec<InkPoint> = (0..40)
        .map(|i| {
            let wobble = if i % 2 == 0 { 1.0 } else { -1.0 };
            InkPoint::new(i as f32 * 3.0, 20.0 + wobble, 0.5)
        })
        .collect();

    let raw_spread = spread(&streamline(&jittery, 0.0));
    let smoothed_spread = spread(&streamline(&jittery, 0.8));

    assert!(
        smoothed_spread < raw_spread * 0.75,
        "streamlining should reduce jitter: raw={raw_spread}, smoothed={smoothed_spread}"
    );
}

fn spread(points: &[InkPoint]) -> f32 {
    let ys: Vec<f32> = points.iter().map(|p| p.pos.y).collect();
    let max = ys.iter().copied().fold(f32::MIN, f32::max);
    let min = ys.iter().copied().fold(f32::MAX, f32::min);
    max - min
}

#[test]
fn streamline_keeps_the_sample_the_pen_lifted_on() {
    // A stroke that slows to a halt: the closing samples fall inside the
    // dedupe threshold, and dropping them would end the stroke short of
    // where the pen actually stopped.
    let mut raw = vec![InkPoint::new(0.0, 0.0, 0.5)];
    for i in 1..=6 {
        raw.push(InkPoint::new(20.0 + i as f32 * 0.02, 0.0, 0.5));
    }

    let smoothed = streamline(&raw, 0.5);
    let last = smoothed.last().unwrap().pos;
    let target = raw.last().unwrap().pos;

    assert!(
        last.x > raw[0].pos.x,
        "the stroke must travel, not collapse to its start"
    );
    assert!(
        (last.x - target.x).abs() < 20.0,
        "the tail should track the final sample, got {} vs {}",
        last.x,
        target.x
    );
}

#[test]
fn streamline_drops_a_coincident_final_sample() {
    // Keeping an exactly duplicated endpoint would add a zero-length
    // segment with no direction for the outline walk to use.
    let raw = vec![
        InkPoint::new(0.0, 0.0, 0.5),
        InkPoint::new(10.0, 0.0, 0.5),
        InkPoint::new(10.0, 0.0, 0.5),
    ];
    let smoothed = streamline(&raw, 0.0);
    for pair in smoothed.windows(2) {
        assert!(
            pair[0].pos.dist(pair[1].pos) > 0.0,
            "no two consecutive points may coincide"
        );
    }
}

#[test]
fn streamline_preserves_endpoints_and_shortens_nothing() {
    let raw = horizontal(30, 0.7);
    let smoothed = streamline(&raw, 0.5);
    assert_eq!(
        smoothed[0].pos, raw[0].pos,
        "stroke must start where the pen landed"
    );
    assert!(smoothed.len() >= 2);
    // Smoothing lags, so the tail trails the raw input but must still track it.
    let last = smoothed.last().unwrap().pos;
    assert!(last.x <= raw.last().unwrap().pos.x + 0.01);
    assert!(
        last.x > raw[raw.len() / 2].pos.x,
        "smoothing must not stall the stroke"
    );
}

#[test]
fn taper_narrows_both_ends() {
    let opts = StrokeOptions::default();
    let total = 100.0;
    let mid = taper_at(50.0, total, &opts);
    let start = taper_at(0.0, total, &opts);
    let end = taper_at(total, total, &opts);

    assert!(
        (mid - 1.0).abs() < f32::EPSILON,
        "middle should be full width"
    );
    assert!(start < 0.01, "stroke should start from a point");
    assert!(end < 0.01, "stroke should end at a point");
}

#[test]
fn sharp_corner_does_not_spike() {
    // A hairpin turn: without a miter limit the join shoots off to infinity.
    let mut pts = Vec::new();
    for i in 0..10 {
        pts.push(InkPoint::new(i as f32 * 3.0, 0.0, 1.0));
    }
    for i in 1..10 {
        pts.push(InkPoint::new(27.0 - i as f32 * 3.0, 0.6, 1.0));
    }

    let opts = StrokeOptions {
        streamline: 0.0,
        ..StrokeOptions::default()
    };
    let poly = outline(&pts, &opts);
    let (min, max) = bounds(&poly).unwrap();

    // The path spans x in [0, 27]; a spike would blow the bounds far past
    // that plus the stroke radius.
    assert!(
        max.x < 27.0 + opts.size * MITER_LIMIT,
        "miter limit should clip the corner, got max.x={}",
        max.x
    );
    assert!(min.x > -opts.size * MITER_LIMIT);
}

#[test]
fn outline_is_centred_on_the_path() {
    let opts = StrokeOptions {
        taper_start: 0.0,
        taper_end: 0.0,
        streamline: 0.0,
        ..StrokeOptions::default()
    };
    let poly = outline(&horizontal(20, 1.0), &opts);
    let (min, max) = bounds(&poly).unwrap();
    let centre_y = (min.y + max.y) * 0.5;
    assert!(
        (centre_y - 50.0).abs() < 0.01,
        "stroke should straddle the pen path, centre was {centre_y}"
    );
}

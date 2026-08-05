//! Pure ink geometry: input smoothing and variable-width outline generation.
//!
//! A pressure-varying stroke cannot be drawn with a plain polyline stroke —
//! that gives one uniform width. Instead each stroke is turned into a *filled
//! polygon* that hugs the pen path, widening and narrowing with pressure. This
//! module owns that conversion.
//!
//! Deliberately free of `iced` and platform types so the whole pipeline is
//! unit testable; [`super::canvas`] converts the output into canvas geometry.

/// A 2D point in logical (scale-factor independent) pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y)
    }

    pub fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y)
    }

    pub fn scale(self, k: f32) -> Self {
        Self::new(self.x * k, self.y * k)
    }

    pub fn dot(self, o: Self) -> f32 {
        self.x * o.x + self.y * o.y
    }

    pub fn len(self) -> f32 {
        self.dot(self).sqrt()
    }

    pub fn dist(self, o: Self) -> f32 {
        self.sub(o).len()
    }

    /// Unit vector, or `None` when the vector is too short to have a direction.
    fn unit(self) -> Option<Self> {
        let len = self.len();
        if len > 1e-6 { Some(self.scale(1.0 / len)) } else { None }
    }

    /// Left-hand perpendicular.
    fn perp(self) -> Self {
        Self::new(-self.y, self.x)
    }

    fn lerp(self, o: Self, t: f32) -> Self {
        Self::new(self.x + (o.x - self.x) * t, self.y + (o.y - self.y) * t)
    }
}

/// One sample along a stroke: a position plus the pen pressure recorded there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InkPoint {
    pub pos: Vec2,
    /// Normalized pen pressure in `0.0..=1.0`.
    pub pressure: f32,
}

impl InkPoint {
    pub const fn new(x: f32, y: f32, pressure: f32) -> Self {
        Self {
            pos: Vec2::new(x, y),
            pressure,
        }
    }
}

/// Tuning for how raw pen samples become a drawn outline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeOptions {
    /// Nominal stroke diameter in logical pixels, at full pressure.
    pub size: f32,
    /// How strongly pressure drives width, `0.0` (uniform) to `1.0` (a light
    /// touch nearly vanishes).
    pub thinning: f32,
    /// Input smoothing, `0.0` (raw samples) to `1.0` (heavily lagged). Tames
    /// digitizer jitter at the cost of a little responsiveness.
    pub streamline: f32,
    /// Length in pixels over which the stroke widens from nothing at the start.
    pub taper_start: f32,
    /// Length in pixels over which the stroke narrows to nothing at the end.
    pub taper_end: f32,
    /// Floor on the half-width, so a feather-light touch still marks the page.
    pub min_radius: f32,
}

impl Default for StrokeOptions {
    fn default() -> Self {
        Self {
            size: 3.2,
            thinning: 0.62,
            streamline: 0.42,
            taper_start: 1.6,
            taper_end: 5.0,
            min_radius: 0.28,
        }
    }
}

/// Miter joins are extended by at most this factor before being clipped, so a
/// hairpin turn produces a blunt corner rather than a long spike.
const MITER_LIMIT: f32 = 2.4;

/// Segments used to draw each round cap. Handwriting strokes are small on
/// screen, so a coarse fan is indistinguishable from a true arc.
const CAP_SEGMENTS: usize = 10;

/// Samples closer together than this are treated as duplicates. Keeps
/// degenerate direction vectors out of the outline walk.
const MIN_SAMPLE_SPACING: f32 = 0.08;

/// Apply the exponential input smoothing described by
/// [`StrokeOptions::streamline`], dropping samples that land on top of each
/// other.
///
/// This is the same idea as a low-pass filter on the digitizer signal: each
/// sample is pulled toward its predecessor, so high-frequency jitter is damped
/// while the overall path is preserved. Pressure is smoothed alongside
/// position — raw pressure is noisy enough to produce visible width chatter.
pub fn streamline(raw: &[InkPoint], streamline: f32) -> Vec<InkPoint> {
    if raw.is_empty() {
        return Vec::new();
    }

    // Map the 0..1 knob onto a usable filter coefficient. At `streamline == 0`
    // samples pass through untouched; at 1.0 each new sample contributes 15%.
    let t = 1.0 - streamline.clamp(0.0, 1.0) * 0.85;

    let mut out: Vec<InkPoint> = Vec::with_capacity(raw.len());
    out.push(raw[0]);

    let last = raw.len() - 1;

    for (offset, sample) in raw[1..].iter().enumerate() {
        let prev = *out.last().expect("out is seeded with raw[0]");
        let pos = prev.pos.lerp(sample.pos, t);
        let pressure = prev.pressure + (sample.pressure - prev.pressure) * t;

        // The final sample is kept even when it lands close to its
        // predecessor, so a stroke ends where the pen was actually lifted
        // rather than short of it — a slow, deliberate finish otherwise loses
        // its tail. Coincident points are still dropped either way: they add a
        // zero-length segment that carries no direction.
        let threshold = if offset + 1 == last {
            f32::EPSILON
        } else {
            MIN_SAMPLE_SPACING
        };
        if pos.dist(prev.pos) <= threshold {
            continue;
        }

        out.push(InkPoint { pos, pressure });
    }

    out
}

/// Half-width of the stroke at `pressure`, before tapering.
fn radius_at(pressure: f32, opts: &StrokeOptions) -> f32 {
    let base = opts.size * 0.5;
    if opts.thinning <= f32::EPSILON {
        return base.max(opts.min_radius);
    }
    // A square-root response keeps light strokes legible: raw pressure spends
    // most of its range near the bottom, so a linear map makes ordinary
    // handwriting look uniformly thin.
    let eased = pressure.clamp(0.0, 1.0).sqrt();
    let thinning = opts.thinning.clamp(0.0, 1.0);
    let scale = (1.0 - thinning) + eased * thinning;
    (base * scale).max(opts.min_radius)
}

/// Taper multiplier for a point `from_start` along a stroke of `total` length.
fn taper_at(from_start: f32, total: f32, opts: &StrokeOptions) -> f32 {
    // Short strokes (a dot on an `i`, a comma) would otherwise be tapered into
    // near-invisibility from both ends at once.
    let start_len = opts.taper_start.min(total * 0.5);
    let end_len = opts.taper_end.min(total * 0.5);

    let mut k: f32 = 1.0;
    if start_len > f32::EPSILON {
        k = k.min((from_start / start_len).clamp(0.0, 1.0).sqrt());
    }
    if end_len > f32::EPSILON {
        let from_end = total - from_start;
        k = k.min((from_end / end_len).clamp(0.0, 1.0).sqrt());
    }
    k
}

/// Generate the closed outline polygon for a stroke.
///
/// The returned points wind continuously around the stroke: up one side, round
/// the end cap, back down the other, round the start cap. Filling this polygon
/// draws the stroke.
///
/// Returns an empty vector for an empty input.
pub fn outline(raw: &[InkPoint], opts: &StrokeOptions) -> Vec<Vec2> {
    let pts = streamline(raw, opts.streamline);

    match pts.len() {
        0 => return Vec::new(),
        // A tap with no travel is a dot; draw it as a filled circle so dotting
        // an `i` leaves a mark rather than nothing.
        1 => {
            let r = radius_at(pts[0].pressure, opts).max(opts.min_radius);
            return circle(pts[0].pos, r);
        }
        _ => {}
    }

    // Arc length up to each point, used for tapering.
    let mut distances = Vec::with_capacity(pts.len());
    let mut running = 0.0;
    distances.push(0.0);
    for pair in pts.windows(2) {
        running += pair[0].pos.dist(pair[1].pos);
        distances.push(running);
    }
    let total = running;

    // Degenerate case: every sample landed on the same spot.
    if total <= MIN_SAMPLE_SPACING {
        let r = radius_at(pts[0].pressure, opts).max(opts.min_radius);
        return circle(pts[0].pos, r);
    }

    let n = pts.len();
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);

    for i in 0..n {
        let p = pts[i].pos;
        let radius =
            (radius_at(pts[i].pressure, opts) * taper_at(distances[i], total, opts)).max(0.01);

        // Direction of travel through this point. Interior points use the
        // bisector of the incoming and outgoing segments so joins stay smooth.
        let incoming = if i > 0 { pts[i].pos.sub(pts[i - 1].pos).unit() } else { None };
        let outgoing = if i + 1 < n { pts[i + 1].pos.sub(pts[i].pos).unit() } else { None };

        let (offset_dir, miter) = match (incoming, outgoing) {
            (Some(a), Some(b)) => {
                // Miter: offsetting both segments by `radius` puts their outer
                // edges' intersection along the bisector, further out than
                // `radius` by 1/cos(half-angle).
                match a.add(b).unit() {
                    Some(bisector) => {
                        let normal = bisector.perp();
                        let cos_half = normal.dot(a.perp()).abs().max(1.0 / MITER_LIMIT);
                        (normal, 1.0 / cos_half)
                    }
                    // Exact reversal (a hairpin): fall back to the incoming
                    // perpendicular; the caps and neighbouring joins cover it.
                    None => (a.perp(), 1.0),
                }
            }
            (Some(a), None) => (a.perp(), 1.0),
            (None, Some(b)) => (b.perp(), 1.0),
            (None, None) => (Vec2::new(0.0, 1.0), 1.0),
        };

        let offset = offset_dir.scale(radius * miter);
        left.push(p.add(offset));
        right.push(p.sub(offset));
    }

    let start_radius = left[0].dist(right[0]) * 0.5;
    let end_radius = left[n - 1].dist(right[n - 1]) * 0.5;

    let mut poly = Vec::with_capacity(2 * n + 2 * CAP_SEGMENTS);
    poly.extend_from_slice(&left);
    poly.extend(arc(pts[n - 1].pos, left[n - 1], right[n - 1], end_radius));
    poly.extend(right.iter().rev().copied());
    poly.extend(arc(pts[0].pos, right[0], left[0], start_radius));
    poly
}

/// Points along the shorter arc from `from` to `to` around `center`, excluding
/// both endpoints (the caller already has them).
fn arc(center: Vec2, from: Vec2, to: Vec2, radius: f32) -> Vec<Vec2> {
    if radius <= f32::EPSILON {
        return Vec::new();
    }

    let start = (from.y - center.y).atan2(from.x - center.x);
    let end = (to.y - center.y).atan2(to.x - center.x);

    // Take the sweep that goes the short way around.
    let mut sweep = end - start;
    while sweep > std::f32::consts::PI {
        sweep -= std::f32::consts::TAU;
    }
    while sweep < -std::f32::consts::PI {
        sweep += std::f32::consts::TAU;
    }

    (1..CAP_SEGMENTS)
        .map(|i| {
            let angle = start + sweep * (i as f32 / CAP_SEGMENTS as f32);
            Vec2::new(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            )
        })
        .collect()
}

/// A closed circle polygon, used for dots and degenerate strokes.
fn circle(center: Vec2, radius: f32) -> Vec<Vec2> {
    let segments = CAP_SEGMENTS * 2;
    (0..segments)
        .map(|i| {
            let angle = std::f32::consts::TAU * (i as f32 / segments as f32);
            Vec2::new(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            )
        })
        .collect()
}

/// Axis-aligned bounds of a point set, as `(min, max)`.
///
/// Used to cull strokes outside the viewport before tessellating them, and to
/// hit-test a lasso selection cheaply.
pub fn bounds(points: &[Vec2]) -> Option<(Vec2, Vec2)> {
    let first = *points.first()?;
    let mut min = first;
    let mut max = first;
    for p in &points[1..] {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
    }
    Some((min, max))
}

/// Whether two axis-aligned boxes overlap, each given as `(min, max)`.
pub fn boxes_overlap(a: (Vec2, Vec2), b: (Vec2, Vec2)) -> bool {
    a.0.x <= b.1.x && a.1.x >= b.0.x && a.0.y <= b.1.y && a.1.y >= b.0.y
}

#[cfg(test)]
mod tests {
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
        assert_eq!(smoothed[0].pos, raw[0].pos, "stroke must start where the pen landed");
        assert!(smoothed.len() >= 2);
        // Smoothing lags, so the tail trails the raw input but must still track it.
        let last = smoothed.last().unwrap().pos;
        assert!(last.x <= raw.last().unwrap().pos.x + 0.01);
        assert!(last.x > raw[raw.len() / 2].pos.x, "smoothing must not stall the stroke");
    }

    #[test]
    fn taper_narrows_both_ends() {
        let opts = StrokeOptions::default();
        let total = 100.0;
        let mid = taper_at(50.0, total, &opts);
        let start = taper_at(0.0, total, &opts);
        let end = taper_at(total, total, &opts);

        assert!((mid - 1.0).abs() < f32::EPSILON, "middle should be full width");
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
}

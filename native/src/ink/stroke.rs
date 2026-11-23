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
        if len > 1e-6 {
            Some(self.scale(1.0 / len))
        } else {
            None
        }
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
        let incoming = if i > 0 {
            pts[i].pos.sub(pts[i - 1].pos).unit()
        } else {
            None
        };
        let outgoing = if i + 1 < n {
            pts[i + 1].pos.sub(pts[i].pos).unit()
        } else {
            None
        };

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
mod tests;

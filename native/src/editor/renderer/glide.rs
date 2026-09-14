//! The caret's glide and blink.
//!
//! The caret is painted where layout puts it plus an offset that eases away.
//! When the caret moves a short way the offset starts out as the distance
//! moved, so the eye sees it travel rather than teleport. The offset is a
//! critically damped spring sampled in closed form ([`Spring`]), so it looks
//! the same at any frame rate.
//!
//! A caret at rest blinks with a soft fade instead of flashing, and after a
//! number of blinks without activity it stays lit, so an idle editor stops
//! asking for frames.

use std::f32::consts::TAU;
use std::time::{Duration, Instant};

use iced::{Point, Rectangle, Vector};

use super::flow::Affinity;
use super::metrics::{content_bounds, text_left};
use super::{Editor, Measure, State};
use crate::motion::Spring;

/// How quickly the caret catches up with its place: about 60ms to cover 95%
/// of a move.
const GLIDE_STIFFNESS: f32 = 80.0;
/// A move further than this, vertically, cuts instead of gliding.
const MAX_GLIDE: f32 = 96.0;
/// The caret stays lit this long after it last moved.
const BLINK_DELAY: Duration = Duration::from_millis(500);
/// One fade out and back in.
const BLINK_PERIOD: Duration = Duration::from_millis(1060);
/// Blinks before an idle caret stays lit for good.
const BLINK_CYCLES: f32 = 18.0;
/// How far the blink's cosine overshoots full and zero opacity before it is
/// clamped: the higher, the longer each is held and the quicker the fades.
const BLINK_SHARPNESS: f32 = 2.5;

/// The caret's position, and where layout puts it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CaretPlace {
    pub line: usize,
    pub col: usize,
    pub affinity: Affinity,
    /// Top-left of the caret relative to the widget, ignoring block scrolling.
    pub at: Point,
}

impl CaretPlace {
    fn same_position(&self, other: &Self) -> bool {
        (self.line, self.col, self.affinity) == (other.line, other.col, other.affinity)
    }
}

/// When the caret next needs a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NextFrame {
    Now,
    At(Instant),
    Never,
}

/// The caret's motion from frame to frame.
#[derive(Debug, Default)]
pub(super) struct CaretMotion {
    /// When the frame being drawn was requested.
    frame: Option<Instant>,
    /// Where the caret was in the previous frame.
    last: Option<CaretPlace>,
    /// How far the painted caret trails its place, per axis.
    offset: Option<(Spring, Spring)>,
    /// When the caret last moved.
    moved_at: Option<Instant>,
}

impl CaretMotion {
    /// Take in where the caret is for the frame at `now` — `None` when no
    /// caret is shown — and say when it next needs a frame.
    pub fn advance(&mut self, now: Instant, place: Option<CaretPlace>) -> NextFrame {
        let Some(place) = place else {
            *self = Self {
                frame: Some(now),
                ..Self::default()
            };
            return NextFrame::Never;
        };
        self.frame = Some(now);
        match self.last {
            Some(last) if !last.same_position(&place) => {
                let moved = last.at - place.at;
                let previous = self.offset;
                self.offset = (moved.y.abs() <= MAX_GLIDE)
                    .then(|| {
                        let rest = Spring::at_rest(0.0, now, GLIDE_STIFFNESS);
                        let (mut x, mut y) = previous.unwrap_or((rest, rest));
                        x.shift(moved.x, now);
                        y.shift(moved.y, now);
                        (x, y)
                    })
                    // Quick moves add up; a caret trailing that far behind cuts.
                    .filter(|(_, y)| y.value(now).abs() <= MAX_GLIDE);
                self.moved_at = Some(now);
            }
            Some(_) => {}
            None => self.moved_at = Some(now),
        }
        self.last = Some(place);

        if let Some((x, y)) = self.offset {
            if !(x.is_settled(now) && y.is_settled(now)) {
                return NextFrame::Now;
            }
            self.offset = None;
        }
        let since = now.saturating_duration_since(self.moved_at.unwrap_or(now));
        match blink_wait(since) {
            Some(wait) if wait.is_zero() => NextFrame::Now,
            Some(wait) => NextFrame::At(now + wait),
            None => NextFrame::Never,
        }
    }

    /// Offset of the painted caret from its place in the frame being drawn.
    pub fn offset(&self) -> Vector {
        match (self.frame, self.offset) {
            (Some(now), Some((x, y))) => Vector::new(x.value(now), y.value(now)),
            _ => Vector::ZERO,
        }
    }

    /// Opacity of the caret in the frame being drawn.
    pub fn alpha(&self) -> f32 {
        match (self.frame, self.moved_at) {
            (Some(now), Some(moved_at)) => blink_alpha(now.saturating_duration_since(moved_at)),
            _ => 1.0,
        }
    }
}

/// Phase within a blink period where a fade out begins: where the stretched
/// cosine drops back below full opacity.
fn fade_start() -> f32 {
    (1.0 / BLINK_SHARPNESS).acos() / TAU
}

/// How far into its blinking the caret is, in periods, `since` it moved, or
/// `None` while it is held lit.
fn blink_phase(since: Duration) -> Option<f32> {
    let t = since.checked_sub(BLINK_DELAY)?.as_secs_f32() / BLINK_PERIOD.as_secs_f32();
    (t < BLINK_CYCLES).then_some(t)
}

/// Caret opacity `since` it last moved.
fn blink_alpha(since: Duration) -> f32 {
    blink_phase(since).map_or(1.0, |t| {
        (0.5 + 0.5 * BLINK_SHARPNESS * (TAU * t).cos()).clamp(0.0, 1.0)
    })
}

/// How long until the blink next changes the caret's opacity: zero while it
/// is fading, `None` once it has stopped for good.
fn blink_wait(since: Duration) -> Option<Duration> {
    let edge = fade_start();
    let Some(t) = blink_phase(since) else {
        return (since < BLINK_DELAY).then(|| BLINK_DELAY - since + BLINK_PERIOD.mul_f32(edge));
    };
    let cycle = t.floor();
    let phase = t - cycle;
    // Opacity changes between the first two marks and between the last two.
    let marks = [edge, 0.5 - edge, 0.5 + edge, 1.0 - edge];
    if (marks[0]..marks[1]).contains(&phase) || (marks[2]..marks[3]).contains(&phase) {
        return Some(Duration::ZERO);
    }
    let next = marks.into_iter().find(|&m| m > phase).unwrap_or(1.0 + edge);
    // The last blink ends lit, where the next fade would have begun.
    let next = (cycle + next).min(BLINK_CYCLES);
    Some(BLINK_PERIOD.mul_f32(next - t))
}

impl<Message> Editor<'_, Message> {
    /// Where the caret is for the frame about to be drawn, if one is shown
    /// within `viewport`.
    pub(super) fn caret_place<R: Measure>(
        &self,
        state: &State,
        layout_bounds: Rectangle,
        viewport: Rectangle,
    ) -> Option<CaretPlace> {
        if !state.is_focused {
            return None;
        }
        let line_idx = self.buffer.cursor_line;
        let line = self.lines.get(line_idx)?;
        let bounds = content_bounds(layout_bounds);
        let (col, affinity) = (self.buffer.cursor_col, self.buffer.cursor_affinity);
        let caret = self.caret_box::<R>(
            line_idx,
            col,
            affinity,
            bounds.width,
            self.is_block_editing(line, true),
            self.active_col(line_idx, true),
        );
        let top = self.line_body_top(line_idx, state) + caret.y;
        let viewport_top = viewport.y - layout_bounds.y;
        if top + caret.height < viewport_top || top > viewport_top + viewport.height {
            return None;
        }
        Some(CaretPlace {
            line: line_idx,
            col,
            affinity,
            at: Point::new(
                bounds.x - layout_bounds.x + text_left(bounds.width) + caret.x,
                top,
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(line: usize, col: usize, x: f32, y: f32) -> Option<CaretPlace> {
        Some(CaretPlace {
            line,
            col,
            affinity: Affinity::Downstream,
            at: Point::new(x, y),
        })
    }

    fn after(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    #[test]
    fn a_short_move_starts_where_the_caret_was_and_settles_where_it_is() {
        let t = Instant::now();
        let mut motion = CaretMotion::default();
        motion.advance(t, place(0, 0, 10.0, 0.0));
        assert_eq!(motion.offset(), Vector::ZERO);

        let moved = place(0, 5, 60.0, 0.0);
        assert_eq!(motion.advance(after(t, 16), moved), NextFrame::Now);
        assert_eq!(motion.offset(), Vector::new(-50.0, 0.0));

        let mut previous = -50.0;
        for frame in 2..12 {
            motion.advance(after(t, 16 * frame), moved);
            let x = motion.offset().x;
            assert!((previous..=0.0).contains(&x), "frame {frame}: {x}");
            previous = x;
        }
        assert_ne!(motion.advance(after(t, 1000), moved), NextFrame::Now);
        assert_eq!(motion.offset(), Vector::ZERO);
    }

    #[test]
    fn short_moves_that_add_up_to_a_long_one_cut() {
        let t = Instant::now();
        let mut motion = CaretMotion::default();
        motion.advance(t, place(0, 0, 10.0, 0.0));
        motion.advance(after(t, 1), place(1, 0, 10.0, 70.0));
        assert_eq!(motion.offset(), Vector::new(0.0, -70.0));
        motion.advance(after(t, 2), place(2, 0, 10.0, 140.0));
        assert_eq!(motion.offset(), Vector::ZERO);
    }

    #[test]
    fn a_long_move_cuts() {
        let t = Instant::now();
        let mut motion = CaretMotion::default();
        motion.advance(t, place(0, 0, 10.0, 0.0));
        motion.advance(after(t, 16), place(9, 0, 10.0, 400.0));
        assert_eq!(motion.offset(), Vector::ZERO);
    }

    #[test]
    fn layout_moving_under_a_still_caret_does_not_glide() {
        let t = Instant::now();
        let mut motion = CaretMotion::default();
        motion.advance(t, place(3, 2, 10.0, 100.0));
        motion.advance(after(t, 16), place(3, 2, 10.0, 180.0));
        assert_eq!(motion.offset(), Vector::ZERO);
    }

    #[test]
    fn the_caret_holds_then_fades_softly_then_rests_lit() {
        let period = BLINK_PERIOD.as_millis() as u64;
        let delay = BLINK_DELAY.as_millis() as u64;
        let at = |millis: u64| blink_alpha(Duration::from_millis(millis));
        assert_eq!(at(0), 1.0);
        assert_eq!(at(delay - 1), 1.0);
        assert_eq!(at(delay + period / 2), 0.0);
        let midway = at(delay + period / 4);
        assert!(midway > 0.0 && midway < 1.0, "{midway}");
        let end = delay + (BLINK_CYCLES as u64) * period;
        assert_eq!(at(end + 1), 1.0);
        assert_eq!(blink_wait(Duration::from_millis(end + 1)), None);
    }

    /// Frames are asked for exactly while the opacity changes: a wait never
    /// skips a change, and never lands before one is due.
    #[test]
    fn frames_are_asked_for_only_while_the_caret_fades() {
        let mut since = Duration::ZERO;
        let end = BLINK_DELAY + BLINK_PERIOD.mul_f32(BLINK_CYCLES);
        while let Some(wait) = blink_wait(since) {
            if wait.is_zero() {
                since += Duration::from_millis(4);
                continue;
            }
            let held = blink_alpha(since);
            for k in 1..8 {
                let probe = since + wait.mul_f32(k as f32 / 8.0);
                assert!(
                    (blink_alpha(probe) - held).abs() < 1e-3,
                    "opacity changed during a wait at {probe:?}"
                );
            }
            since += wait;
            assert!(since <= end + Duration::from_millis(1));
        }
        assert!(since >= end - Duration::from_millis(5));
    }

    #[test]
    fn no_caret_needs_no_frames() {
        let mut motion = CaretMotion::default();
        assert_eq!(motion.advance(Instant::now(), None), NextFrame::Never);
        assert_eq!(motion.offset(), Vector::ZERO);
        assert_eq!(motion.alpha(), 1.0);
    }
}

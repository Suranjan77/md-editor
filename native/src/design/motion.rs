//! Motion primitives (UXC.T3): a tiny tween system driven by the app's
//! existing frame redraws.
//!
//! Calm-by-default contract: a finished tween reports `needs_redraw() ==
//! false`, so subscriptions can sleep when nothing animates — zero redraw
//! requests when fully idle. "Reduce motion" is honored by constructing
//! tweens with [`Tween::instant`].

#![allow(dead_code)] // consumers land with UX-C; math is tested below

use std::time::{Duration, Instant};

/// Easing curves matching the design-token motion language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Easing {
    /// Constant velocity.
    Linear,
    /// Decelerating (standard for movement into place: caret, scroll-to).
    #[default]
    EaseOutCubic,
    /// Accelerate-then-decelerate (panel open/close, cross-fades).
    EaseInOutCubic,
}

impl Easing {
    /// Map linear progress `t ∈ [0, 1]` to eased progress.
    pub(crate) fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
            Easing::EaseInOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
        }
    }
}

/// An f32 value animating from `start` to `target` over `duration`.
#[derive(Debug, Clone)]
pub(crate) struct Tween {
    start: f32,
    target: f32,
    started_at: Instant,
    duration: Duration,
    easing: Easing,
}

impl Tween {
    /// Animate from `start` to `target` over `duration`.
    pub(crate) fn new(
        start: f32,
        target: f32,
        duration: Duration,
        easing: Easing,
        now: Instant,
    ) -> Self {
        Self {
            start,
            target,
            started_at: now,
            duration,
            easing,
        }
    }

    /// A tween that is already complete (reduce-motion path / large jumps).
    pub(crate) fn instant(target: f32, now: Instant) -> Self {
        Self::new(target, target, Duration::ZERO, Easing::Linear, now)
    }

    /// Retarget mid-flight, continuing smoothly from the current value.
    pub(crate) fn retarget(&mut self, target: f32, duration: Duration, now: Instant) {
        self.start = self.value(now);
        self.target = target;
        self.duration = duration;
        self.started_at = now;
    }

    /// Current value at `now`.
    pub(crate) fn value(&self, now: Instant) -> f32 {
        let progress = self.progress(now);
        self.start + (self.target - self.start) * self.easing.apply(progress)
    }

    /// Final value.
    pub(crate) fn target(&self) -> f32 {
        self.target
    }

    /// Linear progress in `[0, 1]`.
    fn progress(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return 1.0;
        }
        let elapsed = now.saturating_duration_since(self.started_at);
        (elapsed.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0)
    }

    /// Whether the tween has reached its target.
    pub(crate) fn is_complete(&self, now: Instant) -> bool {
        self.progress(now) >= 1.0
    }

    /// Whether a frame redraw is still required (false once complete — the
    /// sleep-when-idle contract).
    pub(crate) fn needs_redraw(&self, now: Instant) -> bool {
        !self.is_complete(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn linear_tween_interpolates_proportionally() {
        let t0 = Instant::now();
        let tween = Tween::new(0.0, 100.0, Duration::from_millis(100), Easing::Linear, t0);
        assert_eq!(tween.value(t0), 0.0);
        assert!((tween.value(at(t0, 50)) - 50.0).abs() < 0.5);
        assert_eq!(tween.value(at(t0, 100)), 100.0);
        assert_eq!(tween.value(at(t0, 500)), 100.0); // clamps past end
    }

    #[test]
    fn ease_out_cubic_starts_fast_and_settles() {
        let half = Easing::EaseOutCubic.apply(0.5);
        assert!(half > 0.5, "ease-out should be ahead of linear at midpoint");
        assert_eq!(Easing::EaseOutCubic.apply(0.0), 0.0);
        assert_eq!(Easing::EaseOutCubic.apply(1.0), 1.0);
    }

    #[test]
    fn ease_in_out_is_symmetric_and_bounded() {
        let e = Easing::EaseInOutCubic;
        assert_eq!(e.apply(0.0), 0.0);
        assert_eq!(e.apply(1.0), 1.0);
        assert!((e.apply(0.5) - 0.5).abs() < 1e-6);
        let early = e.apply(0.25);
        let late = e.apply(0.75);
        assert!((early + (1.0 - late)).abs() - 0.5 < 1e-3);
        assert!(early < 0.25, "ease-in-out starts slow");
    }

    #[test]
    fn easing_clamps_out_of_range_progress() {
        for e in [Easing::Linear, Easing::EaseOutCubic, Easing::EaseInOutCubic] {
            assert_eq!(e.apply(-1.0), 0.0);
            assert_eq!(e.apply(2.0), 1.0);
        }
    }

    #[test]
    fn instant_tween_never_requests_redraw() {
        let t0 = Instant::now();
        let tween = Tween::instant(42.0, t0);
        assert_eq!(tween.value(t0), 42.0);
        assert!(tween.is_complete(t0));
        assert!(!tween.needs_redraw(t0)); // sleep-when-idle: no busy redraw
    }

    #[test]
    fn completed_tween_sleeps() {
        let t0 = Instant::now();
        let tween = Tween::new(
            0.0,
            10.0,
            Duration::from_millis(90),
            Easing::EaseOutCubic,
            t0,
        );
        assert!(tween.needs_redraw(at(t0, 45)));
        assert!(!tween.needs_redraw(at(t0, 90)));
        assert!(!tween.needs_redraw(at(t0, 10_000)));
    }

    #[test]
    fn retarget_continues_from_current_value_without_jump() {
        let t0 = Instant::now();
        let mut tween = Tween::new(0.0, 100.0, Duration::from_millis(100), Easing::Linear, t0);
        let mid = at(t0, 50);
        let value_before = tween.value(mid);
        tween.retarget(0.0, Duration::from_millis(100), mid);
        // No discontinuity at the retarget instant.
        assert!((tween.value(mid) - value_before).abs() < 1e-3);
        assert_eq!(tween.target(), 0.0);
        assert_eq!(tween.value(at(t0, 150)), 0.0);
    }
}

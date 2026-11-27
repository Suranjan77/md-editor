//! Motion durations, exit scaling, reduced-motion gate, and spring/panel animations (MASTER §5).

use std::time::{Duration, Instant};

use iced::Animation;
use iced::animation::Easing;

use super::metrics::{BACKLINKS_DEFAULT_WIDTH, SIDEBAR_DEFAULT_WIDTH, TOC_DEFAULT_WIDTH};

pub const EASE: Easing = Easing::EaseOutQuad;

pub const INSTANT: Duration = Duration::ZERO;
/// Longest gap between clicks that still counts as a double (or triple) click.
pub const MULTI_CLICK: Duration = Duration::from_millis(400);
pub const FAST: Duration = Duration::from_millis(100);
pub const BASE: Duration = Duration::from_millis(160);
pub const SLOW: Duration = Duration::from_millis(220);

/// Exit transitions run at ×0.65 of their entry duration (MASTER §5).
pub const fn exit_duration(enter: Duration) -> Duration {
    Duration::from_micros((enter.as_micros() as u64 * 65) / 100)
}

/// User preference for reduced motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReducedMotionPref {
    System,
    Off,
    On,
}

impl ReducedMotionPref {
    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "on" | "true" | "1" => Self::On,
            "off" | "false" | "0" => Self::Off,
            _ => Self::System,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Off => "off",
            Self::On => "on",
        }
    }
}

static REDUCED_MOTION: super::setting::Setting = super::setting::Setting::new(0);
static REDUCED_MOTION_PREF: super::setting::Setting = super::setting::Setting::new(0);

/// The user's reduced-motion preference (Settings → Appearance).
pub fn reduced_motion_pref() -> ReducedMotionPref {
    match REDUCED_MOTION_PREF.get() {
        1 => ReducedMotionPref::Off,
        2 => ReducedMotionPref::On,
        _ => ReducedMotionPref::System,
    }
}

pub fn set_reduced_motion_pref(pref: ReducedMotionPref) {
    let raw = match pref {
        ReducedMotionPref::System => 0,
        ReducedMotionPref::Off => 1,
        ReducedMotionPref::On => 2,
    };
    REDUCED_MOTION_PREF.replace(raw);
}

/// Resolve the preference against the OS setting and switch motion on or off.
pub fn apply_reduced_motion(os_prefers_reduced: bool) {
    set_reduced_motion(match reduced_motion_pref() {
        ReducedMotionPref::System => os_prefers_reduced,
        ReducedMotionPref::Off => false,
        ReducedMotionPref::On => true,
    });
}

pub fn reduced_motion() -> bool {
    REDUCED_MOTION.get() != 0
}

pub fn set_reduced_motion(enabled: bool) {
    REDUCED_MOTION.replace(u64::from(enabled));
}

/// Effective duration after checking the reduced-motion switch.
pub fn duration(base: Duration) -> Duration {
    if reduced_motion() { INSTANT } else { base }
}

/// Animated mirrors of the app's UI-visibility flags and persisted panel widths.
pub struct Motion {
    pub sidebar: Animation<bool>,
    pub toc: Animation<bool>,
    pub backlinks: Animation<bool>,
    pub toast: Animation<bool>,
    pub palette: Animation<bool>,
    pub editor_scroll: Option<ScrollGlide>,
    pub now: Instant,
    pub toast_text: String,
    pub sidebar_target_width: f32,
    pub toc_target_width: f32,
    pub backlinks_target_width: f32,
}

impl Motion {
    pub fn new() -> Self {
        let panel_dur = duration(BASE);
        let fast_dur = duration(FAST);
        let slow_dur = duration(SLOW);
        Self {
            sidebar: Animation::new(true).easing(EASE).duration(panel_dur),
            toc: Animation::new(false).easing(EASE).duration(panel_dur),
            backlinks: Animation::new(false).easing(EASE).duration(panel_dur),
            toast: Animation::new(false).easing(EASE).duration(fast_dur),
            palette: Animation::new(false).easing(EASE).duration(slow_dur),
            editor_scroll: None,
            now: Instant::now(),
            toast_text: String::new(),
            sidebar_target_width: SIDEBAR_DEFAULT_WIDTH,
            toc_target_width: TOC_DEFAULT_WIDTH,
            backlinks_target_width: BACKLINKS_DEFAULT_WIDTH,
        }
    }

    pub fn is_animating(&self) -> bool {
        if reduced_motion() {
            return false;
        }
        let at = self.now;
        self.sidebar.is_animating(at)
            || self.toc.is_animating(at)
            || self.backlinks.is_animating(at)
            || self.toast.is_animating(at)
            || self.palette.is_animating(at)
            || self.editor_scroll.is_some()
    }

    pub fn sync(
        &mut self,
        sidebar: bool,
        toc: bool,
        backlinks: bool,
        toast: Option<&str>,
        palette: bool,
    ) {
        if let Some(text) = toast {
            self.toast_text = text.to_string();
        }
        let toast_on = toast.is_some();
        let now = Instant::now();
        let instant = reduced_motion();

        for (animation, target, enter) in [
            (&mut self.sidebar, sidebar, BASE),
            (&mut self.toc, toc, BASE),
            (&mut self.backlinks, backlinks, BASE),
            (&mut self.toast, toast_on, FAST),
            (&mut self.palette, palette, SLOW),
        ] {
            if animation.value() != target {
                let dur = if instant {
                    INSTANT
                } else if target {
                    enter
                } else {
                    exit_duration(enter)
                };
                *animation = std::mem::replace(animation, Animation::new(target)).duration(dur);
                animation.go_mut(target, now);
            }
        }
        if instant {
            self.editor_scroll = None;
        }
        self.now = now;
    }

    pub fn sidebar_width(&self) -> f32 {
        if reduced_motion() {
            return if self.sidebar.value() {
                self.sidebar_target_width
            } else {
                0.0
            };
        }
        self.sidebar
            .interpolate(0.0, self.sidebar_target_width, self.now)
    }

    pub fn toc_width(&self) -> f32 {
        if reduced_motion() {
            return if self.toc.value() {
                self.toc_target_width
            } else {
                0.0
            };
        }
        self.toc.interpolate(0.0, self.toc_target_width, self.now)
    }

    pub fn backlinks_width(&self) -> f32 {
        if reduced_motion() {
            return if self.backlinks.value() {
                self.backlinks_target_width
            } else {
                0.0
            };
        }
        self.backlinks
            .interpolate(0.0, self.backlinks_target_width, self.now)
    }

    pub fn toast_opacity(&self) -> f32 {
        if reduced_motion() {
            return if self.toast.value() { 1.0 } else { 0.0 };
        }
        self.toast.interpolate(0.0, 1.0, self.now)
    }

    pub fn palette_opacity(&self) -> f32 {
        if reduced_motion() {
            return if self.palette.value() { 1.0 } else { 0.0 };
        }
        self.palette.interpolate(0.0, 1.0, self.now)
    }
}

impl Default for Motion {
    fn default() -> Self {
        Self::new()
    }
}

pub const SCROLL_STIFFNESS: f32 = 32.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollGlide {
    pub spring: Spring,
    pub applied: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    target: f32,
    x0: f32,
    v0: f32,
    start: Instant,
    stiffness: f32,
}

impl Spring {
    const SETTLED: f32 = 0.1;

    /// A spring resting at `value`.
    pub fn at_rest(value: f32, now: Instant, stiffness: f32) -> Self {
        Self {
            target: value,
            x0: 0.0,
            v0: 0.0,
            start: now,
            stiffness,
        }
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    /// Position and velocity at `now`.
    pub fn sample(&self, now: Instant) -> (f32, f32) {
        if reduced_motion() {
            return (self.target, 0.0);
        }
        let t = now.saturating_duration_since(self.start).as_secs_f32();
        let w = self.stiffness;
        let b = self.v0 + w * self.x0;
        let decay = (-w * t).exp();
        let x = (self.x0 + b * t) * decay;
        let v = (self.v0 - w * b * t) * decay;
        (self.target + x, v)
    }

    pub fn value(&self, now: Instant) -> f32 {
        self.sample(now).0
    }

    /// Head for `target` from wherever the spring is at `now`, keeping its
    /// velocity, so a new target bends the path instead of kinking it.
    pub fn retarget(&mut self, target: f32, now: Instant) {
        if reduced_motion() {
            *self = Self::at_rest(target, now, self.stiffness);
            return;
        }
        let (position, velocity) = self.sample(now);
        *self = Self {
            target,
            x0: position - target,
            v0: velocity,
            start: now,
            stiffness: self.stiffness,
        };
    }

    /// Displace the spring by `delta` at `now`, keeping its velocity: the
    /// target stays, and the spring sets off again from further away.
    pub fn shift(&mut self, delta: f32, now: Instant) {
        if reduced_motion() {
            return;
        }
        let (position, velocity) = self.sample(now);
        *self = Self {
            x0: position + delta - self.target,
            v0: velocity,
            start: now,
            ..*self
        };
    }

    /// Whether the remaining distance and momentum are below what can be seen.
    pub fn is_settled(&self, now: Instant) -> bool {
        if reduced_motion() {
            return true;
        }
        let (position, velocity) = self.sample(now);
        (position - self.target).abs() + velocity.abs() / self.stiffness < Self::SETTLED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The clip width each panel animates to must match the width the panel
    /// itself lays out at, or the panel would sit permanently cropped.
    #[test]
    fn open_widths_match_the_panels_they_reveal() {
        let mut motion = Motion::new();
        // Drive everything open, then sample well past the longest duration.
        motion.sync(true, true, true, None, false);
        motion.now = Instant::now() + BASE + std::time::Duration::from_millis(50);

        assert_eq!(motion.sidebar_width(), motion.sidebar_target_width);
        assert_eq!(motion.toc_width(), motion.toc_target_width);
        assert_eq!(motion.backlinks_width(), motion.backlinks_target_width);
    }

    #[test]
    fn closed_panels_take_no_space() {
        let mut motion = Motion::new();
        motion.sync(false, false, false, None, false);
        motion.now = Instant::now() + BASE + std::time::Duration::from_millis(50);

        assert_eq!(motion.sidebar_width(), 0.0);
        assert_eq!(motion.toc_width(), 0.0);
        assert_eq!(motion.backlinks_width(), 0.0);
    }

    /// A settled UI must not keep the per-frame subscription alive.
    #[test]
    fn settles_and_stops_animating() {
        let mut motion = Motion::new();
        motion.sync(false, true, false, Some("saved"), false);
        assert!(motion.is_animating(), "a just-changed flag is in motion");

        motion.now = Instant::now() + BASE + std::time::Duration::from_millis(50);
        assert!(
            !motion.is_animating(),
            "once the durations elapse nothing should still be moving"
        );
    }

    fn after(start: Instant, seconds: f32) -> Instant {
        start + std::time::Duration::from_secs_f32(seconds)
    }

    #[test]
    fn a_spring_moves_from_its_position_to_its_target_without_overshoot() {
        let start = Instant::now();
        let mut spring = Spring::at_rest(0.0, start, SCROLL_STIFFNESS);
        spring.retarget(500.0, start);
        assert_eq!(spring.value(start), 0.0);

        let mut previous = 0.0;
        for frame in 1..=120 {
            let value = spring.value(after(start, frame as f32 / 120.0));
            assert!(
                (previous..=500.0).contains(&value),
                "frame {frame}: {value} after {previous}"
            );
            previous = value;
        }
        assert!(spring.is_settled(after(start, 1.0)));
        assert!(!spring.is_settled(after(start, 0.05)));
    }

    #[test]
    fn retargeting_keeps_position_and_velocity_continuous() {
        let start = Instant::now();
        let mut spring = Spring::at_rest(0.0, start, SCROLL_STIFFNESS);
        spring.retarget(300.0, start);
        let midway = after(start, 0.04);
        let before = spring.sample(midway);
        spring.retarget(-120.0, midway);
        let after_retarget = spring.sample(midway);
        assert!((before.0 - after_retarget.0).abs() < 1e-3);
        assert!((before.1 - after_retarget.1).abs() < 1e-2);
    }

    /// The toast text has to outlive `ui.toast` so the fade-out has something
    /// to draw.
    #[test]
    fn toast_text_survives_the_message_clearing() {
        let mut motion = Motion::new();
        motion.sync(true, false, false, Some("File saved"), false);
        assert_eq!(motion.toast_text, "File saved");

        motion.sync(true, false, false, None, false);
        assert_eq!(
            motion.toast_text, "File saved",
            "the text must remain while the toast fades out"
        );
    }
}

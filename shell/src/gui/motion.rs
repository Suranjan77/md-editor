use std::time::{Duration, Instant};

use super::session::MdSession;
use super::{Message, Shell};
use iced::{Subscription, Task};

const SCROLL_DURATION: Duration = Duration::from_millis(120);
const CARET_FADE_DURATION: Duration = Duration::from_millis(90);
/// qvfade/qvdim reveal length for overlays (CSS uses 0.12–0.16s).
const OVERLAY_REVEAL_DURATION: Duration = Duration::from_millis(160);

pub(super) struct ScrollAnimation {
    from: f32,
    to: f32,
    started: Instant,
}

impl Shell {
    pub(super) fn motion_subscription(&self) -> Option<Subscription<Message>> {
        let now = Instant::now();
        let active = !self.reduce_motion
            && (self.sessions.md.values().any(|s| s.has_active_motion(now))
                || self.overlay_reveal_active(now));
        active.then(|| iced::time::every(Duration::from_millis(16)).map(Message::AnimationTick))
    }

    pub(super) fn advance_motion(&mut self, now: Instant) -> Task<Message> {
        for session in self.sessions.md.values_mut() {
            session.advance_motion(now);
        }
        Task::none()
    }

    /// True while an open overlay is still playing its reveal — keeps the
    /// frame ticker alive until the qvfade/qvdim animation lands.
    fn overlay_reveal_active(&self, now: Instant) -> bool {
        self.overlay.is_some()
            && self.overlay_revealed_at.is_some_and(|at| {
                now.saturating_duration_since(at) < OVERLAY_REVEAL_DURATION
            })
    }

    /// Eased 0→1 reveal progress for the open overlay (qvfade slide + qvdim
    /// scrim alpha). 1.0 immediately when motion is reduced or nothing is open.
    pub(super) fn overlay_reveal(&self) -> f32 {
        if self.reduce_motion {
            return 1.0;
        }
        match self.overlay_revealed_at {
            Some(at) => ease_reveal(Instant::now().saturating_duration_since(at)),
            None => 1.0,
        }
    }
}

/// Ease-out cubic over [`OVERLAY_REVEAL_DURATION`]: 0 at the start, 1 once the
/// reveal has elapsed, clamped beyond.
fn ease_reveal(elapsed: Duration) -> f32 {
    let progress = elapsed.as_secs_f32() / OVERLAY_REVEAL_DURATION.as_secs_f32();
    1.0 - (1.0 - progress.clamp(0.0, 1.0)).powi(3)
}

impl MdSession {
    pub(super) fn scroll_by_animated(&mut self, dy: f32, reduce_motion: bool) {
        if reduce_motion {
            self.scroll_by(dy);
            return;
        }
        let max = (self.doc.layout().total_height() as f32 - self.viewport_h
            + super::editor_canvas::LINE_HEIGHT)
            .max(0.0);
        let to = self
            .scroll_animation
            .as_ref()
            .map_or(self.scroll, |animation| animation.to);
        self.scroll_animation = Some(ScrollAnimation {
            from: self.scroll,
            to: (to + dy).clamp(0.0, max),
            started: Instant::now(),
        });
    }

    pub(super) fn advance_motion(&mut self, now: Instant) {
        let Some(animation) = self.scroll_animation.as_ref() else {
            return;
        };
        let progress = now
            .saturating_duration_since(animation.started)
            .as_secs_f32()
            / SCROLL_DURATION.as_secs_f32();
        if progress >= 1.0 {
            self.scroll = animation.to;
            self.scroll_animation = None;
            return;
        }
        let eased = 1.0 - (1.0 - progress.clamp(0.0, 1.0)).powi(3);
        self.scroll = animation.from + (animation.to - animation.from) * eased;
    }

    pub(super) fn finish_motion(&mut self) {
        if let Some(animation) = self.scroll_animation.take() {
            self.scroll = animation.to;
        }
    }

    pub(super) fn has_active_motion(&self, now: Instant) -> bool {
        self.scroll_animation.is_some()
            || now.saturating_duration_since(self.caret_moved_at) < CARET_FADE_DURATION
    }

    pub(super) fn caret_opacity(&self, now: Instant, reduce_motion: bool) -> f32 {
        if reduce_motion {
            return 1.0;
        }
        let progress = now
            .saturating_duration_since(self.caret_moved_at)
            .as_secs_f32()
            / CARET_FADE_DURATION.as_secs_f32();
        0.35 + 0.65 * progress.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> MdSession {
        MdSession::new(
            "note.md",
            &(0..100).map(|i| format!("line {i}\n")).collect::<String>(),
            super::super::shaped_measurer::ShapedMeasurer::new(std::sync::Arc::new(
                std::sync::Mutex::new(cosmic_text::FontSystem::new()),
            )),
        )
    }

    #[test]
    fn animated_scroll_converges_and_reduce_motion_jumps() {
        let mut session = session();
        session.viewport_h = 100.0;
        session.scroll_by_animated(240.0, false);
        let started = session.scroll_animation.as_ref().map(|a| a.started);
        assert_eq!(session.scroll, 0.0);
        if let Some(started) = started {
            session.advance_motion(started + SCROLL_DURATION);
        }
        assert_eq!(session.scroll, 240.0);
        assert!(session.scroll_animation.is_none());

        session.scroll_by_animated(60.0, true);
        assert_eq!(session.scroll, 300.0);
        assert!(session.scroll_animation.is_none());
    }

    #[test]
    fn reduce_motion_keeps_caret_fully_opaque() {
        let session = session();
        assert_eq!(session.caret_opacity(Instant::now(), true), 1.0);
        assert!(session.caret_opacity(session.caret_moved_at, false) < 1.0);
    }

    #[test]
    fn overlay_reveal_eases_in_then_settles() {
        // Starts hidden, eases up, and is fully revealed once the duration
        // has elapsed (and stays clamped beyond it).
        assert_eq!(ease_reveal(Duration::ZERO), 0.0);
        let mid = ease_reveal(OVERLAY_REVEAL_DURATION / 2);
        assert!(mid > 0.0 && mid < 1.0, "mid reveal was {mid}");
        assert_eq!(ease_reveal(OVERLAY_REVEAL_DURATION), 1.0);
        assert_eq!(ease_reveal(OVERLAY_REVEAL_DURATION * 2), 1.0);
    }
}

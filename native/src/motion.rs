//! Motion tokens and the animation state they drive.
//!
//! Before this module the app had no motion at all: panels appeared and
//! vanished between one frame and the next, and toasts blinked in and out.
//! Instant state changes are the cheapest thing to get wrong about polish —
//! nothing is *missing* from a snapshot of the UI, but using it feels abrupt,
//! because the eye is given no continuity between the two states.
//!
//! The vocabulary is deliberately small: one easing curve and three durations.
//! Every animated transition in the app picks one of them, for the same reason
//! every size and gap picks a step from the scales in [`crate::theme`].

use std::time::Instant;

use iced::Animation;
use iced::animation::Easing;

/// The single easing curve. Decelerating motion reads as a thing coming to
/// rest, which is what opening a panel or landing a toast is; symmetric or
/// accelerating curves read as mechanical by comparison.
pub const EASE: Easing = Easing::EaseOutQuad;

/// Small, frequent transitions: a toast arriving, a chip changing state.
pub const FAST: std::time::Duration = std::time::Duration::from_millis(120);
/// Layout-level transitions: a side panel opening or closing.
pub const PANEL: std::time::Duration = std::time::Duration::from_millis(180);
/// Overlays that cover the workspace: the command palette, modals.
pub const OVERLAY: std::time::Duration = std::time::Duration::from_millis(140);

// Each panel's open width must match the width the panel itself lays out at,
// or the clip that produces the slide would crop it permanently.
/// Width of the file tree when open; see `views::sidebar`.
pub const SIDEBAR_WIDTH: f32 = 260.0;
/// Width of the table-of-contents panel when open; see `views::toc`.
pub const TOC_WIDTH: f32 = 250.0;
/// Width of the backlinks panel when open; see `views::backlinks`.
pub const BACKLINKS_WIDTH: f32 = 220.0;

/// Animated mirrors of the app's UI-visibility flags.
///
/// The booleans themselves stay where they already live (sidebar visibility on
/// the vault state, the table of contents on the editor pane, and so on); this
/// only tracks how far each one has travelled between hidden and shown, so the
/// view can render an in-between frame.
pub struct Motion {
    pub sidebar: Animation<bool>,
    pub toc: Animation<bool>,
    pub backlinks: Animation<bool>,
    pub toast: Animation<bool>,
    pub palette: Animation<bool>,

    /// The timestamp the current frame is being drawn for. Updated by the
    /// per-frame subscription while anything is moving; `view` reads it rather
    /// than calling `Instant::now()` itself, so every animation in a frame is
    /// sampled at the same instant.
    pub now: Instant,

    /// The last toast message shown. Kept after `ui.toast` clears so the toast
    /// can fade out rather than vanishing mid-transition.
    pub toast_text: String,
}

impl Motion {
    pub fn new() -> Self {
        Self {
            sidebar: Animation::new(true).easing(EASE).duration(PANEL),
            toc: Animation::new(false).easing(EASE).duration(PANEL),
            backlinks: Animation::new(false).easing(EASE).duration(PANEL),
            toast: Animation::new(false).easing(EASE).duration(FAST),
            palette: Animation::new(false).easing(EASE).duration(OVERLAY),
            now: Instant::now(),
            toast_text: String::new(),
        }
    }

    /// Whether any transition is still in flight. The per-frame subscription is
    /// armed only while this holds, so a settled UI does not redraw for nothing.
    pub fn is_animating(&self) -> bool {
        let at = self.now;
        self.sidebar.is_animating(at)
            || self.toc.is_animating(at)
            || self.backlinks.is_animating(at)
            || self.toast.is_animating(at)
            || self.palette.is_animating(at)
    }

    /// Drive every animation to match the flags that own the real state.
    ///
    /// Called once per update rather than at each toggle site, so a flag
    /// changed anywhere — a shortcut, a toolbar button, a command, a modal
    /// dismissing itself — animates without that code having to know motion
    /// exists.
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
        let toast = toast.is_some();
        let now = Instant::now();
        for (animation, target) in [
            (&mut self.sidebar, sidebar),
            (&mut self.toc, toc),
            (&mut self.backlinks, backlinks),
            (&mut self.toast, toast),
            (&mut self.palette, palette),
        ] {
            if animation.value() != target {
                animation.go_mut(target, now);
            }
        }
        self.now = now;
    }

    /// Current width of the file tree, part-way through opening or closing.
    pub fn sidebar_width(&self) -> f32 {
        self.sidebar.interpolate(0.0, SIDEBAR_WIDTH, self.now)
    }

    /// Current width of the table-of-contents panel.
    pub fn toc_width(&self) -> f32 {
        self.toc.interpolate(0.0, TOC_WIDTH, self.now)
    }

    /// Current width of the backlinks panel.
    pub fn backlinks_width(&self) -> f32 {
        self.backlinks.interpolate(0.0, BACKLINKS_WIDTH, self.now)
    }

    /// Opacity of the transient toast, so it fades rather than blinking.
    pub fn toast_opacity(&self) -> f32 {
        self.toast.interpolate(0.0, 1.0, self.now)
    }

    /// Opacity of the command palette overlay.
    pub fn palette_opacity(&self) -> f32 {
        self.palette.interpolate(0.0, 1.0, self.now)
    }
}

impl Default for Motion {
    fn default() -> Self {
        Self::new()
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
        motion.now = Instant::now() + PANEL + std::time::Duration::from_millis(50);

        assert_eq!(motion.sidebar_width(), SIDEBAR_WIDTH);
        assert_eq!(motion.toc_width(), TOC_WIDTH);
        assert_eq!(motion.backlinks_width(), BACKLINKS_WIDTH);
    }

    #[test]
    fn closed_panels_take_no_space() {
        let mut motion = Motion::new();
        motion.sync(false, false, false, None, false);
        motion.now = Instant::now() + PANEL + std::time::Duration::from_millis(50);

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

        motion.now = Instant::now() + PANEL + std::time::Duration::from_millis(50);
        assert!(
            !motion.is_animating(),
            "once the durations elapse nothing should still be moving"
        );
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

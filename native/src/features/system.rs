use crate::messages::{Message, Shortcut};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum SystemMessage {
    Tick,
    KeyboardShortcut(Shortcut),
    FocusNext,
    FocusPrevious,
    ScaleFactorChanged(f32),
    SpinnerTick,
    MarkdownIndexFinished(Result<(), String>),
    AnnotationDebounceElapsed,
}

#[allow(dead_code, non_snake_case, non_upper_case_globals)]
impl Message {
    pub(crate) const Tick: Self = Self::System(SystemMessage::Tick);
    pub(crate) const FocusNext: Self = Self::System(SystemMessage::FocusNext);
    pub(crate) const FocusPrevious: Self = Self::System(SystemMessage::FocusPrevious);
    pub(crate) const SpinnerTick: Self = Self::System(SystemMessage::SpinnerTick);
    pub(crate) const AnnotationDebounceElapsed: Self =
        Self::System(SystemMessage::AnnotationDebounceElapsed);

    pub(crate) fn KeyboardShortcut(shortcut: Shortcut) -> Self {
        Self::System(SystemMessage::KeyboardShortcut(shortcut))
    }

    pub(crate) fn ScaleFactorChanged(scale: f32) -> Self {
        Self::System(SystemMessage::ScaleFactorChanged(scale))
    }

    pub(crate) fn MarkdownIndexFinished(result: Result<(), String>) -> Self {
        Self::System(SystemMessage::MarkdownIndexFinished(result))
    }
}

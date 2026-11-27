//! Feedback toast stack (up to 3 items; errors persist with optional Retry).

use iced::widget::Column;
use iced::{Alignment, Element, Renderer, Theme};

use crate::design::metrics::SPACE_2;
use crate::messages::{Message, ShellMessage};
use crate::ui::{ToastKind, toast};
use crate::ui_state::ToastEntry;

/// Render the active toast stack at `opacity` in the bottom-right corner.
pub fn view_stack<'a>(
    queue: &'a [ToastEntry],
    fallback_text: &'a str,
    opacity: f32,
) -> Element<'a, Message, Theme, Renderer> {
    if queue.is_empty() {
        return toast(
            fallback_text,
            ToastKind::Info,
            None,
            Message::Shell(ShellMessage::ToastHide),
            opacity,
        );
    }

    let items = queue.iter().map(|entry| {
        let item_opacity = if entry.persistent { 1.0 } else { opacity };
        toast(
            &entry.text,
            entry.kind,
            entry
                .can_retry_save
                .then_some(("Retry", Message::Shell(ShellMessage::RetrySave))),
            Message::Shell(ShellMessage::DismissToast(entry.id)),
            item_opacity,
        )
    });

    Column::with_children(items)
        .spacing(SPACE_2)
        .align_x(Alignment::End)
        .into()
}

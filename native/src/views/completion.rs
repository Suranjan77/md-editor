//! The `[[` / `/` completion popup: a list of suggestions floated next to the caret in the note.

use iced::widget::{column, container};
use iced::{Element, Length, Padding, Theme};

use crate::autocomplete::{Completion, CompletionKind};
use crate::design::metrics::{ControlSize, POPOVER_MAX_WIDTH, SPACE_1, SPACE_2};
use crate::editor::renderer::CaretView;
use crate::messages::{EditorMessage, Message};
use crate::ui::{Icon, ListRowProps, list_row, popover};

/// Width of the popup.
const WIDTH: f32 = 320.0;

/// The popup, positioned in a layer the size of the editor's scroll area:
/// below the caret's line, or above it when there is no room below.
/// `inset` is the padding around the editor widget inside that area.
pub fn view<'a>(
    completion: &'a Completion,
    caret: &CaretView,
    inset: f32,
    area_width: f32,
) -> Element<'a, Message, Theme, iced::Renderer> {
    let row_height = ControlSize::Md.height() + SPACE_1;
    let height = completion.items.len() as f32 * row_height + 2.0 * SPACE_2;
    let below = caret.caret_bottom - caret.viewport_top + SPACE_1;
    let above = caret.caret_top - caret.viewport_top - SPACE_1 - height;
    let top = if below + height <= caret.viewport_height || above < 0.0 {
        below
    } else {
        above
    };
    let left = (caret.caret_x + inset).min((area_width - WIDTH - SPACE_2).max(0.0));

    let icon = match completion.kind {
        CompletionKind::Wikilink => Icon::Link,
        CompletionKind::Citation => Icon::FileText,
        CompletionKind::Slash => Icon::Plus,
    };
    let rows = completion.items.iter().enumerate().map(|(index, item)| {
        let mut props = ListRowProps::simple(
            item.label.as_str(),
            Some(icon),
            index == completion.selected,
            Message::Editor(EditorMessage::CompletionPick(index)),
        );
        props.subtitle = item.detail.as_deref().map(Into::into);
        list_row(props)
    });
    let list = popover(
        column(rows).width(Length::Fixed(WIDTH)),
        Some(POPOVER_MAX_WIDTH),
    );
    container(list)
        .padding(Padding {
            top: top.max(0.0),
            left: left.max(0.0),
            ..Padding::ZERO
        })
        .into()
}

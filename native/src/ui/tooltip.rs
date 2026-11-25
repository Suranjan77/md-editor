//! `Tooltip`: 500 ms delayed floating hint with label and optional `Kbd` shortcut (MASTER §6).

use std::time::Duration;

use iced::widget::{container, row, text, tooltip as iced_tooltip};
use iced::{Alignment, Background, Border, Element, Renderer, Theme};

use super::kbd::kbd;
use crate::design;
use crate::design::TextRole;
use crate::design::metrics::{BORDER_WIDTH, Elevation, RADIUS_MD, SPACE_1, SPACE_2};
use crate::design::motion::reduced_motion;

const TOOLTIP_DELAY: Duration = Duration::from_millis(500); // token-allow: MASTER §6 500ms tooltip delay

pub use iced::widget::tooltip::Position as TooltipPosition;

pub fn with_tooltip<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    label: &str,
    shortcut: Option<&str>,
    position: TooltipPosition,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let role = TextRole::Caption;
    let tip_row: Element<'a, Message, Theme, Renderer> = match shortcut.filter(|s| !s.is_empty()) {
        Some(keys) => row![
            text(label.to_string())
                .size(role.size())
                .line_height(role.line_height())
                .font(role.font())
                .color(t.fg_primary),
            kbd(keys),
        ]
        .spacing(SPACE_2)
        .align_y(Alignment::Center)
        .into(),
        None => text(label.to_string())
            .size(role.size())
            .line_height(role.line_height())
            .font(role.font())
            .color(t.fg_primary)
            .into(),
    };

    let bg = t.bg_raised;
    let border_col = t.border_default;
    let tip_box = container(tip_row).padding([SPACE_1, SPACE_2]);
    let delay = if reduced_motion() {
        Duration::ZERO
    } else {
        TOOLTIP_DELAY
    };

    iced_tooltip(content, tip_box, position)
        .delay(delay)
        .gap(SPACE_1)
        .padding(0.0)
        .snap_within_viewport(true)
        .style(move |_| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: border_col,
                width: BORDER_WIDTH,
                radius: RADIUS_MD.into(),
            },
            shadow: Elevation::Popover.shadow(),
            text_color: Some(t.fg_primary),
            snap: true,
        })
        .into()
}

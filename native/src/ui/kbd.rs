//! `Kbd`: renders keyboard shortcut chords (`Ctrl` `Shift` `P`) in monospace keycaps (MASTER §6).

use iced::widget::{container, row, text};
use iced::{Alignment, Background, Border, Element, Renderer, Theme};

use crate::design;
use crate::design::TextRole;
use crate::design::metrics::{BORDER_WIDTH, RADIUS_SM, SPACE_1};

/// Render a shortcut string such as `"Ctrl+Shift+P"` as individual keycaps.
pub fn kbd<'a, Message: 'a>(shortcut: &str) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let role = TextRole::MonoCaption;
    let parts: Vec<&str> = shortcut
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    let mut caps = Vec::with_capacity(parts.len());
    for part in parts {
        let bg = t.bg_raised;
        let border = t.border_default;
        let fg = t.fg_secondary;
        caps.push(
            container(
                text(part.to_string())
                    .size(role.size())
                    .line_height(role.line_height())
                    .font(role.font())
                    .color(fg),
            )
            .padding([0.0, SPACE_1])
            .style(move |_| container::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    color: border,
                    width: BORDER_WIDTH,
                    radius: RADIUS_SM.into(),
                },
                ..Default::default()
            })
            .into(),
        );
    }

    row(caps).spacing(SPACE_1).align_y(Alignment::Center).into()
}

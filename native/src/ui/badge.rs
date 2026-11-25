//! `Badge` and `Chip`: status and metadata pills that always pair colour with text/icon (MASTER §6).

use iced::widget::{button, container, row, text};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use super::icon::{Icon, icon};
use crate::design;
use crate::design::metrics::{
    BORDER_WIDTH, ControlSize, IconSize, RADIUS_FULL, RADIUS_SM, SPACE_1, SPACE_2,
};
use crate::design::{TextRole, fade};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeTone {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
    Info,
}

pub fn badge<'a, Message: 'a>(
    label: &str,
    tone: BadgeTone,
    leading_icon: Option<Icon>,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let (fg, bg, border_col) = match tone {
        BadgeTone::Neutral => (t.fg_secondary, t.bg_raised, t.border_default),
        BadgeTone::Accent => (t.accent, t.accent_muted, fade(t.accent, 0.35)),
        BadgeTone::Success => (
            t.status_success,
            fade(t.status_success, 0.14),
            fade(t.status_success, 0.35),
        ),
        BadgeTone::Warning => (
            t.status_warning,
            fade(t.status_warning, 0.14),
            fade(t.status_warning, 0.35),
        ),
        BadgeTone::Danger => (
            t.status_danger,
            fade(t.status_danger, 0.14),
            fade(t.status_danger, 0.40),
        ),
        BadgeTone::Info => (
            t.status_info,
            fade(t.status_info, 0.14),
            fade(t.status_info, 0.35),
        ),
    };

    let role = TextRole::Caption;
    let mut items: Vec<Element<'a, Message, Theme, Renderer>> = Vec::with_capacity(2);
    if let Some(ic) = leading_icon {
        items.push(icon(ic, IconSize::Sm.px(), fg));
    }
    items.push(
        text(label.to_string())
            .size(role.size())
            .line_height(role.line_height())
            .font(role.font())
            .color(fg)
            .into(),
    );

    container(row(items).spacing(SPACE_1).align_y(Alignment::Center))
        .padding([0.0, SPACE_2])
        .height(Length::Fixed(ControlSize::Sm.height()))
        .align_y(Alignment::Center)
        .style(move |_| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: border_col,
                width: BORDER_WIDTH,
                radius: RADIUS_SM.into(),
            },
            ..Default::default()
        })
        .into()
}

/// Interactive filter `Chip` (used in graph filters and search tags).
pub fn filter_chip<'a, Message: Clone + 'a>(
    label: &str,
    active: bool,
    leading_icon: Option<Icon>,
    on_press: Message,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let role = TextRole::Caption;
    let fg = if active { t.fg_primary } else { t.fg_secondary };
    let icon_col = if active { t.accent } else { t.fg_tertiary };

    let mut items: Vec<Element<'a, Message, Theme, Renderer>> = Vec::new();
    if active {
        items.push(icon(Icon::Check, IconSize::Sm.px(), t.accent));
    } else if let Some(ic) = leading_icon {
        items.push(icon(ic, IconSize::Sm.px(), icon_col));
    }
    items.push(
        text(label.to_string())
            .size(role.size())
            .line_height(role.line_height())
            .font(role.font())
            .color(fg)
            .into(),
    );

    button(
        row(items)
            .spacing(SPACE_1)
            .height(Length::Fill)
            .align_y(Alignment::Center),
    )
    .on_press(on_press)
    .height(Length::Fixed(ControlSize::Sm.height()))
    .padding([0.0, SPACE_2])
    .style(move |_theme, status| {
        let t = design::tokens();
        let bg = match (active, status) {
            (true, button::Status::Pressed) => t.bg_overlay,
            (true, _) => t.accent_muted,
            (false, button::Status::Hovered) => t.bg_raised,
            (false, button::Status::Pressed) => t.bg_overlay,
            (false, _) => t.bg_surface,
        };
        let border = if active { t.accent } else { t.border_control };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: fg,
            border: Border {
                color: border,
                width: BORDER_WIDTH,
                radius: RADIUS_FULL.into(),
            },
            ..Default::default()
        }
    })
    .into()
}

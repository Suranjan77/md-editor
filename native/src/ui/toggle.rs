//! `Toggle` switch and `Checkbox` (on, off, mixed, disabled, focused; MASTER §6).

use iced::widget::{button, container, row, text};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use super::focus::ring_border;
use super::icon::{Icon, icon};
use crate::design;
use crate::design::metrics::{
    ControlSize, IconSize, RADIUS_FULL, RADIUS_SM, SPACE_0, SPACE_1, SPACE_2,
};
use crate::design::{TextRole, fade};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    Off,
    On,
    Mixed,
}

impl From<bool> for CheckState {
    fn from(v: bool) -> Self {
        if v { Self::On } else { Self::Off }
    }
}

pub fn checkbox<'a, Message: Clone + 'a>(
    label: &str,
    state: impl Into<CheckState>,
    on_toggle: Option<Message>,
    focused: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let state = state.into();
    let t = design::tokens();
    let enabled = on_toggle.is_some();
    let active = matches!(state, CheckState::On | CheckState::Mixed);
    let box_size = IconSize::Md.px() + SPACE_1;

    let indicator: Element<'a, Message, Theme, Renderer> = match state {
        CheckState::On => icon(Icon::Check, IconSize::Sm.px(), t.on_accent),
        CheckState::Mixed => icon(Icon::Minus, IconSize::Sm.px(), t.on_accent),
        CheckState::Off => container(text("")).into(),
    };

    let bg = if active {
        if enabled {
            t.accent
        } else {
            fade(t.accent, 0.45)
        }
    } else {
        t.bg_raised
    };
    let border_col = if active { t.accent } else { t.border_control };

    let check_box = container(indicator)
        .width(Length::Fixed(box_size))
        .height(Length::Fixed(box_size))
        .center_x(Length::Fixed(box_size))
        .center_y(Length::Fixed(box_size))
        .style(move |_| container::Style {
            background: Some(Background::Color(bg)),
            border: ring_border(focused, RADIUS_SM, Some(border_col)),
            ..Default::default()
        });

    let role = TextRole::Body;
    let fg = if enabled { t.fg_primary } else { t.fg_tertiary };
    let body: Element<'a, Message, Theme, Renderer> = if label.is_empty() {
        check_box.into()
    } else {
        row![
            check_box,
            text(label.to_string())
                .size(role.size())
                .line_height(role.line_height())
                .font(role.font())
                .color(fg),
        ]
        .spacing(SPACE_2)
        .align_y(Alignment::Center)
        .into()
    };

    button(body)
        .padding(SPACE_0)
        .on_press_maybe(on_toggle)
        .style(|_theme, _status| button::Style {
            background: None,
            ..Default::default()
        })
        .into()
}

pub fn toggle_switch<'a, Message: Clone + 'a>(
    label: &str,
    on: bool,
    on_toggle: Option<Message>,
    focused: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let enabled = on_toggle.is_some();
    let track_h = ControlSize::Sm.height();
    let track_w = track_h * 1.75;
    let knob_px = track_h - SPACE_2;

    let knob_bg = if on { t.on_accent } else { t.fg_primary };
    let knob = container(text(""))
        .width(Length::Fixed(knob_px))
        .height(Length::Fixed(knob_px))
        .style(move |_| container::Style {
            background: Some(Background::Color(knob_bg)),
            border: Border {
                radius: RADIUS_FULL.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    let track_bg = if on {
        if enabled {
            t.accent
        } else {
            fade(t.accent, 0.45)
        }
    } else {
        t.bg_overlay
    };
    let track_border = if on { t.accent } else { t.border_control };
    let align_x = if on {
        iced::alignment::Horizontal::Right
    } else {
        iced::alignment::Horizontal::Left
    };

    let track = container(knob)
        .width(Length::Fixed(track_w))
        .height(Length::Fixed(track_h))
        .padding([0.0, SPACE_1])
        .align_x(align_x)
        .align_y(Alignment::Center)
        .style(move |_| container::Style {
            background: Some(Background::Color(track_bg)),
            border: ring_border(focused, RADIUS_FULL, Some(track_border)),
            ..Default::default()
        });

    let role = TextRole::Body;
    let fg = if enabled { t.fg_primary } else { t.fg_tertiary };
    let content: Element<'a, Message, Theme, Renderer> = if label.is_empty() {
        track.into()
    } else {
        row![
            track,
            text(label.to_string())
                .size(role.size())
                .line_height(role.line_height())
                .font(role.font())
                .color(fg),
        ]
        .spacing(SPACE_2)
        .align_y(Alignment::Center)
        .into()
    };

    button(content)
        .padding(SPACE_0)
        .on_press_maybe(on_toggle)
        .style(|_theme, _status| button::Style {
            background: None,
            ..Default::default()
        })
        .into()
}

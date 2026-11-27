//! `IconButton`: 28x28 (or 24x24 sm) icon control that ALWAYS requires an accessible label + tooltip (MASTER §6).

use iced::widget::button as iced_button;
use iced::{Background, Element, Length, Renderer, Theme};

use super::focus::ring_border;
use super::icon::{Icon, icon};
use super::tooltip::{TooltipPosition, with_tooltip};
use crate::design;
use crate::design::fade;
use crate::design::metrics::{ControlSize, IconSize, RADIUS_MD, SPACE_0};

pub struct IconButton<'a, Message> {
    which: Icon,
    label: String,
    shortcut: Option<String>,
    selected: bool,
    danger: bool,
    focused: bool,
    size: ControlSize,
    tooltip_position: TooltipPosition,
    on_press: Option<Message>,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a, Message: Clone + 'a> IconButton<'a, Message> {
    /// Every `IconButton` requires a human-readable `label` so no icon-only
    /// control ever ships without a tooltip and future AccessKit name.
    pub fn new(which: Icon, label: impl Into<String>) -> Self {
        Self {
            which,
            label: label.into(),
            shortcut: None,
            selected: false,
            danger: false,
            focused: false,
            size: ControlSize::Md,
            tooltip_position: TooltipPosition::Bottom,
            on_press: None,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn shortcut(mut self, keys: impl Into<String>) -> Self {
        let s = keys.into();
        if !s.is_empty() {
            self.shortcut = Some(s);
        }
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    pub fn tooltip_position(mut self, pos: TooltipPosition) -> Self {
        self.tooltip_position = pos;
        self
    }

    pub fn on_press(mut self, msg: Message) -> Self {
        self.on_press = Some(msg);
        self
    }

    pub fn on_press_maybe(mut self, msg: Option<Message>) -> Self {
        self.on_press = msg;
        self
    }

    pub fn build(self) -> Element<'a, Message, Theme, Renderer> {
        let t = design::tokens();
        let enabled = self.on_press.is_some();
        let selected = self.selected;
        let danger = self.danger;
        let focused = self.focused;

        let glyph = if selected {
            self.which.selected_variant()
        } else {
            self.which
        };

        let icon_color = if !enabled {
            fade(t.fg_tertiary, 0.45)
        } else if danger {
            t.status_danger
        } else if selected {
            t.accent
        } else {
            t.fg_secondary
        };

        let icon_px = match self.size {
            ControlSize::Sm => IconSize::Sm.px(),
            ControlSize::Md => IconSize::Md.px(),
            ControlSize::Lg => IconSize::Lg.px(),
        };
        let box_px = self.size.height();

        let btn = iced_button(icon(glyph, icon_px, icon_color))
            .width(Length::Fixed(box_px))
            .height(Length::Fixed(box_px))
            .padding(SPACE_0)
            .on_press_maybe(self.on_press)
            .style(move |_theme, status| {
                let t = design::tokens();
                let bg = match (selected, status) {
                    (_, iced_button::Status::Disabled) => None,
                    (true, iced_button::Status::Pressed) => Some(t.bg_overlay),
                    (true, _) => Some(t.accent_muted),
                    (false, iced_button::Status::Hovered) => Some(t.bg_raised),
                    (false, iced_button::Status::Pressed) => Some(t.bg_overlay),
                    (false, _) => None,
                };
                let rest_border = if selected {
                    Some(fade(t.accent, 0.45))
                } else {
                    None
                };
                iced_button::Style {
                    background: bg.map(Background::Color),
                    text_color: icon_color,
                    border: ring_border(focused, RADIUS_MD, rest_border),
                    ..Default::default()
                }
            });

        with_tooltip(
            btn,
            &self.label,
            self.shortcut.as_deref(),
            self.tooltip_position,
        )
    }
}

impl<'a, Message: Clone + 'a> From<IconButton<'a, Message>>
    for Element<'a, Message, Theme, Renderer>
{
    fn from(btn: IconButton<'a, Message>) -> Self {
        btn.build()
    }
}

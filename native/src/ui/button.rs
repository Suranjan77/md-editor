//! `Button`: primary, secondary, ghost and danger variants with sm/md/lg sizes and full state coverage (MASTER §6).

use iced::widget::{button as iced_button, row, text};
use iced::{Alignment, Background, Color, Element, Length, Renderer, Theme};

use super::focus::ring_border;
use super::icon::{Icon, icon};
use super::tooltip::{TooltipPosition, with_tooltip};
use crate::design;
use crate::design::metrics::{ControlSize, IconSize, RADIUS_MD, SPACE_1};
use crate::design::{TextRole, fade};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    #[default]
    Secondary,
    Primary,
    Ghost,
    Danger,
}

pub struct Button<'a, Message> {
    label: String,
    variant: ButtonVariant,
    size: ControlSize,
    leading_icon: Option<Icon>,
    on_press: Option<Message>,
    focused: bool,
    loading: bool,
    width: Length,
    tooltip: Option<(String, Option<String>)>,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a, Message: Clone + 'a> Button<'a, Message> {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            variant: ButtonVariant::Secondary,
            size: ControlSize::Md,
            leading_icon: None,
            on_press: None,
            focused: false,
            loading: false,
            width: Length::Shrink,
            tooltip: None,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn primary(label: impl Into<String>) -> Self {
        Self::new(label).variant(ButtonVariant::Primary)
    }

    pub fn secondary(label: impl Into<String>) -> Self {
        Self::new(label).variant(ButtonVariant::Secondary)
    }

    pub fn ghost(label: impl Into<String>) -> Self {
        Self::new(label).variant(ButtonVariant::Ghost)
    }

    pub fn danger(label: impl Into<String>) -> Self {
        Self::new(label).variant(ButtonVariant::Danger)
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    pub fn icon(mut self, which: Icon) -> Self {
        self.leading_icon = Some(which);
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

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn tooltip(mut self, tip: impl Into<String>, shortcut: Option<&str>) -> Self {
        self.tooltip = Some((tip.into(), shortcut.map(str::to_string)));
        self
    }

    pub fn build(self) -> Element<'a, Message, Theme, Renderer> {
        let t = design::tokens();
        let enabled = self.on_press.is_some() && !self.loading;
        let variant = self.variant;
        let focused = self.focused;

        let fg_base = match variant {
            ButtonVariant::Primary => t.on_accent,
            ButtonVariant::Danger => t.on_accent,
            ButtonVariant::Secondary | ButtonVariant::Ghost => t.fg_primary,
        };
        let fg = if enabled {
            fg_base
        } else {
            fade(fg_base, 0.48)
        };
        let role = match variant {
            ButtonVariant::Primary | ButtonVariant::Danger => TextRole::BodyStrong,
            ButtonVariant::Secondary | ButtonVariant::Ghost => TextRole::Body,
        };

        let icon_px = match self.size {
            ControlSize::Sm => IconSize::Sm.px(),
            ControlSize::Md | ControlSize::Lg => IconSize::Md.px(),
        };

        let mut children: Vec<Element<'a, Message, Theme, Renderer>> = Vec::with_capacity(2);
        if self.loading {
            children.push(icon(Icon::Spinner, icon_px, fg));
        } else if let Some(ic) = self.leading_icon {
            children.push(icon(ic, icon_px, fg));
        }
        children.push(
            text(self.label)
                .size(role.size())
                .line_height(role.line_height())
                .font(role.font())
                .color(fg)
                .into(),
        );

        // Fill the button's height so the label is centred in it, not top-aligned.
        let content = row(children)
            .spacing(SPACE_1)
            .height(Length::Fill)
            .align_y(Alignment::Center);
        let pad_x = self.size.horizontal_padding();
        let mut btn = iced_button(content)
            .height(Length::Fixed(self.size.height()))
            .width(self.width)
            .padding([0.0, pad_x])
            .style(move |_theme, status| button_style(variant, status, focused));

        if enabled && let Some(msg) = self.on_press {
            btn = btn.on_press(msg);
        }

        let element: Element<'a, Message, Theme, Renderer> = btn.into();
        if let Some((tip, shortcut)) = self.tooltip {
            with_tooltip(element, &tip, shortcut.as_deref(), TooltipPosition::Bottom)
        } else {
            element
        }
    }
}

impl<'a, Message: Clone + 'a> From<Button<'a, Message>> for Element<'a, Message, Theme, Renderer> {
    fn from(btn: Button<'a, Message>) -> Self {
        btn.build()
    }
}

fn button_style(
    variant: ButtonVariant,
    status: iced_button::Status,
    focused: bool,
) -> iced_button::Style {
    let t = design::tokens();
    let (bg, fg, rest_border) = match (variant, status) {
        (ButtonVariant::Primary, iced_button::Status::Hovered) => {
            (Some(fade(t.accent, 0.88)), t.on_accent, None)
        }
        (ButtonVariant::Primary, iced_button::Status::Pressed) => {
            (Some(fade(t.accent, 0.76)), t.on_accent, None)
        }
        (ButtonVariant::Primary, iced_button::Status::Disabled) => {
            (Some(fade(t.accent, 0.40)), fade(t.on_accent, 0.55), None)
        }
        (ButtonVariant::Primary, iced_button::Status::Active) => {
            (Some(t.accent), t.on_accent, None)
        }

        (ButtonVariant::Danger, iced_button::Status::Hovered) => {
            (Some(fade(t.status_danger, 0.88)), Color::WHITE, None)
        }
        (ButtonVariant::Danger, iced_button::Status::Pressed) => {
            (Some(fade(t.status_danger, 0.76)), Color::WHITE, None)
        }
        (ButtonVariant::Danger, iced_button::Status::Disabled) => (
            Some(fade(t.status_danger, 0.40)),
            fade(Color::WHITE, 0.55),
            None,
        ),
        (ButtonVariant::Danger, iced_button::Status::Active) => {
            (Some(t.status_danger), Color::WHITE, None)
        }

        (ButtonVariant::Secondary, iced_button::Status::Hovered) => {
            (Some(t.bg_overlay), t.fg_primary, Some(t.border_control))
        }
        (ButtonVariant::Secondary, iced_button::Status::Pressed) => {
            (Some(t.bg_surface), t.fg_primary, Some(t.border_control))
        }
        (ButtonVariant::Secondary, iced_button::Status::Disabled) => (
            Some(fade(t.bg_raised, 0.50)),
            fade(t.fg_tertiary, 0.60),
            Some(t.border_default),
        ),
        (ButtonVariant::Secondary, iced_button::Status::Active) => {
            (Some(t.bg_raised), t.fg_primary, Some(t.border_control))
        }

        (ButtonVariant::Ghost, iced_button::Status::Hovered) => {
            (Some(t.bg_raised), t.fg_primary, None)
        }
        (ButtonVariant::Ghost, iced_button::Status::Pressed) => {
            (Some(t.bg_overlay), t.fg_primary, None)
        }
        (ButtonVariant::Ghost, iced_button::Status::Disabled) => {
            (None, fade(t.fg_tertiary, 0.50), None)
        }
        (ButtonVariant::Ghost, iced_button::Status::Active) => (None, t.fg_secondary, None),
    };

    iced_button::Style {
        background: bg.map(Background::Color),
        text_color: fg,
        border: ring_border(focused, RADIUS_MD, rest_border),
        ..Default::default()
    }
}

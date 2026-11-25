//! `Segmented` control: single-choice pill group (MASTER §6).

use iced::widget::{button, container, row, text};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use super::icon::{Icon, icon};
use crate::design;
use crate::design::TextRole;
use crate::design::metrics::{
    BORDER_WIDTH, ControlSize, IconSize, RADIUS_MD, RADIUS_SM, SPACE_1, SPACE_2, SPACE_PX,
};

pub struct SegmentOption<Message> {
    pub label: String,
    pub icon: Option<Icon>,
    pub selected: bool,
    pub on_select: Message,
}

impl<Message> SegmentOption<Message> {
    pub fn new(label: impl Into<String>, selected: bool, on_select: Message) -> Self {
        Self {
            label: label.into(),
            icon: None,
            selected,
            on_select,
        }
    }

    pub fn icon(mut self, which: Icon) -> Self {
        self.icon = Some(which);
        self
    }
}

/// Single-choice group over typed values: one segment per `(value, label, icon)`,
/// the one equal to `current` selected, `on_select(value)` sent on press.
pub fn segmented_choice<'a, T: Copy + PartialEq, Message: Clone + 'a>(
    options: &[(T, &str, Option<Icon>)],
    current: T,
    on_select: impl Fn(T) -> Message,
    size: ControlSize,
) -> Element<'a, Message, Theme, Renderer> {
    let options = options
        .iter()
        .map(|&(value, label, which)| {
            let option = SegmentOption::new(label, value == current, on_select(value));
            match which {
                Some(which) => option.icon(which),
                None => option,
            }
        })
        .collect();
    segmented(options, size)
}

pub fn segmented<'a, Message: Clone + 'a>(
    options: Vec<SegmentOption<Message>>,
    size: ControlSize,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let item_h = (size.height() - SPACE_1).max(20.0);

    let mut items: Vec<Element<'a, Message, Theme, Renderer>> = Vec::with_capacity(options.len());
    for opt in options {
        let selected = opt.selected;
        let role = if selected {
            TextRole::BodyStrong
        } else {
            TextRole::Body
        };
        let fg = if selected {
            t.fg_primary
        } else {
            t.fg_secondary
        };
        let icon_col = if selected { t.accent } else { t.fg_secondary };

        let mut inner: Vec<Element<'a, Message, Theme, Renderer>> = Vec::with_capacity(2);
        if let Some(ic) = opt.icon {
            let which = if selected { ic.selected_variant() } else { ic };
            inner.push(icon(which, IconSize::Sm.px(), icon_col));
        }
        if !opt.label.is_empty() {
            inner.push(
                text(opt.label)
                    .size(role.size())
                    .line_height(role.line_height())
                    .font(role.font())
                    .color(fg)
                    .into(),
            );
        }

        items.push(
            button(
                row(inner)
                    .spacing(SPACE_1)
                    .height(Length::Fill)
                    .align_y(Alignment::Center),
            )
            .on_press(opt.on_select)
            .height(Length::Fixed(item_h))
            .padding([0.0, SPACE_2])
            .style(move |_theme, status| {
                let t = design::tokens();
                let bg = match (selected, status) {
                    (true, _) => Some(t.bg_overlay),
                    (false, button::Status::Hovered) => Some(t.bg_surface),
                    (false, button::Status::Pressed) => Some(t.bg_overlay),
                    (false, _) => None,
                };
                button::Style {
                    background: bg.map(Background::Color),
                    text_color: fg,
                    border: Border {
                        color: if selected {
                            t.border_control
                        } else {
                            iced::Color::TRANSPARENT
                        },
                        width: if selected { BORDER_WIDTH } else { 0.0 },
                        radius: RADIUS_SM.into(),
                    },
                    ..Default::default()
                }
            })
            .into(),
        );
    }

    let bg = t.bg_raised;
    let border = t.border_default;
    container(row(items).spacing(SPACE_PX).align_y(Alignment::Center))
        .padding(SPACE_PX * 2.0)
        .height(Length::Fixed(size.height()))
        .align_y(Alignment::Center)
        .style(move |_| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: border,
                width: BORDER_WIDTH,
                radius: RADIUS_MD.into(),
            },
            ..Default::default()
        })
        .into()
}

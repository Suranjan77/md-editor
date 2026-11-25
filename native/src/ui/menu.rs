//! `Menu` and `ContextMenu`: action lists with icons, separators, disabled states and `Kbd` shortcuts (MASTER §6).

use iced::widget::{Space, button, column, container, mouse_area, row, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Renderer, Theme};

use super::icon::{Icon, icon};
use super::kbd::kbd;
use super::popover::popover;
use crate::design;
use crate::design::metrics::{
    ControlSize, IconSize, RADIUS_MD, SPACE_1, SPACE_2, SPACE_3, SPACE_PX,
};
use crate::design::{TextRole, fade};

pub enum MenuEntry<Message> {
    Item {
        label: String,
        icon: Option<Icon>,
        shortcut: Option<String>,
        danger: bool,
        on_select: Option<Message>,
    },
    Separator,
}

impl<Message> MenuEntry<Message> {
    pub fn item(label: impl Into<String>, on_select: Option<Message>) -> Self {
        Self::Item {
            label: label.into(),
            icon: None,
            shortcut: None,
            danger: false,
            on_select,
        }
    }

    pub fn with_icon(mut self, which: Icon) -> Self {
        if let Self::Item { icon, .. } = &mut self {
            *icon = Some(which);
        }
        self
    }

    pub fn with_shortcut(mut self, keys: impl Into<String>) -> Self {
        let s = keys.into();
        if !s.is_empty()
            && let Self::Item { shortcut, .. } = &mut self
        {
            *shortcut = Some(s);
        }
        self
    }

    pub fn danger(mut self) -> Self {
        if let Self::Item { danger, .. } = &mut self {
            *danger = true;
        }
        self
    }
}

pub fn menu<'a, Message: Clone + 'a>(
    entries: Vec<MenuEntry<Message>>,
    min_width: f32,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let role = TextRole::Body;
    let mut rows: Vec<Element<'a, Message, Theme, Renderer>> = Vec::with_capacity(entries.len());

    for entry in entries {
        match entry {
            MenuEntry::Separator => {
                let sep_col = t.border_subtle;
                rows.push(
                    container(
                        container(Space::new())
                            .width(Length::Fill)
                            .height(Length::Fixed(SPACE_PX))
                            .style(move |_| container::Style {
                                background: Some(Background::Color(sep_col)),
                                ..Default::default()
                            }),
                    )
                    .padding([SPACE_1, 0.0])
                    .width(Length::Fill)
                    .into(),
                );
            }
            MenuEntry::Item {
                label,
                icon: item_icon,
                shortcut,
                danger,
                on_select,
            } => {
                let enabled = on_select.is_some();
                let fg = if !enabled {
                    fade(t.fg_tertiary, 0.55)
                } else if danger {
                    t.status_danger
                } else {
                    t.fg_primary
                };
                let icon_col = if !enabled {
                    fade(t.fg_tertiary, 0.45)
                } else if danger {
                    t.status_danger
                } else {
                    t.fg_secondary
                };

                let mut line: Vec<Element<'a, Message, Theme, Renderer>> = Vec::with_capacity(4);
                if let Some(ic) = item_icon {
                    line.push(icon(ic, IconSize::Md.px(), icon_col));
                }
                line.push(
                    text(label)
                        .size(role.size())
                        .line_height(role.line_height())
                        .font(role.font())
                        .color(fg)
                        .into(),
                );
                line.push(Space::new().width(Length::Fill).into());
                if let Some(keys) = shortcut {
                    line.push(kbd(&keys));
                }

                rows.push(
                    button(
                        row(line)
                            .spacing(SPACE_2)
                            .height(Length::Fill)
                            .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .height(Length::Fixed(ControlSize::Md.height()))
                    .padding([0.0, SPACE_3])
                    .on_press_maybe(on_select)
                    .style(move |_theme, status| {
                        let t = design::tokens();
                        let bg = match status {
                            button::Status::Hovered if enabled => Some(t.bg_overlay),
                            button::Status::Pressed if enabled => Some(t.accent_muted),
                            _ => None,
                        };
                        button::Style {
                            background: bg.map(Background::Color),
                            text_color: fg,
                            border: Border {
                                color: Color::TRANSPARENT,
                                width: 0.0,
                                radius: RADIUS_MD.into(),
                            },
                            ..Default::default()
                        }
                    })
                    .into(),
                );
            }
        }
    }

    popover(
        container(column(rows).spacing(SPACE_PX)).width(Length::Fixed(min_width)),
        Some(min_width + SPACE_3 * 2.0),
    )
}

/// Position a context menu at `(x, y)` inside the window, snapping within `(win_w, win_h)`,
/// over a transparent click-away catcher that fires `on_dismiss`.
pub fn positioned_context_menu<'a, Message: Clone + 'a>(
    entries: Vec<MenuEntry<Message>>,
    x: f32,
    y: f32,
    win_w: f32,
    win_h: f32,
    on_dismiss: Message,
) -> Element<'a, Message, Theme, Renderer> {
    let menu_w = 236.0;
    let est_h = (entries.len() as f32 * ControlSize::Md.height()) + SPACE_3 * 2.0;
    let clamped_x = x.clamp(SPACE_2, (win_w - menu_w - SPACE_3).max(SPACE_2));
    let clamped_y = y.clamp(SPACE_2, (win_h - est_h - SPACE_3).max(SPACE_2));

    let menu_card = mouse_area(menu(entries, menu_w));
    let positioned = container(menu_card)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding {
            top: clamped_y,
            right: 0.0,
            bottom: 0.0,
            left: clamped_x,
        });

    mouse_area(positioned)
        .on_press(on_dismiss.clone())
        .on_right_press(on_dismiss)
        .into()
}

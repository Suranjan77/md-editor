//! `ListRow`: single-line list row for palette commands, search hits and recent vaults, with ellipsis + tooltip (MASTER §6).

use iced::widget::text::Wrapping;
use iced::widget::{Space, button, container, row, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Renderer, Theme};

use super::focus::ring_border;
use super::icon::{Icon, icon};
use super::kbd::kbd;
use super::tooltip::{TooltipPosition, with_tooltip};
use crate::design;
use crate::design::TextRole;
use crate::design::metrics::{
    BORDER_WIDTH, ControlSize, IconSize, RADIUS_MD, RADIUS_SM, SPACE_0, SPACE_1, SPACE_2, SPACE_3,
};

pub fn ellipsize_single_line(input: &str, max_chars: usize) -> (String, bool) {
    let single = input.lines().next().unwrap_or(input);
    let count = single.chars().count();
    if count <= max_chars {
        (single.to_string(), single.len() != input.len())
    } else {
        let kept: String = single.chars().take(max_chars.saturating_sub(1)).collect();
        (format!("{kept}…"), true)
    }
}

pub struct ListRowProps<'a, Message> {
    pub title: std::borrow::Cow<'a, str>,
    pub subtitle: Option<std::borrow::Cow<'a, str>>,
    pub leading_icon: Option<Icon>,
    pub shortcut: Option<&'a str>,
    pub selected: bool,
    pub active_doc: bool,
    pub focused: bool,
    pub drag_over: bool,
    pub max_title_chars: usize,
    pub on_press: Option<Message>,
}

impl<'a, Message> ListRowProps<'a, Message> {
    /// A plain clickable row: `title`, an optional leading icon, no subtitle or shortcut.
    pub fn simple(
        title: impl Into<std::borrow::Cow<'a, str>>,
        leading_icon: Option<Icon>,
        selected: bool,
        on_press: Message,
    ) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            leading_icon,
            shortcut: None,
            selected,
            active_doc: false,
            focused: false,
            drag_over: false,
            max_title_chars: 48,
            on_press: Some(on_press),
        }
    }
}

pub fn list_row<'a, Message: Clone + 'a>(
    props: ListRowProps<'a, Message>,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let (display_title, truncated) =
        ellipsize_single_line(&props.title, props.max_title_chars.max(16));
    let full_tip = match props.subtitle.as_deref() {
        Some(sub) if !sub.is_empty() => format!("{} — {sub}", props.title),
        _ => props.title.to_string(),
    };

    let role = if props.active_doc || props.selected {
        TextRole::BodyStrong
    } else {
        TextRole::Body
    };
    // MASTER §2.2 Light-theme rule: selected rows keep fg.primary text and show accent in bar/icon.
    let fg = t.fg_primary;
    let icon_col = if props.active_doc || props.selected {
        t.accent
    } else {
        t.fg_secondary
    };

    let mut children: Vec<Element<'a, Message, Theme, Renderer>> = Vec::with_capacity(6);
    if props.active_doc {
        let bar_col = t.accent;
        children.push(
            container(Space::new())
                .width(Length::Fixed(SPACE_1 * 0.75))
                .height(Length::Fixed(IconSize::Md.px()))
                .style(move |_| container::Style {
                    background: Some(Background::Color(bar_col)),
                    border: Border {
                        radius: RADIUS_SM.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .into(),
        );
    }
    if let Some(ic) = props.leading_icon {
        children.push(icon(ic, IconSize::Md.px(), icon_col));
    }
    children.push(
        text(display_title)
            .size(role.size())
            .line_height(role.line_height())
            .font(role.font())
            .wrapping(Wrapping::None)
            .color(fg)
            .into(),
    );

    if let Some(sub) = props.subtitle.as_deref().filter(|s| !s.is_empty()) {
        let (short_sub, _) = ellipsize_single_line(sub, 32);
        let cap = TextRole::Caption;
        children.push(
            text(short_sub)
                .size(cap.size())
                .line_height(cap.line_height())
                .font(cap.font())
                .wrapping(Wrapping::None)
                .color(t.fg_tertiary)
                .into(),
        );
    }

    children.push(Space::new().width(Length::Fill).into());
    if let Some(keys) = props.shortcut.filter(|s| !s.is_empty()) {
        children.push(kbd(keys));
    }

    let selected = props.selected;
    let active_doc = props.active_doc;
    let focused = props.focused;
    let drag_over = props.drag_over;

    let btn = button(
        row(children)
            .spacing(SPACE_2)
            .align_y(Alignment::Center)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fixed(ControlSize::Md.height() + SPACE_1))
    .padding([SPACE_0, SPACE_3])
    .clip(true)
    .on_press_maybe(props.on_press)
    .style(move |_theme, status| {
        let t = design::tokens();
        let bg = if drag_over || selected {
            Some(t.bg_overlay)
        } else if active_doc {
            Some(t.accent_muted)
        } else {
            match status {
                button::Status::Hovered => Some(t.bg_raised),
                button::Status::Pressed => Some(t.bg_overlay),
                _ => None,
            }
        };
        let rest_border = if drag_over {
            Some(t.accent)
        } else if selected {
            Some(Color {
                a: 0.45,
                ..t.border_control
            })
        } else {
            None
        };
        button::Style {
            background: bg.map(Background::Color),
            text_color: fg,
            border: ring_border(focused, RADIUS_MD, rest_border),
            ..Default::default()
        }
    });

    if truncated {
        with_tooltip(btn, &full_tip, None, TooltipPosition::Right)
    } else {
        btn.into()
    }
}

/// Dummy helper used in `ListRowProps` tests.
impl<'a, Message> ListRowProps<'a, Message> {
    pub const fn border_width() -> f32 {
        BORDER_WIDTH
    }
}

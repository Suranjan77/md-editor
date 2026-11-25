//! `Card`: flat Swiss-style info and stat cards on `bg.raised` (MASTER §6).

use iced::widget::{column, container, row, text};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use super::icon::{Icon, icon};
use crate::design;
use crate::design::TextRole;
use crate::design::metrics::{BORDER_WIDTH, IconSize, RADIUS_LG, SPACE_1, SPACE_2, SPACE_4};

pub fn card<'a, Message: 'a>(
    body: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let bg = t.bg_inset;
    let border = t.border_default;

    container(body)
        .padding(SPACE_4)
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: border,
                width: BORDER_WIDTH,
                radius: RADIUS_LG.into(),
            },
            ..Default::default()
        })
        .into()
}

pub fn stat_card<'a, Message: 'a>(
    label: &str,
    value: &str,
    detail: Option<&str>,
    leading_icon: Option<Icon>,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let val_role = TextRole::Heading;
    let lbl_role = TextRole::Caption;

    let mut label_row: Vec<Element<'a, Message, Theme, Renderer>> = Vec::with_capacity(2);
    if let Some(ic) = leading_icon {
        label_row.push(icon(ic, IconSize::Sm.px(), t.accent));
    }
    label_row.push(
        text(label.to_string())
            .size(lbl_role.size())
            .line_height(lbl_role.line_height())
            .font(lbl_role.font())
            .color(t.fg_secondary)
            .into(),
    );

    let mut col: Vec<Element<'a, Message, Theme, Renderer>> = vec![
        row(label_row)
            .spacing(SPACE_1)
            .align_y(Alignment::Center)
            .into(),
        text(value.to_string())
            .size(val_role.size())
            .line_height(val_role.line_height())
            .font(val_role.font())
            .color(t.fg_primary)
            .into(),
    ];
    if let Some(sub) = detail.filter(|s| !s.is_empty()) {
        col.push(
            text(sub.to_string())
                .size(lbl_role.size())
                .line_height(lbl_role.line_height())
                .font(lbl_role.font())
                .color(t.fg_tertiary)
                .into(),
        );
    }

    card(column(col).spacing(SPACE_2))
}

//! `Panel`: standard sidebar and right-hand inspector/outline/backlinks frame (MASTER §6).

use iced::widget::{Space, column, container, row, text};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use super::focus::ring_border;
use crate::design;
use crate::design::TextRole;
use crate::design::metrics::{BORDER_WIDTH, SPACE_0, SPACE_1, SPACE_2, SPACE_3, SPACE_PX};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSide {
    Left,
    Right,
}

pub fn panel<'a, Message: 'a>(
    title: &str,
    badge_text: Option<&str>,
    actions: Vec<Element<'a, Message, Theme, Renderer>>,
    body: impl Into<Element<'a, Message, Theme, Renderer>>,
    side: PanelSide,
    focused: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let title_role = TextRole::TitleSm;
    let cap_role = TextRole::Caption;

    let mut header_items: Vec<Element<'a, Message, Theme, Renderer>> = vec![
        text(title.to_string())
            .size(title_role.size())
            .line_height(title_role.line_height())
            .font(title_role.font())
            .color(t.fg_primary)
            .into(),
    ];
    if let Some(sub) = badge_text.filter(|s| !s.is_empty()) {
        header_items.push(
            text(sub.to_string())
                .size(cap_role.size())
                .line_height(cap_role.line_height())
                .font(cap_role.font())
                .color(t.fg_tertiary)
                .into(),
        );
    }
    header_items.push(Space::new().width(Length::Fill).into());
    header_items.extend(actions);

    let header = container(
        row(header_items)
            .spacing(SPACE_1)
            .align_y(Alignment::Center),
    )
    .padding([SPACE_2, SPACE_3])
    .width(Length::Fill);

    let divider_col = t.border_subtle;
    let divider = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(SPACE_PX))
        .style(move |_| container::Style {
            background: Some(Background::Color(divider_col)),
            ..Default::default()
        });

    let bg = t.bg_surface;
    let edge = t.border_default;
    let inner = column![header, divider, body.into()]
        .spacing(SPACE_0)
        .width(Length::Fill)
        .height(Length::Fill);

    let side_line = container(Space::new())
        .width(Length::Fixed(BORDER_WIDTH))
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(edge)),
            ..Default::default()
        });

    let framed: Element<'a, Message, Theme, Renderer> = match side {
        PanelSide::Left => row![inner, side_line].height(Length::Fill).into(),
        PanelSide::Right => row![side_line, inner].height(Length::Fill).into(),
    };

    container(framed)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(bg)),
            border: if focused {
                ring_border(true, 0.0, None)
            } else {
                Border::default()
            },
            ..Default::default()
        })
        .into()
}

//! `Dialog`: confirm, form and destructive modals at Elevation 2 over `bg.scrim` (MASTER §6).

use iced::widget::{Space, column, container, mouse_area, row, text};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use super::icon::Icon;
use super::icon_button::IconButton;
use crate::design;
use crate::design::metrics::{
    BORDER_WIDTH, DIALOG_WIDTH_SM, Elevation, RADIUS_XL, SPACE_2, SPACE_4, SPACE_5,
};
use crate::design::{TextRole, fade};

pub fn dialog<'a, Message: Clone + 'a>(
    title: &str,
    body: impl Into<Element<'a, Message, Theme, Renderer>>,
    footer_actions: Vec<Element<'a, Message, Theme, Renderer>>,
    on_close: Message,
    width: Option<f32>,
    opacity: f32,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let title_role = TextRole::Title;
    let w = width.unwrap_or(DIALOG_WIDTH_SM);

    let header = row![
        text(title.to_string())
            .size(title_role.size())
            .line_height(title_role.line_height())
            .font(title_role.font())
            .color(fade(t.fg_primary, opacity)),
        Space::new().width(Length::Fill),
        IconButton::new(Icon::X, "Close")
            .shortcut("Esc")
            .on_press(on_close.clone()),
    ]
    .align_y(Alignment::Center);

    let mut content_col: Vec<Element<'a, Message, Theme, Renderer>> =
        vec![header.into(), body.into()];

    if !footer_actions.is_empty() {
        let mut action_row: Vec<Element<'a, Message, Theme, Renderer>> =
            vec![Space::new().width(Length::Fill).into()];
        action_row.extend(footer_actions);
        content_col.push(
            row(action_row)
                .spacing(SPACE_2)
                .align_y(Alignment::Center)
                .into(),
        );
    }

    let card_bg = fade(t.bg_raised, opacity);
    let card_border = fade(t.border_default, opacity);
    let mut shadow = Elevation::Dialog.shadow();
    shadow.color = fade(shadow.color, opacity);

    let card = mouse_area(
        container(column(content_col).spacing(SPACE_4))
            .width(Length::Fixed(w))
            .padding(SPACE_5)
            .style(move |_| container::Style {
                background: Some(Background::Color(card_bg)),
                border: Border {
                    color: card_border,
                    width: BORDER_WIDTH,
                    radius: RADIUS_XL.into(),
                },
                shadow,
                ..Default::default()
            }),
    );

    let scrim_col = fade(t.bg_scrim, opacity);
    mouse_area(
        container(card)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(scrim_col)),
                ..Default::default()
            }),
    )
    .on_press(on_close)
    .into()
}

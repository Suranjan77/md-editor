//! `ProgressBar` and `EmptyState` components (MASTER §6).

use iced::widget::{Space, column, container, row, text};
use iced::{Alignment, Background, Border, Element, Length, Renderer, Theme};

use super::icon::{Icon, icon};
use crate::design;
use crate::design::TextRole;
use crate::design::metrics::{IconSize, RADIUS_FULL, SPACE_1, SPACE_2, SPACE_6};

/// Determinate progress bar accompanied by a tabular percentage label so
/// progress is never communicated by colour/fill alone.
pub fn progress_bar<'a, Message: 'a>(
    fraction: f32,
    show_percent: bool,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let clamped = fraction.clamp(0.0, 1.0);
    let filled_portion = ((clamped * 1000.0).round() as u16).max(if clamped > 0.0 { 1 } else { 0 });
    let empty_portion = 1000_u16.saturating_sub(filled_portion);

    let bar_h = SPACE_2;
    let fill_col = t.accent;
    let track_col = t.bg_overlay;

    let mut segments: Vec<Element<'a, Message, Theme, Renderer>> = Vec::with_capacity(2);
    if filled_portion > 0 {
        segments.push(
            container(Space::new())
                .width(Length::FillPortion(filled_portion))
                .height(Length::Fixed(bar_h))
                .style(move |_| container::Style {
                    background: Some(Background::Color(fill_col)),
                    border: Border {
                        radius: RADIUS_FULL.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .into(),
        );
    }
    if empty_portion > 0 {
        segments.push(
            container(Space::new())
                .width(Length::FillPortion(empty_portion))
                .height(Length::Fixed(bar_h))
                .into(),
        );
    }

    let track = container(row(segments))
        .width(Length::Fill)
        .height(Length::Fixed(bar_h))
        .style(move |_| container::Style {
            background: Some(Background::Color(track_col)),
            border: Border {
                radius: RADIUS_FULL.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    if show_percent {
        let mono = TextRole::MonoCaption;
        let pct = (clamped * 100.0).round() as u32;
        row![
            track,
            text(format!("{pct}%"))
                .size(mono.size())
                .line_height(mono.line_height())
                .font(mono.font())
                .color(t.fg_secondary),
        ]
        .spacing(SPACE_2)
        .align_y(Alignment::Center)
        .into()
    } else {
        track.into()
    }
}

/// Standard empty/error state: Phosphor icon, title, explanation and optional action button.
pub fn empty_state<'a, Message: 'a>(
    which: Icon,
    title: &str,
    description: &str,
    action: Option<Element<'a, Message, Theme, Renderer>>,
) -> Element<'a, Message, Theme, Renderer> {
    let t = design::tokens();
    let title_role = TextRole::TitleSm;
    let body_role = TextRole::Caption;

    let mut items: Vec<Element<'a, Message, Theme, Renderer>> = vec![
        icon(which, IconSize::Lg.px() * 1.5, t.fg_tertiary),
        text(title.to_string())
            .size(title_role.size())
            .line_height(title_role.line_height())
            .font(title_role.font())
            .color(t.fg_primary)
            .into(),
        text(description.to_string())
            .size(body_role.size())
            .line_height(body_role.line_height())
            .font(body_role.font())
            .color(t.fg_tertiary)
            .into(),
    ];

    if let Some(act) = action {
        items.push(container(act).padding([SPACE_1, 0.0]).into());
    }

    container(
        column(items)
            .spacing(SPACE_2)
            .align_x(Alignment::Center)
            .max_width(360.0),
    )
    .padding(SPACE_6)
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

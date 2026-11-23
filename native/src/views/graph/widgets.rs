//! Shared widget builders and styles for the graph views.

use super::*;

// ──────────────────────────── shared widgets ────────────────────────

pub(super) fn panel_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::BG_SECONDARY)),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    }
}

/// A control cluster that sits over the canvas: slightly translucent so the
/// graph behind it stays legible, but opaque enough to read against nodes.
pub(super) fn floating<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .padding([7, 9])
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.09, 0.10, 0.11, 0.92))),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 9.0.into(),
            },
            ..Default::default()
        })
        .into()
}

pub(super) fn divider<'a>() -> Element<'a, Message> {
    container(
        Space::new()
            .width(Length::Fixed(1.0))
            .height(Length::Fixed(16.0)),
    )
    .style(|_| container::Style {
        background: Some(Background::Color(theme::BORDER)),
        ..Default::default()
    })
    .into()
}

pub(super) fn segmented<'a>(segments: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    container(row(segments).spacing(2).align_y(Alignment::Center))
        .padding(2)
        .style(|_| container::Style {
            background: Some(Background::Color(theme::BG_PRIMARY)),
            border: Border {
                color: theme::BORDER_SUBTLE,
                width: 1.0,
                radius: 7.0.into(),
            },
            ..Default::default()
        })
        .into()
}

pub(super) fn segment<'a>(label: &'a str, active: bool, message: Message) -> Element<'a, Message> {
    button(text(label).size(11))
        .on_press(message)
        .padding([5, 10])
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(if active {
                    theme::ACCENT
                } else if hovered {
                    theme::BG_TERTIARY
                } else {
                    Color::TRANSPARENT
                })),
                text_color: if active {
                    theme::BG_PRIMARY
                } else if hovered {
                    theme::TEXT_PRIMARY
                } else {
                    theme::TEXT_SECONDARY
                },
                border: Border {
                    radius: 5.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .into()
}

/// A count that is also its own on/off switch.
pub(super) fn filter_chip<'a>(
    label: &'a str,
    count: usize,
    active: bool,
    accent: Color,
    message: Message,
) -> Element<'a, Message> {
    button(
        row![
            text("●")
                .size(7)
                .color(if active { accent } else { theme::TEXT_MUTED }),
            text(label).size(10),
            text(count.to_string())
                .size(10)
                .color(if active { accent } else { theme::TEXT_MUTED }),
        ]
        .spacing(5)
        .align_y(Alignment::Center),
    )
    .on_press(message)
    .padding([5, 9])
    .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(if active {
                theme::BG_TERTIARY
            } else if hovered {
                theme::BG_PRIMARY
            } else {
                Color::TRANSPARENT
            })),
            text_color: if active {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_MUTED
            },
            border: Border {
                color: if active {
                    theme::BORDER
                } else {
                    theme::BORDER_SUBTLE
                },
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        }
    })
    .into()
}

pub(super) fn icon_button<'a>(
    icon: Icon,
    active: bool,
    enabled: bool,
    message: Message,
) -> Element<'a, Message> {
    let color = if !enabled {
        theme::BORDER
    } else if active {
        theme::ACCENT
    } else {
        theme::TEXT_SECONDARY
    };
    button(icons::view(icon, color, 14.0))
        .on_press_maybe(enabled.then_some(message))
        .padding(6)
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(if active || hovered {
                    theme::BG_TERTIARY
                } else {
                    Color::TRANSPARENT
                })),
                text_color: color,
                border: Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .into()
}

pub(super) fn ghost_style(_: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then_some(Background::Color(theme::BG_TERTIARY)),
        text_color: if hovered {
            theme::TEXT_PRIMARY
        } else {
            theme::TEXT_MUTED
        },
        border: Border {
            radius: 5.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub(super) fn row_style(_: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then_some(Background::Color(theme::BG_TERTIARY)),
        text_color: if hovered {
            theme::TEXT_PRIMARY
        } else {
            theme::TEXT_SECONDARY
        },
        border: Border {
            radius: 5.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub(super) fn primary_style(_: &Theme, status: button::Status) -> button::Style {
    match status {
        button::Status::Disabled => button::Style {
            background: Some(Background::Color(theme::BG_TERTIARY)),
            text_color: theme::TEXT_MUTED,
            border: Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(Background::Color(theme::ACCENT_SECONDARY)),
            text_color: theme::BG_PRIMARY,
            border: Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        button::Status::Active => button::Style {
            background: Some(Background::Color(theme::ACCENT)),
            text_color: theme::BG_PRIMARY,
            border: Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
    }
}

/// Wrap a control in an explanatory tooltip. Icon-only buttons are compact but
/// opaque; a hover label is what makes them safe to use.
pub(super) fn hinted<'a>(
    content: impl Into<Element<'a, Message>>,
    tip: &'a str,
) -> Element<'a, Message> {
    tooltip(
        content,
        container(text(tip).size(10).color(theme::TEXT_SECONDARY))
            .padding([5, 8])
            .style(|_| container::Style {
                background: Some(Background::Color(theme::BG_TERTIARY)),
                border: Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            }),
        tooltip::Position::Bottom,
    )
    .gap(6)
    .padding(0)
    .snap_within_viewport(true)
    .into()
}

#[derive(Debug, Clone, Copy)]
pub(super) struct GraphSummary {
    pub(super) nodes: usize,
    pub(super) links: usize,
}

pub(super) fn summarize(graph: &GraphSnapshot) -> GraphSummary {
    GraphSummary {
        nodes: graph.nodes.len(),
        links: graph.edges.iter().map(|edge| edge.weight.max(1)).sum(),
    }
}

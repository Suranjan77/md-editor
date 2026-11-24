//! Canvas layers: GPU graph, label overlay, legend, viewport controls and empty states.

use super::*;
use crate::messages::GraphMessage;
use iced::widget::column;

// ──────────────────────────── canvas layers ─────────────────────────

pub(super) fn canvas_layers<'a>(state: &'a GraphState, frame: GraphFrame) -> Element<'a, Message> {
    // Bottom: GPU-rendered nodes/edges (also owns all pointer interaction).
    // Middle: transparent label canvas that never captures events.
    // Top: floating controls — only the buttons themselves take input, so
    // pans, drags, and clicks elsewhere fall straight through to the shader.
    let render_layer = iced::widget::shader(GpuGraphProgram::new(frame.clone()))
        .width(Length::Fill)
        .height(Length::Fill);
    let label_layer = canvas(GraphLabels::new(frame.clone()))
        .width(Length::Fill)
        .height(Length::Fill);

    let mut layers = stack![render_layer, label_layer]
        .width(Length::Fill)
        .height(Length::Fill);

    if frame.nodes().is_empty() {
        layers = layers.push(
            container(empty_state(state))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
        );
    }

    let notice: Element<'a, Message> = if state.orphans_dominate() {
        container(hairball_notice(state))
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .into()
    } else {
        Space::new().height(Length::Fixed(0.0)).into()
    };

    layers = layers.push(
        container(
            column![
                notice,
                Space::new().height(Length::Fill),
                row![
                    legend(),
                    Space::new().width(Length::Fill),
                    viewport_controls(state),
                ]
                .align_y(Alignment::End),
            ]
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(12),
    );

    layers.into()
}

/// Shown when almost nothing in the vault is linked. Drawing ten thousand
/// isolated dots is technically faithful and practically useless, so say so and
/// offer the one click that turns it into a readable graph.
pub(super) fn hairball_notice<'a>(state: &'a GraphState) -> Element<'a, Message> {
    let stats = state.stats();
    let total = stats.documents() + stats.missing;
    floating(
        row![
            text(format!(
                "{} of {total} notes have no links — the canvas is mostly isolated dots.",
                stats.unconnected
            ))
            .size(10)
            .color(theme::TEXT_SECONDARY),
            button(text("Hide unlinked").size(10))
                .on_press(Message::Graph(GraphMessage::ShowOrphansToggled(false)))
                .padding([4, 9])
                .style(primary_style),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
}

pub(super) fn legend<'a>() -> Element<'a, Message> {
    let swatch = |color: Color, label: &'a str| {
        row![
            text("●").size(9).color(color),
            text(label).size(10).color(theme::TEXT_MUTED),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
    };

    floating(
        column![
            row![
                swatch(kind_color(GraphNodeKind::Markdown), "Note"),
                swatch(kind_color(GraphNodeKind::Pdf), "PDF"),
                swatch(kind_color(GraphNodeKind::Missing), "Unresolved"),
            ]
            .spacing(10),
            text("Double-click opens · drag to move · right-click pins")
                .size(9)
                .color(theme::TEXT_MUTED),
        ]
        .spacing(5),
    )
}

pub(super) fn viewport_controls<'a>(state: &'a GraphState) -> Element<'a, Message> {
    let zoom = state.zoom;
    let zoom_label = if zoom < 0.1 {
        format!("{:.1}%", zoom * 100.0)
    } else {
        format!("{}%", (zoom * 100.0).round() as i32)
    };
    let running = state.physics.running;

    floating(
        row![
            hinted(
                icon_button(
                    Icon::Minus,
                    false,
                    zoom > MIN_ZOOM,
                    Message::Graph(GraphMessage::ZoomBy(0.8))
                ),
                "Zoom out",
            ),
            text(zoom_label)
                .size(10)
                .color(theme::TEXT_SECONDARY)
                .width(Length::Fixed(42.0))
                .align_x(Alignment::Center),
            hinted(
                icon_button(
                    Icon::Plus,
                    false,
                    true,
                    Message::Graph(GraphMessage::ZoomBy(1.25))
                ),
                "Zoom in",
            ),
            divider(),
            hinted(
                icon_button(
                    Icon::Maximize,
                    false,
                    true,
                    Message::Graph(GraphMessage::FitView)
                ),
                "Fit graph to view",
            ),
            hinted(
                icon_button(
                    if running { Icon::Pause } else { Icon::Play },
                    running,
                    true,
                    Message::Graph(GraphMessage::PhysicsToggled),
                ),
                if running {
                    "Pause layout — arrange nodes by hand"
                } else {
                    "Resume layout"
                },
            ),
            hinted(
                icon_button(
                    Icon::Refresh,
                    false,
                    true,
                    Message::Graph(GraphMessage::ResetLayout)
                ),
                "Rebuild layout from scratch",
            ),
        ]
        .spacing(3)
        .align_y(Alignment::Center),
    )
}

pub(super) fn empty_state<'a>(state: &'a GraphState) -> Element<'a, Message> {
    let (headline, detail, action): (String, String, Option<(&'a str, Message)>) =
        match state.empty_reason() {
            EmptyReason::Loading => (
                "Reading the vault…".to_string(),
                "Building the link graph from your notes.".to_string(),
                None,
            ),
            EmptyReason::NoDocuments => (
                "Nothing to graph yet".to_string(),
                "Add notes with [[wiki links]] and they will appear here.".to_string(),
                None,
            ),
            EmptyReason::NeedsActiveDocument => (
                "No note in focus".to_string(),
                "This view maps the neighbourhood around the note you have open.".to_string(),
                Some((
                    "Show the whole vault",
                    Message::Graph(GraphMessage::ScopeGlobal),
                )),
            ),
            EmptyReason::NoSearchMatch => (
                format!("No matches for “{}”", state.query.trim()),
                "Search looks at note titles and paths.".to_string(),
                Some((
                    "Clear search",
                    Message::Graph(GraphMessage::QueryChanged(String::new())),
                )),
            ),
            EmptyReason::FiltersHideEverything => (
                "Filters hide every node".to_string(),
                "Turn a chip back on in the toolbar to bring nodes back.".to_string(),
                Some(("Reset filters", Message::Graph(GraphMessage::FiltersReset))),
            ),
        };

    let mut content = column![
        text(headline).size(14).color(theme::TEXT_PRIMARY),
        text(detail).size(11).color(theme::TEXT_MUTED),
    ]
    .spacing(6)
    .align_x(Alignment::Center);

    if let Some((label, message)) = action {
        content = content.push(
            button(text(label).size(11))
                .on_press(message)
                .padding([7, 13])
                .style(primary_style),
        );
    }

    container(content)
        .padding([18, 24])
        .style(|_| container::Style {
            background: Some(Background::Color(theme::BG_SECONDARY)),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 10.0.into(),
            },
            ..Default::default()
        })
        .into()
}

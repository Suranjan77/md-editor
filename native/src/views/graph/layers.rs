//! Canvas layers: GPU graph, label overlay, legend, viewport controls and empty states.

use super::*;
use crate::design::metrics::{
    BORDER_WIDTH, ControlSize, Elevation, RADIUS_LG, SPACE_1, SPACE_2, SPACE_3,
};
use crate::messages::GraphMessage;
use crate::ui::{Button, card, toolbar_separator};
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
        .padding(SPACE_3),
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
            role_text(
                format!(
                    "{} of {total} notes have no links — the canvas is mostly isolated dots.",
                    stats.unconnected
                ),
                TextRole::Caption,
                tokens().fg_secondary,
            ),
            Button::primary("Hide unlinked")
                .size(ControlSize::Sm)
                .on_press(Message::Graph(GraphMessage::ShowOrphansToggled(false)))
                .build(),
        ]
        .spacing(SPACE_3)
        .align_y(Alignment::Center),
    )
}

/// Kind legend: an icon and a name per kind, so colour is never the only cue.
pub(super) fn legend<'a>() -> Element<'a, Message> {
    let t = tokens();
    let entry = |which: Icon, kind: GraphNodeKind, label: &'a str| {
        row![
            icon(which, IconSize::Sm.px(), kind_color(kind)),
            role_text(label, TextRole::Caption, t.fg_secondary),
        ]
        .spacing(SPACE_1)
        .align_y(Alignment::Center)
    };

    floating(
        column![
            row![
                entry(Icon::FileText, GraphNodeKind::Markdown, "Note"),
                entry(Icon::FilePdf, GraphNodeKind::Pdf, "PDF"),
                entry(Icon::WarningCircle, GraphNodeKind::Missing, "Unresolved"),
            ]
            .spacing(SPACE_3),
            role_text(
                "Double-click opens · drag to move · right-click pins",
                TextRole::Caption,
                t.fg_tertiary,
            ),
        ]
        .spacing(SPACE_1),
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
            IconButton::new(Icon::Minus, "Zoom out")
                .size(ControlSize::Sm)
                .on_press_maybe(
                    (zoom > MIN_ZOOM).then_some(Message::Graph(GraphMessage::ZoomBy(0.8)))
                )
                .build(),
            container(role_text(
                zoom_label,
                TextRole::MonoCaption,
                tokens().fg_secondary
            ))
            .width(Length::Fixed(ZOOM_LABEL_WIDTH))
            .center_x(Length::Fixed(ZOOM_LABEL_WIDTH)),
            IconButton::new(Icon::Plus, "Zoom in")
                .size(ControlSize::Sm)
                .on_press(Message::Graph(GraphMessage::ZoomBy(1.25)))
                .build(),
            toolbar_separator(),
            IconButton::new(Icon::Maximize, "Fit graph to view")
                .size(ControlSize::Sm)
                .on_press(Message::Graph(GraphMessage::FitView))
                .build(),
            IconButton::new(
                if running { Icon::Pause } else { Icon::Play },
                if running {
                    "Pause layout — arrange nodes by hand"
                } else {
                    "Resume layout"
                },
            )
            .size(ControlSize::Sm)
            .selected(running)
            .on_press(Message::Graph(GraphMessage::PhysicsToggled))
            .build(),
            IconButton::new(Icon::Refresh, "Rebuild layout from scratch")
                .size(ControlSize::Sm)
                .on_press(Message::Graph(GraphMessage::ResetLayout))
                .build(),
        ]
        .spacing(SPACE_1)
        .align_y(Alignment::Center),
    )
}

pub(super) fn empty_state<'a>(state: &'a GraphState) -> Element<'a, Message> {
    let (headline, detail, action): (String, &str, Option<(&'a str, Message)>) =
        match state.empty_reason() {
            EmptyReason::Loading => (
                "Reading the vault…".to_string(),
                "Building the link graph from your notes.",
                None,
            ),
            EmptyReason::NoDocuments => (
                "Nothing to graph yet".to_string(),
                "Add notes with [[wiki links]] and they will appear here.",
                None,
            ),
            EmptyReason::NeedsActiveDocument => (
                "No note in focus".to_string(),
                "This view maps the neighbourhood around the note you have open.",
                Some((
                    "Show the whole vault",
                    Message::Graph(GraphMessage::ScopeGlobal),
                )),
            ),
            EmptyReason::NoSearchMatch => (
                format!("No matches for “{}”", state.query.trim()),
                "Search looks at note titles and paths.",
                Some((
                    "Clear search",
                    Message::Graph(GraphMessage::QueryChanged(String::new())),
                )),
            ),
            EmptyReason::FiltersHideEverything => (
                "Filters hide every node".to_string(),
                "Turn a chip back on in the toolbar to bring nodes back.",
                Some(("Reset filters", Message::Graph(GraphMessage::FiltersReset))),
            ),
        };
    let action = action.map(|(label, message)| Button::primary(label).on_press(message).build());
    card(crate::ui::empty_state(
        Icon::Network,
        &headline,
        detail,
        action,
    ))
}

/// A control cluster over the canvas: translucent so the graph behind stays
/// legible, opaque enough to read against nodes.
fn floating<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .padding([SPACE_2, SPACE_3])
        .style(|_| {
            let t = tokens();
            container::Style {
                background: Some(Background::Color(crate::design::fade(t.bg_raised, 0.92))),
                border: Border {
                    color: t.border_default,
                    width: BORDER_WIDTH,
                    radius: RADIUS_LG.into(),
                },
                shadow: Elevation::Popover.shadow(),
                ..Default::default()
            }
        })
        .into()
}

/// Room for "100%" in tabular figures.
const ZOOM_LABEL_WIDTH: f32 = 44.0;

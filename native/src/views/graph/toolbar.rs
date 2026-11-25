//! The toolbar: scope, depth, search, filter chips, counts and view controls, on shared components.

use super::*;
use crate::design::metrics::{ControlSize, SPACE_2, SPACE_3};
use crate::messages::GraphMessage;
use crate::ui::{Button, SegmentOption, TextField, filter_chip, segmented, toolbar_frame};
use iced::widget::column;

pub(super) fn toolbar<'a>(
    state: &'a GraphState,
    summary: &GraphSummary,
    stats: &crate::graph_state::GraphStats,
) -> Element<'a, Message> {
    let t = tokens();
    let local_depth = match state.scope {
        GraphScope::Global => 1,
        GraphScope::Local { depth } => depth,
    };
    let is_local = matches!(state.scope, GraphScope::Local { .. });

    let mut primary: Vec<Element<'a, Message>> = vec![
        icon(Icon::Network, IconSize::Md.px(), t.accent),
        role_text("Knowledge graph", TextRole::TitleSm, t.fg_primary),
        segmented(
            vec![
                SegmentOption::new(
                    "Whole vault",
                    !is_local,
                    Message::Graph(GraphMessage::ScopeGlobal),
                ),
                SegmentOption::new(
                    "Around this note",
                    is_local,
                    Message::Graph(GraphMessage::ScopeLocal),
                ),
            ],
            ControlSize::Sm,
        ),
    ];
    // The depth choice only exists inside local scope, so it appears with it
    // rather than sitting permanently greyed out.
    if is_local {
        primary.push(segmented(
            vec![
                SegmentOption::new(
                    "1 hop",
                    local_depth == 1,
                    Message::Graph(GraphMessage::LocalDepthChanged(1)),
                ),
                SegmentOption::new(
                    "2 hops",
                    local_depth == 2,
                    Message::Graph(GraphMessage::LocalDepthChanged(2)),
                ),
            ],
            ControlSize::Sm,
        ));
    }
    primary.push(Space::new().width(Length::Fill).into());

    let query = state.query.trim();
    if !query.is_empty() {
        let matches = state.query_match_count();
        primary.push(role_text(
            if matches == 1 {
                "1 match".to_string()
            } else {
                format!("{matches} matches")
            },
            TextRole::MonoCaption,
            if matches == 0 {
                t.status_danger
            } else {
                t.fg_secondary
            },
        ));
    }
    primary.push(
        container(
            TextField::search("Search notes…", &state.query)
                .size(ControlSize::Sm)
                .on_input(|value| Message::Graph(GraphMessage::QueryChanged(value)))
                .on_clear(Message::Graph(GraphMessage::QueryChanged(String::new())))
                .build(),
        )
        .width(Length::Fixed(SEARCH_WIDTH))
        .into(),
    );
    primary.push(
        IconButton::new(
            Icon::PanelRight,
            if state.inspector_visible {
                "Hide inspector"
            } else {
                "Show inspector"
            },
        )
        .selected(state.inspector_visible)
        .on_press(Message::Graph(GraphMessage::InspectorToggled))
        .build(),
    );
    primary.push(
        IconButton::new(Icon::X, "Close graph")
            .shortcut("Esc")
            .on_press(Message::Graph(GraphMessage::Toggle))
            .build(),
    );

    let secondary = filter_row(state, summary, stats, query);

    column![
        toolbar_frame(primary, false),
        container(secondary).padding([SPACE_2, SPACE_3]),
    ]
    .into()
}

/// Width of the graph's search field.
const SEARCH_WIDTH: f32 = 240.0;

/// Second row: filter chips with their counts, Reset, pinned nodes and totals.
fn filter_row<'a>(
    state: &'a GraphState,
    summary: &GraphSummary,
    stats: &crate::graph_state::GraphStats,
    query: &str,
) -> Element<'a, Message> {
    let t = tokens();
    // Counts double as the filter controls: a chip shows how many nodes of a
    // kind exist *and* whether they are on canvas, so there is one place to
    // look and one place to click.
    let mut secondary: Vec<Element<'a, Message>> = vec![
        filter_chip(
            &format!("PDFs {}", stats.pdfs),
            state.show_pdfs,
            Some(Icon::FilePdf),
            Message::Graph(GraphMessage::ShowPdfsToggled(!state.show_pdfs)),
        ),
        filter_chip(
            &format!("Unresolved {}", stats.missing),
            state.show_missing,
            Some(Icon::WarningCircle),
            Message::Graph(GraphMessage::ShowMissingToggled(!state.show_missing)),
        ),
        filter_chip(
            &format!("Orphans {}", stats.unconnected),
            state.show_orphans,
            Some(Icon::Info),
            Message::Graph(GraphMessage::ShowOrphansToggled(!state.show_orphans)),
        ),
    ];
    if state.filters_active() || !query.is_empty() {
        secondary.push(
            Button::ghost("Reset")
                .size(ControlSize::Sm)
                .on_press(Message::Graph(GraphMessage::FiltersReset))
                .build(),
        );
    }
    secondary.push(Space::new().width(Length::Fill).into());

    let pinned = state.pinned_count();
    if pinned > 0 {
        secondary.push(
            row![
                icon(Icon::PinFill, IconSize::Sm.px(), t.accent),
                role_text(
                    format!("{pinned} pinned"),
                    TextRole::Caption,
                    t.fg_secondary
                ),
                Button::ghost("Release")
                    .size(ControlSize::Sm)
                    .on_press(Message::Graph(GraphMessage::UnpinAll))
                    .tooltip("Let the layout move pinned nodes again", None)
                    .build(),
            ]
            .spacing(SPACE_2)
            .align_y(Alignment::Center)
            .into(),
        );
    }
    let total_nodes = stats.documents() + stats.missing;
    secondary.push(role_text(
        format!(
            "{} of {total_nodes} nodes · {} links · {} clusters",
            summary.nodes, summary.links, stats.components
        ),
        TextRole::Caption,
        t.fg_tertiary,
    ));

    row(secondary)
        .spacing(SPACE_2)
        .align_y(Alignment::Center)
        .into()
}

//! The toolbar: scope, depth, search, filters and view controls.

use super::*;
use iced::widget::column;

// ─────────────────────────────── toolbar ────────────────────────────

pub(super) fn toolbar<'a>(
    state: &'a GraphState,
    summary: &GraphSummary,
    stats: &crate::graph_state::GraphStats,
) -> Element<'a, Message> {
    let local_depth = match state.scope {
        GraphScope::Global => 1,
        GraphScope::Local { depth } => depth,
    };
    let is_local = matches!(state.scope, GraphScope::Local { .. });

    let scope_control = segmented(vec![
        segment("Whole vault", !is_local, Message::GraphScopeGlobal),
        segment("Around this note", is_local, Message::GraphScopeLocal),
    ]);

    // The depth choice only exists inside local scope, so it appears with it
    // rather than sitting permanently greyed out.
    let depth_control: Element<'a, Message> = if is_local {
        segmented(vec![
            segment(
                "1 hop",
                local_depth == 1,
                Message::GraphLocalDepthChanged(1),
            ),
            segment(
                "2 hops",
                local_depth == 2,
                Message::GraphLocalDepthChanged(2),
            ),
        ])
    } else {
        Space::new().width(Length::Fixed(0.0)).into()
    };

    let query = state.query.trim();
    let match_readout: Element<'a, Message> = if query.is_empty() {
        Space::new().width(Length::Fixed(0.0)).into()
    } else {
        let matches = state.query_match_count();
        text(format!("{matches}"))
            .size(10)
            .color(if matches == 0 {
                theme::DANGER
            } else {
                theme::ACCENT
            })
            .into()
    };
    let clear_search: Element<'a, Message> = if query.is_empty() {
        Space::new().width(Length::Fixed(0.0)).into()
    } else {
        button(icons::view(Icon::X, theme::TEXT_MUTED, 11.0))
            .on_press(Message::GraphQueryChanged(String::new()))
            .padding(3)
            .style(ghost_style)
            .into()
    };

    let search = container(
        row![
            icons::view(Icon::Search, theme::TEXT_MUTED, 13.0),
            text_input("Search notes…", &state.query)
                .on_input(Message::GraphQueryChanged)
                .padding(0)
                .size(12)
                .width(Length::Fill)
                .style(|_, _| text_input::Style {
                    background: Background::Color(Color::TRANSPARENT),
                    border: Border::default(),
                    icon: theme::TEXT_MUTED,
                    placeholder: theme::TEXT_MUTED,
                    value: theme::TEXT_PRIMARY,
                    selection: theme::ACCENT_DIM,
                }),
            match_readout,
            clear_search,
        ]
        .spacing(7)
        .align_y(Alignment::Center),
    )
    .padding([6, 9])
    .width(Length::Fixed(238.0))
    .style(|_| container::Style {
        background: Some(Background::Color(theme::BG_PRIMARY)),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 7.0.into(),
        },
        ..Default::default()
    });

    let primary_row = row![
        icons::view(Icon::Network, theme::ACCENT, 16.0),
        text("Knowledge Graph").size(13).color(theme::TEXT_PRIMARY),
        Space::new().width(Length::Fixed(6.0)),
        scope_control,
        depth_control,
        Space::new().width(Length::Fill),
        search,
        hinted(
            icon_button(
                Icon::PanelRight,
                state.inspector_visible,
                true,
                Message::GraphInspectorToggled,
            ),
            if state.inspector_visible {
                "Hide inspector"
            } else {
                "Show inspector"
            },
        ),
        hinted(
            icon_button(Icon::X, false, true, Message::GraphToggle),
            "Close graph  ·  Esc",
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // Counts double as the filter controls: a chip shows how many nodes of a
    // kind exist *and* whether they are currently on canvas, so there is only
    // one place to look and one place to click.
    let mut filters = row![
        filter_chip(
            "PDFs",
            stats.pdfs,
            state.show_pdfs,
            kind_color(GraphNodeKind::Pdf),
            Message::GraphShowPdfsToggled(!state.show_pdfs),
        ),
        filter_chip(
            "Unresolved",
            stats.missing,
            state.show_missing,
            theme::DANGER,
            Message::GraphShowMissingToggled(!state.show_missing),
        ),
        filter_chip(
            "Orphans",
            stats.unconnected,
            state.show_orphans,
            theme::WARNING,
            Message::GraphShowOrphansToggled(!state.show_orphans),
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    if state.filters_active() || !query.is_empty() {
        filters = filters.push(
            button(text("Reset").size(10))
                .on_press(Message::GraphFiltersReset)
                .padding([4, 8])
                .style(ghost_style),
        );
    }

    let pinned = state.pinned_count();
    let pin_readout: Element<'a, Message> = if pinned == 0 {
        Space::new().width(Length::Fixed(0.0)).into()
    } else {
        row![
            icons::view(Icon::Pin, theme::ACCENT, 11.0),
            text(format!("{pinned} pinned"))
                .size(10)
                .color(theme::ACCENT),
            button(text("release").size(10))
                .on_press(Message::GraphUnpinAll)
                .padding([3, 6])
                .style(ghost_style),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into()
    };

    let total_nodes = stats.documents() + stats.missing;
    let secondary_row = row![
        filters,
        Space::new().width(Length::Fill),
        pin_readout,
        text(format!(
            "{} of {total_nodes} nodes · {} links · {} clusters",
            summary.nodes, summary.links, stats.components
        ))
        .size(10)
        .color(theme::TEXT_MUTED),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    container(
        column![
            container(primary_row).padding([9, 12]),
            container(Space::new().height(Length::Fixed(1.0)).width(Length::Fill)).style(|_| {
                container::Style {
                    background: Some(Background::Color(theme::BORDER_SUBTLE)),
                    ..Default::default()
                }
            }),
            container(secondary_row).padding([7, 12]),
        ]
        .spacing(0),
    )
    .width(Length::Fill)
    .style(panel_style)
    .into()
}

//! The inspector panel: the selected node, its links, related notes and vault insights.

use super::*;
use crate::messages::GraphMessage;
use iced::widget::column;

// ─────────────────────────────── inspector ──────────────────────────

pub(super) fn inspector<'a>(state: &'a GraphState) -> Element<'a, Message> {
    let selected = state
        .selected_path
        .as_deref()
        .and_then(|path| state.node(path));

    // Node detail and vault-wide insights are alternatives, not a stack: a
    // selected node gets the whole panel, and the insight lists come back the
    // moment nothing is selected. That keeps the panel one screen tall.
    let content = match selected {
        Some(node) => node_card(state, node),
        None => insights(state),
    };

    container(scrollable(container(content).padding(11)).height(Length::Fill))
        .width(Length::Fixed(INSPECTOR_WIDTH))
        .height(Length::Fill)
        .style(panel_style)
        .into()
}

pub(super) fn node_card<'a>(state: &'a GraphState, node: &'a GraphNode) -> Element<'a, Message> {
    let kind_label = match node.kind {
        GraphNodeKind::Markdown => "Markdown note",
        GraphNodeKind::Pdf => "PDF reference",
        GraphNodeKind::Missing => "Unresolved link target",
    };
    let pinned = state.is_pinned(&node.path);

    let header = column![
        row![
            text("●").size(9).color(kind_color(node.kind)),
            text(kind_label).size(10).color(kind_color(node.kind)),
            Space::new().width(Length::Fill),
            button(text("Clear").size(10))
                .on_press(Message::Graph(GraphMessage::NodeSelected(None)))
                .padding([3, 7])
                .style(ghost_style),
        ]
        .spacing(5)
        .align_y(Alignment::Center),
        text(node.label.clone()).size(16).color(theme::TEXT_PRIMARY),
        text(node.path.clone()).size(9).color(theme::TEXT_MUTED),
    ]
    .spacing(4);

    let actions = row![
        button(text("Open note").size(11))
            .on_press_maybe(
                node.exists
                    .then(|| Message::Graph(GraphMessage::NodeOpen(node.path.clone())))
            )
            .padding([7, 12])
            .style(primary_style),
        hinted(
            icon_button(
                Icon::Target,
                false,
                true,
                Message::Graph(GraphMessage::NodeFocused(node.path.clone())),
            ),
            "Center on this node",
        ),
        hinted(
            icon_button(
                Icon::Pin,
                pinned,
                true,
                Message::Graph(GraphMessage::PinToggled(node.path.clone())),
            ),
            if pinned {
                "Release — let the layout move it again"
            } else {
                "Pin — hold this node in place"
            },
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let mut card = column![
        header,
        actions,
        row![
            metric("In", node.incoming),
            metric("Out", node.outgoing),
            metric("Degree", node.incoming + node.outgoing),
        ]
        .spacing(6),
    ]
    .spacing(10);

    if !node.exists {
        card = card.push(note_banner(
            "Nothing links here yet — this target does not exist on disk.",
        ));
    }

    let incoming = state.incoming(&node.path);
    let outgoing = state.outgoing(&node.path);
    let related: Vec<(String, String, String)> = state
        .related(&node.path, 6)
        .into_iter()
        .map(|item| {
            let detail = if item.shared_connections == 1 {
                "1 shared link".to_string()
            } else {
                format!("{} shared links", item.shared_connections)
            };
            (item.node.path, item.node.label, detail)
        })
        .collect();

    card = card
        .push(node_list(
            "LINKS TO",
            outgoing,
            theme::TEXT_SECONDARY,
            "Nothing yet",
        ))
        .push(node_list(
            "LINKED FROM",
            incoming,
            theme::TEXT_SECONDARY,
            "No backlinks",
        ))
        .push(link_list(
            "SIMILAR NOTES",
            related,
            theme::ACCENT_SECONDARY,
            "No shared neighbours",
        ));

    card.into()
}

pub(super) fn insights<'a>(state: &'a GraphState) -> Element<'a, Message> {
    let stats = state.stats();

    let hubs = state
        .hubs(6)
        .into_iter()
        .filter(|node| node.incoming + node.outgoing > 0)
        .map(|node| {
            let degree = node.incoming + node.outgoing;
            (node.path, node.label, format!("{degree} links"))
        })
        .collect();
    let orphans = state
        .unconnected(6)
        .into_iter()
        .map(|node| (node.path, node.label, "unlinked".to_string()))
        .collect();
    let unresolved = state
        .missing(6)
        .into_iter()
        .map(|node| (node.path, node.label, "missing".to_string()))
        .collect();

    column![
        column![
            text("VAULT OVERVIEW").size(9).color(theme::ACCENT),
            text(format!(
                "{} documents · {} connections · {} clusters",
                stats.documents(),
                stats.connections,
                stats.components
            ))
            .size(11)
            .color(theme::TEXT_SECONDARY),
            text("Select a node on the canvas to inspect it.")
                .size(10)
                .color(theme::TEXT_MUTED),
        ]
        .spacing(4),
        link_list(
            "MOST CONNECTED",
            hubs,
            theme::ACCENT,
            "No links in this vault yet",
        ),
        link_list(
            "UNLINKED NOTES",
            orphans,
            theme::WARNING,
            "Every note is connected",
        ),
        link_list("BROKEN LINKS", unresolved, theme::DANGER, "No broken links",),
    ]
    .spacing(10)
    .into()
}

pub(super) fn note_banner<'a>(message: &'a str) -> Element<'a, Message> {
    container(text(message).size(10).color(theme::DANGER))
        .padding([7, 9])
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(theme::BG_PRIMARY)),
            border: Border {
                color: theme::DANGER,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        })
        .into()
}

pub(super) fn metric<'a>(label: &'static str, value: usize) -> Element<'a, Message> {
    container(
        column![
            text(value.to_string()).size(15).color(theme::ACCENT),
            text(label).size(9).color(theme::TEXT_MUTED),
        ]
        .spacing(1)
        .align_x(Alignment::Center),
    )
    .padding([7, 8])
    .width(Length::FillPortion(1))
    .style(|_| container::Style {
        background: Some(Background::Color(theme::BG_TERTIARY)),
        border: Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

pub(super) fn node_list<'a>(
    title: &'static str,
    nodes: Vec<GraphNode>,
    accent: Color,
    empty: &'static str,
) -> Element<'a, Message> {
    let items = nodes
        .into_iter()
        .map(|node| {
            let degree = node.incoming + node.outgoing;
            (node.path, node.label, format!("{degree} links"))
        })
        .collect();
    link_list(title, items, accent, empty)
}

/// A titled list where every row reveals its node on the canvas. Clicking a
/// name in the inspector and *not* being shown where it is was the main way to
/// get lost in the old panel, so rows recenter the view rather than only
/// changing the selection.
pub(super) fn link_list<'a>(
    title: &'static str,
    items: Vec<(String, String, String)>,
    accent: Color,
    empty: &'static str,
) -> Element<'a, Message> {
    let mut list = column![].spacing(1);
    if items.is_empty() {
        list = list.push(text(empty).size(10).color(theme::TEXT_MUTED));
    } else {
        for (path, label, detail) in items {
            list = list.push(
                button(
                    row![
                        text("●").size(7).color(accent),
                        text(label).size(11),
                        Space::new().width(Length::Fill),
                        text(detail).size(9).color(theme::TEXT_MUTED),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .on_press(Message::Graph(GraphMessage::NodeFocused(path)))
                .padding([5, 6])
                .width(Length::Fill)
                .style(row_style),
            );
        }
    }

    container(column![text(title).size(9).color(accent), list,].spacing(5))
        .padding(9)
        .width(Length::Fill)
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

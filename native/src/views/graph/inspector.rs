//! The inspector panel: the selected node, its links, related notes and vault insights, as a standard right `Panel`.

use super::*;
use crate::design::metrics::{ControlSize, SPACE_1, SPACE_2, SPACE_3};
use crate::messages::GraphMessage;
use crate::ui::{Button, ListRowProps, PanelSide, list_row, panel, stat_card};
use iced::widget::column;

pub(super) fn inspector<'a>(state: &'a GraphState) -> Element<'a, Message> {
    let selected = state
        .selected_path
        .as_deref()
        .and_then(|path| state.node(path));

    // Node detail and vault-wide insights are alternatives, not a stack: a
    // selected node gets the whole panel, and the insight lists come back the
    // moment nothing is selected. That keeps the panel one screen tall.
    let (title, actions, content): (&str, Vec<Element<'a, Message>>, Element<'a, Message>) =
        match selected {
            Some(node) => (
                "Node",
                vec![
                    IconButton::new(Icon::X, "Clear selection")
                        .size(ControlSize::Sm)
                        .on_press(Message::Graph(GraphMessage::NodeSelected(None)))
                        .build(),
                ],
                node_card(state, node),
            ),
            None => ("Overview", Vec::new(), insights(state)),
        };

    container(panel(
        title,
        None,
        actions,
        scrollable(container(content).padding(SPACE_3)).height(Length::Fill),
        PanelSide::Right,
        false,
    ))
    .width(Length::Fixed(INSPECTOR_WIDTH))
    .height(Length::Fill)
    .into()
}

pub(super) fn node_card<'a>(state: &'a GraphState, node: &'a GraphNode) -> Element<'a, Message> {
    let t = tokens();
    let kind_label = match node.kind {
        GraphNodeKind::Markdown => "Markdown note",
        GraphNodeKind::Pdf => "PDF reference",
        GraphNodeKind::Missing => "Unresolved link target",
    };
    let kind_icon = match node.kind {
        GraphNodeKind::Markdown => Icon::FileText,
        GraphNodeKind::Pdf => Icon::FilePdf,
        GraphNodeKind::Missing => Icon::WarningCircle,
    };
    let pinned = state.is_pinned(&node.path);

    let header = column![
        row![
            icon(kind_icon, IconSize::Sm.px(), kind_color(node.kind)),
            role_text(kind_label, TextRole::Caption, t.fg_secondary),
        ]
        .spacing(SPACE_1)
        .align_y(Alignment::Center),
        role_text(node.label.clone(), TextRole::Title, t.fg_primary),
        role_text(node.path.clone(), TextRole::MonoCaption, t.fg_tertiary),
    ]
    .spacing(SPACE_1);

    let actions = row![
        Button::primary("Open")
            .size(ControlSize::Sm)
            .icon(Icon::ArrowSquareOut)
            .on_press_maybe(
                node.exists
                    .then(|| Message::Graph(GraphMessage::NodeOpen(node.path.clone())))
            )
            .build(),
        IconButton::new(Icon::Target, "Centre on this node")
            .on_press(Message::Graph(GraphMessage::NodeFocused(node.path.clone())))
            .build(),
        IconButton::new(
            Icon::Pin,
            if pinned {
                "Release — let the layout move it again"
            } else {
                "Pin — hold this node in place"
            },
        )
        .selected(pinned)
        .on_press(Message::Graph(GraphMessage::PinToggled(node.path.clone())))
        .build(),
    ]
    .spacing(SPACE_2)
    .align_y(Alignment::Center);

    let mut card = column![
        header,
        actions,
        row![
            stat_card("In", &node.incoming.to_string(), None, None),
            stat_card("Out", &node.outgoing.to_string(), None, None),
            stat_card(
                "Degree",
                &(node.incoming + node.outgoing).to_string(),
                None,
                None
            ),
        ]
        .spacing(SPACE_2),
    ]
    .spacing(SPACE_3);

    if !node.exists {
        card = card.push(crate::ui::badge(
            "No file on disk — only links point here",
            crate::ui::BadgeTone::Danger,
            Some(Icon::WarningCircle),
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

    card.push(node_list("Links to", outgoing, "Nothing yet"))
        .push(node_list("Linked from", incoming, "No backlinks"))
        .push(link_list("Similar notes", related, "No shared neighbours"))
        .into()
}

pub(super) fn insights<'a>(state: &'a GraphState) -> Element<'a, Message> {
    let t = tokens();
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
        row![
            stat_card("Notes", &stats.documents().to_string(), None, None),
            stat_card("Links", &stats.connections.to_string(), None, None),
            stat_card("Clusters", &stats.components.to_string(), None, None),
        ]
        .spacing(SPACE_2),
        role_text(
            "Select a node on the canvas to inspect it.",
            TextRole::Caption,
            t.fg_tertiary
        ),
        link_list("Most connected", hubs, "No links in this vault yet"),
        link_list("Unlinked notes", orphans, "Every note is connected"),
        link_list("Broken links", unresolved, "No broken links"),
    ]
    .spacing(SPACE_3)
    .into()
}

pub(super) fn node_list<'a>(
    title: &'static str,
    nodes: Vec<GraphNode>,
    empty: &'static str,
) -> Element<'a, Message> {
    let items = nodes
        .into_iter()
        .map(|node| {
            let degree = node.incoming + node.outgoing;
            (node.path, node.label, format!("{degree} links"))
        })
        .collect();
    link_list(title, items, empty)
}

/// A titled list where every row reveals its node on the canvas: clicking a
/// name and *not* being shown where it is was the main way to get lost, so
/// rows recentre the view rather than only changing the selection.
pub(super) fn link_list<'a>(
    title: &'static str,
    items: Vec<(String, String, String)>,
    empty: &'static str,
) -> Element<'a, Message> {
    let t = tokens();
    let mut list = column![role_text(title, TextRole::Label, t.fg_tertiary)].spacing(SPACE_1);
    if items.is_empty() {
        list = list.push(role_text(empty, TextRole::Caption, t.fg_tertiary));
    }
    for (path, label, detail) in items {
        list = list.push(list_row(ListRowProps {
            title: label.into(),
            subtitle: Some(detail.into()),
            leading_icon: None,
            shortcut: None,
            selected: false,
            active_doc: false,
            focused: false,
            drag_over: false,
            max_title_chars: 30,
            on_press: Some(Message::Graph(GraphMessage::NodeFocused(path))),
        }));
    }
    list.into()
}

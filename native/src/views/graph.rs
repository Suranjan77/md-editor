use std::collections::HashSet;
use std::sync::Arc;

use iced::widget::{
    Space, button, canvas, column, container, row, scrollable, stack, text, text_input, tooltip,
};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme,
    Vector, mouse,
};

use md_editor_core::types::{GraphNode, GraphNodeKind, GraphSnapshot};

use crate::graph_state::{
    EmptyReason, FilteredGraph, GraphPoint, GraphScope, GraphState, MIN_ZOOM,
};
use crate::messages::Message;
use crate::theme;
use crate::views::gpu_graph::GpuGraphProgram;
use crate::views::icons::{self, Icon};

const INSPECTOR_WIDTH: f32 = 324.0;
/// Below this node count every label fits on screen, so the density budget is
/// skipped and everything is drawn.
const LABEL_ALL_BELOW: usize = 60;

/// An immutable, per-frame handle on everything both graph render layers need:
/// the filtered topology, node world positions, and the shared pan/zoom
/// transform. Built once per `view` and cloned into the GPU
/// [`shader`](iced::widget::shader) layer and the label [`canvas`] layer so the
/// two never disagree about where a node is.
///
/// The heavy parts are [`Arc`]s owned and cached by [`GraphState`], so building
/// a frame is O(1) in the graph size. Nodes are addressed by their position in
/// `graph.snapshot.nodes` rather than by path: at vault scale, hashing a path
/// per node per frame is the whole frame budget.
#[derive(Debug, Clone)]
pub(crate) struct GraphFrame {
    pub graph: Arc<FilteredGraph>,
    pub positions: Arc<Vec<GraphPoint>>,
    pub pinned: HashSet<usize>,
    pub selected: Option<usize>,
    pub hovered: Option<usize>,
    pub active: Option<usize>,
    pub pan: Vector,
    pub zoom: f32,
    pub fit_revision: u64,
    pub view_revision: u64,
}

impl GraphFrame {
    pub(crate) fn from_state(state: &GraphState) -> Self {
        let graph = state.filtered();
        let resolve = |path: Option<&String>| path.and_then(|p| graph.position_of(p));
        let pinned = state
            .pinned_paths()
            .iter()
            .filter_map(|path| graph.position_of(path))
            .collect();
        Self {
            selected: resolve(state.selected_path.as_ref()),
            hovered: resolve(state.hovered_path.as_ref()),
            active: resolve(state.active_path.as_ref()),
            pinned,
            positions: state.frame_positions(),
            graph,
            pan: Vector::new(state.pan.0, state.pan.1),
            zoom: state.zoom,
            fit_revision: state.fit_revision,
            view_revision: state.view_revision,
        }
    }

    pub(crate) fn nodes(&self) -> &[GraphNode] {
        &self.graph.snapshot.nodes
    }

    pub(crate) fn world(&self, index: usize) -> Point {
        self.positions
            .get(index)
            .map_or(Point::ORIGIN, |p| Point::new(p.x, p.y))
    }

    /// A `nodes()`-aligned mask of the focused node and its direct neighbours.
    /// Built once per draw so the per-node test is an index, not a set lookup.
    pub(crate) fn focus_mask(&self) -> Option<(usize, Vec<bool>)> {
        let focus = self.hovered.or(self.selected)?;
        let mut mask = vec![false; self.nodes().len()];
        if let Some(slot) = mask.get_mut(focus) {
            *slot = true;
        }
        for neighbor in self.graph.neighbors.get(focus).into_iter().flatten() {
            if let Some(slot) = mask.get_mut(*neighbor as usize) {
                *slot = true;
            }
        }
        Some((focus, mask))
    }
}

/// Complete research-graph workspace.
///
/// The layout is deliberately canvas-first: one slim toolbar, then the graph
/// filling everything that is left, with viewport controls and the legend
/// *floating* over the canvas instead of stealing rows from it. The GPU shader
/// layer renders nodes/edges and owns all interaction; the label canvas above it
/// draws crisp text and the hover card; a third, inert overlay layer holds the
/// floating controls. Durable filters, selection, pins, moved-node positions,
/// the shared transform, and fit requests all live in [`GraphState`].
pub fn view(state: &GraphState) -> Element<'_, Message, Theme, Renderer> {
    if !state.visible {
        return container(Space::new())
            .width(Length::Fixed(0.0))
            .height(Length::Fixed(0.0))
            .into();
    }

    let frame = GraphFrame::from_state(state);
    let summary = summarize(&frame.graph.snapshot);
    let stats = state.stats();

    let canvas_panel = container(canvas_layers(state, frame))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(theme::BG_PRIMARY)),
            border: Border {
                color: theme::BORDER_SUBTLE,
                width: 1.0,
                radius: 10.0.into(),
            },
            ..Default::default()
        });

    let mut body = row![canvas_panel].spacing(10).height(Length::Fill);
    if state.inspector_visible {
        body = body.push(inspector(state));
    }

    container(column![toolbar(state, &summary, &stats), body].spacing(10))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(12)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.88))),
            ..Default::default()
        })
        .into()
}

// ─────────────────────────────── toolbar ────────────────────────────

fn toolbar<'a>(
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
            text(format!("{pinned} pinned")).size(10).color(theme::ACCENT),
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

// ──────────────────────────── canvas layers ─────────────────────────

fn canvas_layers<'a>(state: &'a GraphState, frame: GraphFrame) -> Element<'a, Message> {
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
fn hairball_notice<'a>(state: &'a GraphState) -> Element<'a, Message> {
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
                .on_press(Message::GraphShowOrphansToggled(false))
                .padding([4, 9])
                .style(primary_style),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
}

fn legend<'a>() -> Element<'a, Message> {
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

fn viewport_controls<'a>(state: &'a GraphState) -> Element<'a, Message> {
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
                icon_button(Icon::Minus, false, zoom > MIN_ZOOM, Message::GraphZoomBy(0.8)),
                "Zoom out",
            ),
            text(zoom_label)
                .size(10)
                .color(theme::TEXT_SECONDARY)
                .width(Length::Fixed(42.0))
                .align_x(Alignment::Center),
            hinted(
                icon_button(Icon::Plus, false, true, Message::GraphZoomBy(1.25)),
                "Zoom in",
            ),
            divider(),
            hinted(
                icon_button(Icon::Maximize, false, true, Message::GraphFitView),
                "Fit graph to view",
            ),
            hinted(
                icon_button(
                    if running { Icon::Pause } else { Icon::Play },
                    running,
                    true,
                    Message::GraphPhysicsToggled,
                ),
                if running {
                    "Pause layout — arrange nodes by hand"
                } else {
                    "Resume layout"
                },
            ),
            hinted(
                icon_button(Icon::Refresh, false, true, Message::GraphResetLayout),
                "Rebuild layout from scratch",
            ),
        ]
        .spacing(3)
        .align_y(Alignment::Center),
    )
}

fn empty_state<'a>(state: &'a GraphState) -> Element<'a, Message> {
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
                Some(("Show the whole vault", Message::GraphScopeGlobal)),
            ),
            EmptyReason::NoSearchMatch => (
                format!("No matches for “{}”", state.query.trim()),
                "Search looks at note titles and paths.".to_string(),
                Some(("Clear search", Message::GraphQueryChanged(String::new()))),
            ),
            EmptyReason::FiltersHideEverything => (
                "Filters hide every node".to_string(),
                "Turn a chip back on in the toolbar to bring nodes back.".to_string(),
                Some(("Reset filters", Message::GraphFiltersReset)),
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

// ─────────────────────────────── inspector ──────────────────────────

fn inspector<'a>(state: &'a GraphState) -> Element<'a, Message> {
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

fn node_card<'a>(state: &'a GraphState, node: &'a GraphNode) -> Element<'a, Message> {
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
                .on_press(Message::GraphNodeSelected(None))
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
            .on_press_maybe(node.exists.then(|| Message::GraphNodeOpen(node.path.clone())))
            .padding([7, 12])
            .style(primary_style),
        hinted(
            icon_button(
                Icon::Target,
                false,
                true,
                Message::GraphNodeFocused(node.path.clone()),
            ),
            "Center on this node",
        ),
        hinted(
            icon_button(
                Icon::Pin,
                pinned,
                true,
                Message::GraphPinToggled(node.path.clone()),
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

fn insights<'a>(state: &'a GraphState) -> Element<'a, Message> {
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
        link_list(
            "BROKEN LINKS",
            unresolved,
            theme::DANGER,
            "No broken links",
        ),
    ]
    .spacing(10)
    .into()
}

fn note_banner<'a>(message: &'a str) -> Element<'a, Message> {
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

fn metric<'a>(label: &'static str, value: usize) -> Element<'a, Message> {
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

fn node_list<'a>(
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
fn link_list<'a>(
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
                .on_press(Message::GraphNodeFocused(path))
                .padding([5, 6])
                .width(Length::Fill)
                .style(row_style),
            );
        }
    }

    container(
        column![
            text(title).size(9).color(accent),
            list,
        ]
        .spacing(5),
    )
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

// ──────────────────────────── shared widgets ────────────────────────

fn panel_style(_: &Theme) -> container::Style {
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
fn floating<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
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

fn divider<'a>() -> Element<'a, Message> {
    container(Space::new().width(Length::Fixed(1.0)).height(Length::Fixed(16.0)))
        .style(|_| container::Style {
            background: Some(Background::Color(theme::BORDER)),
            ..Default::default()
        })
        .into()
}

fn segmented<'a>(segments: Vec<Element<'a, Message>>) -> Element<'a, Message> {
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

fn segment<'a>(label: &'a str, active: bool, message: Message) -> Element<'a, Message> {
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
fn filter_chip<'a>(
    label: &'a str,
    count: usize,
    active: bool,
    accent: Color,
    message: Message,
) -> Element<'a, Message> {
    button(
        row![
            text("●").size(7).color(if active {
                accent
            } else {
                theme::TEXT_MUTED
            }),
            text(label).size(10),
            text(count.to_string()).size(10).color(if active {
                accent
            } else {
                theme::TEXT_MUTED
            }),
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
                color: if active { theme::BORDER } else { theme::BORDER_SUBTLE },
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        }
    })
    .into()
}

fn icon_button<'a>(
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

fn ghost_style(_: &Theme, status: button::Status) -> button::Style {
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

fn row_style(_: &Theme, status: button::Status) -> button::Style {
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

fn primary_style(_: &Theme, status: button::Status) -> button::Style {
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
fn hinted<'a>(content: impl Into<Element<'a, Message>>, tip: &'a str) -> Element<'a, Message> {
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
struct GraphSummary {
    nodes: usize,
    links: usize,
}

fn summarize(graph: &GraphSnapshot) -> GraphSummary {
    GraphSummary {
        nodes: graph.nodes.len(),
        links: graph.edges.iter().map(|edge| edge.weight.max(1)).sum(),
    }
}

// ─────────────────────────── label overlay ──────────────────────────

/// The transparent middle layer. Renders node labels, pin markers, and the
/// hover card with Iced's text pipeline (crisp, unlike text baked into the
/// shader), and is deliberately inert: it captures no events and reports
/// `Interaction::None`, so the GPU layer underneath receives every pointer
/// gesture with a live cursor.
pub(crate) struct GraphLabels {
    frame: GraphFrame,
}

impl GraphLabels {
    pub(crate) fn new(frame: GraphFrame) -> Self {
        Self { frame }
    }
}

impl canvas::Program<Message> for GraphLabels {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        _event: &canvas::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        None
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        if self.frame.nodes().is_empty() {
            // The empty-state card is a real widget layer above this one.
            return vec![frame.into_geometry()];
        }

        let transform = ViewTransform {
            pan: self.frame.pan,
            zoom: self.frame.zoom,
        };
        let focus_mask = self.frame.focus_mask();
        let focus = focus_mask.as_ref().map(|(focus, _)| *focus);
        let in_focus = |index: usize| {
            focus_mask
                .as_ref()
                .is_some_and(|(_, mask)| mask.get(index).copied().unwrap_or(false))
        };
        let degree_cutoff = label_degree_cutoff(&self.frame.graph.snapshot, transform.zoom);

        for (index, node) in self.frame.nodes().iter().enumerate() {
            let position = world_to_screen(self.frame.world(index), bounds, transform);
            let radius = node_screen_radius(node, transform.zoom);
            // Generous margin so a label anchored just off-screen still draws.
            if !circle_may_be_visible(position, radius + 140.0, bounds) {
                continue;
            }

            let selected = self.frame.selected == Some(index);
            let hovered = self.frame.hovered == Some(index);
            let active = self.frame.active == Some(index);
            let is_focus = focus == Some(index);
            let neighbor = !is_focus && in_focus(index);

            // A pin is a promise the node will stay put, so it needs to be
            // visible without hovering or selecting anything.
            if self.frame.pinned.contains(&index) {
                frame.stroke(
                    &canvas::Path::circle(position, radius + 4.0),
                    canvas::Stroke {
                        line_dash: canvas::LineDash {
                            segments: &[2.0, 3.0],
                            offset: 0,
                        },
                        ..canvas::Stroke::default()
                            .with_color(Color {
                                a: 0.75,
                                ..theme::ACCENT
                            })
                            .with_width(1.0)
                    },
                );
            }

            let important = (node.incoming + node.outgoing) >= degree_cutoff;
            if !(hovered || selected || active || neighbor || is_focus || important) {
                continue;
            }

            let emphasized = selected || hovered || is_focus;
            let faded = focus.is_some() && !emphasized && !neighbor;
            frame.fill_text(canvas::Text {
                content: truncate_label(&node.label, if emphasized { 48 } else { 26 }),
                position: Point::new(position.x, position.y + radius + 4.0),
                color: if faded {
                    Color {
                        a: 0.45,
                        ..theme::TEXT_MUTED
                    }
                } else if emphasized {
                    theme::TEXT_PRIMARY
                } else {
                    theme::TEXT_SECONDARY
                },
                size: iced::Pixels(if emphasized { 12.0 } else { 10.0 }),
                align_x: iced::advanced::text::Alignment::Center,
                align_y: iced::alignment::Vertical::Top,
                ..canvas::Text::default()
            });
        }

        if let (Some(index), Some(cursor)) = (self.frame.hovered, cursor.position_in(bounds))
            && let Some(node) = self.frame.nodes().get(index)
        {
            draw_hover_card(
                &mut frame,
                node,
                self.frame.pinned.contains(&index),
                cursor,
                bounds,
            );
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        // Report `None` so the stack does not levitate the cursor away from the
        // GPU layer below, which is what actually handles interaction.
        mouse::Interaction::None
    }
}

/// The minimum degree a node needs for its label to be drawn unconditionally.
///
/// Labelling every node in a large vault produces an unreadable smear and makes
/// text shaping the frame budget, so the budget grows with zoom: zoomed out you
/// see the hubs, zoomed in you see everything nearby.
fn label_degree_cutoff(graph: &GraphSnapshot, zoom: f32) -> usize {
    if graph.nodes.len() <= LABEL_ALL_BELOW || zoom >= 1.3 {
        return 0;
    }
    let budget = (graph.nodes.len() as f32 * (0.10 + 0.5 * (zoom - 0.3).max(0.0)))
        .round()
        .clamp(12.0, 160.0) as usize;
    if budget >= graph.nodes.len() {
        return 0;
    }
    let mut degrees: Vec<usize> = graph
        .nodes
        .iter()
        .map(|node| node.incoming + node.outgoing)
        .collect();
    degrees.sort_unstable_by(|a, b| b.cmp(a));
    degrees[budget.saturating_sub(1)].max(1)
}

fn truncate_label(label: &str, limit: usize) -> String {
    if label.chars().count() <= limit {
        return label.to_string();
    }
    let mut out: String = label.chars().take(limit.saturating_sub(1)).collect();
    out.push('…');
    out
}

// ─────────────────────────── shared geometry ────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ViewTransform {
    pub pan: Vector,
    pub zoom: f32,
}

impl Default for ViewTransform {
    fn default() -> Self {
        Self {
            pan: Vector::new(0.0, 0.0),
            zoom: 1.0,
        }
    }
}

pub(crate) fn kind_color(kind: GraphNodeKind) -> Color {
    match kind {
        GraphNodeKind::Markdown => theme::ACCENT,
        GraphNodeKind::Pdf => Color::from_rgb8(111, 170, 220),
        GraphNodeKind::Missing => theme::DANGER,
    }
}

/// Zoom below which the graph is being read as a whole rather than clicked,
/// so nodes are allowed to shrink toward points.
const OVERVIEW_ZOOM: f32 = 0.35;

pub(crate) fn node_screen_radius(node: &GraphNode, zoom: f32) -> f32 {
    (base_radius(node) * radius_scale(zoom)).clamp(0.6, 20.0)
}

fn base_radius(node: &GraphNode) -> f32 {
    5.5 + (node_degree(node) + 1.0).ln() * 2.2
}

/// Nodes are mostly screen-space so they stay clickable at working zoom, but
/// below [`OVERVIEW_ZOOM`] they shrink with it.
///
/// Without this the radius had a flat 4.5 px floor, so ten thousand nodes at
/// 2% zoom each painted a 4.5 px disc with a wider halo on top — they overlapped
/// into one solid blob instead of reading as a field of separate points.
fn radius_scale(zoom: f32) -> f32 {
    let screen_space = 0.75 + zoom.sqrt() * 0.25;
    screen_space * (zoom / OVERVIEW_ZOOM).clamp(0.12, 1.0)
}

fn node_degree(node: &GraphNode) -> f32 {
    (node.incoming + node.outgoing) as f32
}

/// How brightly a node burns, 0..1, from its connectedness — hubs read as
/// bright stars and leaves as faint ones, which is what gives a large graph
/// visible structure instead of uniform fill.
pub(crate) fn node_magnitude(node: &GraphNode) -> f32 {
    ((node_degree(node) + 1.0).ln() / 3.0).clamp(0.0, 1.0)
}

/// The hover card. Sized from its contents and flipped away from whichever
/// edge the cursor is near, so it never gets clipped or covers the node it
/// describes.
fn draw_hover_card(
    frame: &mut canvas::Frame<Renderer>,
    node: &GraphNode,
    pinned: bool,
    cursor: Point,
    bounds: Rectangle,
) {
    let kind_label = match node.kind {
        GraphNodeKind::Markdown => "Note",
        GraphNodeKind::Pdf => "PDF",
        GraphNodeKind::Missing => "Unresolved",
    };
    let detail = format!(
        "{} in · {} out{}",
        node.incoming,
        node.outgoing,
        if pinned { " · pinned" } else { "" }
    );

    let width = 236.0;
    let height = 62.0;
    // Flip rather than clamp: a card pinned against the edge sits on top of the
    // cursor, which hides the very node the user is pointing at.
    let x = if cursor.x + 16.0 + width > bounds.width {
        (cursor.x - 16.0 - width).max(6.0)
    } else {
        cursor.x + 16.0
    };
    let y = if cursor.y + 16.0 + height > bounds.height {
        (cursor.y - 16.0 - height).max(6.0)
    } else {
        cursor.y + 16.0
    };

    let card = canvas::Path::rounded_rectangle(
        Point::new(x, y),
        Size::new(width, height),
        7.0.into(),
    );
    frame.fill(&card, Color::from_rgba(0.07, 0.08, 0.09, 0.97));
    frame.stroke(
        &card,
        canvas::Stroke::default()
            .with_color(theme::BORDER)
            .with_width(1.0),
    );
    frame.fill(
        &canvas::Path::circle(Point::new(x + 13.0, y + 15.0), 3.0),
        kind_color(node.kind),
    );
    frame.fill_text(canvas::Text {
        content: truncate_label(&node.label, 30),
        position: Point::new(x + 23.0, y + 8.0),
        color: theme::TEXT_PRIMARY,
        size: iced::Pixels(12.0),
        ..canvas::Text::default()
    });
    frame.fill_text(canvas::Text {
        content: format!("{kind_label} · {detail}"),
        position: Point::new(x + 11.0, y + 26.0),
        color: theme::TEXT_SECONDARY,
        size: iced::Pixels(9.5),
        max_width: width - 22.0,
        ..canvas::Text::default()
    });
    frame.fill_text(canvas::Text {
        content: node.path.clone(),
        position: Point::new(x + 11.0, y + 42.0),
        color: theme::TEXT_MUTED,
        size: iced::Pixels(9.0),
        max_width: width - 22.0,
        ..canvas::Text::default()
    });
}

pub(crate) fn world_to_screen(world: Point, bounds: Rectangle, transform: ViewTransform) -> Point {
    Point::new(
        bounds.width / 2.0 + transform.pan.x + world.x * transform.zoom,
        bounds.height / 2.0 + transform.pan.y + world.y * transform.zoom,
    )
}

pub(crate) fn screen_to_world(screen: Point, bounds: Rectangle, transform: ViewTransform) -> Point {
    Point::new(
        (screen.x - bounds.width / 2.0 - transform.pan.x) / transform.zoom,
        (screen.y - bounds.height / 2.0 - transform.pan.y) / transform.zoom,
    )
}

pub(crate) fn zoom_about(
    transform: ViewTransform,
    bounds: Rectangle,
    anchor: Point,
    factor: f32,
    min_zoom: f32,
    max_zoom: f32,
) -> ViewTransform {
    let world = screen_to_world(anchor, bounds, transform);
    let zoom = (transform.zoom * factor).clamp(min_zoom, max_zoom);
    ViewTransform {
        zoom,
        pan: Vector::new(
            anchor.x - bounds.width / 2.0 - world.x * zoom,
            anchor.y - bounds.height / 2.0 - world.y * zoom,
        ),
    }
}

pub(crate) fn fit_transform(points: &[Point], bounds: Rectangle) -> ViewTransform {
    if points.is_empty() {
        return ViewTransform::default();
    }
    let (mut min_x, mut max_x, mut min_y, mut max_y) =
        (points[0].x, points[0].x, points[0].y, points[0].y);
    for point in &points[1..] {
        min_x = min_x.min(point.x);
        max_x = max_x.max(point.x);
        min_y = min_y.min(point.y);
        max_y = max_y.max(point.y);
    }
    let width = (max_x - min_x).max(80.0);
    let height = (max_y - min_y).max(80.0);
    let zoom = ((bounds.width - 110.0).max(80.0) / width)
        .min((bounds.height - 110.0).max(80.0) / height)
        .clamp(MIN_ZOOM, 1.65);
    let center = Point::new((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
    ViewTransform {
        zoom,
        pan: Vector::new(-center.x * zoom, -center.y * zoom),
    }
}

pub(crate) fn distance(a: Point, b: Point) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn circle_may_be_visible(center: Point, radius: f32, bounds: Rectangle) -> bool {
    center.x + radius >= 0.0
        && center.y + radius >= 0.0
        && center.x - radius <= bounds.width
        && center.y - radius <= bounds.height
}

#[cfg(test)]
fn line_may_be_visible(from: Point, to: Point, bounds: Rectangle, margin: f32) -> bool {
    let min_x = from.x.min(to.x);
    let max_x = from.x.max(to.x);
    let min_y = from.y.min(to.y);
    let max_y = from.y.max(to.y);
    max_x >= -margin
        && max_y >= -margin
        && min_x <= bounds.width + margin
        && min_y <= bounds.height + margin
}

#[cfg(test)]
fn hit_test_points(screen: Point, points: &[(Point, f32)]) -> Option<usize> {
    points
        .iter()
        .enumerate()
        .filter_map(|(index, (center, radius))| {
            let distance = distance(screen, *center);
            (distance <= *radius).then_some((index, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_state::MAX_ZOOM;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.001
    }

    #[test]
    fn world_screen_transform_round_trips() {
        let bounds = Rectangle::new(Point::new(0.0, 0.0), Size::new(900.0, 600.0));
        let transform = ViewTransform {
            pan: Vector::new(37.0, -19.0),
            zoom: 1.75,
        };
        let world = Point::new(-82.5, 144.25);
        let round_trip =
            screen_to_world(world_to_screen(world, bounds, transform), bounds, transform);
        assert!(close(world.x, round_trip.x));
        assert!(close(world.y, round_trip.y));
    }

    #[test]
    fn zoom_keeps_pointer_anchor_stable_and_clamps() {
        let bounds = Rectangle::new(Point::new(0.0, 0.0), Size::new(800.0, 500.0));
        let anchor = Point::new(175.0, 220.0);
        let before = screen_to_world(anchor, bounds, ViewTransform::default());
        let zoomed = zoom_about(ViewTransform::default(), bounds, anchor, 2.0, MIN_ZOOM, MAX_ZOOM);
        let after = screen_to_world(anchor, bounds, zoomed);
        assert!(close(before.x, after.x));
        assert!(close(before.y, after.y));
        assert_eq!(
            zoom_about(zoomed, bounds, anchor, 100.0, MIN_ZOOM, MAX_ZOOM).zoom,
            MAX_ZOOM
        );
    }

    #[test]
    fn hit_test_chooses_nearest_overlapping_node() {
        let points = [
            (Point::new(20.0, 20.0), 15.0),
            (Point::new(28.0, 20.0), 15.0),
        ];
        assert_eq!(hit_test_points(Point::new(27.0, 20.0), &points), Some(1));
        assert_eq!(hit_test_points(Point::new(100.0, 100.0), &points), None);
    }

    #[test]
    fn fit_adapts_to_canvas_resize_and_large_graphs() {
        let points = [
            Point::new(-10_000.0, -4_000.0),
            Point::new(10_000.0, 4_000.0),
        ];
        let large = Rectangle::new(Point::ORIGIN, Size::new(1_200.0, 800.0));
        let small = Rectangle::new(Point::ORIGIN, Size::new(420.0, 280.0));

        let large_fit = fit_transform(&points, large);
        let small_fit = fit_transform(&points, small);

        assert!(small_fit.zoom < large_fit.zoom);
        assert!(small_fit.zoom > MIN_ZOOM);
        assert!(close(large_fit.pan.x, 0.0));
        assert!(close(large_fit.pan.y, 0.0));
    }

    #[test]
    fn viewport_rejection_keeps_crossing_edges() {
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(500.0, 300.0));
        assert!(circle_may_be_visible(Point::new(5.0, 5.0), 8.0, bounds));
        assert!(!circle_may_be_visible(
            Point::new(-100.0, -100.0),
            8.0,
            bounds
        ));
        assert!(line_may_be_visible(
            Point::new(-100.0, 150.0),
            Point::new(600.0, 150.0),
            bounds,
            12.0
        ));
        assert!(!line_may_be_visible(
            Point::new(-100.0, -40.0),
            Point::new(600.0, -40.0),
            bounds,
            12.0
        ));
    }

    fn graph_with(node_count: usize) -> GraphSnapshot {
        let nodes = (0..node_count)
            .map(|index| GraphNode {
                path: format!("n{index}.md"),
                label: format!("n{index}"),
                kind: GraphNodeKind::Markdown,
                exists: true,
                // A steep degree gradient so the cutoff has something to cut.
                incoming: node_count - index,
                outgoing: 0,
            })
            .collect();
        GraphSnapshot {
            nodes,
            edges: Vec::new(),
        }
    }

    #[test]
    fn small_graphs_and_close_zoom_label_everything() {
        assert_eq!(label_degree_cutoff(&graph_with(40), 0.4), 0);
        assert_eq!(label_degree_cutoff(&graph_with(600), 1.6), 0);
    }

    #[test]
    fn label_budget_tightens_as_the_view_zooms_out() {
        let graph = graph_with(600);
        let far = label_degree_cutoff(&graph, 0.35);
        let near = label_degree_cutoff(&graph, 1.0);
        assert!(far > near, "zooming out should demand a higher degree");
        assert!(near > 0);
    }

    #[test]
    fn labels_are_truncated_on_character_boundaries() {
        assert_eq!(truncate_label("short", 10), "short");
        assert_eq!(truncate_label("a much longer label", 6), "a muc…");
        // Multi-byte input must not panic or split a character.
        assert_eq!(truncate_label("émojis ✨ everywhere", 4), "émo…");
    }
}

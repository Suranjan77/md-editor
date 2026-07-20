use std::collections::{HashMap, HashSet};

use iced::widget::{
    Space, button, canvas, checkbox, column, container, row, scrollable, stack, text, text_input,
};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme,
    Vector, mouse,
};

use md_editor_core::types::{GraphNode, GraphNodeKind, GraphSnapshot};

use crate::graph_state::{GraphScope, GraphState, MIN_ZOOM};
use crate::messages::Message;
use crate::theme;
use crate::views::gpu_graph::GpuGraphProgram;

const INSPECTOR_WIDTH: f32 = 310.0;

/// An immutable, per-frame snapshot of everything both graph render layers need:
/// the filtered topology, node world positions, adjacency for focus dimming, and
/// the shared pan/zoom transform. Built once per `view` and cloned into the GPU
/// [`shader`](iced::widget::shader) layer and the label [`canvas`] layer so the
/// two never disagree about where a node is.
#[derive(Debug, Clone)]
pub(crate) struct GraphFrame {
    pub graph: GraphSnapshot,
    pub positions: HashMap<String, Point>,
    pub neighbors: HashMap<String, HashSet<String>>,
    pub selected: Option<String>,
    pub hovered: Option<String>,
    pub active: Option<String>,
    pub pan: Vector,
    pub zoom: f32,
    pub fit_revision: u64,
}

impl GraphFrame {
    pub(crate) fn from_state(state: &GraphState) -> Self {
        let graph = state.filtered_graph();
        let mut positions = HashMap::with_capacity(graph.nodes.len());
        for node in &graph.nodes {
            if let Some(position) = state.graph_layout().get(&node.path) {
                positions.insert(node.path.clone(), Point::new(position.x, position.y));
            }
        }
        let mut neighbors: HashMap<String, HashSet<String>> = HashMap::new();
        for edge in &graph.edges {
            neighbors
                .entry(edge.source.clone())
                .or_default()
                .insert(edge.target.clone());
            neighbors
                .entry(edge.target.clone())
                .or_default()
                .insert(edge.source.clone());
        }
        Self {
            graph,
            positions,
            neighbors,
            selected: state.selected_path.clone(),
            hovered: state.hovered_path.clone(),
            active: state.active_path.clone(),
            pan: Vector::new(state.pan.0, state.pan.1),
            zoom: state.zoom,
            fit_revision: state.fit_revision,
        }
    }
}

/// Complete research-graph overlay. The GPU shader layer renders nodes/edges and
/// owns all interaction; the label canvas layer on top draws crisp text and the
/// hover tooltip. Durable filters, selection, moved-node positions, the shared
/// transform, and fit requests all live in [`GraphState`].
pub fn view(state: &GraphState) -> Element<'_, Message, Theme, Renderer> {
    if !state.visible {
        return container(Space::new())
            .width(Length::Fixed(0.0))
            .height(Length::Fixed(0.0))
            .into();
    }

    let frame = GraphFrame::from_state(state);
    let summary = summarize(&frame.graph);
    let vault_stats = state.stats();
    let local_depth = match state.scope {
        GraphScope::Global => 2,
        GraphScope::Local { depth } => depth,
    };

    let scope_controls = row![
        button(text("Global").size(12))
            .on_press(Message::GraphScopeGlobal)
            .padding([7, 12])
            .style(if matches!(state.scope, GraphScope::Global) {
                button::primary
            } else {
                button::text
            }),
        button(text("Local").size(12))
            .on_press(Message::GraphScopeLocal)
            .padding([7, 12])
            .style(if matches!(state.scope, GraphScope::Local { .. }) {
                button::primary
            } else {
                button::text
            }),
        button(text("−").size(15))
            .on_press_maybe(matches!(state.scope, GraphScope::Local { .. }).then_some(
                Message::GraphLocalDepthChanged(local_depth.saturating_sub(1).max(1)),
            ))
            .padding([5, 9])
            .style(button::text),
        text(format!(
            "{} hop{}",
            local_depth,
            if local_depth == 1 { "" } else { "s" }
        ))
        .size(11)
        .color(theme::TEXT_MUTED),
        button(text("+").size(15))
            .on_press_maybe(matches!(state.scope, GraphScope::Local { .. }).then_some(
                Message::GraphLocalDepthChanged(local_depth.saturating_add(1).min(2)),
            ))
            .padding([5, 9])
            .style(button::text),
    ]
    .spacing(3)
    .align_y(Alignment::Center);

    let search = text_input("Search graph…", &state.query)
        .on_input(Message::GraphQueryChanged)
        .padding([8, 11])
        .size(13)
        .width(Length::Fixed(220.0));

    let header = container(
        row![
            column![
                text("KNOWLEDGE GRAPH").size(11).color(theme::ACCENT),
                text("Connections, gaps, and high-leverage notes")
                    .size(11)
                    .color(theme::TEXT_MUTED),
            ]
            .spacing(2),
            Space::new().width(Length::Fill),
            scope_controls,
            Space::new().width(Length::Fixed(8.0)),
            search,
            button(text("Fit").size(12))
                .on_press(Message::GraphFitView)
                .padding([7, 12])
                .style(button::text),
            button(text("Relayout").size(12))
                .on_press(Message::GraphResetLayout)
                .padding([7, 12])
                .style(button::text),
            button(text("✕").size(15).color(theme::TEXT_MUTED))
                .on_press(Message::GraphToggle)
                .padding(8)
                .style(button::text),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .padding([10, 14]),
    )
    .width(Length::Fill)
    .style(panel_style);

    let filter_bar = container(
        row![
            checkbox(state.show_pdfs)
                .label("PDFs")
                .on_toggle(Message::GraphShowPdfsToggled)
                .size(14),
            checkbox(state.show_missing)
                .label("Unresolved")
                .on_toggle(Message::GraphShowMissingToggled)
                .size(14),
            checkbox(state.show_orphans)
                .label("Orphans")
                .on_toggle(Message::GraphShowOrphansToggled)
                .size(14),
            Space::new().width(Length::Fill),
            stat_chip("Nodes", summary.nodes, theme::ACCENT),
            stat_chip("Links", summary.links, theme::TEXT_SECONDARY),
            stat_chip("Orphans", summary.orphans, theme::WARNING),
            stat_chip("Unresolved", summary.missing, theme::DANGER),
            stat_chip("Groups", vault_stats.components, theme::TEXT_SECONDARY),
        ]
        .spacing(15)
        .align_y(Alignment::Center)
        .padding([8, 14]),
    )
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(theme::BG_PRIMARY)),
        border: Border {
            color: theme::BORDER_SUBTLE,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    });

    // Bottom: GPU-rendered nodes/edges (also owns all pointer interaction).
    // Top: transparent label canvas that never captures events, so pans, drags,
    // and clicks fall straight through to the shader beneath it.
    let render_layer = iced::widget::shader(GpuGraphProgram::new(frame.clone()))
        .width(Length::Fill)
        .height(Length::Fill);
    let label_layer = canvas(GraphLabels::new(frame))
        .width(Length::Fill)
        .height(Length::Fill);
    let graph_canvas = stack![render_layer, label_layer]
        .width(Length::Fill)
        .height(Length::Fill);

    let canvas_panel = container(graph_canvas)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(theme::BG_PRIMARY)),
            border: Border {
                color: theme::BORDER_SUBTLE,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        });

    let body = row![canvas_panel, inspector(state)]
        .spacing(10)
        .height(Length::Fill);

    container(column![header, filter_bar, body].spacing(0))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(12)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.82))),
            ..Default::default()
        })
        .into()
}

fn panel_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::BG_SECONDARY)),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

fn stat_chip<'a>(label: &'static str, value: usize, color: Color) -> Element<'a, Message> {
    container(
        row![
            text(label).size(10).color(theme::TEXT_MUTED),
            text(value.to_string()).size(11).color(color),
        ]
        .spacing(5)
        .align_y(Alignment::Center),
    )
    .padding([5, 8])
    .style(|_| container::Style {
        background: Some(Background::Color(theme::BG_TERTIARY)),
        border: Border {
            color: theme::BORDER_SUBTLE,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    })
    .into()
}

#[derive(Debug, Clone, Copy)]
struct GraphSummary {
    nodes: usize,
    links: usize,
    orphans: usize,
    missing: usize,
}

fn summarize(graph: &GraphSnapshot) -> GraphSummary {
    GraphSummary {
        nodes: graph.nodes.len(),
        links: graph.edges.iter().map(|edge| edge.weight.max(1)).sum(),
        orphans: graph
            .nodes
            .iter()
            .filter(|node| {
                node.kind == GraphNodeKind::Markdown && node.incoming + node.outgoing == 0
            })
            .count(),
        missing: graph.nodes.iter().filter(|node| !node.exists).count(),
    }
}

fn inspector<'a>(state: &'a GraphState) -> Element<'a, Message> {
    let selected_node = state
        .selected_path
        .as_deref()
        .and_then(|path| state.node(path));

    let selected_card: Element<'a, Message> = if let Some(node) = selected_node {
        let kind = match node.kind {
            GraphNodeKind::Markdown => "Markdown note",
            GraphNodeKind::Pdf => "PDF reference",
            GraphNodeKind::Missing => "Unresolved target",
        };
        let open = button(text("Open").size(12))
            .on_press_maybe(
                node.exists
                    .then(|| Message::GraphNodeOpen(node.path.clone())),
            )
            .padding([7, 12])
            .style(button::primary);

        let incoming = connection_list("LINKED FROM", state.incoming(&node.path), "incoming");
        let outgoing = connection_list("LINKS TO", state.outgoing(&node.path), "outgoing");
        let related = state
            .related(&node.path, 6)
            .into_iter()
            .map(|item| {
                (
                    item.node.path,
                    item.node.label,
                    format!("{} shared", item.shared_connections),
                )
            })
            .collect();
        let related = health_list("RELATED NOTES", related, theme::ACCENT_SECONDARY);

        container(
            column![
                row![
                    column![
                        text(node.label.clone()).size(17).color(theme::TEXT_PRIMARY),
                        text(kind).size(10).color(kind_color(node.kind)),
                    ]
                    .spacing(3),
                    Space::new().width(Length::Fill),
                    open,
                ]
                .align_y(Alignment::Center),
                text(node.path.clone()).size(10).color(theme::TEXT_MUTED),
                row![
                    metric("Incoming", node.incoming),
                    metric("Outgoing", node.outgoing),
                    metric("Degree", node.incoming + node.outgoing),
                ]
                .spacing(6),
                incoming,
                outgoing,
                related,
            ]
            .spacing(9),
        )
        .padding(12)
        .style(panel_style)
        .into()
    } else {
        container(
            column![
                text("Select a node").size(14).color(theme::TEXT_SECONDARY),
                text("Click to inspect • use Open above • drag to arrange")
                    .size(10)
                    .color(theme::TEXT_MUTED),
            ]
            .spacing(5),
        )
        .padding(14)
        .style(panel_style)
        .into()
    };

    let stats = state.stats();
    let hubs = state
        .hubs(6)
        .into_iter()
        .filter(|node| node.incoming + node.outgoing > 0)
        .map(|node| {
            (
                node.path,
                node.label,
                format!("{} links", node.incoming + node.outgoing),
            )
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

    let content = column![
        text("INSPECTOR").size(10).color(theme::ACCENT),
        text(format!(
            "{} documents · {} connections · {} groups",
            stats.documents(),
            stats.connections,
            stats.components
        ))
        .size(10)
        .color(theme::TEXT_MUTED),
        selected_card,
        health_list("HUB NOTES", hubs, theme::ACCENT),
        health_list("ORPHANS", orphans, theme::WARNING),
        health_list("UNRESOLVED", unresolved, theme::DANGER),
    ]
    .spacing(9)
    .padding(10);

    container(scrollable(content).height(Length::Fill))
        .width(Length::Fixed(INSPECTOR_WIDTH))
        .height(Length::Fill)
        .style(panel_style)
        .into()
}

fn metric<'a>(label: &'static str, value: usize) -> Element<'a, Message> {
    container(
        column![
            text(value.to_string()).size(14).color(theme::ACCENT),
            text(label).size(8).color(theme::TEXT_MUTED),
        ]
        .spacing(1)
        .align_x(Alignment::Center),
    )
    .padding([6, 8])
    .width(Length::FillPortion(1))
    .style(|_| container::Style {
        background: Some(Background::Color(theme::BG_TERTIARY)),
        border: Border {
            radius: 5.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

fn connection_list<'a>(
    title: &'static str,
    nodes: Vec<GraphNode>,
    detail: &'static str,
) -> Element<'a, Message> {
    let items = nodes
        .into_iter()
        .map(|node| (node.path, node.label, detail.to_string()))
        .collect();
    health_list(title, items, theme::TEXT_SECONDARY)
}

fn health_list<'a>(
    title: &'static str,
    items: Vec<(String, String, String)>,
    accent: Color,
) -> Element<'a, Message> {
    let mut list = column![].spacing(2);
    if items.is_empty() {
        list = list.push(text("None found").size(10).color(theme::TEXT_MUTED));
    } else {
        for (path, label, detail) in items {
            list = list.push(
                button(
                    row![
                        text("●").size(8).color(accent),
                        text(label).size(11).color(theme::TEXT_SECONDARY),
                        Space::new().width(Length::Fill),
                        text(detail).size(9).color(theme::TEXT_MUTED),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .on_press(Message::GraphNodeSelected(Some(path)))
                .padding([5, 6])
                .width(Length::Fill)
                .style(button::text),
            );
        }
    }
    container(column![text(title).size(9).color(accent), list].spacing(5))
        .padding(9)
        .style(|_| container::Style {
            background: Some(Background::Color(theme::BG_PRIMARY)),
            border: Border {
                color: theme::BORDER_SUBTLE,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        })
        .into()
}

// ─────────────────────────── label overlay ──────────────────────────

/// The transparent top layer. Renders node labels and the hover tooltip with
/// Iced's text pipeline (crisp, unlike text baked into the shader), and is
/// deliberately inert: it captures no events and reports `Interaction::None`, so
/// the GPU layer underneath receives every pointer gesture with a live cursor.
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

        if self.frame.graph.nodes.is_empty() {
            frame.fill_text(canvas::Text {
                content: "No notes match this graph view".to_string(),
                position: Point::new(bounds.width / 2.0 - 95.0, bounds.height / 2.0),
                color: theme::TEXT_MUTED,
                size: iced::Pixels(13.0),
                ..canvas::Text::default()
            });
            return vec![frame.into_geometry()];
        }

        let transform = ViewTransform {
            pan: self.frame.pan,
            zoom: self.frame.zoom,
        };
        let focus = self.frame.hovered.as_ref().or(self.frame.selected.as_ref());
        let focus_neighbors = focus.and_then(|path| self.frame.neighbors.get(path));
        let show_all = transform.zoom > 0.72 && self.frame.graph.nodes.len() <= 90;

        for node in &self.frame.graph.nodes {
            let world = self
                .frame
                .positions
                .get(&node.path)
                .copied()
                .unwrap_or(Point::ORIGIN);
            let position = world_to_screen(world, bounds, transform);
            let radius = node_screen_radius(node, transform.zoom);
            // Generous margin so a label anchored just off-screen still draws.
            if !circle_may_be_visible(position, radius + 140.0, bounds) {
                continue;
            }
            let selected = self.frame.selected.as_deref() == Some(node.path.as_str());
            let hovered = self.frame.hovered.as_deref() == Some(node.path.as_str());
            let active = self.frame.active.as_deref() == Some(node.path.as_str());
            let neighbor = focus_neighbors.is_some_and(|set| set.contains(&node.path));
            let is_focus = focus == Some(&node.path);
            if !(hovered || selected || active || neighbor || is_focus || show_all) {
                continue;
            }
            let faded = focus.is_some() && !selected && !hovered && !neighbor && !is_focus;
            frame.fill_text(canvas::Text {
                content: node.label.clone(),
                position: Point::new(position.x + radius + 5.0, position.y - 6.0),
                color: if faded {
                    theme::TEXT_MUTED
                } else {
                    theme::TEXT_SECONDARY
                },
                size: iced::Pixels(if selected || hovered { 12.0 } else { 10.0 }),
                ..canvas::Text::default()
            });
        }

        if let (Some(path), Some(cursor)) = (self.frame.hovered.as_deref(), cursor.position_in(bounds))
            && let Some(node) = self.frame.graph.nodes.iter().find(|node| node.path == path)
        {
            draw_tooltip(&mut frame, node, cursor, bounds);
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

pub(crate) fn node_screen_radius(node: &GraphNode, zoom: f32) -> f32 {
    let degree = (node.incoming + node.outgoing) as f32;
    let base = 5.5 + (degree + 1.0).ln() * 2.2;
    (base * (0.75 + zoom.sqrt() * 0.25)).clamp(4.5, 20.0)
}

fn draw_tooltip(
    frame: &mut canvas::Frame<Renderer>,
    node: &GraphNode,
    cursor: Point,
    bounds: Rectangle,
) {
    let width = 215.0;
    let height = 48.0;
    let x = (cursor.x + 14.0).min((bounds.width - width - 8.0).max(8.0));
    let y = (cursor.y + 14.0).min((bounds.height - height - 8.0).max(8.0));
    frame.fill(
        &canvas::Path::rounded_rectangle(Point::new(x, y), Size::new(width, height), 6.0.into()),
        Color::from_rgba(0.08, 0.09, 0.10, 0.96),
    );
    frame.stroke(
        &canvas::Path::rounded_rectangle(Point::new(x, y), Size::new(width, height), 6.0.into()),
        canvas::Stroke::default()
            .with_color(theme::BORDER)
            .with_width(1.0),
    );
    frame.fill_text(canvas::Text {
        content: node.label.clone(),
        position: Point::new(x + 9.0, y + 8.0),
        color: theme::TEXT_PRIMARY,
        size: iced::Pixels(11.0),
        ..canvas::Text::default()
    });
    frame.fill_text(canvas::Text {
        content: format!(
            "{} in • {} out  ·  {}",
            node.incoming, node.outgoing, node.path
        ),
        position: Point::new(x + 9.0, y + 27.0),
        color: theme::TEXT_MUTED,
        size: iced::Pixels(9.0),
        max_width: width - 18.0,
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
}

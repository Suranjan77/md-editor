//! The knowledge-graph screen: layout of toolbar, canvas layers and inspector over `GraphState`.

use std::collections::HashSet;
use std::sync::Arc;

use iced::widget::{Space, canvas, column, container, row, scrollable, stack, text};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme,
    Vector, mouse,
};

use md_editor_core::types::{GraphNode, GraphNodeKind, GraphSnapshot};

use crate::design::metrics::{BORDER_WIDTH, IconSize, RADIUS_LG, SPACE_3};
use crate::design::{TextRole, tokens};
use crate::graph_state::{
    EmptyReason, FilteredGraph, GraphPoint, GraphScope, GraphState, MIN_ZOOM,
};
use crate::messages::Message;
use crate::ui::{Icon, IconButton, icon};
use crate::views::gpu_graph::GpuGraphProgram;

mod geometry;
mod inspector;
mod labels;
mod layers;
mod toolbar;

pub(crate) use geometry::*;
use inspector::*;
pub(crate) use labels::*;
use layers::*;
use toolbar::*;

/// Same width as the other right-hand panels.
const INSPECTOR_WIDTH: f32 = crate::design::metrics::INSPECTOR_DEFAULT_WIDTH;

/// Text in a type role and colour.
fn role_text<'a>(content: impl Into<String>, role: TextRole, color: Color) -> Element<'a, Message> {
    text(content.into())
        .size(role.size())
        .line_height(role.line_height())
        .font(role.font())
        .color(color)
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
        .style(|_| {
            let t = tokens();
            container::Style {
                background: Some(Background::Color(t.bg_canvas)),
                border: Border {
                    color: t.border_subtle,
                    width: BORDER_WIDTH,
                    radius: RADIUS_LG.into(),
                },
                ..Default::default()
            }
        });

    let mut body = row![canvas_panel].spacing(SPACE_3).height(Length::Fill);
    if state.inspector_visible {
        body = body.push(inspector(state));
    }

    container(column![toolbar(state, &summary, &stats), body].spacing(SPACE_3))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(SPACE_3)
        .style(|_| container::Style {
            background: Some(Background::Color(tokens().bg_surface)),
            ..Default::default()
        })
        .into()
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
        let zoomed = zoom_about(
            ViewTransform::default(),
            bounds,
            anchor,
            2.0,
            MIN_ZOOM,
            MAX_ZOOM,
        );
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

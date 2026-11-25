//! GPU-accelerated knowledge-graph renderer.
//!
//! This is the bottom layer of the graph view (a thin label [`canvas`] sits on
//! top). It renders edges and nodes as instanced quads — soft glowing discs and
//! anti-aliased links — and owns every pointer interaction (pan, zoom, drag,
//! hover, selection, fit), publishing the resulting transform/hover/selection
//! back into [`GraphState`] so the label layer stays in lock-step.
//!
//! Node positions are owned by the CPU force simulation in `graph_state.rs`;
//! this layer only *draws* them, plus resolves pointer hits against them. There
//! are deliberately no storage buffers or compute passes: all per-instance data
//! travels through ordinary instance-rate vertex buffers, which is what keeps
//! the pipeline within Iced's default device limits.
//!
//! [`canvas`]: iced::widget::canvas

use iced::widget::shader;
use iced::{Point, Rectangle, Vector, mouse};

use crate::graph_state::{MAX_ZOOM, MIN_ZOOM};
use crate::messages::{GraphMessage, Message};
use crate::views::graph::{
    GraphFrame, ViewTransform, distance, kind_color, node_magnitude, node_screen_radius,
    screen_to_world, world_to_screen,
};

mod pipeline;

use pipeline::GpuGraphPrimitive;
use pipeline::{EdgeInstance, Globals, NodeInstance};

/// Extra pixels added to a node's radius when resolving a pointer hit.
const HIT_SLOP: f32 = 4.0;

// ───────────────────────────── program ──────────────────────────────

/// Per-widget transient state. The pan/zoom here is the *authoritative* render
/// transform — updated synchronously while handling an event so the current
/// frame draws with it immediately (no one-frame lag), then mirrored into
/// [`GraphState`] via `GraphSetView` so the label overlay follows along.
#[derive(Debug)]
pub struct Interaction {
    pan: Vector,
    zoom: f32,
    initialized: bool,
    applied_fit: u64,
    /// Last externally-published transform this widget has adopted.
    applied_view: u64,
    fitted_size: Option<iced::Size>,
    gesture: Option<Gesture>,
    /// Node and time of the previous left-press, for double-click detection.
    last_click: Option<(String, std::time::Instant)>,
}

impl Default for Interaction {
    fn default() -> Self {
        Self {
            pan: Vector::new(0.0, 0.0),
            zoom: 1.0,
            initialized: false,
            applied_fit: 0,
            applied_view: 0,
            fitted_size: None,
            gesture: None,
            last_click: None,
        }
    }
}

/// Two left-presses on the same node within this window open the document.
use crate::design::motion::MULTI_CLICK as DOUBLE_CLICK;

#[derive(Debug, Clone)]
enum Gesture {
    Pan {
        start: Point,
        initial_pan: Vector,
    },
    Node {
        path: String,
        grab_offset: Vector,
        /// Where the press landed, so a click can be told from a drag.
        origin: Point,
        /// Set once the pointer has travelled past [`DRAG_THRESHOLD`].
        dragging: bool,
    },
}

/// How far the pointer must travel before a press on a node counts as a drag.
///
/// Without this, the jitter between press and release on an ordinary click
/// emitted a move — which pinned the node, permanently exempting it from the
/// layout. Selecting a node must not silently pin it.
const DRAG_THRESHOLD: f32 = 3.0;

/// The interactive GPU program. Constructed fresh from [`GraphState`] each
/// `view`, so its fields are an immutable snapshot of the current frame.
pub struct GpuGraphProgram {
    frame: GraphFrame,
}

impl GpuGraphProgram {
    pub fn new(frame: GraphFrame) -> Self {
        Self { frame }
    }

    fn fit(&self, bounds: Rectangle) -> ViewTransform {
        self.frame.fit(bounds)
    }

    /// The transform to render/interact with. Once this widget has applied the
    /// current fit request its own authoritative pan/zoom is used; until then a
    /// freshly computed fit frames the graph so even the very first frame — and
    /// any frame `draw` runs before `update` has committed — is correct.
    fn transform(&self, state: &Interaction, bounds: Rectangle) -> ViewTransform {
        let fit_applied = state.initialized
            && state.applied_fit == self.frame.fit_revision
            && state.fitted_size == Some(bounds.size());
        if !fit_applied {
            return self.fit(bounds);
        }
        // A zoom button or "reveal in graph" published a transform this widget
        // has not adopted yet — honour it now so the frame drawn before the
        // next `update` already shows the new framing.
        if state.applied_view != self.frame.view_revision {
            return ViewTransform {
                pan: self.frame.pan,
                zoom: self.frame.zoom,
            };
        }
        ViewTransform {
            pan: state.pan,
            zoom: state.zoom,
        }
    }

    fn hit_node(
        &self,
        bounds: Rectangle,
        transform: ViewTransform,
        screen: Point,
    ) -> Option<usize> {
        self.frame
            .nodes()
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                let center = world_to_screen(self.frame.world(index), bounds, transform);
                let radius = node_screen_radius(node, transform.zoom) + HIT_SLOP;
                let d = distance(screen, center);
                (d <= radius).then_some((index, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| index)
    }

    /// Commit a pending fit, or adopt a transform the app published, before
    /// handling the event. Returns the fit to publish, if one was committed.
    fn commit_view(
        &self,
        state: &mut Interaction,
        bounds: Rectangle,
    ) -> Option<shader::Action<Message>> {
        let needs_fit = !state.initialized
            || state.applied_fit != self.frame.fit_revision
            || state.fitted_size != Some(bounds.size());
        if needs_fit {
            let fit = self.fit(bounds);
            state.pan = fit.pan;
            state.zoom = fit.zoom;
            state.initialized = true;
            state.applied_fit = self.frame.fit_revision;
            state.applied_view = self.frame.view_revision;
            state.fitted_size = Some(bounds.size());
            state.gesture = None;
            // Mirror the fit into the shared state so the label overlay adopts
            // the same framing. Publishing already schedules a redraw; no
            // capture so a coincident gesture still reaches the handlers.
            return Some(shader::Action::publish(Message::Graph(
                GraphMessage::SetView {
                    pan_x: fit.pan.x,
                    pan_y: fit.pan.y,
                    zoom: fit.zoom,
                    fitted: Some((self.frame.fit_revision, bounds.width, bounds.height)),
                },
            )));
        }
        // Adopt a transform the app published (zoom buttons, reveal-in-graph).
        // `GraphState` already holds it, so there is nothing to publish back.
        if state.applied_view != self.frame.view_revision {
            state.pan = self.frame.pan;
            state.zoom = self.frame.zoom;
            state.applied_view = self.frame.view_revision;
            state.gesture = None;
        }
        None
    }

    /// Left press: open on double-click, start a node drag, or start a pan.
    fn on_left_press(
        &self,
        state: &mut Interaction,
        bounds: Rectangle,
        transform: ViewTransform,
        point: Point,
    ) -> shader::Action<Message> {
        let Some(index) = self.hit_node(bounds, transform, point) else {
            state.last_click = None;
            state.gesture = Some(Gesture::Pan {
                start: point,
                initial_pan: transform.pan,
            });
            return publish_captured(GraphMessage::NodeSelected(None));
        };
        let node = &self.frame.nodes()[index];
        let now = std::time::Instant::now();
        let double = state.last_click.as_ref().is_some_and(|(path, at)| {
            path == &node.path && now.duration_since(*at) <= DOUBLE_CLICK
        });
        if double {
            // Second click of a double: open the note instead of starting
            // another drag, and require a fresh pair.
            state.last_click = None;
            state.gesture = None;
            return publish_captured(GraphMessage::NodeOpen(node.path.clone()));
        }
        state.last_click = Some((node.path.clone(), now));
        let pointer_world = screen_to_world(point, bounds, transform);
        state.gesture = Some(Gesture::Node {
            path: node.path.clone(),
            grab_offset: pointer_world - self.frame.world(index),
            origin: point,
            dragging: false,
        });
        publish_captured(GraphMessage::NodeSelected(Some(node.path.clone())))
    }

    /// Pointer motion: continue a pan or node drag, or update hover.
    fn on_cursor_moved(
        &self,
        state: &mut Interaction,
        bounds: Rectangle,
        transform: ViewTransform,
        cursor_pos: Option<Point>,
    ) -> Option<shader::Action<Message>> {
        match &mut state.gesture {
            Some(Gesture::Pan { start, initial_pan }) => {
                let pan = *initial_pan + (cursor_pos? - *start);
                state.pan = pan;
                Some(publish_captured(set_view(pan, state.zoom)))
            }
            Some(Gesture::Node {
                path,
                grab_offset,
                origin,
                dragging,
            }) => {
                let point = cursor_pos?;
                if !*dragging && distance(point, *origin) < DRAG_THRESHOLD {
                    // Still within click tolerance — swallow the movement
                    // rather than turning a selection into a pinned node.
                    return Some(shader::Action::capture());
                }
                *dragging = true;
                let world = screen_to_world(point, bounds, transform) - *grab_offset;
                Some(publish_captured(GraphMessage::NodeMoved {
                    path: path.clone(),
                    x: world.x,
                    y: world.y,
                }))
            }
            None => {
                let hovered = cursor_pos
                    .and_then(|point| self.hit_node(bounds, transform, point))
                    .map(|index| self.frame.nodes()[index].path.clone());
                let current = self
                    .frame
                    .hovered
                    .map(|index| self.frame.nodes()[index].path.clone());
                (hovered != current).then(|| {
                    shader::Action::publish(Message::Graph(GraphMessage::Hovered(hovered)))
                })
            }
        }
    }
}

fn set_view(pan: Vector, zoom: f32) -> GraphMessage {
    GraphMessage::SetView {
        pan_x: pan.x,
        pan_y: pan.y,
        zoom,
        fitted: None,
    }
}

fn publish_captured(message: GraphMessage) -> shader::Action<Message> {
    shader::Action::publish(Message::Graph(message)).and_capture()
}

impl shader::Program<Message> for GpuGraphProgram {
    type State = Interaction;
    type Primitive = GpuGraphPrimitive;

    fn update(
        &self,
        state: &mut Self::State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<shader::Action<Message>> {
        use iced::Event;

        if let Some(action) = self.commit_view(state, bounds) {
            return Some(action);
        }
        let transform = ViewTransform {
            pan: state.pan,
            zoom: state.zoom,
        };
        let cursor_pos = cursor.position_in(bounds);

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                Some(self.on_left_press(state, bounds, transform, cursor_pos?))
            }
            // Right-click pins a node where it sits (or releases it) — the fast
            // path for the arrange-by-hand workflow the inspector button also
            // offers.
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                let index = self.hit_node(bounds, transform, cursor_pos?)?;
                let path = self.frame.nodes()[index].path.clone();
                Some(publish_captured(GraphMessage::PinToggled(path)))
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle)) => {
                state.gesture = Some(Gesture::Pan {
                    start: cursor_pos?,
                    initial_pan: transform.pan,
                });
                Some(shader::Action::capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                self.on_cursor_moved(state, bounds, transform, cursor_pos)
            }
            Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left | mouse::Button::Middle,
            )) => state.gesture.take().map(|_| shader::Action::capture()),
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let amount = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y * 0.13,
                    mouse::ScrollDelta::Pixels { y, .. } => y * 0.0025,
                };
                let zoomed = crate::views::graph::zoom_about(
                    transform,
                    bounds,
                    cursor_pos?,
                    amount.exp(),
                    MIN_ZOOM,
                    MAX_ZOOM,
                );
                state.pan = zoomed.pan;
                state.zoom = zoomed.zoom;
                Some(publish_captured(set_view(zoomed.pan, zoomed.zoom)))
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Self::State,
        _cursor: mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        let transform = self.transform(state, bounds);
        let w = bounds.width.max(1.0);
        let h = bounds.height.max(1.0);
        let globals = Globals {
            transform: [
                2.0 * transform.zoom / w,
                -2.0 * transform.zoom / h,
                2.0 * transform.pan.x / w,
                -2.0 * transform.pan.y / h,
            ],
            viewport: [w, h],
            srgb: 0.0,
            _pad: 0.0,
        };

        // One allocation up front instead of a set lookup per node per frame.
        let focus_mask = self.frame.focus_mask();
        let focus = focus_mask.as_ref().map(|(focus, _)| *focus);
        let in_focus = |index: usize| {
            focus_mask
                .as_ref()
                .is_some_and(|(_, mask)| mask.get(index).copied().unwrap_or(false))
        };

        let mut nodes = Vec::with_capacity(self.frame.nodes().len());
        for (index, node) in self.frame.nodes().iter().enumerate() {
            let is_focus = focus == Some(index);
            let is_neighbor = !is_focus && in_focus(index);
            // Dimming exists to make a neighbourhood pop, not to erase the map
            // around it — the rest of the graph has to stay readable enough to
            // aim the next click at.
            let dim = if focus.is_some() && !in_focus(index) {
                0.42
            } else {
                1.0
            };
            let selected = self.frame.selected == Some(index);
            let hovered = self.frame.hovered == Some(index);
            let active = self.frame.active == Some(index);
            // The open document keeps a quiet ring of its own so "where am I?"
            // is answerable at a glance without hovering anything.
            let ring = if selected {
                2.0
            } else if hovered {
                1.0
            } else if active {
                3.0
            } else {
                0.0
            };
            // Quiet nodes glow by connectedness, so a zoomed-out vault reads
            // as a sky of varying magnitudes rather than uniform fill.
            let glow = if selected || hovered {
                0.95
            } else if is_neighbor || active {
                0.7
            } else {
                0.22 + node_magnitude(node) * 0.5
            };
            let color = kind_color(node.kind);
            let world = self.frame.world(index);
            nodes.push(NodeInstance {
                center: [world.x, world.y],
                radius: node_screen_radius(node, transform.zoom),
                glow,
                color: [color.r, color.g, color.b, 1.0],
                ring,
                dim,
                _pad: [0.0, 0.0],
            });
        }

        let tokens = crate::design::tokens();
        let mut edges = Vec::with_capacity(self.frame.graph.snapshot.edges.len());
        for (edge, &(source, target)) in self
            .frame
            .graph
            .snapshot
            .edges
            .iter()
            .zip(&self.frame.graph.edge_pairs)
        {
            if source == usize::MAX || target == usize::MAX {
                continue;
            }
            let from = self.frame.world(source);
            let to = self.frame.world(target);
            let focused = focus.is_some_and(|focus| source == focus || target == focus);
            let weight = edge.weight.max(1) as f32;
            // Links are the whole point of the view, so even the quietest state
            // stays visible against the canvas (`pages/graph.md`: `border.control`).
            let link = tokens.border_control;
            let (color, alpha, thickness) = if focused {
                (
                    crate::design::fade(tokens.accent, 0.5),
                    0.95,
                    1.8 + weight.ln() * 0.5,
                )
            } else if focus.is_some() {
                (link, 0.24, 1.0 + weight.ln() * 0.4)
            } else {
                (link, 0.48, 1.0 + weight.ln() * 0.4)
            };
            edges.push(EdgeInstance {
                p0: [from.x, from.y],
                p1: [to.x, to.y],
                color: [color.r, color.g, color.b, alpha],
                thickness,
                _pad: [0.0, 0.0, 0.0],
            });
        }

        GpuGraphPrimitive {
            nodes,
            edges,
            globals,
        }
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.gesture.is_some() {
            return mouse::Interaction::Grabbing;
        }
        let transform = self.transform(state, bounds);
        let over_node = cursor
            .position_in(bounds)
            .and_then(|point| self.hit_node(bounds, transform, point))
            .is_some();
        if over_node {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::Grab
        }
    }
}

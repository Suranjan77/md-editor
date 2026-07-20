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

use iced::wgpu;
use iced::widget::shader::{self, Primitive};
use iced::{Point, Rectangle, Vector, mouse};

use bytemuck::{Pod, Zeroable};

use crate::graph_state::{MAX_ZOOM, MIN_ZOOM};
use crate::messages::Message;
use crate::theme;
use crate::views::graph::{
    GraphFrame, ViewTransform, distance, fit_transform, kind_color, node_screen_radius,
    screen_to_world, world_to_screen,
};

/// Extra pixels added to a node's radius when resolving a pointer hit.
const HIT_SLOP: f32 = 4.0;

// ───────────────────────────── GPU data ─────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct Globals {
    /// `[sx, sy, tx, ty]` — `clip = (sx*wx + tx, sy*wy + ty)`.
    transform: [f32; 4],
    /// Logical widget size in points.
    viewport: [f32; 2],
    /// `1.0` when the render target is an sRGB format.
    srgb: f32,
    _pad: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct NodeInstance {
    center: [f32; 2],
    radius: f32,
    glow: f32,
    color: [f32; 4],
    ring: f32,
    dim: f32,
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct EdgeInstance {
    p0: [f32; 2],
    p1: [f32; 2],
    color: [f32; 4],
    thickness: f32,
    _pad: [f32; 3],
}

// ───────────────────────────── primitive ────────────────────────────

/// One frame's worth of graph geometry, handed to the `wgpu` backend.
#[derive(Debug)]
pub struct GpuGraphPrimitive {
    nodes: Vec<NodeInstance>,
    edges: Vec<EdgeInstance>,
    globals: Globals,
}

impl Primitive for GpuGraphPrimitive {
    type Pipeline = GpuGraphPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &shader::Viewport,
    ) {
        // Record the physical widget rectangle so `render` can map clip space
        // onto exactly this widget rather than the whole surface.
        let scale = viewport.scale_factor() as f32;
        pipeline.viewport_rect = [
            bounds.x * scale,
            bounds.y * scale,
            (bounds.width * scale).max(1.0),
            (bounds.height * scale).max(1.0),
        ];

        pipeline.ensure_capacity(device, self.nodes.len(), self.edges.len());

        let mut globals = self.globals;
        globals.srgb = if pipeline.is_srgb { 1.0 } else { 0.0 };
        queue.write_buffer(&pipeline.globals_buffer, 0, bytemuck::bytes_of(&globals));

        if !self.nodes.is_empty() {
            queue.write_buffer(
                &pipeline.nodes_buffer,
                0,
                bytemuck::cast_slice(&self.nodes),
            );
        }
        if !self.edges.is_empty() {
            queue.write_buffer(
                &pipeline.edges_buffer,
                0,
                bytemuck::cast_slice(&self.edges),
            );
        }
    }

    fn render(
        &self,
        pipeline: &Self::Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        if self.nodes.is_empty() {
            return;
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("graph render pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        let [vx, vy, vw, vh] = pipeline.viewport_rect;
        pass.set_viewport(vx, vy, vw, vh, 0.0, 1.0);
        pass.set_scissor_rect(
            clip_bounds.x,
            clip_bounds.y,
            clip_bounds.width.max(1),
            clip_bounds.height.max(1),
        );
        pass.set_bind_group(0, &pipeline.bind_group, &[]);

        if !self.edges.is_empty() {
            pass.set_pipeline(&pipeline.edges_pipeline);
            pass.set_vertex_buffer(0, pipeline.edges_buffer.slice(..));
            pass.draw(0..6, 0..self.edges.len() as u32);
        }

        pass.set_pipeline(&pipeline.nodes_pipeline);
        pass.set_vertex_buffer(0, pipeline.nodes_buffer.slice(..));
        pass.draw(0..6, 0..self.nodes.len() as u32);
    }
}

// ───────────────────────────── pipeline ─────────────────────────────

pub struct GpuGraphPipeline {
    nodes_pipeline: wgpu::RenderPipeline,
    edges_pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    globals_buffer: wgpu::Buffer,
    nodes_buffer: wgpu::Buffer,
    edges_buffer: wgpu::Buffer,
    nodes_capacity: usize,
    edges_capacity: usize,
    is_srgb: bool,
    viewport_rect: [f32; 4],
}

impl shader::Pipeline for GpuGraphPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("graph shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/graph.wgsl").into()),
        });

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("graph globals layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("graph pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let blend = Some(wgpu::BlendState::ALPHA_BLENDING);

        let node_attrs = wgpu::vertex_attr_array![
            0 => Float32x2, // center
            1 => Float32,   // radius
            2 => Float32,   // glow
            3 => Float32x4, // color
            4 => Float32,   // ring
            5 => Float32,   // dim
        ];
        let edge_attrs = wgpu::vertex_attr_array![
            0 => Float32x2, // p0
            1 => Float32x2, // p1
            2 => Float32x4, // color
            3 => Float32,   // thickness
        ];

        let nodes_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("graph nodes pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_node"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<NodeInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &node_attrs,
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_node"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let edges_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("graph edges pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_edge"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<EdgeInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &edge_attrs,
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_edge"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("graph globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let nodes_capacity = 256;
        let edges_capacity = 512;
        let nodes_buffer = new_instance_buffer::<NodeInstance>(device, nodes_capacity, "graph nodes");
        let edges_buffer = new_instance_buffer::<EdgeInstance>(device, edges_capacity, "graph edges");

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("graph globals bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });

        Self {
            nodes_pipeline,
            edges_pipeline,
            bind_group,
            globals_buffer,
            nodes_buffer,
            edges_buffer,
            nodes_capacity,
            edges_capacity,
            is_srgb: format.is_srgb(),
            viewport_rect: [0.0, 0.0, 1.0, 1.0],
        }
    }
}

impl GpuGraphPipeline {
    fn ensure_capacity(&mut self, device: &wgpu::Device, nodes: usize, edges: usize) {
        if nodes > self.nodes_capacity {
            self.nodes_capacity = (nodes * 2).max(256);
            self.nodes_buffer =
                new_instance_buffer::<NodeInstance>(device, self.nodes_capacity, "graph nodes");
        }
        if edges > self.edges_capacity {
            self.edges_capacity = (edges * 2).max(512);
            self.edges_buffer =
                new_instance_buffer::<EdgeInstance>(device, self.edges_capacity, "graph edges");
        }
    }
}

fn new_instance_buffer<T>(device: &wgpu::Device, capacity: usize, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (capacity * std::mem::size_of::<T>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

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
    fitted_size: Option<iced::Size>,
    gesture: Option<Gesture>,
}

impl Default for Interaction {
    fn default() -> Self {
        Self {
            pan: Vector::new(0.0, 0.0),
            zoom: 1.0,
            initialized: false,
            applied_fit: 0,
            fitted_size: None,
            gesture: None,
        }
    }
}

#[derive(Debug, Clone)]
enum Gesture {
    Pan { start: Point, initial_pan: Vector },
    Node { path: String, grab_offset: Vector },
}

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
        let points: Vec<Point> = self
            .frame
            .graph
            .nodes
            .iter()
            .map(|node| self.world_position(&node.path))
            .collect();
        fit_transform(&points, bounds)
    }

    fn world_position(&self, path: &str) -> Point {
        self.frame
            .positions
            .get(path)
            .copied()
            .unwrap_or(Point::ORIGIN)
    }

    /// The transform to render/interact with. Once this widget has applied the
    /// current fit request its own authoritative pan/zoom is used; until then a
    /// freshly computed fit frames the graph so even the very first frame — and
    /// any frame `draw` runs before `update` has committed — is correct.
    fn transform(&self, state: &Interaction, bounds: Rectangle) -> ViewTransform {
        let applied = state.initialized
            && state.applied_fit == self.frame.fit_revision
            && state.fitted_size == Some(bounds.size());
        if applied {
            ViewTransform {
                pan: state.pan,
                zoom: state.zoom,
            }
        } else {
            self.fit(bounds)
        }
    }

    fn hit_node(&self, bounds: Rectangle, transform: ViewTransform, screen: Point) -> Option<usize> {
        self.frame
            .graph
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                let center = world_to_screen(self.world_position(&node.path), bounds, transform);
                let radius = node_screen_radius(node, transform.zoom) + HIT_SLOP;
                let d = distance(screen, center);
                (d <= radius).then_some((index, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| index)
    }
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

        // Commit a pending fit before handling anything else, mirroring it into
        // the shared state so the label overlay adopts the same framing.
        let needs_fit = !state.initialized
            || state.applied_fit != self.frame.fit_revision
            || state.fitted_size != Some(bounds.size());
        if needs_fit {
            let fit = self.fit(bounds);
            state.pan = fit.pan;
            state.zoom = fit.zoom;
            state.initialized = true;
            state.applied_fit = self.frame.fit_revision;
            state.fitted_size = Some(bounds.size());
            state.gesture = None;
            // Publishing already schedules a redraw; no capture so a coincident
            // gesture on this same event still reaches the handlers below.
            return Some(shader::Action::publish(Message::GraphSetView {
                pan_x: fit.pan.x,
                pan_y: fit.pan.y,
                zoom: fit.zoom,
            }));
        }

        let transform = ViewTransform {
            pan: state.pan,
            zoom: state.zoom,
        };
        let cursor_pos = cursor.position_in(bounds);

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let point = cursor_pos?;
                if let Some(index) = self.hit_node(bounds, transform, point) {
                    let node = &self.frame.graph.nodes[index];
                    let world = self.world_position(&node.path);
                    let pointer_world = screen_to_world(point, bounds, transform);
                    state.gesture = Some(Gesture::Node {
                        path: node.path.clone(),
                        grab_offset: pointer_world - world,
                    });
                    Some(
                        shader::Action::publish(Message::GraphNodeSelected(Some(node.path.clone())))
                            .and_capture(),
                    )
                } else {
                    state.gesture = Some(Gesture::Pan {
                        start: point,
                        initial_pan: transform.pan,
                    });
                    Some(shader::Action::publish(Message::GraphNodeSelected(None)).and_capture())
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle)) => {
                let point = cursor_pos?;
                state.gesture = Some(Gesture::Pan {
                    start: point,
                    initial_pan: transform.pan,
                });
                Some(shader::Action::capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => match state.gesture.clone() {
                Some(Gesture::Pan { start, initial_pan }) => {
                    let point = cursor_pos?;
                    let pan = initial_pan + (point - start);
                    state.pan = pan;
                    Some(
                        shader::Action::publish(Message::GraphSetView {
                            pan_x: pan.x,
                            pan_y: pan.y,
                            zoom: state.zoom,
                        })
                        .and_capture(),
                    )
                }
                Some(Gesture::Node { path, grab_offset }) => {
                    let point = cursor_pos?;
                    let world = screen_to_world(point, bounds, transform) - grab_offset;
                    Some(
                        shader::Action::publish(Message::GraphNodeMoved {
                            path,
                            x: world.x,
                            y: world.y,
                        })
                        .and_capture(),
                    )
                }
                None => {
                    let hovered = cursor_pos
                        .and_then(|point| self.hit_node(bounds, transform, point))
                        .map(|index| self.frame.graph.nodes[index].path.clone());
                    if hovered != self.frame.hovered {
                        Some(shader::Action::publish(Message::GraphHovered(hovered)))
                    } else {
                        None
                    }
                }
            },
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left | mouse::Button::Middle)) => {
                match state.gesture.take() {
                    Some(_) => Some(shader::Action::capture()),
                    None => None,
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let point = cursor_pos?;
                let amount = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y * 0.13,
                    mouse::ScrollDelta::Pixels { y, .. } => y * 0.0025,
                };
                let zoomed = crate::views::graph::zoom_about(
                    transform,
                    bounds,
                    point,
                    amount.exp(),
                    MIN_ZOOM,
                    MAX_ZOOM,
                );
                state.pan = zoomed.pan;
                state.zoom = zoomed.zoom;
                Some(
                    shader::Action::publish(Message::GraphSetView {
                        pan_x: zoomed.pan.x,
                        pan_y: zoomed.pan.y,
                        zoom: zoomed.zoom,
                    })
                    .and_capture(),
                )
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

        let focus = self.frame.hovered.as_ref().or(self.frame.selected.as_ref());
        let focus_neighbors = focus.and_then(|path| self.frame.neighbors.get(path));

        let mut nodes = Vec::with_capacity(self.frame.graph.nodes.len());
        for node in &self.frame.graph.nodes {
            let is_focus = focus == Some(&node.path);
            let is_neighbor = focus_neighbors.is_some_and(|set| set.contains(&node.path));
            let dim = if focus.is_some() && !is_focus && !is_neighbor {
                0.28
            } else {
                1.0
            };
            let selected = self.frame.selected.as_deref() == Some(node.path.as_str());
            let hovered = self.frame.hovered.as_deref() == Some(node.path.as_str());
            let active = self.frame.active.as_deref() == Some(node.path.as_str());
            let ring = if selected {
                2.0
            } else if hovered {
                1.0
            } else {
                0.0
            };
            let glow = if selected || hovered {
                0.95
            } else if is_neighbor || active {
                0.65
            } else {
                0.5
            };
            let color = kind_color(node.kind);
            let world = self.world_position(&node.path);
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

        let index: std::collections::HashMap<&str, usize> = self
            .frame
            .graph
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| (node.path.as_str(), i))
            .collect();

        let mut edges = Vec::with_capacity(self.frame.graph.edges.len());
        for edge in &self.frame.graph.edges {
            let (Some(&source), Some(&target)) = (
                index.get(edge.source.as_str()),
                index.get(edge.target.as_str()),
            ) else {
                continue;
            };
            let from = self.world_position(&self.frame.graph.nodes[source].path);
            let to = self.world_position(&self.frame.graph.nodes[target].path);
            let focused =
                focus.is_some_and(|path| &edge.source == path || &edge.target == path);
            let weight = edge.weight.max(1) as f32;
            let (color, alpha, thickness) = if focused {
                (theme::ACCENT_GLOW, 0.85, 1.6 + weight.ln())
            } else if focus.is_some() {
                (
                    iced::Color::from_rgb(0.45, 0.52, 0.54),
                    0.10,
                    0.7 + weight.ln() * 0.35,
                )
            } else {
                (
                    iced::Color::from_rgb(0.45, 0.52, 0.54),
                    0.30,
                    0.7 + weight.ln() * 0.35,
                )
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

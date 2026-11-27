//! The graph's `wgpu` side: instance layouts, the per-frame primitive and the render pipeline.

use iced::Rectangle;
use iced::wgpu;
use iced::widget::shader::{self, Primitive};

use bytemuck::{Pod, Zeroable};

// ───────────────────────────── GPU data ─────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(super) struct Globals {
    /// `[sx, sy, tx, ty]` — `clip = (sx*wx + tx, sy*wy + ty)`.
    pub(super) transform: [f32; 4],
    /// Logical widget size in points.
    pub(super) viewport: [f32; 2],
    /// `1.0` when the render target is an sRGB format.
    pub(super) srgb: f32,
    pub(super) _pad: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(super) struct NodeInstance {
    pub(super) center: [f32; 2],
    pub(super) radius: f32,
    pub(super) glow: f32,
    pub(super) color: [f32; 4],
    pub(super) ring: f32,
    pub(super) dim: f32,
    /// 0.0 Circle (Markdown), 1.0 Rounded Rect (PDF), 2.0 Diamond (Sketch), 3.0 Hollow Ring (Missing).
    pub(super) shape: f32,
    pub(super) _pad: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(super) struct EdgeInstance {
    pub(super) p0: [f32; 2],
    pub(super) p1: [f32; 2],
    pub(super) color: [f32; 4],
    pub(super) thickness: f32,
    /// 0.0 Solid (WikiLink/PdfAnnotation), 1.0 Supports, 2.0 Contradicts, 3.0 Derives.
    pub(super) style: f32,
    pub(super) _pad: [f32; 2],
}

// ───────────────────────────── primitive ────────────────────────────

/// One frame's worth of graph geometry, handed to the `wgpu` backend.
#[derive(Debug)]
pub struct GpuGraphPrimitive {
    pub(super) nodes: Vec<NodeInstance>,
    pub(super) edges: Vec<EdgeInstance>,
    pub(super) globals: Globals,
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
        let scale = viewport.scale_factor();
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
            queue.write_buffer(&pipeline.nodes_buffer, 0, bytemuck::cast_slice(&self.nodes));
        }
        if !self.edges.is_empty() {
            queue.write_buffer(&pipeline.edges_buffer, 0, bytemuck::cast_slice(&self.edges));
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
            source: wgpu::ShaderSource::Wgsl(include_str!("../../../shaders/graph.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
            6 => Float32,   // shape
        ];
        let edge_attrs = wgpu::vertex_attr_array![
            0 => Float32x2, // p0
            1 => Float32x2, // p1
            2 => Float32x4, // color
            3 => Float32,   // thickness
            4 => Float32,   // style
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
        let nodes_buffer =
            new_instance_buffer::<NodeInstance>(device, nodes_capacity, "graph nodes");
        let edges_buffer =
            new_instance_buffer::<EdgeInstance>(device, edges_capacity, "graph edges");

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

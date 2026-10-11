//! Solid rectangles, queued during a frame and drawn in one call. A
//! cut-down Fenix `rect.rs`: Flint needs no rounded corners, borders or
//! shadows yet, so it has none of their shader cost.

use crate::layout::Rect;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

const INITIAL_RECTS: usize = 64;

/// The rectangle pipeline and this frame's rectangles.
pub struct Rects {
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    capacity: usize,
    vertices: Vec<Vertex>,
    count: u32,
}

impl Rects {
    /// Builds the pipeline for surfaces of `format`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Rects {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("flint-rect-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("rect.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("flint-rect-layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("flint-rect-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let capacity = INITIAL_RECTS * 6;
        Rects {
            pipeline,
            buffer: Self::buffer(device, capacity),
            capacity,
            vertices: Vec::new(),
            count: 0,
        }
    }

    fn buffer(device: &wgpu::Device, vertices: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("flint-rect-vertices"),
            size: (vertices * std::mem::size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Starts a frame.
    pub fn clear(&mut self) {
        self.vertices.clear();
    }

    /// Queues `rect` (physical pixels) on a `width`×`height` surface.
    pub fn push(&mut self, rect: Rect, color: [f32; 4], width: f32, height: f32) {
        let x = |px: f32| px / width * 2.0 - 1.0;
        let y = |py: f32| 1.0 - py / height * 2.0;
        let v = |px, py| Vertex {
            position: [x(px), y(py)],
            color,
        };
        let (l, t, r, b) = (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
        self.vertices
            .extend([v(l, t), v(r, t), v(l, b), v(r, t), v(r, b), v(l, b)]);
    }

    /// Uploads the queued rectangles, growing the buffer when needed.
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.vertices.len() > self.capacity {
            self.capacity = self.vertices.len().next_power_of_two();
            self.buffer = Self::buffer(device, self.capacity);
        }
        if !self.vertices.is_empty() {
            queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&self.vertices));
        }
        self.count = self.vertices.len() as u32;
    }

    /// Draws them.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.buffer.slice(..));
        pass.draw(0..self.count, 0..1);
    }
}

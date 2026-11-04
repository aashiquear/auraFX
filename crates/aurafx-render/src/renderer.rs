//! Offscreen wgpu renderer: draws a set of nodes into an MSAA color target that the UI
//! shows as an image. Also provides pixel readback for screenshots.

use crate::camera::Camera;
use crate::tessellate::{MeshData, Vertex};
use anyhow::Result;
use aurafx_pipeline::{colormap, DisplayProps, NodeId};
use bytemuck::{Pod, Zeroable};
use glam::DVec3;
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

const MSAA: u32 = 4;
pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct NodeUniform {
    view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 4],
    light_dir: [f32; 4],
    viewport: [f32; 4],
    color: [f32; 4],
    range: [f32; 4],
    flags: [f32; 4],
}

/// What to draw for one pipeline node this frame.
pub struct NodeDraw {
    pub id: NodeId,
    pub mesh: Arc<MeshData>,
    pub display: DisplayProps,
}

#[derive(Debug, Clone)]
pub struct RenderSettings {
    pub background: [f32; 3],
    pub background_top: Option<[f32; 3]>,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self { background: [0.12, 0.13, 0.16], background_top: None }
    }
}

struct GpuNode {
    mesh_ptr: usize,
    tris: Option<(wgpu::Buffer, u32)>,
    lines: Option<(wgpu::Buffer, u32)>,
    points: Option<(wgpu::Buffer, u32)>,
    uniform: wgpu::Buffer,
    lut_name: String,
    lut: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

pub struct Renderer {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    pub width: u32,
    pub height: u32,
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    msaa_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipe_tris: wgpu::RenderPipeline,
    pipe_tris_blend: wgpu::RenderPipeline,
    pipe_lines: wgpu::RenderPipeline,
    pipe_points: wgpu::RenderPipeline,
    nodes: HashMap<NodeId, GpuNode>,
    pub adapter_info: String,
}

fn vertex_layout(step: wgpu::VertexStepMode) -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: step,
        attributes: &[
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 12, shader_location: 1 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 16, shader_location: 2 },
        ],
    }
}

impl Renderer {
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>, adapter_info: String) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aurafx mesh shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/mesh.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("node bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D1, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&bgl)], immediate_size: 0 });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let make_pipeline = |label: &str, vs: &str, fs: &str, topology: wgpu::PrimitiveTopology, step: wgpu::VertexStepMode, blend: bool, bias: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some(vs), buffers: &[Some(vertex_layout(step))], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend: Some(if blend { wgpu::BlendState::ALPHA_BLENDING } else { wgpu::BlendState::REPLACE }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState { topology, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(!blend),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: if bias { wgpu::DepthBiasState { constant: 2, slope_scale: 1.0, clamp: 0.0 } } else { Default::default() },
                }),
                multisample: wgpu::MultisampleState { count: MSAA, ..Default::default() },
                multiview_mask: None,
                cache: None,
            })
        };
        let pipe_tris = make_pipeline("tris", "vs_mesh", "fs_mesh", wgpu::PrimitiveTopology::TriangleList, wgpu::VertexStepMode::Vertex, false, true);
        let pipe_tris_blend = make_pipeline("tris blend", "vs_mesh", "fs_mesh", wgpu::PrimitiveTopology::TriangleList, wgpu::VertexStepMode::Vertex, true, true);
        let pipe_lines = make_pipeline("lines", "vs_mesh", "fs_line", wgpu::PrimitiveTopology::LineList, wgpu::VertexStepMode::Vertex, true, false);
        let pipe_points = make_pipeline("points", "vs_point", "fs_point", wgpu::PrimitiveTopology::TriangleList, wgpu::VertexStepMode::Instance, true, false);

        let (color, color_view, msaa_view, depth_view) = Self::make_targets(&device, 16, 16);
        Self {
            device,
            queue,
            width: 16,
            height: 16,
            color,
            color_view,
            msaa_view,
            depth_view,
            bgl,
            sampler,
            pipe_tris,
            pipe_tris_blend,
            pipe_lines,
            pipe_points,
            nodes: HashMap::new(),
            adapter_info,
        }
    }

    fn make_targets(device: &wgpu::Device, w: u32, h: u32) -> (wgpu::Texture, wgpu::TextureView, wgpu::TextureView, wgpu::TextureView) {
        let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport color"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let msaa = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport msaa"),
            size,
            mip_level_count: 1,
            sample_count: MSAA,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport depth"),
            size,
            mip_level_count: 1,
            sample_count: MSAA,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let cv = color.create_view(&Default::default());
        let mv = msaa.create_view(&Default::default());
        let dv = depth.create_view(&Default::default());
        (color, cv, mv, dv)
    }

    /// Returns true if the target was recreated (the UI must re-register the texture).
    pub fn resize(&mut self, w: u32, h: u32) -> bool {
        let (w, h) = (w.max(1), h.max(1));
        if w == self.width && h == self.height {
            return false;
        }
        let (c, cv, mv, dv) = Self::make_targets(&self.device, w, h);
        self.color = c;
        self.color_view = cv;
        self.msaa_view = mv;
        self.depth_view = dv;
        self.width = w;
        self.height = h;
        true
    }

    pub fn color_view(&self) -> &wgpu::TextureView {
        &self.color_view
    }

    pub fn device(&self) -> &Arc<wgpu::Device> {
        &self.device
    }

    fn lut_texture(&self, name: &str) -> wgpu::Texture {
        let data = colormap::get(name).lut_rgba8();
        self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: Some("lut"),
                size: wgpu::Extent3d { width: 256, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D1,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &data,
        )
    }

    fn make_bind_group(&self, uniform: &wgpu::Buffer, lut: &wgpu::Texture) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("node bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&lut.create_view(&Default::default())) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        })
    }

    fn upload(&self, label: &str, verts: &[Vertex]) -> Option<(wgpu::Buffer, u32)> {
        if verts.is_empty() {
            return None;
        }
        let buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytemuck::cast_slice(verts), usage: wgpu::BufferUsages::VERTEX });
        Some((buf, verts.len() as u32))
    }

    fn sync_node(&mut self, draw: &NodeDraw) {
        let ptr = Arc::as_ptr(&draw.mesh) as usize;
        let needs_mesh = self.nodes.get(&draw.id).map(|g| g.mesh_ptr != ptr).unwrap_or(true);
        if needs_mesh {
            let tris = self.upload("tris", &draw.mesh.tris);
            let lines = self.upload("lines", &draw.mesh.lines);
            let points = self.upload("points", &draw.mesh.points);
            match self.nodes.get_mut(&draw.id) {
                Some(g) => {
                    g.tris = tris;
                    g.lines = lines;
                    g.points = points;
                    g.mesh_ptr = ptr;
                }
                None => {
                    let uniform = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("node uniform"),
                        size: std::mem::size_of::<NodeUniform>() as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    let lut = self.lut_texture(&draw.display.colormap);
                    let bind_group = self.make_bind_group(&uniform, &lut);
                    self.nodes.insert(draw.id, GpuNode { mesh_ptr: ptr, tris, lines, points, uniform, lut_name: draw.display.colormap.clone(), lut, bind_group });
                }
            }
        }
        let g = self.nodes.get(&draw.id).unwrap();
        if g.lut_name != draw.display.colormap {
            let lut = self.lut_texture(&draw.display.colormap);
            let bind_group = self.make_bind_group(&g.uniform, &lut);
            let g = self.nodes.get_mut(&draw.id).unwrap();
            g.lut = lut;
            g.bind_group = bind_group;
            g.lut_name = draw.display.colormap.clone();
        }
    }

    /// Draw all nodes. `draws` should only contain visible nodes.
    pub fn render(&mut self, camera: &Camera, draws: &[NodeDraw], settings: &RenderSettings) {
        // Drop GPU resources of nodes that no longer exist.
        let live: std::collections::HashSet<NodeId> = draws.iter().map(|d| d.id).collect();
        self.nodes.retain(|id, _| live.contains(id));
        for d in draws {
            self.sync_node(d);
        }
        let aspect = self.width as f64 / self.height.max(1) as f64;
        let light = {
            let f = (camera.eye - camera.target).normalize_or_zero();
            let r = camera.right();
            let u = r.cross(-f).normalize_or_zero();
            (f * 1.0 + r * 0.3 + u * 0.5).normalize_or_zero()
        };
        for d in draws {
            let g = &self.nodes[&d.id];
            let origin = DVec3::from_array(d.mesh.origin);
            let vp = camera.view_proj_f32(aspect, -origin);
            let cam_rel = (camera.eye - origin).as_vec3();
            let (lo, hi) = d.mesh.scalar_range.unwrap_or((0.0, 1.0));
            let u = NodeUniform {
                view_proj: vp.to_cols_array_2d(),
                camera_pos: [cam_rel.x, cam_rel.y, cam_rel.z, 1.0],
                light_dir: [light.x as f32, light.y as f32, light.z as f32, 0.0],
                viewport: [self.width as f32, self.height as f32, 0.0, 0.0],
                color: [d.display.solid_color[0], d.display.solid_color[1], d.display.solid_color[2], d.display.opacity],
                range: [lo as f32, hi as f32, if d.display.log_scale { 1.0 } else { 0.0 }, if d.mesh.has_scalars { 1.0 } else { 0.0 }],
                flags: [if d.display.lighting { 1.0 } else { 0.0 }, d.display.point_size, if d.display.points_as_spheres { 1.0 } else { 0.0 }, 0.0],
            };
            self.queue.write_buffer(&g.uniform, 0, bytemuck::bytes_of(&u));
        }

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("aurafx render") });
        {
            let bg = settings.background;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.msaa_view,
                    depth_slice: None,
                    resolve_target: Some(&self.color_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: srgb_to_linear(bg[0]), g: srgb_to_linear(bg[1]), b: srgb_to_linear(bg[2]), a: 1.0 }),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            // Opaque first, then translucent.
            let mut order: Vec<&NodeDraw> = draws.iter().collect();
            order.sort_by(|a, b| (b.display.opacity >= 1.0).cmp(&(a.display.opacity >= 1.0)));
            for d in order {
                let g = &self.nodes[&d.id];
                pass.set_bind_group(0, &g.bind_group, &[]);
                let translucent = d.display.opacity < 1.0;
                if let Some((buf, n)) = &g.tris {
                    pass.set_pipeline(if translucent { &self.pipe_tris_blend } else { &self.pipe_tris });
                    pass.set_vertex_buffer(0, buf.slice(..));
                    pass.draw(0..*n, 0..1);
                }
                if let Some((buf, n)) = &g.lines {
                    pass.set_pipeline(&self.pipe_lines);
                    pass.set_vertex_buffer(0, buf.slice(..));
                    pass.draw(0..*n, 0..1);
                }
                if let Some((buf, n)) = &g.points {
                    pass.set_pipeline(&self.pipe_points);
                    pass.set_vertex_buffer(0, buf.slice(..));
                    pass.draw(0..6, 0..*n);
                }
            }
        }
        self.queue.submit(Some(encoder.finish()));
    }

    /// Read back the current color target as RGBA8 (sRGB) pixels.
    pub fn read_pixels(&self) -> Result<(u32, u32, Vec<u8>)> {
        let (w, h) = (self.width, self.height);
        let bytes_per_row = (4 * w).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor { label: Some("readback"), size: (bytes_per_row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &self.color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit(Some(enc.finish()));
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        rx.recv()??;
        let data = slice.get_mapped_range()?;
        let mut out = Vec::with_capacity((4 * w * h) as usize);
        for row in 0..h {
            let start = (row * bytes_per_row) as usize;
            out.extend_from_slice(&data[start..start + (4 * w) as usize]);
        }
        drop(data);
        buf.unmap();
        Ok((w, h, out))
    }

    pub fn save_png(&self, path: &std::path::Path) -> Result<()> {
        let (w, h, px) = self.read_pixels()?;
        image::save_buffer(path, &px, w, h, image::ColorType::Rgba8)?;
        Ok(())
    }
}

fn srgb_to_linear(c: f32) -> f64 {
    let c = c as f64;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

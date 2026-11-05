//! The path tracer on the GPU (a compute shader, `trace.wgsl`): same scene, materials, lights
//! and environment as [`crate::raytrace::Tracer`], many times faster.

use crate::raytrace::{encode, slope_bias, CameraFrame, Scene, View};
use crate::ImageLayer;
use anyhow::{bail, Result};
use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    eye: [f32; 4],
    fwd: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    view: [f32; 4],
    frame: [f32; 4],
    sky: [f32; 4],
    background: [f32; 4],
    view_proj: [[f32; 4]; 4],
    light_dirs: [[f32; 4]; 8],
    light_colors: [[f32; 4]; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuNode {
    min: [f32; 3],
    escape: u32,
    max: [f32; 3],
    first_count: u32,
}

/// Escape links of the BVH (children of node i at first, first + 1): where traversal goes
/// when a node is missed or a leaf is done.
fn threaded(scene: &Scene) -> Result<Vec<GpuNode>> {
    let n = &scene.nodes;
    let mut out = vec![GpuNode::zeroed(); n.len()];
    // (node, escape)
    let mut todo = vec![(0usize, u32::MAX)];
    while let Some((i, esc)) = todo.pop() {
        let node = &n[i];
        let first_count = if node.count > 0 {
            if node.first >= 1 << 28 || node.count >= 16 {
                bail!("too many triangles for the GPU tracer");
            }
            node.first | (node.count << 28)
        } else {
            todo.push((node.first as usize, node.first + 1));
            todo.push((node.first as usize + 1, esc));
            node.first
        };
        out[i] = GpuNode { min: node.min.to_array(), escape: esc, max: node.max.to_array(), first_count };
    }
    Ok(out)
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuTri {
    n: [[f32; 4]; 3],
    s: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuTriPos {
    p0: [f32; 4],
    e1: [f32; 4],
    e2: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuMaterial {
    solid: [f32; 4],
    range: [f32; 4],
    mat: [f32; 4],
}

pub struct GpuTracer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    params: Params,
    params_buf: wgpu::Buffer,
    accum: wgpu::Buffer,
    depth: wgpu::Buffer,
    width: u32,
    height: u32,
    pub passes: u32,
}

impl GpuTracer {
    /// Upload `scene` for `view`. Fails when the scene is empty or too big for the device's
    /// storage buffers (the CPU tracer handles those).
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, scene: &Scene, view: &View) -> Result<Self> {
        if scene.nodes.is_empty() {
            bail!("nothing to trace");
        }
        let limit = device.limits().max_storage_buffer_binding_size as usize;
        let ntri = scene.material.len();
        let (w, h) = (view.width.max(1), view.height.max(1));
        let npix = (w * h) as usize;
        if ntri * std::mem::size_of::<GpuTri>() > limit || npix * 16 > limit || scene.materials.len() * 256 * 16 > limit {
            bail!("the scene is too big for this GPU's buffers");
        }
        // Triangles in BVH order, so leaves index them directly.
        let v4 = |v: Vec3, w: f32| [v.x, v.y, v.z, w];
        let tris: Vec<GpuTri> = scene
            .order
            .iter()
            .map(|&t| {
                let t = t as usize;
                GpuTri {
                    n: [v4(scene.normal[3 * t], 0.0), v4(scene.normal[3 * t + 1], 0.0), v4(scene.normal[3 * t + 2], 0.0)],
                    s: [scene.scalar[3 * t], scene.scalar[3 * t + 1], scene.scalar[3 * t + 2], f32::from_bits(scene.material[t] as u32)],
                }
            })
            .collect();
        let tri_pos: Vec<GpuTriPos> = scene
            .order
            .iter()
            .map(|&t| {
                let (a, b, c) = (scene.pos[3 * t as usize], scene.pos[3 * t as usize + 1], scene.pos[3 * t as usize + 2]);
                GpuTriPos { p0: v4(a, 0.0), e1: v4(b - a, 0.0), e2: v4(c - a, 0.0) }
            })
            .collect();
        let nodes = threaded(scene)?;
        let materials: Vec<GpuMaterial> = scene
            .materials
            .iter()
            .map(|m| GpuMaterial {
                solid: v4(m.solid, if m.lit { 1.0 } else { 0.0 }),
                range: [m.range.0, m.range.1, if m.log { 1.0 } else { 0.0 }, if m.scalars { 1.0 } else { 0.0 }],
                mat: [m.metallic, m.roughness, m.specular, m.power],
            })
            .collect();
        let luts: Vec<[f32; 4]> = scene.materials.iter().flat_map(|m| m.lut.iter().map(|c| v4(*c, 1.0))).collect();

        let fr = CameraFrame::new(view, scene.origin);
        let mut light_dirs = [[0.0f32; 4]; 8];
        let mut light_colors = [[0.0f32; 4]; 8];
        for (k, (d, c)) in fr.lights.iter().take(8).enumerate() {
            light_dirs[k] = v4(*d, 0.0);
            light_colors[k] = v4(*c, 0.0);
        }
        let aspect = w as f64 / h as f64;
        let l = &view.lighting;
        let params = Params {
            eye: v4(fr.eye, 0.0),
            fwd: v4(fr.fwd, 0.0),
            right: v4(fr.right, 0.0),
            up: v4(fr.up, 0.0),
            view: [w as f32, h as f32, fr.half_h, if view.camera.orthographic { 1.0 } else { 0.0 }],
            frame: [0.0, view.bounces as f32, 2e-5 * scene.radius, fr.lights.len().min(8) as f32],
            sky: [fr.ambient, if view.environment.is_some() { 1.0 } else { 0.0 }, l.environment_intensity, if l.show_environment { 1.0 } else { 0.0 }],
            background: [view.background[0], view.background[1], view.background[2], 0.0],
            view_proj: view.camera.view_proj_f32(aspect, scene.origin).to_cols_array_2d(),
            light_dirs,
            light_colors,
        };

        let storage = |label: &str, bytes: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytes, usage: wgpu::BufferUsages::STORAGE })
        };
        let nodes_buf = storage("trace nodes", bytemuck::cast_slice(&nodes));
        let tris_buf = storage("trace triangles", bytemuck::cast_slice(&tris));
        let pos_buf = storage("trace triangle corners", bytemuck::cast_slice(&tri_pos));
        let mat_buf = storage("trace materials", bytemuck::cast_slice(&materials));
        let lut_buf = storage("trace color maps", bytemuck::cast_slice(&luts));
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("trace params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let accum = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("trace accumulation"),
            contents: bytemuck::cast_slice(&vec![[0.0f32; 4]; npix]),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let depth = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("trace depth"),
            contents: bytemuck::cast_slice(&vec![1.0f32; npix]),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        // The environment (first level), or a black pixel.
        let (ew, eh, texels): (u32, u32, Vec<u16>) = match &view.environment {
            Some(e) => (
                e.width as u32,
                e.height as u32,
                e.pixels.iter().flat_map(|c| [c.x, c.y, c.z, 1.0]).map(|v| half::f16::from_f32(v.min(60000.0)).to_bits()).collect(),
            ),
            None => (1, 1, vec![0, 0, 0, half::f16::ONE.to_bits()]),
        };
        let env_tex = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("trace environment"),
                size: wgpu::Extent3d { width: ew, height: eh, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(&texels),
        );
        let env_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("trace.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/trace.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("path tracer"),
            layout: None,
            module: &shader,
            entry_point: Some("trace"),
            compilation_options: Default::default(),
            cache: None,
        });
        let env_view = env_tex.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("path tracer"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: nodes_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: tris_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: mat_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: lut_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: accum.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 6, resource: depth.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&env_view) },
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(&env_sampler) },
                wgpu::BindGroupEntry { binding: 9, resource: pos_buf.as_entire_binding() },
            ],
        });
        Ok(Self { device: device.clone(), queue: queue.clone(), pipeline, bind_group, params, params_buf, accum, depth, width: w, height: h, passes: 0 })
    }

    /// One more sample per pixel (queued on the GPU; `image` waits for it).
    pub fn pass(&mut self) {
        self.params.frame[0] = self.passes as f32;
        self.queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&self.params));
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("trace pass") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("trace"), timestamp_writes: None });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.dispatch_workgroups(self.width.div_ceil(8), self.height.div_ceil(8), 1);
        }
        self.queue.submit(Some(enc.finish()));
        self.passes += 1;
    }

    /// Wait for the queued passes to finish.
    pub fn finish(&self) -> Result<()> {
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        Ok(())
    }

    fn read(&self, buf: &wgpu::Buffer) -> Result<Vec<u8>> {
        let size = buf.size();
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("trace readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(buf, 0, &staging, 0, size);
        self.queue.submit(Some(enc.finish()));
        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        rx.recv()??;
        let data = slice.get_mapped_range()?.to_vec();
        staging.unmap();
        Ok(data)
    }

    /// The accumulated picture (sRGB) and depth.
    pub fn image(&self) -> Result<ImageLayer> {
        let accum = self.read(&self.accum)?;
        let depth = self.read(&self.depth)?;
        let sums: &[[f32; 4]] = bytemuck::cast_slice(&accum);
        let k = 1.0 / self.passes.max(1) as f32;
        let color = encode(sums.iter().map(|s| Vec3::new(s[0], s[1], s[2])), k);
        let depth: &[f32] = bytemuck::cast_slice(&depth);
        let depth = slope_bias(depth, self.width as usize, self.height as usize);
        Ok(ImageLayer { width: self.width, height: self.height, color, depth })
    }
}

/// Either tracer, behind one interface.
pub enum AnyTracer {
    Cpu(crate::raytrace::Tracer),
    Gpu(GpuTracer),
}

impl AnyTracer {
    /// The GPU tracer when a device is given and the scene fits, else the CPU one.
    pub fn new(gpu: Option<(&wgpu::Device, &wgpu::Queue)>, scene: std::sync::Arc<Scene>, view: View) -> AnyTracer {
        if let Some((d, q)) = gpu {
            match GpuTracer::new(d, q, &scene, &view) {
                Ok(t) => return AnyTracer::Gpu(t),
                Err(e) => tracing::info!("ray tracing on the CPU: {e:#}"),
            }
        }
        AnyTracer::Cpu(crate::raytrace::Tracer::new(scene, view))
    }

    pub fn pass(&mut self) {
        match self {
            AnyTracer::Cpu(t) => t.pass(),
            AnyTracer::Gpu(t) => t.pass(),
        }
    }

    pub fn passes(&self) -> u32 {
        match self {
            AnyTracer::Cpu(t) => t.passes,
            AnyTracer::Gpu(t) => t.passes,
        }
    }

    pub fn image(&self) -> Result<ImageLayer> {
        match self {
            AnyTracer::Cpu(t) => Ok(t.image()),
            AnyTracer::Gpu(t) => t.image(),
        }
    }

    /// Wait for queued work (GPU); the CPU tracer works synchronously.
    pub fn finish(&self) {
        if let AnyTracer::Gpu(t) = self {
            let _ = t.finish();
        }
    }

    pub fn on_gpu(&self) -> bool {
        matches!(self, AnyTracer::Gpu(_))
    }
}

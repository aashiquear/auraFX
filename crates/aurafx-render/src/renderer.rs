//! Offscreen wgpu renderer: draws a set of nodes into an MSAA color target that the UI
//! shows as an image. Also provides pixel readback for screenshots.

use crate::camera::Camera;
use crate::tessellate::{MeshData, Vertex};
use anyhow::Result;
use aurafx_pipeline::{DisplayProps, NodeId};
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
    shadow_vp: [[f32; 4]; 4],
    light_info: [f32; 4],
    light_dirs: [[f32; 4]; 8],
    light_colors: [[f32; 4]; 8],
    material: [f32; 4],
    env: [f32; 4],
    sh: [[f32; 4]; 9],
}

/// What to draw for one pipeline node this frame.
#[derive(Clone)]
pub struct NodeDraw {
    pub id: NodeId,
    pub mesh: Arc<MeshData>,
    pub display: DisplayProps,
    /// Drawn last, ignoring depth (selection highlights that must show through surfaces).
    pub overlay: bool,
}

#[derive(Debug, Clone)]
pub struct RenderSettings {
    pub background: [f32; 3],
    pub background_top: Option<[f32; 3]>,
    /// Lights, shadows and screen effects.
    pub lighting: aurafx_pipeline::Lighting,
    /// Clear to transparent (for screenshots): the color target then holds premultiplied
    /// color, see [`Renderer::save_png_with`].
    pub transparent: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self { background: [0.12, 0.13, 0.16], background_top: None, lighting: Default::default(), transparent: false }
    }
}

const SHADOW_SIZE: u32 = 2048;
/// Surface LIC contrast: scales the averaged noise's deviation from grey by sqrt(samples).
const LIC_CONTRAST: f32 = 1.0;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct VolumeUniform {
    box_min: [f32; 4],
    box_max: [f32; 4],
    dims: [f32; 4],
    params: [f32; 4],
}

struct GpuVolume {
    _texture: wgpu::Texture,
    params: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

struct GpuNode {
    volume: Option<GpuVolume>,
    /// The mesh uploaded (kept alive so a new mesh cannot reuse its address).
    mesh: Arc<crate::tessellate::MeshData>,
    /// Vertex buffers, split to stay within the device's buffer size limit.
    tris: Vec<(wgpu::Buffer, u32)>,
    lines: Vec<(wgpu::Buffer, u32)>,
    points: Vec<(wgpu::Buffer, u32)>,
    /// Polygon edges of Surface With Edges, drawn in the edge color with their own uniform.
    edges: Vec<(wgpu::Buffer, u32)>,
    edge_uniform: wgpu::Buffer,
    edge_bg: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    lut_name: String,
    lut: wgpu::Texture,
    /// Image file shown on the surface ("" = none) and its texture.
    tex_key: String,
    tex: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    /// The uniform alone, for the shadow pass (which writes the shadow map).
    shadow_bg: wgpu::BindGroup,
    /// Surface LIC vectors, one buffer per `tris` buffer.
    lic: Vec<wgpu::Buffer>,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pub width: u32,
    pub height: u32,
    color: wgpu::Texture,
    /// sRGB view used as the resolve target (hardware encodes linear -> sRGB).
    color_view: wgpu::TextureView,
    /// Plain-format view of the same bytes for display by egui (which expects gamma-encoded data).
    display_view: wgpu::TextureView,
    msaa_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipe_tris: wgpu::RenderPipeline,
    pipe_overlay_tris: wgpu::RenderPipeline,
    pipe_overlay_lines: wgpu::RenderPipeline,
    pipe_overlay_points: wgpu::RenderPipeline,
    pipe_volume: wgpu::RenderPipeline,
    pipe_id: wgpu::RenderPipeline,
    id_targets: Option<(u32, u32, wgpu::Texture, wgpu::TextureView, wgpu::TextureView)>,
    vol_bgl: wgpu::BindGroupLayout,
    scene_bgl: wgpu::BindGroupLayout,
    scene_bg: wgpu::BindGroup,
    vol_sampler: wgpu::Sampler,
    nodes: HashMap<NodeId, GpuNode>,
    pub adapter_info: String,
    pipe_resolve_depth: wgpu::RenderPipeline,
    pipe_layer: wgpu::RenderPipeline,
    layer_bgl: wgpu::BindGroupLayout,
    /// Images drawn into the scene (server-drawn objects), with the image they came from.
    layers: Vec<(Arc<ImageLayer>, wgpu::BindGroup, wgpu::Buffer)>,
    samples: u32,
    pipe_lines_opaque: wgpu::RenderPipeline,
    pipe_points_opaque: wgpu::RenderPipeline,
    pipe_tris_oit: wgpu::RenderPipeline,
    pipe_lines_oit: wgpu::RenderPipeline,
    pipe_wide_lines: wgpu::RenderPipeline,
    pipe_overlay_wide_lines: wgpu::RenderPipeline,
    pipe_wide_lines_oit: wgpu::RenderPipeline,
    pipe_points_oit: wgpu::RenderPipeline,
    pipe_edl: wgpu::RenderPipeline,
    edl_uniform: wgpu::Buffer,
    edl_bg: wgpu::BindGroup,
    pipe_oit_composite: wgpu::RenderPipeline,
    oit_bgl: wgpu::BindGroupLayout,
    oit: OitTargets,
    /// Set from the render settings each frame.
    light_kit: bool,
    env_intensity: f32,
    shadow_view: wgpu::TextureView,
    shadow_sampler: wgpu::Sampler,
    shadow_bgl: wgpu::BindGroupLayout,
    pipe_shadow: wgpu::RenderPipeline,
    /// World view-projection of the shadow light this frame (None: no shadows).
    shadow_vp: Option<glam::DMat4>,
    /// Lights this frame: (world direction towards the light, color * intensity, casts shadows).
    lights: Vec<([f32; 3], [f32; 3], bool)>,
    ambient: f32,
    pipe_ssao: wgpu::RenderPipeline,
    ao_uniform: wgpu::Buffer,
    ao_bg: wgpu::BindGroup,
    env: GpuEnv,
    env_sampler: wgpu::Sampler,
    /// Images shown on surfaces (display texture), by file path; None when it failed to load.
    textures: HashMap<String, Option<wgpu::TextureView>>,
    /// 1×1 white texture bound when a node shows no image.
    white: wgpu::TextureView,
    tex_sampler: wgpu::Sampler,
    /// An environment that could not be loaded (not retried every frame).
    env_failed: Option<String>,
    pipe_sky: wgpu::RenderPipeline,
    sky_bgl: wgpu::BindGroupLayout,
    sky_uniform: wgpu::Buffer,
    sky_bg: wgpu::BindGroup,
    pipe_ao_blur: wgpu::RenderPipeline,
    ao_blur_bgl: wgpu::BindGroupLayout,
    ao_target: AoTarget,
    pipe_lic_vec: wgpu::RenderPipeline,
    pipe_lic: wgpu::RenderPipeline,
    lic_uniform: wgpu::Buffer,
    lic_target: LicTarget,
    pipe_final: wgpu::RenderPipeline,
    final_bgl: wgpu::BindGroupLayout,
    final_uniform: wgpu::Buffer,
    /// The resolved picture before the final pass (FXAA, tone mapping).
    post_src: PostTarget,
}

const AO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
const LIC_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;

/// Surface LIC: screen directions and depth of the LIC surfaces (with their own depth buffer).
struct LicTarget {
    view: wgpu::TextureView,
    depth: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

impl LicTarget {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, uniform: &wgpu::Buffer, w: u32, h: u32) -> Self {
        let tex = |label: &str, format: wgpu::TextureFormat, usage: wgpu::TextureUsages| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let view = tex("lic vectors", LIC_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let depth = tex("lic depth", DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lic"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
            ],
        });
        Self { view, depth, bind_group }
    }
}

/// Raw (noisy) ambient occlusion, blurred onto the scene by a second pass.
struct AoTarget {
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

impl AoTarget {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, uniform: &wgpu::Buffer, w: u32, h: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ambient occlusion"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: AO_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ao blur"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
            ],
        });
        Self { view, bind_group }
    }
}

/// Where the scene is resolved when a final pass (FXAA, tone mapping) follows.
struct PostTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

impl PostTarget {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, uniform: &wgpu::Buffer, w: u32, h: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("post source"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[COLOR_FORMAT],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor { format: Some(COLOR_FORMAT), ..Default::default() });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("final"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: uniform.as_entire_binding() },
            ],
        });
        Self { texture, view, bind_group }
    }
}

const OIT_ACCUM: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const OIT_REVEAL: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

/// Targets of weighted blended transparency: accumulated color and revealage, drawn with the
/// scene's sample count and resolved for the composite pass.
struct OitTargets {
    accum: wgpu::TextureView,
    reveal: wgpu::TextureView,
    accum_resolved: Option<wgpu::TextureView>,
    reveal_resolved: Option<wgpu::TextureView>,
    bind_group: wgpu::BindGroup,
}

impl OitTargets {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, w: u32, h: u32, samples: u32) -> Self {
        let make = |label: &str, format: wgpu::TextureFormat, count: u32| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: count,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | if count == 1 { wgpu::TextureUsages::TEXTURE_BINDING } else { wgpu::TextureUsages::empty() },
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let accum = make("oit accum", OIT_ACCUM, samples);
        let reveal = make("oit reveal", OIT_REVEAL, samples);
        let (accum_resolved, reveal_resolved) =
            if samples > 1 { (Some(make("oit accum resolved", OIT_ACCUM, 1)), Some(make("oit reveal resolved", OIT_REVEAL, 1))) } else { (None, None) };
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oit"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(accum_resolved.as_ref().unwrap_or(&accum)) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(reveal_resolved.as_ref().unwrap_or(&reveal)) },
            ],
        });
        Self { accum, reveal, accum_resolved, reveal_resolved, bind_group }
    }
}

/// A picture of some objects with its depth, drawn into the scene as if it were geometry
/// (for objects drawn on a server). `color` is sRGB RGBA8 and `depth` the depth-buffer values
/// (1.0 = nothing there), both row by row from the top, rendered with the same camera.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageLayer {
    pub width: u32,
    pub height: u32,
    pub color: Vec<u8>,
    pub depth: Vec<f32>,
}

fn env_tex_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// The environment map on the GPU (a black pixel when there is none).
struct GpuEnv {
    env: Option<std::sync::Arc<crate::env::Environment>>,
    view: wgpu::TextureView,
    mips: u32,
}

impl GpuEnv {
    fn upload(device: &wgpu::Device, queue: &wgpu::Queue, env: Option<std::sync::Arc<crate::env::Environment>>) -> Self {
        let levels = match &env {
            Some(e) => e.mips(),
            None => vec![(1, 1, vec![half::f16::ZERO, half::f16::ZERO, half::f16::ZERO, half::f16::ONE])],
        };
        let (w, h) = (levels[0].0 as u32, levels[0].1 as u32);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("environment"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, (lw, lh, data)) in levels.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: level as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                bytemuck::cast_slice(&data.iter().map(|h| h.to_bits()).collect::<Vec<u16>>()),
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(8 * *lw as u32), rows_per_image: Some(*lh as u32) },
                wgpu::Extent3d { width: *lw as u32, height: *lh as u32, depth_or_array_layers: 1 },
            );
        }
        Self { env, view: texture.create_view(&Default::default()), mips: levels.len() as u32 }
    }
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

/// A line segment's two vertices per instance (wide lines: `vs_wide_line`).
fn segment_layout() -> wgpu::VertexBufferLayout<'static> {
    let v = std::mem::size_of::<Vertex>() as u64;
    wgpu::VertexBufferLayout {
        array_stride: 2 * v,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &SEGMENT_ATTRIBUTES,
    }
}

const SEGMENT_ATTRIBUTES: [wgpu::VertexAttribute; 6] = {
    let v = std::mem::size_of::<Vertex>() as u64;
    [
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 12, shader_location: 1 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 16, shader_location: 2 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: v, shader_location: 3 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: v + 12, shader_location: 4 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: v + 16, shader_location: 5 },
    ]
};

impl Renderer {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, adapter_info: String) -> Self {
        Self::with_samples(device, queue, adapter_info, MSAA)
    }

    /// A renderer with `samples` samples per pixel (1 = no multisampling: edges are not blended
    /// with the background, which pictures composited from pieces need).
    pub fn with_samples(device: wgpu::Device, queue: wgpu::Queue, adapter_info: String, samples: u32) -> Self {
        let samples = if samples > 1 { MSAA } else { 1 };
        // Shaders reading the scene depth see a plain texture without multisampling.
        let ms = |src: &str| if samples > 1 { src.to_string() } else { src.replace("texture_depth_multisampled_2d", "texture_depth_2d") };
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D1,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                env_tex_entry(5),
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&bgl)], immediate_size: 0 });
        // Shadows: a depth map seen from the shadow light, sampled with comparison.
        let shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow map"),
            size: wgpu::Extent3d { width: SHADOW_SIZE, height: SHADOW_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_tex.create_view(&Default::default());
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let shadow_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let shadow_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&shadow_bgl)], immediate_size: 0 });
        let pipe_shadow = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow map"),
            layout: Some(&shadow_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_shadow"),
                buffers: &[Some(vertex_layout(wgpu::VertexStepMode::Vertex))],
                compilation_options: Default::default(),
            },
            fragment: None,
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let env_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("environment"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let env = GpuEnv::upload(&device, &queue, None);
        let tex_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("surface texture"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let white = upload_image(&device, &queue, 1, 1, &[255, 255, 255, 255]);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let make_pipeline =
            |label: &str, vs: &str, fs: &str, topology: wgpu::PrimitiveTopology, step: wgpu::VertexStepMode, blend: bool, bias: bool, overlay: bool| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some(vs),
                        buffers: &[Some(if vs == "vs_wide_line" { segment_layout() } else { vertex_layout(step) })],
                        compilation_options: Default::default(),
                    },
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
                        depth_write_enabled: Some(!blend && !overlay),
                        depth_compare: Some(if overlay { wgpu::CompareFunction::Always } else { wgpu::CompareFunction::LessEqual }),
                        stencil: Default::default(),
                        bias: if bias { wgpu::DepthBiasState { constant: 2, slope_scale: 1.0, clamp: 0.0 } } else { Default::default() },
                    }),
                    multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
                    multiview_mask: None,
                    cache: None,
                })
            };
        use wgpu::PrimitiveTopology::{LineList, TriangleList};
        use wgpu::VertexStepMode::{Instance, Vertex};
        let pipe_tris = make_pipeline("tris", "vs_mesh", "fs_mesh", TriangleList, Vertex, false, true, false);
        let pipe_overlay_tris = make_pipeline("overlay tris", "vs_mesh", "fs_mesh", TriangleList, Vertex, true, false, true);
        let pipe_overlay_lines = make_pipeline("overlay lines", "vs_mesh", "fs_line", LineList, Vertex, true, false, true);
        let pipe_overlay_points = make_pipeline("overlay points", "vs_point", "fs_point", TriangleList, Instance, true, false, true);
        // Opaque lines and points write depth (for occlusion and Eye-Dome Lighting).
        let pipe_lines_opaque = make_pipeline("lines opaque", "vs_mesh", "fs_line", LineList, Vertex, false, false, false);
        let pipe_points_opaque = make_pipeline("points opaque", "vs_point", "fs_point", TriangleList, Instance, false, false, false);
        // Lines wider than a pixel: a quad per segment.
        let pipe_wide_lines = make_pipeline("wide lines", "vs_wide_line", "fs_line", TriangleList, Instance, false, false, false);
        let pipe_overlay_wide_lines = make_pipeline("overlay wide lines", "vs_wide_line", "fs_line", TriangleList, Instance, true, false, true);
        // Translucent geometry: weighted blended order-independent transparency.
        let make_oit = |label: &str, vs: &str, fs: &str, topology: wgpu::PrimitiveTopology, step: wgpu::VertexStepMode| {
            let add = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
            let keep =
                wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::OneMinusSrc, operation: wgpu::BlendOperation::Add };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    buffers: &[Some(if vs == "vs_wide_line" { segment_layout() } else { vertex_layout(step) })],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    targets: &[
                        Some(wgpu::ColorTargetState {
                            format: OIT_ACCUM,
                            blend: Some(wgpu::BlendState { color: add, alpha: add }),
                            write_mask: wgpu::ColorWrites::ALL,
                        }),
                        Some(wgpu::ColorTargetState {
                            format: OIT_REVEAL,
                            blend: Some(wgpu::BlendState { color: keep, alpha: keep }),
                            write_mask: wgpu::ColorWrites::ALL,
                        }),
                    ],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState { topology, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
                multiview_mask: None,
                cache: None,
            })
        };
        let pipe_tris_oit = make_oit("tris oit", "vs_mesh", "fs_mesh_oit", TriangleList, Vertex);
        let pipe_lines_oit = make_oit("lines oit", "vs_mesh", "fs_line_oit", LineList, Vertex);
        let pipe_points_oit = make_oit("points oit", "vs_point", "fs_point_oit", TriangleList, Instance);
        let pipe_wide_lines_oit = make_oit("wide lines oit", "vs_wide_line", "fs_line_oit", TriangleList, Instance);

        let vol_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aurafx volume shader"),
            source: wgpu::ShaderSource::Wgsl(ms(include_str!("shaders/volume.wgsl")).into()),
        });
        let vol_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("volume bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
        });
        let scene_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene depth bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: samples > 1,
                },
                count: None,
            }],
        });
        let vol_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bgl), Some(&vol_bgl), Some(&scene_bgl)],
            immediate_size: 0,
        });
        let pipe_volume = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("volume"),
            layout: Some(&vol_layout),
            vertex: wgpu::VertexState { module: &vol_shader, entry_point: Some("vs_volume"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &vol_shader,
                entry_point: Some("fs_volume"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: Some(wgpu::Face::Front), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });
        let id_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aurafx id shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/id.wgsl").into()),
        });
        let pipe_id = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cell ids"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &id_shader,
                entry_point: Some("vs_id"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<crate::tessellate::Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint32, offset: 28, shader_location: 3 },
                    ],
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &id_shader,
                entry_point: Some("fs_id"),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::R32Uint, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let vol_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let layer_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aurafx layer shader"),
            source: wgpu::ShaderSource::Wgsl(ms(include_str!("shaders/layer.wgsl")).into()),
        });
        let resolve_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&scene_bgl)], immediate_size: 0 });
        let pipe_resolve_depth = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("resolve depth"),
            layout: Some(&resolve_layout),
            vertex: wgpu::VertexState { module: &layer_shader, entry_point: Some("vs_full"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &layer_shader,
                entry_point: Some("fs_resolve_depth"),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::R32Float, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let tex_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layer_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("layer bgl"),
            entries: &[
                tex_entry(0),
                tex_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
        });
        let layer_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layer_bgl)], immediate_size: 0 });
        let pipe_layer = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image layer"),
            layout: Some(&layer_layout),
            vertex: wgpu::VertexState { module: &layer_shader, entry_point: Some("vs_full"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &layer_shader,
                entry_point: Some("fs_layer"),
                targets: &[Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: Some(wgpu::BlendState::REPLACE), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });

        let post_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aurafx post shader"),
            source: wgpu::ShaderSource::Wgsl(ms(include_str!("shaders/post.wgsl")).into()),
        });
        let uniform_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let edl_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("edl bgl"), entries: &[uniform_entry(0)] });
        let edl_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("edl uniform"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let edl_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("edl"),
            layout: &edl_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: edl_uniform.as_entire_binding() }],
        });
        let edl_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&scene_bgl), Some(&edl_bgl)],
            immediate_size: 0,
        });
        let multiply = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::Src, operation: wgpu::BlendOperation::Add };
        let pipe_edl = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("eye-dome lighting"),
            layout: Some(&edl_layout),
            vertex: wgpu::VertexState { module: &post_shader, entry_point: Some("vs_full"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &post_shader,
                entry_point: Some("fs_edl"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: Some(wgpu::BlendState { color: multiply, alpha: wgpu::BlendComponent::OVER }),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });
        let oit_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("oit bgl"), entries: &[tex_entry(0), tex_entry(1)] });
        let oit_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&oit_bgl)], immediate_size: 0 });
        let pipe_oit_composite = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("transparency composite"),
            layout: Some(&oit_layout),
            vertex: wgpu::VertexState { module: &post_shader, entry_point: Some("vs_full"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &post_shader,
                entry_point: Some("fs_oit"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });

        let ao_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ssao uniform"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ao_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ssao"),
            layout: &edl_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ao_uniform.as_entire_binding() }],
        });
        let pipe_ssao = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ambient occlusion"),
            layout: Some(&edl_layout),
            vertex: wgpu::VertexState { module: &post_shader, entry_point: Some("vs_full"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &post_shader,
                entry_point: Some("fs_ssao"),
                targets: &[Some(wgpu::ColorTargetState { format: AO_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let ao_blur_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("ao blur bgl"), entries: &[uniform_entry(0), tex_entry(1)] });
        let ao_blur_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&scene_bgl), Some(&ao_blur_bgl)],
            immediate_size: 0,
        });
        let pipe_ao_blur = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ambient occlusion blur"),
            layout: Some(&ao_blur_layout),
            vertex: wgpu::VertexState { module: &post_shader, entry_point: Some("vs_full"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &post_shader,
                entry_point: Some("fs_ao_blur"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: Some(wgpu::BlendState { color: multiply, alpha: wgpu::BlendComponent::OVER }),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });
        // Surface LIC: directions into a float target, then noise convolved along them.
        let pipe_lic_vec = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lic vectors"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_lic"),
                buffers: &[
                    Some(vertex_layout(wgpu::VertexStepMode::Vertex)),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: 16,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 0, shader_location: 3 }],
                    }),
                ],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_lic_vec"),
                targets: &[Some(wgpu::ColorTargetState { format: LIC_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let pipe_lic = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("surface lic"),
            layout: Some(&ao_blur_layout),
            vertex: wgpu::VertexState { module: &post_shader, entry_point: Some("vs_full"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &post_shader,
                entry_point: Some("fs_lic"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: Some(wgpu::BlendState { color: multiply, alpha: wgpu::BlendComponent::OVER }),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });
        let lic_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lic uniform"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let final_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("final bgl"), entries: &[tex_entry(0), uniform_entry(1)] });
        let final_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&final_bgl)], immediate_size: 0 });
        let pipe_final = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("final"),
            layout: Some(&final_layout),
            vertex: wgpu::VertexState { module: &post_shader, entry_point: Some("vs_full"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &post_shader,
                entry_point: Some("fs_final"),
                targets: &[Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sky_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky bgl"),
            entries: &[
                uniform_entry(0),
                env_tex_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sky_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&sky_bgl)], immediate_size: 0 });
        let pipe_sky = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
            layout: Some(&sky_layout),
            vertex: wgpu::VertexState { module: &post_shader, entry_point: Some("vs_full"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &post_shader,
                entry_point: Some("fs_sky"),
                targets: &[Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });
        let sky_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sky uniform"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sky_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky"),
            layout: &sky_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: sky_uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&env.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&env_sampler) },
            ],
        });
        let final_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("final uniform"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let (color, color_view, display_view, msaa_view, depth_view) = Self::make_targets(&device, 16, 16, samples);
        let scene_bg = Self::make_scene_bg(&device, &scene_bgl, &depth_view);
        let oit = OitTargets::new(&device, &oit_bgl, 16, 16, samples);
        let post_src = PostTarget::new(&device, &final_bgl, &final_uniform, 16, 16);
        let ao_target = AoTarget::new(&device, &ao_blur_bgl, &ao_uniform, 16, 16);
        let lic_target = LicTarget::new(&device, &ao_blur_bgl, &lic_uniform, 16, 16);
        Self {
            device,
            queue,
            width: 16,
            height: 16,
            color,
            color_view,
            display_view,
            msaa_view,
            depth_view,
            bgl,
            sampler,
            pipe_tris,
            pipe_overlay_tris,
            pipe_overlay_lines,
            pipe_overlay_points,
            pipe_volume,
            pipe_id,
            id_targets: None,
            vol_bgl,
            scene_bgl,
            scene_bg,
            vol_sampler,
            nodes: HashMap::new(),
            adapter_info,
            pipe_resolve_depth,
            pipe_layer,
            layer_bgl,
            layers: Vec::new(),
            samples,
            pipe_lines_opaque,
            pipe_points_opaque,
            pipe_tris_oit,
            pipe_lines_oit,
            pipe_wide_lines,
            pipe_overlay_wide_lines,
            pipe_wide_lines_oit,
            pipe_points_oit,
            pipe_edl,
            edl_uniform,
            edl_bg,
            pipe_oit_composite,
            oit_bgl,
            oit,
            light_kit: true,
            env_intensity: 1.0,
            shadow_view,
            shadow_sampler,
            shadow_bgl,
            pipe_shadow,
            shadow_vp: None,
            lights: Vec::new(),
            ambient: 0.15,
            pipe_ssao,
            ao_uniform,
            ao_bg,
            env,
            env_sampler,
            textures: HashMap::new(),
            white,
            tex_sampler,
            env_failed: None,
            pipe_sky,
            sky_bgl,
            sky_uniform,
            sky_bg,
            pipe_ao_blur,
            ao_blur_bgl,
            ao_target,
            pipe_lic_vec,
            pipe_lic,
            lic_uniform,
            lic_target,
            pipe_final,
            final_bgl,
            final_uniform,
            post_src,
        }
    }

    /// Images to draw into the scene from now on (uploaded only when they change).
    pub fn set_layers(&mut self, layers: &[Arc<ImageLayer>]) {
        let unchanged = layers.len() == self.layers.len() && layers.iter().zip(&self.layers).all(|(a, (b, _, _))| Arc::ptr_eq(a, b));
        if unchanged {
            return;
        }
        let old = std::mem::take(&mut self.layers);
        for l in layers {
            if let Some((_, bg, u)) = old.iter().find(|(o, _, _)| Arc::ptr_eq(o, l)) {
                self.layers.push((l.clone(), bg.clone(), u.clone()));
                continue;
            }
            if l.width == 0 || l.height == 0 || l.color.len() < (4 * l.width * l.height) as usize || l.depth.len() < (l.width * l.height) as usize {
                continue;
            }
            let size = wgpu::Extent3d { width: l.width, height: l.height, depth_or_array_layers: 1 };
            let make = |label: &str, format: wgpu::TextureFormat, bytes: &[u8], bpp: u32| {
                self.device.create_texture_with_data(
                    &self.queue,
                    &wgpu::TextureDescriptor {
                        label: Some(label),
                        size,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    &bytes[..(bpp * l.width * l.height) as usize],
                )
            };
            let color = make("layer color", wgpu::TextureFormat::Rgba8UnormSrgb, &l.color, 4);
            let depth = make("layer depth", wgpu::TextureFormat::R32Float, bytemuck::cast_slice(&l.depth), 4);
            let uniform = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("layer uniform"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("layer"),
                layout: &self.layer_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&color.create_view(&Default::default())) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&depth.create_view(&Default::default())) },
                    wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
                ],
            });
            self.layers.push((l.clone(), bg, uniform));
        }
    }

    /// The depth buffer of the last `render` (sample 0 of each pixel), row by row from the top.
    pub fn read_depth(&self) -> Result<Vec<f32>> {
        let (w, h) = (self.width, self.height);
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("resolved depth"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let bytes_per_row = (4 * w).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("depth readback"),
            size: (bytes_per_row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("resolve depth"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 1.0, g: 0.0, b: 0.0, a: 0.0 }), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(&self.pipe_resolve_depth);
            pass.set_bind_group(0, &self.scene_bg, &[]);
            pass.draw(0..3, 0..1);
        }
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: Some(h) },
            },
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
        let mut out = Vec::with_capacity((w * h) as usize);
        for row in 0..h {
            let start = (row * bytes_per_row) as usize;
            out.extend_from_slice(bytemuck::cast_slice(&data[start..start + (4 * w) as usize]));
        }
        drop(data);
        buf.unmap();
        Ok(out)
    }

    fn make_targets(
        device: &wgpu::Device,
        w: u32,
        h: u32,
        samples: u32,
    ) -> (wgpu::Texture, wgpu::TextureView, wgpu::TextureView, wgpu::TextureView, wgpu::TextureView) {
        let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport color"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[COLOR_FORMAT],
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
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let cv = color.create_view(&wgpu::TextureViewDescriptor { format: Some(COLOR_FORMAT), ..Default::default() });
        let disp = color.create_view(&wgpu::TextureViewDescriptor { format: Some(wgpu::TextureFormat::Rgba8Unorm), ..Default::default() });
        // Without multisampling, draw straight into the color target.
        let mv = if samples > 1 {
            msaa.create_view(&Default::default())
        } else {
            color.create_view(&wgpu::TextureViewDescriptor { format: Some(COLOR_FORMAT), ..Default::default() })
        };
        let dv = depth.create_view(&Default::default());
        (color, cv, disp, mv, dv)
    }

    fn make_scene_bg(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, depth: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene depth"),
            layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(depth) }],
        })
    }

    /// Returns true if the target was recreated (the UI must re-register the texture).
    pub fn resize(&mut self, w: u32, h: u32) -> bool {
        let (w, h) = (w.max(1), h.max(1));
        if w == self.width && h == self.height {
            return false;
        }
        let (c, cv, disp, mv, dv) = Self::make_targets(&self.device, w, h, self.samples);
        self.color = c;
        self.color_view = cv;
        self.display_view = disp;
        self.msaa_view = mv;
        self.scene_bg = Self::make_scene_bg(&self.device, &self.scene_bgl, &dv);
        self.depth_view = dv;
        self.oit = OitTargets::new(&self.device, &self.oit_bgl, w, h, self.samples);
        self.post_src = PostTarget::new(&self.device, &self.final_bgl, &self.final_uniform, w, h);
        self.ao_target = AoTarget::new(&self.device, &self.ao_blur_bgl, &self.ao_uniform, w, h);
        self.lic_target = LicTarget::new(&self.device, &self.ao_blur_bgl, &self.lic_uniform, w, h);
        self.width = w;
        self.height = h;
        true
    }

    /// View to hand to the UI for display.
    pub fn color_view(&self) -> &wgpu::TextureView {
        &self.display_view
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    fn lut_texture(&self, display: &DisplayProps) -> wgpu::Texture {
        let data = display.effective_colormap().lut_rgba8();
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

    /// The texture of an image file shown on surfaces (loaded once; white if it cannot be read).
    fn surface_texture(&mut self, path: &str) -> wgpu::TextureView {
        if !self.textures.contains_key(path) {
            let t = match image::open(path) {
                Ok(img) => {
                    let img = img.to_rgba8();
                    let max = self.device.limits().max_texture_dimension_2d;
                    let img = if img.width() > max || img.height() > max {
                        let f = max as f64 / img.width().max(img.height()) as f64;
                        image::imageops::resize(&img, (img.width() as f64 * f) as u32, (img.height() as f64 * f) as u32, image::imageops::FilterType::Triangle)
                    } else {
                        img
                    };
                    Some(upload_image(&self.device, &self.queue, img.width(), img.height(), img.as_raw()))
                }
                Err(e) => {
                    tracing::warn!("texture {path}: {e}");
                    None
                }
            };
            self.textures.insert(path.to_string(), t);
        }
        self.textures[path].clone().unwrap_or_else(|| self.white.clone())
    }

    fn make_bind_group(&self, uniform: &wgpu::Buffer, lut: &wgpu::Texture, tex: &wgpu::TextureView) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("node bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&lut.create_view(&Default::default())) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.shadow_view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.shadow_sampler) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&self.env.view) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(&self.env_sampler) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(tex) },
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(&self.tex_sampler) },
            ],
        })
    }

    /// The environment in use (for the ray tracer).
    pub fn environment(&self) -> Option<std::sync::Arc<crate::env::Environment>> {
        self.env.env.clone()
    }

    /// Load the environment the settings name (once), and rebind every node to it.
    fn sync_environment(&mut self, lighting: &aurafx_pipeline::Lighting) {
        let want = lighting.environment.trim();
        let have = self.env.env.as_ref().map(|e| e.name.as_str()).unwrap_or("");
        if want == have || self.env_failed.as_deref() == Some(want) {
            return;
        }
        let env = if want.is_empty() {
            None
        } else {
            match crate::env::Environment::load(want) {
                Ok(e) => Some(std::sync::Arc::new(e)),
                Err(e) => {
                    tracing::warn!("environment: {e:#}");
                    self.env_failed = Some(want.to_string());
                    None
                }
            }
        };
        if env.is_some() {
            self.env_failed = None;
        }
        if env.is_none() && self.env.env.is_none() {
            return;
        }
        self.env = GpuEnv::upload(&self.device, &self.queue, env);
        self.sky_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky"),
            layout: &self.sky_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.sky_uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.env.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.env_sampler) },
            ],
        });
        let ids: Vec<NodeId> = self.nodes.keys().copied().collect();
        for id in ids {
            let g = &self.nodes[&id];
            let bg = self.make_bind_group(&g.uniform, &g.lut, &g.tex);
            let ebg = self.make_bind_group(&g.edge_uniform, &g.lut, &self.white);
            let g = self.nodes.get_mut(&id).unwrap();
            g.bind_group = bg;
            g.edge_bg = ebg;
        }
    }

    fn upload_volume(&self, v: &crate::tessellate::VolumeData) -> GpuVolume {
        let size = wgpu::Extent3d { width: v.dims[0].max(1), height: v.dims[1].max(1), depth_or_array_layers: v.dims[2].max(1) };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("volume"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::R16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(&v.values.iter().map(|h| h.to_bits()).collect::<Vec<u16>>()),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(2 * size.width), rows_per_image: Some(size.height) },
            size,
        );
        let params = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("volume params"),
            size: std::mem::size_of::<VolumeUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D3), ..Default::default() });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("volume bg"),
            layout: &self.vol_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.vol_sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: params.as_entire_binding() },
            ],
        });
        GpuVolume { _texture: texture, params, bind_group }
    }

    /// Upload vertices in pieces of whole primitives (`group` vertices each) that fit the
    /// device's largest buffer.
    /// LIC vectors split like the triangle buffers they go with.
    fn upload_lic(&self, lic: &[[f32; 4]], tris: &[(wgpu::Buffer, u32)]) -> Vec<wgpu::Buffer> {
        if lic.is_empty() {
            return Vec::new();
        }
        let mut start = 0usize;
        tris.iter()
            .map(|(_, n)| {
                let end = (start + *n as usize).min(lic.len());
                let buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("lic vectors"),
                    contents: bytemuck::cast_slice(&lic[start..end]),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                start = end;
                buf
            })
            .collect()
    }

    fn upload(&self, label: &str, verts: &[Vertex], group: usize) -> Vec<(wgpu::Buffer, u32)> {
        let max = (self.device.limits().max_buffer_size.min(1 << 30) as usize / std::mem::size_of::<Vertex>()) / group * group;
        verts
            .chunks(max.max(group))
            .map(|c| {
                let buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytemuck::cast_slice(c),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                (buf, c.len() as u32)
            })
            .collect()
    }

    fn sync_node(&mut self, draw: &NodeDraw) {
        let needs_mesh = self.nodes.get(&draw.id).map(|g| !Arc::ptr_eq(&g.mesh, &draw.mesh)).unwrap_or(true);
        if needs_mesh {
            let volume = draw.mesh.volume.as_ref().map(|v| self.upload_volume(v));
            let tris = self.upload("tris", &draw.mesh.tris, 3);
            let lines = self.upload("lines", &draw.mesh.lines, 2);
            let points = self.upload("points", &draw.mesh.points, 1);
            let edges = self.upload("edges", &draw.mesh.edges, 2);
            let lic = self.upload_lic(&draw.mesh.lic, &tris);
            match self.nodes.get_mut(&draw.id) {
                Some(g) => {
                    g.lic = lic;
                    g.volume = volume;
                    g.tris = tris;
                    g.lines = lines;
                    g.points = points;
                    g.edges = edges;
                    g.mesh = draw.mesh.clone();
                }
                None => {
                    let uniform = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("node uniform"),
                        size: std::mem::size_of::<NodeUniform>() as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    let lut = self.lut_texture(&draw.display);
                    let tex = self.white.clone();
                    let bind_group = self.make_bind_group(&uniform, &lut, &tex);
                    let edge_uniform = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("node edge uniform"),
                        size: std::mem::size_of::<NodeUniform>() as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    let edge_bg = self.make_bind_group(&edge_uniform, &lut, &self.white);
                    let shadow_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("node shadow"),
                        layout: &self.shadow_bgl,
                        entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
                    });
                    self.nodes.insert(
                        draw.id,
                        GpuNode {
                            volume,
                            mesh: draw.mesh.clone(),
                            tris,
                            lines,
                            points,
                            edges,
                            edge_uniform,
                            edge_bg,
                            uniform,
                            lut_name: draw.display.colormap_key(),
                            lut,
                            tex_key: String::new(),
                            tex,
                            bind_group,
                            shadow_bg,
                            lic,
                        },
                    );
                }
            }
        }
        let key = draw.display.colormap_key();
        if self.nodes[&draw.id].lut_name != key {
            let lut = self.lut_texture(&draw.display);
            let ebg = self.make_bind_group(&self.nodes[&draw.id].edge_uniform, &lut, &self.white);
            let g = self.nodes.get_mut(&draw.id).unwrap();
            g.lut = lut;
            g.edge_bg = ebg;
            g.lut_name = key;
            g.tex_key = "\0".into(); // rebuild the bind group below
        }
        let tex_key = if draw.mesh.textured { draw.display.texture.clone().unwrap_or_default() } else { String::new() };
        if self.nodes[&draw.id].tex_key != tex_key {
            let tex = if tex_key.is_empty() { self.white.clone() } else { self.surface_texture(&tex_key) };
            let g = &self.nodes[&draw.id];
            let bind_group = self.make_bind_group(&g.uniform, &g.lut, &tex);
            let g = self.nodes.get_mut(&draw.id).unwrap();
            g.tex = tex;
            g.tex_key = tex_key;
            g.bind_group = bind_group;
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
        self.light_kit = settings.lighting.light_kit;
        self.sync_environment(&settings.lighting);
        self.env_intensity = settings.lighting.environment_intensity.max(0.0);
        // Lights in world coordinates (camera-relative ones follow the view).
        let (lights, ambient) = settings.lighting.effective_lights();
        let f = (camera.eye - camera.target).normalize_or_zero();
        let r = camera.right();
        let u = r.cross(-f).normalize_or_zero();
        self.lights = lights
            .iter()
            .map(|l| {
                let d = DVec3::new(l.direction[0] as f64, l.direction[1] as f64, l.direction[2] as f64);
                let w = if l.camera_relative { r * d.x + u * d.y + f * d.z } else { d };
                let w = w.normalize_or_zero();
                ([w.x as f32, w.y as f32, w.z as f32], [l.color[0] * l.intensity, l.color[1] * l.intensity, l.color[2] * l.intensity], l.shadows)
            })
            .collect();
        self.ambient = ambient;
        // Bounds of what is drawn (for the shadow map and occlusion radius).
        let scene = draws.iter().filter(|d| !d.overlay).fold(aurafx_data::Bounds::EMPTY, |b, d| b.union(&d.mesh.bounds));
        self.shadow_vp = None;
        if settings.lighting.shadows && !scene.is_empty() {
            if let Some((dir, _, _)) = self.lights.iter().find(|l| l.2) {
                let l = DVec3::new(dir[0] as f64, dir[1] as f64, dir[2] as f64);
                let c = DVec3::from_array(scene.center());
                let rad = (scene.diagonal() * 0.5).max(1e-9);
                let eye = c + l * rad * 2.0;
                let up = if l.cross(DVec3::Z).length() > 1e-3 { DVec3::Z } else { DVec3::Y };
                let lc = Camera { eye, target: c, up, orthographic: true, ortho_height: rad, near: 0.0, far: rad * 4.0, ..Camera::default() };
                self.shadow_vp = Some(lc.proj_matrix(1.0) * lc.view_matrix());
            }
        }
        self.write_uniforms(camera, draws, None);

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("aurafx render") });
        let opaque: Vec<&NodeDraw> = draws.iter().filter(|d| !d.overlay && d.mesh.volume.is_none() && d.display.opacity >= 1.0).collect();
        let volumes: Vec<&NodeDraw> = draws.iter().filter(|d| !d.overlay && d.mesh.volume.is_some()).collect();
        let translucent: Vec<&NodeDraw> = draws.iter().filter(|d| !d.overlay && d.mesh.volume.is_none() && d.display.opacity < 1.0).collect();
        let overlays: Vec<&NodeDraw> = draws.iter().filter(|d| d.overlay).collect();
        let bg = settings.background;
        // 0. Shadow map: depth of the opaque surfaces seen from the shadow light.
        if self.shadow_vp.is_some() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow map"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.pipe_shadow);
            for d in &opaque {
                let g = &self.nodes[&d.id];
                pass.set_bind_group(0, &g.shadow_bg, &[]);
                for (buf, n) in &g.tris {
                    pass.set_vertex_buffer(0, buf.slice(..));
                    pass.draw(0..*n, 0..1);
                }
            }
        }
        // 1. Opaque geometry (and server pictures) writes depth.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("opaque"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.msaa_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(if settings.transparent {
                            wgpu::Color::TRANSPARENT
                        } else {
                            wgpu::Color { r: srgb_to_linear(bg[0]), g: srgb_to_linear(bg[1]), b: srgb_to_linear(bg[2]), a: 1.0 }
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            if settings.lighting.show_environment && self.env.env.is_some() && !settings.transparent {
                let aspect = self.width as f64 / self.height.max(1) as f64;
                let f = camera.forward();
                let r = camera.right();
                let u = r.cross(f).normalize_or_zero();
                let half = if camera.orthographic { 0.35 } else { (camera.fov_y_deg.to_radians() * 0.5).tan() };
                let v = |a: DVec3| [a.x as f32, a.y as f32, a.z as f32, 0.0];
                let data = [v(f), v(r * half * aspect), v(u * half), [self.env_intensity, self.width as f32, self.height as f32, 0.0]];
                self.queue.write_buffer(&self.sky_uniform, 0, bytemuck::cast_slice(&data));
                pass.set_pipeline(&self.pipe_sky);
                pass.set_bind_group(0, &self.sky_bg, &[]);
                pass.draw(0..3, 0..1);
            }
            for (l, bg, u) in &self.layers {
                let size = [l.width as f32, l.height as f32, self.width as f32, self.height as f32];
                self.queue.write_buffer(u, 0, bytemuck::cast_slice(&size));
                pass.set_pipeline(&self.pipe_layer);
                pass.set_bind_group(0, bg, &[]);
                pass.draw(0..3, 0..1);
            }
            for d in &opaque {
                self.draw_node(&mut pass, d);
            }
        }
        let load_pass = |encoder: &mut wgpu::CommandEncoder, label: &'static str, view: &wgpu::TextureView, depth: Option<&wgpu::TextureView>| {
            encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(label),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: depth.map(|d| wgpu::RenderPassDepthStencilAttachment { view: d, depth_ops: None, stencil_ops: None }),
                    ..Default::default()
                })
                .forget_lifetime()
        };
        // 1b. Surface LIC: streaks along the vector field darken the surface.
        let lic: Vec<&&NodeDraw> =
            opaque.iter().filter(|d| d.display.representation == aurafx_pipeline::Representation::SurfaceLic && !d.mesh.lic.is_empty()).collect();
        if !lic.is_empty() {
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("lic vectors"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.lic_target.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.lic_target.depth,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_pipeline(&self.pipe_lic_vec);
                for d in &lic {
                    let g = &self.nodes[&d.id];
                    pass.set_bind_group(0, &g.bind_group, &[]);
                    for ((buf, n), v) in g.tris.iter().zip(&g.lic) {
                        pass.set_vertex_buffer(0, buf.slice(..));
                        pass.set_vertex_buffer(1, v.slice(..));
                        pass.draw(0..*n, 0..1);
                    }
                }
            }
            let steps = lic.iter().map(|d| d.display.lic_length).max().unwrap_or(20).clamp(1, 200);
            let u = [
                camera.near as f32,
                camera.far as f32,
                if camera.orthographic { 1.0 } else { 0.0 },
                0.0,
                self.width as f32,
                self.height as f32,
                steps as f32,
                LIC_CONTRAST,
            ];
            self.queue.write_buffer(&self.lic_uniform, 0, bytemuck::cast_slice(&u));
            let mut pass = load_pass(&mut encoder, "surface lic", &self.msaa_view, None);
            pass.set_pipeline(&self.pipe_lic);
            pass.set_bind_group(0, &self.scene_bg, &[]);
            pass.set_bind_group(1, &self.lic_target.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        // 2. Eye-Dome Lighting darkens what lies behind its neighbours.
        if settings.lighting.edl && (!opaque.is_empty() || !self.layers.is_empty()) {
            let u = [
                camera.near as f32,
                camera.far as f32,
                if camera.orthographic { 1.0 } else { 0.0 },
                settings.lighting.edl_strength,
                self.width as f32,
                self.height as f32,
                (self.width.max(self.height) as f32 / 900.0).max(1.0).round(),
                0.0,
            ];
            self.queue.write_buffer(&self.edl_uniform, 0, bytemuck::cast_slice(&u));
            let mut pass = load_pass(&mut encoder, "edl", &self.msaa_view, None);
            pass.set_pipeline(&self.pipe_edl);
            pass.set_bind_group(0, &self.scene_bg, &[]);
            pass.set_bind_group(1, &self.edl_bg, &[]);
            pass.draw(0..3, 0..1);
        }
        // 2b. Ambient occlusion darkens creases and cavities.
        if settings.lighting.ssao && !opaque.is_empty() && !scene.is_empty() {
            let half = if camera.orthographic { camera.ortho_height } else { (camera.fov_y_deg.to_radians() * 0.5).tan() };
            let u = [
                camera.near as f32,
                camera.far as f32,
                if camera.orthographic { 1.0 } else { 0.0 },
                half as f32,
                self.width as f32,
                self.height as f32,
                (settings.lighting.ssao_radius as f64 * scene.diagonal()) as f32,
                settings.lighting.ssao_strength,
            ];
            self.queue.write_buffer(&self.ao_uniform, 0, bytemuck::cast_slice(&u));
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ambient occlusion"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.ao_target.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    ..Default::default()
                });
                pass.set_pipeline(&self.pipe_ssao);
                pass.set_bind_group(0, &self.scene_bg, &[]);
                pass.set_bind_group(1, &self.ao_bg, &[]);
                pass.draw(0..3, 0..1);
            }
            let mut pass = load_pass(&mut encoder, "ambient occlusion blur", &self.msaa_view, None);
            pass.set_pipeline(&self.pipe_ao_blur);
            pass.set_bind_group(0, &self.scene_bg, &[]);
            pass.set_bind_group(1, &self.ao_target.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        // 3. Volumes (they read the opaque depth).
        if !volumes.is_empty() {
            let mut pass = load_pass(&mut encoder, "volumes", &self.msaa_view, Some(&self.depth_view));
            for d in &volumes {
                self.draw_node(&mut pass, d);
            }
        }
        // 4. Translucent geometry, independent of drawing order, then composited over the scene.
        if !translucent.is_empty() {
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("transparency"),
                    color_attachments: &[
                        Some(wgpu::RenderPassColorAttachment {
                            view: &self.oit.accum,
                            depth_slice: None,
                            resolve_target: self.oit.accum_resolved.as_ref(),
                            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                        }),
                        Some(wgpu::RenderPassColorAttachment {
                            view: &self.oit.reveal,
                            depth_slice: None,
                            resolve_target: self.oit.reveal_resolved.as_ref(),
                            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
                        }),
                    ],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &self.depth_view, depth_ops: None, stencil_ops: None }),
                    ..Default::default()
                });
                for d in &translucent {
                    self.draw_node(&mut pass, d);
                }
            }
            let mut pass = load_pass(&mut encoder, "transparency composite", &self.msaa_view, None);
            pass.set_pipeline(&self.pipe_oit_composite);
            pass.set_bind_group(0, &self.oit.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        // 5. Overlays (selection highlights) show through everything.
        if !overlays.is_empty() {
            let mut pass = load_pass(&mut encoder, "overlays", &self.msaa_view, Some(&self.depth_view));
            for d in &overlays {
                self.draw_node(&mut pass, d);
            }
        }
        // 6. Resolve the samples (into the post source when a final pass follows).
        let post =
            settings.lighting.fxaa || settings.lighting.tone_mapping != aurafx_pipeline::ToneMapping::None || (settings.lighting.exposure - 1.0).abs() > 1e-6;
        if self.samples > 1 {
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("resolve"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.msaa_view,
                    depth_slice: None,
                    resolve_target: Some(if post { &self.post_src.view } else { &self.color_view }),
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Discard },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
        } else if post {
            let size = wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 };
            encoder.copy_texture_to_texture(self.color.as_image_copy(), self.post_src.texture.as_image_copy(), size);
        }
        // 7. Final pass: FXAA and tone mapping.
        if post {
            let tm = match settings.lighting.tone_mapping {
                aurafx_pipeline::ToneMapping::None => 0.0,
                aurafx_pipeline::ToneMapping::Reinhard => 1.0,
                aurafx_pipeline::ToneMapping::Aces => 2.0,
            };
            self.queue.write_buffer(
                &self.final_uniform,
                0,
                bytemuck::cast_slice(&[if settings.lighting.fxaa { 1.0f32 } else { 0.0 }, tm, settings.lighting.exposure, 0.0]),
            );
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("final"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(&self.pipe_final);
            pass.set_bind_group(0, &self.post_src.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
    }

    /// Per-node uniforms; `target` marks the node whose cell ids an ID pass should record.
    fn write_uniforms(&self, camera: &Camera, draws: &[NodeDraw], target: Option<NodeId>) {
        let aspect = self.width as f64 / self.height.max(1) as f64;
        let light = self.lights.first().map(|l| DVec3::new(l.0[0] as f64, l.0[1] as f64, l.0[2] as f64)).unwrap_or(DVec3::Z);
        let mut dirs = [[0.0f32; 4]; 8];
        let mut colors = [[0.0f32; 4]; 8];
        let mut shadow_done = false;
        for (k, (d, c, sh)) in self.lights.iter().take(8).enumerate() {
            let casts = *sh && self.shadow_vp.is_some() && !shadow_done;
            shadow_done |= casts;
            dirs[k] = [d[0], d[1], d[2], if casts { 1.0 } else { 0.0 }];
            colors[k] = [c[0], c[1], c[2], 0.0];
        }
        let mut sh = [[0.0f32; 4]; 9];
        if let Some(e) = &self.env.env {
            for (k, c) in e.sh.iter().enumerate() {
                sh[k] = [c.x, c.y, c.z, 0.0];
            }
        }
        let light_info = [self.lights.len().min(8) as f32, self.ambient, if self.shadow_vp.is_some() { 1.0 } else { 0.0 }, 0.0015];
        for d in draws {
            let g = &self.nodes[&d.id];
            let origin = DVec3::from_array(d.mesh.origin);
            let vp = camera.view_proj_f32(aspect, origin);
            let cam_rel = (camera.eye - origin).as_vec3();
            let (lo, hi) = d.mesh.scalar_range.unwrap_or((0.0, 1.0));
            let u = NodeUniform {
                view_proj: vp.to_cols_array_2d(),
                camera_pos: [cam_rel.x, cam_rel.y, cam_rel.z, 1.0],
                light_dir: [light.x as f32, light.y as f32, light.z as f32, 0.0],
                viewport: [self.width as f32, self.height as f32, d.display.lic_strength.clamp(0.0, 1.0), d.display.line_width.max(0.1)],
                color: [d.display.solid_color[0], d.display.solid_color[1], d.display.solid_color[2], d.display.opacity],
                range: [lo as f32, hi as f32, if d.display.log_scale { 1.0 } else { 0.0 }, if d.mesh.has_scalars { 1.0 } else { 0.0 }],
                flags: [
                    if d.display.lighting { 1.0 } else { 0.0 },
                    d.display.point_size,
                    if d.display.points_as_spheres { 1.0 } else { 0.0 },
                    if target == Some(d.id) { 1.0 } else { 0.0 },
                ],
                shadow_vp: self
                    .shadow_vp
                    .map(|m| (m * glam::DMat4::from_translation(origin)).as_mat4().to_cols_array_2d())
                    .unwrap_or(glam::Mat4::IDENTITY.to_cols_array_2d()),
                light_info,
                light_dirs: dirs,
                light_colors: colors,
                material: [
                    d.display.metallic.clamp(0.0, 1.0),
                    d.display.roughness.clamp(0.0, 1.0),
                    d.display.specular.max(0.0),
                    aurafx_pipeline::display::specular_power(d.display.roughness),
                ],
                env: [
                    if self.env.env.is_some() { 1.0 } else { 0.0 },
                    self.env_intensity,
                    self.env.mips.saturating_sub(1) as f32,
                    if d.mesh.textured && !g.tex_key.is_empty() {
                        1.0
                    } else if d.mesh.direct_rgb {
                        2.0
                    } else {
                        0.0
                    },
                ],
                sh,
            };
            self.queue.write_buffer(&g.uniform, 0, bytemuck::bytes_of(&u));
            if !g.edges.is_empty() {
                let e = d.display.edge_color;
                let eu = NodeUniform {
                    color: [e[0], e[1], e[2], d.display.opacity],
                    range: [0.0, 1.0, 0.0, 0.0],
                    flags: [0.0, u.flags[1], u.flags[2], u.flags[3]],
                    env: [u.env[0], u.env[1], u.env[2], 0.0],
                    ..u
                };
                self.queue.write_buffer(&g.edge_uniform, 0, bytemuck::bytes_of(&eu));
            }
            if let (Some(gv), Some(v)) = (&g.volume, &d.mesh.volume) {
                let vu = VolumeUniform {
                    box_min: [v.box_min[0], v.box_min[1], v.box_min[2], 0.0],
                    box_max: [v.box_max[0], v.box_max[1], v.box_max[2], 0.0],
                    dims: [v.dims[0] as f32, v.dims[1] as f32, v.dims[2] as f32, 0.0],
                    params: [d.display.volume_opacity, d.display.volume_threshold, 512.0, 0.0],
                };
                self.queue.write_buffer(&gv.params, 0, bytemuck::bytes_of(&vu));
            }
        }
    }

    /// Ids of the cells of node `target` that are visible inside the pixel rectangle
    /// `[x0, y0, x1, y1)` (call after `render` in the same frame). Other nodes still occlude.
    pub fn pick_ids(&mut self, camera: &Camera, draws: &[NodeDraw], target: NodeId, rect: [u32; 4]) -> Result<Vec<u32>> {
        let (w, h) = (self.width, self.height);
        if self.id_targets.as_ref().map(|t| (t.0, t.1) != (w, h)).unwrap_or(true) {
            let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("ids"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Uint,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let depth = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("id depth"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let (tv, dv) = (tex.create_view(&Default::default()), depth.create_view(&Default::default()));
            self.id_targets = Some((w, h, tex, tv, dv));
        }
        self.write_uniforms(camera, draws, Some(target));
        let (_, _, tex, tv, dv) = self.id_targets.as_ref().unwrap();
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ids"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: tv,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: dv,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.pipe_id);
            for d in draws.iter().filter(|d| !d.overlay && d.display.opacity >= 1.0) {
                let Some(g) = self.nodes.get(&d.id) else { continue };
                for (buf, n) in &g.tris {
                    pass.set_bind_group(0, &g.bind_group, &[]);
                    pass.set_vertex_buffer(0, buf.slice(..));
                    pass.draw(0..*n, 0..1);
                }
            }
        }
        let x0 = rect[0].min(w.saturating_sub(1));
        let y0 = rect[1].min(h.saturating_sub(1));
        let rw = rect[2].clamp(x0 + 1, w) - x0;
        let rh = rect[3].clamp(y0 + 1, h) - y0;
        let bpr = (4 * rw).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("id readback"),
            size: (bpr * rh) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d { x: x0, y: y0, z: 0 }, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(rh) } },
            wgpu::Extent3d { width: rw, height: rh, depth_or_array_layers: 1 },
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
        let mut ids = std::collections::BTreeSet::new();
        for row in 0..rh {
            let start = (row * bpr) as usize;
            for c in data[start..start + (4 * rw) as usize].chunks_exact(4) {
                let v = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                if v > 0 {
                    ids.insert(v - 1);
                }
            }
        }
        drop(data);
        buf.unmap();
        // Restore normal uniforms for the next frame.
        self.write_uniforms(camera, draws, None);
        Ok(ids.into_iter().collect())
    }

    fn draw_node(&self, pass: &mut wgpu::RenderPass<'_>, d: &NodeDraw) {
        let g = &self.nodes[&d.id];
        pass.set_bind_group(0, &g.bind_group, &[]);
        if let Some(gv) = &g.volume {
            pass.set_pipeline(&self.pipe_volume);
            pass.set_bind_group(1, &gv.bind_group, &[]);
            pass.set_bind_group(2, &self.scene_bg, &[]);
            pass.draw(0..36, 0..1);
            return;
        }
        let translucent = d.display.opacity < 1.0;
        let (pt, pl, pp, pw) = if d.overlay {
            (&self.pipe_overlay_tris, &self.pipe_overlay_lines, &self.pipe_overlay_points, &self.pipe_overlay_wide_lines)
        } else if translucent {
            (&self.pipe_tris_oit, &self.pipe_lines_oit, &self.pipe_points_oit, &self.pipe_wide_lines_oit)
        } else {
            (&self.pipe_tris, &self.pipe_lines_opaque, &self.pipe_points_opaque, &self.pipe_wide_lines)
        };
        for (buf, n) in &g.tris {
            pass.set_pipeline(pt);
            pass.set_vertex_buffer(0, buf.slice(..));
            pass.draw(0..*n, 0..1);
        }
        // Hairlines for a width of 1 pixel (or less); wider lines as a quad per segment.
        let wide = d.display.line_width > 1.0;
        let draw_lines = |pass: &mut wgpu::RenderPass<'_>, bufs: &[(wgpu::Buffer, u32)]| {
            for (buf, n) in bufs {
                pass.set_vertex_buffer(0, buf.slice(..));
                if wide {
                    pass.set_pipeline(pw);
                    pass.draw(0..6, 0..*n / 2);
                } else {
                    pass.set_pipeline(pl);
                    pass.draw(0..*n, 0..1);
                }
            }
        };
        draw_lines(pass, &g.lines);
        if !g.edges.is_empty() {
            pass.set_bind_group(0, &g.edge_bg, &[]);
            draw_lines(pass, &g.edges);
            pass.set_bind_group(0, &g.bind_group, &[]);
        }
        for (buf, n) in &g.points {
            pass.set_pipeline(pp);
            pass.set_vertex_buffer(0, buf.slice(..));
            pass.draw(0..6, 0..*n);
        }
    }

    /// Read back the current color target as RGBA8 (sRGB) pixels.
    pub fn read_pixels(&self) -> Result<(u32, u32, Vec<u8>)> {
        self.read_texture(&self.color)
    }

    fn read_texture(&self, texture: &wgpu::Texture) -> Result<(u32, u32, Vec<u8>)> {
        let (w, h) = (self.width, self.height);
        let bytes_per_row = (4 * w).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (bytes_per_row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: Some(h) },
            },
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

    /// The last picture with something drawn over it (scalar bar, labels...) by `paint`, which
    /// gets the device, queue and a view (Rgba8Unorm: values are sRGB-encoded, as egui wants)
    /// of a copy of the picture; the picture shown is left as it was. Returns (width, height, sRGB RGBA8 pixels).
    pub fn read_pixels_with(&self, paint: impl FnOnce(&wgpu::Device, &wgpu::Queue, &wgpu::TextureView, u32, u32) -> Result<()>) -> Result<(u32, u32, Vec<u8>)> {
        let (w, h) = (self.width, self.height);
        let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        let copy = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("annotated picture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[COLOR_FORMAT],
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_texture(self.color.as_image_copy(), copy.as_image_copy(), size);
        self.queue.submit(Some(enc.finish()));
        let view = copy.create_view(&Default::default());
        paint(&self.device, &self.queue, &view, w, h)?;
        self.read_texture(&copy)
    }

    /// The largest picture this device can render.
    pub fn max_size(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }

    pub fn save_png(&self, path: &std::path::Path) -> Result<()> {
        self.save_png_with(path, false)
    }

    /// Save the last picture; `transparent` when it was rendered with a transparent background
    /// (the premultiplied colors are divided by coverage, so edges keep their color).
    pub fn save_png_with(&self, path: &std::path::Path, transparent: bool) -> Result<()> {
        let (w, h, mut px) = self.read_pixels()?;
        if transparent {
            unpremultiply(&mut px);
        }
        image::save_buffer(path, &px, w, h, image::ColorType::Rgba8)?;
        Ok(())
    }
}

/// sRGB-encoded premultiplied RGBA8 to straight alpha (premultiplication happened in linear).
pub fn unpremultiply(px: &mut [u8]) {
    let lin: Vec<f32> = (0..256).map(|i| srgb_to_linear(i as f32 / 255.0) as f32).collect();
    for p in px.chunks_mut(4) {
        let a = p[3] as f32 / 255.0;
        if a <= 0.0 {
            p[0] = 0;
            p[1] = 0;
            p[2] = 0;
        } else if a < 1.0 {
            for c in &mut p[..3] {
                let l = (lin[*c as usize] / a).min(1.0);
                let s = if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
                *c = (s * 255.0 + 0.5) as u8;
            }
        }
    }
}

fn srgb_to_linear(c: f32) -> f64 {
    let c = c as f64;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Create a renderer on a headless device (no window), for batch rendering and tests.
pub fn headless(width: u32, height: u32) -> Result<Renderer> {
    headless_with_samples(width, height, MSAA)
}

/// [`headless`] with a sample count (1: no multisampling).
pub fn headless_with_samples(width: u32, height: u32, samples: u32) -> Result<Renderer> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster_block(
        instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() }),
    )?;
    let info = adapter.get_info();
    let (device, queue) = pollster_block(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
    let mut r = Renderer::with_samples(device, queue, format!("{} ({:?})", info.name, info.backend), samples);
    r.resize(width, height);
    Ok(r)
}

fn pollster_block<F: std::future::Future>(f: F) -> F::Output {
    // Minimal executor: wgpu's native futures resolve without an external reactor.
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn noop(_: *const ()) {}
    fn clone(p: *const ()) -> RawWaker {
        RawWaker::new(p, &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut cx = Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

/// An RGBA8 (sRGB) image as a texture with mipmaps (made on the CPU), for surfaces.
fn upload_image(device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32, rgba: &[u8]) -> wgpu::TextureView {
    let levels = 32 - w.max(h).max(1).leading_zeros();
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("surface texture"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut img = image::RgbaImage::from_raw(w, h, rgba.to_vec()).expect("image size");
    for level in 0..levels {
        let (lw, lh) = (img.width(), img.height());
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: level, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            img.as_raw(),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * lw), rows_per_image: Some(lh) },
            wgpu::Extent3d { width: lw, height: lh, depth_or_array_layers: 1 },
        );
        if level + 1 < levels {
            img = image::imageops::resize(&img, (lw / 2).max(1), (lh / 2).max(1), image::imageops::FilterType::Triangle);
        }
    }
    tex.create_view(&Default::default())
}

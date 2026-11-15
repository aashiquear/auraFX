//! GPU compute kernels for filters, on any GPU wgpu reaches (Vulkan, Metal, DX12, GL).
//!
//! [`install`] creates a headless device and hands the kernels to `aurafx_filters::gpu`; the
//! filters there decide when to use them and finish every result in 64-bit on the CPU (see that
//! module). Big arrays are sent in chunks that fit the device's buffer limits.

use aurafx_filters::gpu::GpuKernels;
use std::sync::Mutex;

const CANDIDATES: &str = r#"
struct P { nx: u32, ny: u32, nzc: u32, nranges: u32, cells: u32, groups_x: u32, _a: u32, _b: u32 }
@group(0) @binding(0) var<storage, read> vals: array<f32>;
@group(0) @binding(1) var<storage, read> ranges: array<vec2<f32>>;
@group(0) @binding(2) var<uniform> p: P;
@group(0) @binding(3) var<storage, read_write> bits: array<atomic<u32>>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = id.x + id.y * p.groups_x * 256u;
    if (c >= p.cells) { return; }
    let cx = p.nx - 1u;
    let cy = p.ny - 1u;
    let i = c % cx;
    let j = (c / cx) % cy;
    let k = c / (cx * cy);
    var lo = 3.4e38;
    var hi = -3.4e38;
    for (var q = 0u; q < 8u; q++) {
        let ii = i + (q & 1u);
        let jj = j + ((q >> 1u) & 1u);
        let kk = k + (q >> 2u);
        let v = vals[ii + p.nx * (jj + p.ny * kk)];
        lo = min(lo, v);
        hi = max(hi, v);
    }
    // (Values that are not numbers never get here: the caller keeps such data on the CPU,
    // because shaders may assume there are none.)
    var hit = false;
    for (var r = 0u; r < p.nranges; r++) {
        if (lo <= ranges[r].y && hi >= ranges[r].x) { hit = true; }
    }
    if (hit) { atomicOr(&bits[c / 32u], 1u << (c % 32u)); }
}
"#;

const PLANE: &str = r#"
struct P { nx: u32, ny: u32, nzc: u32, cells: u32, groups_x: u32, k0: u32, _a: u32, _b: u32,
           origin: vec4<f32>, spacing: vec4<f32>, ext: vec4<f32>, plane: vec4<f32>, normal: vec4<f32> }
@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var<storage, read_write> bits: array<atomic<u32>>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = id.x + id.y * p.groups_x * 256u;
    if (c >= p.cells) { return; }
    let cx = p.nx - 1u;
    let cy = p.ny - 1u;
    let i = c % cx;
    let j = (c / cx) % cy;
    let k = c / (cx * cy) + p.k0;
    var lo = 3.4e38;
    var hi = -3.4e38;
    for (var q = 0u; q < 8u; q++) {
        let ijk = vec3<f32>(f32(i + (q & 1u)), f32(j + ((q >> 1u) & 1u)), f32(k + (q >> 2u)));
        let x = p.origin.xyz + (p.ext.xyz + ijk) * p.spacing.xyz;
        let v = dot(x - p.plane.xyz, p.normal.xyz);
        lo = min(lo, v);
        hi = max(hi, v);
    }
    let m = p.plane.w;
    if (lo <= m && hi >= -m) { atomicOr(&bits[c / 32u], 1u << (c % 32u)); }
}
"#;

pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    name: String,
    /// Largest storage buffer binding, in bytes.
    max_binding: u64,
    pipes: Mutex<Option<[wgpu::ComputePipeline; 2]>>,
}

/// Create the GPU and hand its kernels to the filters. Does nothing without a GPU, or with
/// `AURAFX_GPU_FILTERS=0`. Returns the device name.
pub fn install() -> Option<String> {
    if std::env::var("AURAFX_GPU_FILTERS").is_ok_and(|v| v == "0") {
        return None;
    }
    let g = Gpu::new().map_err(|e| tracing::info!("no GPU for filters: {e:#}")).ok()?;
    let name = g.name.clone();
    tracing::info!("filters use the GPU: {name}");
    aurafx_filters::gpu::install(Box::new(g));
    Some(name)
}

fn block<F: std::future::Future>(f: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn noop(_: *const ()) {}
    fn clone(p: *const ()) -> RawWaker {
        RawWaker::new(p, &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    // SAFETY: the waker does nothing and holds no data.
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

impl Gpu {
    pub fn new() -> anyhow::Result<Self> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
        desc.flags = wgpu::InstanceFlags::empty().with_env();
        let instance = wgpu::Instance::new(desc);
        let adapter =
            block(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() }))?;
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu {
            anyhow::bail!("only a software device ({})", info.name);
        }
        let al = adapter.limits();
        let limits = wgpu::Limits {
            max_storage_buffer_binding_size: al.max_storage_buffer_binding_size,
            max_buffer_size: al.max_buffer_size,
            ..wgpu::Limits::default()
        };
        let (device, queue) = block(adapter.request_device(&wgpu::DeviceDescriptor { required_limits: limits, ..Default::default() }))?;
        let max_binding = (al.max_storage_buffer_binding_size as u64).min(al.max_buffer_size).min(1 << 30);
        Ok(Self { device, queue, name: format!("{} ({:?})", info.name, info.backend), max_binding, pipes: Mutex::new(None) })
    }

    fn pipelines(&self) -> [wgpu::ComputePipeline; 2] {
        let mut p = self.pipes.lock().unwrap();
        p.get_or_insert_with(|| {
            let make = |src: &str, label: &str| {
                let m = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(src.into()) });
                self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(label),
                    layout: None,
                    module: &m,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                })
            };
            [make(CANDIDATES, "grid candidates"), make(PLANE, "plane candidates")]
        })
        .clone()
    }

    fn storage(&self, bytes: &[u8], label: &str) -> wgpu::Buffer {
        use wgpu::util::DeviceExt;
        self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            // (Never empty, and at least one element of the widest array bound.)
            contents: if bytes.len() < 8 { &[0u8; 8] } else { bytes },
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        })
    }

    fn uniform(&self, bytes: &[u8]) -> wgpu::Buffer {
        use wgpu::util::DeviceExt;
        self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("params"), contents: bytes, usage: wgpu::BufferUsages::UNIFORM })
    }

    /// Run `f` (work on the device) and give what the device reported while it ran (a rejected
    /// call, memory that ran out, a driver failure) as an error, so the caller falls back to
    /// the CPU. Without this such reports end the program.
    fn checked<T>(&self, f: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
        let memory = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let internal = self.device.push_error_scope(wgpu::ErrorFilter::Internal);
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let r = f();
        // The scopes end in the reverse order (all of them, whatever they report).
        let reports = [block(validation.pop()), block(internal.pop()), block(memory.pop())];
        match reports.into_iter().flatten().next() {
            Some(wgpu::Error::OutOfMemory { .. }) => anyhow::bail!("the GPU is out of memory"),
            Some(e) => anyhow::bail!("{e}"),
            None => r,
        }
    }

    /// Run `pipe` over `n` items with these buffers bound in order; read back `out`.
    fn run(&self, pipe: &wgpu::ComputePipeline, buffers: &[&wgpu::Buffer], n: u32, outs: &[&wgpu::Buffer]) -> anyhow::Result<Vec<Vec<u8>>> {
        let entries: Vec<wgpu::BindGroupEntry> =
            buffers.iter().enumerate().map(|(i, b)| wgpu::BindGroupEntry { binding: i as u32, resource: b.as_entire_binding() }).collect();
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &pipe.get_bind_group_layout(0), entries: &entries });
        let groups = n.div_ceil(256).max(1);
        let gx = groups.min(65535);
        let gy = groups.div_ceil(gx);
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
            pass.set_pipeline(pipe);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(gx, gy, 1);
        }
        let stagings: Vec<wgpu::Buffer> = outs
            .iter()
            .map(|o| {
                let s = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("readback"),
                    size: o.size(),
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                enc.copy_buffer_to_buffer(o, 0, &s, 0, o.size());
                s
            })
            .collect();
        self.queue.submit(Some(enc.finish()));
        let mut res = Vec::new();
        for s in &stagings {
            let slice = s.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            self.device.poll(wgpu::PollType::wait_indefinitely())?;
            rx.recv()??;
            res.push(slice.get_mapped_range()?.to_vec());
            s.unmap();
        }
        Ok(res)
    }

    /// Point planes per chunk along k so a chunk's values fit one binding.
    fn planes_per_chunk(&self, nx: usize, ny: usize, bytes_per_point: usize) -> usize {
        ((self.max_binding as usize / (nx * ny * bytes_per_point).max(1)).max(2)).min(1 << 20)
    }

    /// The candidate cells of chunks along k: `chunk(k0, k1)` gives the bit words of cells in
    /// planes [k0, k1) of cells.
    fn collect(cells_per_plane: usize, nzc: usize, step: usize, mut chunk: impl FnMut(usize, usize) -> anyhow::Result<Vec<u32>>) -> anyhow::Result<Vec<u32>> {
        let mut out = Vec::new();
        let mut k0 = 0;
        while k0 < nzc {
            let k1 = (k0 + step).min(nzc);
            let words = chunk(k0, k1)?;
            let base = k0 * cells_per_plane;
            let n = (k1 - k0) * cells_per_plane;
            for (w, &bits) in words.iter().enumerate() {
                let mut b = bits;
                while b != 0 {
                    let t = b.trailing_zeros() as usize;
                    let c = w * 32 + t;
                    if c < n {
                        out.push((base + c) as u32);
                    }
                    b &= b - 1;
                }
            }
            k0 = k1;
        }
        Ok(out)
    }

    fn try_grid_candidates(&self, values: &[f32], dims: [usize; 3], ranges: &[(f32, f32)]) -> anyhow::Result<Vec<u32>> {
        self.checked(|| self.grid_candidates_unchecked(values, dims, ranges))
    }

    fn grid_candidates_unchecked(&self, values: &[f32], dims: [usize; 3], ranges: &[(f32, f32)]) -> anyhow::Result<Vec<u32>> {
        let [nx, ny, nz] = dims;
        anyhow::ensure!(nx >= 2 && ny >= 2 && nz >= 2 && values.len() == nx * ny * nz, "not a 3-D grid");
        anyhow::ensure!((nx - 1) * (ny - 1) * (nz - 1) < u32::MAX as usize, "too many cells");
        anyhow::ensure!(!ranges.is_empty(), "no ranges");
        let [pipe, _] = self.pipelines();
        let ranges_b: Vec<[f32; 2]> = ranges.iter().map(|&(a, b)| [a, b]).collect();
        let rb = self.storage(bytemuck::cast_slice(&ranges_b), "ranges");
        let cpp = (nx - 1) * (ny - 1);
        let step = self.planes_per_chunk(nx, ny, 4) - 1;
        Self::collect(cpp, nz - 1, step, |k0, k1| {
            let vals = &values[k0 * nx * ny..(k1 + 1) * nx * ny];
            let vb = self.storage(bytemuck::cast_slice(vals), "values");
            let cells = (k1 - k0) * cpp;
            let groups = (cells as u32).div_ceil(256).max(1);
            let params: [u32; 8] = [nx as u32, ny as u32, (k1 - k0 + 1) as u32, ranges.len() as u32, cells as u32, groups.min(65535), 0, 0];
            let pb = self.uniform(bytemuck::cast_slice(&params));
            let ob = self.storage(&vec![0u8; cells.div_ceil(32) * 4], "bits");
            let r = self.run(&pipe, &[&vb, &rb, &pb, &ob], cells as u32, &[&ob])?;
            Ok(bytemuck::cast_slice::<u8, u32>(&r[0]).to_vec())
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn try_plane_candidates(
        &self,
        extent: [i64; 6],
        origin: [f64; 3],
        spacing: [f64; 3],
        plane: [f64; 3],
        normal: [f64; 3],
        margin: f64,
    ) -> anyhow::Result<Vec<u32>> {
        self.checked(|| self.plane_candidates_unchecked(extent, origin, spacing, plane, normal, margin))
    }

    #[allow(clippy::too_many_arguments)]
    fn plane_candidates_unchecked(
        &self,
        extent: [i64; 6],
        origin: [f64; 3],
        spacing: [f64; 3],
        plane: [f64; 3],
        normal: [f64; 3],
        margin: f64,
    ) -> anyhow::Result<Vec<u32>> {
        let dims: [usize; 3] = std::array::from_fn(|a| (extent[2 * a + 1] - extent[2 * a] + 1).max(0) as usize);
        let [nx, ny, nz] = dims;
        anyhow::ensure!(nx >= 2 && ny >= 2 && nz >= 2, "not a 3-D grid");
        anyhow::ensure!((nx - 1) * (ny - 1) * (nz - 1) < u32::MAX as usize, "too many cells");
        let [_, pipe] = self.pipelines();
        let cpp = (nx - 1) * (ny - 1);
        // Output bits only: chunks bounded by the binding size of the bit buffer.
        let step = ((self.max_binding as usize * 8) / cpp.max(1)).clamp(1, 1 << 16);
        Self::collect(cpp, nz - 1, step, |k0, k1| {
            let cells = (k1 - k0) * cpp;
            let groups = (cells as u32).div_ceil(256).max(1);
            let mut params = [0u8; 112];
            let head: [u32; 8] = [nx as u32, ny as u32, (k1 - k0 + 1) as u32, cells as u32, groups.min(65535), k0 as u32, 0, 0];
            params[..32].copy_from_slice(bytemuck::cast_slice(&head));
            let v4 = |v: [f64; 3], w: f64| [v[0] as f32, v[1] as f32, v[2] as f32, w as f32];
            let tail = [v4(origin, 0.0), v4(spacing, 0.0), v4([extent[0] as f64, extent[2] as f64, extent[4] as f64], 0.0), v4(plane, margin), v4(normal, 0.0)];
            params[32..112].copy_from_slice(bytemuck::cast_slice(&tail));
            let pb = self.uniform(&params);
            let ob = self.storage(&vec![0u8; cells.div_ceil(32) * 4], "bits");
            let r = self.run(&pipe, &[&pb, &ob], cells as u32, &[&ob])?;
            Ok(bytemuck::cast_slice::<u8, u32>(&r[0]).to_vec())
        })
    }
}

impl GpuKernels for Gpu {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn grid_candidates(&self, values: &[f32], dims: [usize; 3], ranges: &[(f32, f32)]) -> Option<Vec<u32>> {
        // Nothing to look for, or values that are not numbers (which a shader may not tell
        // from numbers): the CPU does it.
        if ranges.is_empty() || values.iter().any(|v| v.is_nan()) {
            return None;
        }
        self.try_grid_candidates(values, dims, ranges).map_err(|e| tracing::warn!("GPU contour cells: {e:#}; using the CPU")).ok()
    }
    fn image_plane_candidates(
        &self,
        extent: [i64; 6],
        origin: [f64; 3],
        spacing: [f64; 3],
        plane: [f64; 3],
        normal: [f64; 3],
        margin: f64,
    ) -> Option<Vec<u32>> {
        self.try_plane_candidates(extent, origin, spacing, plane, normal, margin).map_err(|e| tracing::warn!("GPU slice cells: {e:#}; using the CPU")).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the device rejects comes back as an error (the caller then uses the CPU), and the
    /// device still works afterwards. Skipped without a GPU.
    #[test]
    fn device_errors_are_returned() {
        let Ok(g) = Gpu::new() else {
            eprintln!("no GPU: skipped");
            return;
        };
        let r = g.checked(|| {
            // A buffer that may be mapped for reading cannot also be a storage buffer.
            let _ = g.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rejected"),
                size: 64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
            Ok(())
        });
        assert!(r.is_err(), "a rejected call is an error");
        // No ranges, or values that are not numbers: left to the CPU.
        let vals = vec![0.0f32; 27];
        assert!(g.grid_candidates(&vals, [3, 3, 3], &[]).is_none());
        let mut nan = vals.clone();
        nan[13] = f32::NAN;
        assert!(g.grid_candidates(&nan, [3, 3, 3], &[(-1.0, 1.0)]).is_none());
        assert_eq!(g.grid_candidates(&vals, [3, 3, 3], &[(-1.0, 1.0)]).map(|c| c.len()), Some(8), "the device still works");
        assert_eq!(g.grid_candidates(&vals, [3, 3, 3], &[(1.0, 2.0)]).map(|c| c.len()), Some(0));
    }
}

//! Ray tracing in a 3D view: a background thread path-traces the view's opaque surfaces and
//! sends progressively better pictures, which replace those surfaces once one is ready for the
//! current camera. While the camera moves the rasterizer draws as usual.

use aurafx_render::raytrace::{traced, Scene, View};
use aurafx_render::raytrace_gpu::AnyTracer;
use aurafx_render::{Camera, ImageLayer, MeshData, NodeDraw, RenderSettings};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Start tracing once the view has been still this long.
const SETTLE: Duration = Duration::from_millis(120);

enum Msg {
    Scene(u64, Arc<Scene>),
    Image(u64, ImageLayer, u32),
}

#[derive(Default)]
pub struct RayTracing {
    /// Last built scene: (key, scene).
    scene: Option<(u64, Arc<Scene>)>,
    /// What is being traced: (view key, stop flag, messages).
    job: Option<(u64, Arc<AtomicBool>, Receiver<Msg>)>,
    /// The view key seen last frame and since when.
    seen: Option<(u64, Instant)>,
    /// Latest picture: (view key, picture, passes).
    latest: Option<(u64, Arc<ImageLayer>, u32)>,
    /// Draws without their triangles (edges, points), by mesh pointer.
    stripped: HashMap<usize, Arc<MeshData>>,
    /// The renderer's environment map (set each frame).
    pub environment: Option<Arc<aurafx_render::env::Environment>>,
    /// The renderer's GPU, for tracing there (set each frame).
    pub gpu: Option<(wgpu::Device, wgpu::Queue)>,
    /// Whether the last tracer ran on the GPU (for the status line).
    pub on_gpu: Arc<AtomicBool>,
    /// A picture traced for a screenshot: (view key, result, picture once done).
    still: Option<(u64, Receiver<ImageLayer>, Option<Arc<ImageLayer>>)>,
}

/// What the view should draw this frame.
pub struct Frame {
    /// The draws for the rasterizer (traced triangles removed when a picture is shown).
    pub draws: Vec<NodeDraw>,
    /// The traced picture to composite, if one is ready for this view.
    pub layer: Option<Arc<ImageLayer>>,
    /// Passes done and wanted, while tracing.
    pub progress: Option<(u32, u32)>,
}

fn scene_key(draws: &[&NodeDraw]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for d in draws {
        (d.id, Arc::as_ptr(&d.mesh) as usize, format!("{:?}", d.display)).hash(&mut h);
    }
    h.finish()
}

fn view_key(scene: u64, camera: &Camera, settings: &RenderSettings, w: u32, h: u32, env: Option<&str>) -> u64 {
    let mut hs = std::collections::hash_map::DefaultHasher::new();
    (scene, env).hash(&mut hs);
    for v in camera.eye.to_array().iter().chain(&camera.target.to_array()).chain(&camera.up.to_array()).chain(&[
        camera.fov_y_deg,
        camera.ortho_height,
        camera.near,
        camera.far,
    ]) {
        v.to_bits().hash(&mut hs);
    }
    (camera.orthographic, w, h, settings.transparent, format!("{:?}{:?}", settings.lighting, settings.background)).hash(&mut hs);
    hs.finish()
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

impl RayTracing {
    /// Stop tracing and forget pictures (ray tracing switched off).
    pub fn stop(&mut self) {
        if let Some((_, stop, _)) = self.job.take() {
            stop.store(true, Ordering::Relaxed);
        }
        self.latest = None;
        self.seen = None;
    }

    pub fn frame(&mut self, ctx: &egui::Context, draws: Vec<NodeDraw>, camera: &Camera, settings: &RenderSettings, w: u32, h: u32) -> Frame {
        let traced_draws: Vec<&NodeDraw> = draws.iter().filter(|d| traced(d)).collect();
        if traced_draws.is_empty() {
            self.stop();
            return Frame { draws, layer: None, progress: None };
        }
        let skey = scene_key(&traced_draws);
        let vkey = view_key(skey, camera, settings, w, h, self.environment.as_ref().map(|e| e.name.as_str()));
        let wanted = settings.lighting.ray_samples.max(1);
        // Messages from the tracer.
        if let Some((key, _, rx)) = &self.job {
            while let Ok(m) = rx.try_recv() {
                match m {
                    Msg::Scene(k, s) => self.scene = Some((k, s)),
                    Msg::Image(k, img, passes) if k == *key => self.latest = Some((k, Arc::new(img), passes)),
                    Msg::Image(..) => {}
                }
            }
        }
        // (Re)start once the view has settled.
        let now = Instant::now();
        let settled = match self.seen {
            Some((k, since)) if k == vkey => now - since >= SETTLE,
            _ => {
                self.seen = Some((vkey, now));
                false
            }
        };
        if self.job.as_ref().map(|j| j.0) != Some(vkey) {
            if let Some((_, stop, _)) = self.job.take() {
                stop.store(true, Ordering::Relaxed);
            }
            if settled {
                self.start(ctx, &traced_draws, skey, vkey, camera, settings, w, h);
            } else {
                ctx.request_repaint_after(SETTLE);
            }
        }
        let current = self.latest.as_ref().filter(|l| l.0 == vkey);
        let progress = match current {
            Some((_, _, p)) if *p >= wanted => None,
            Some((_, _, p)) => Some((*p, wanted)),
            None => Some((0, wanted)),
        };
        if progress.is_some() {
            ctx.request_repaint_after(Duration::from_millis(60));
        }
        let Some((_, layer, _)) = current else {
            return Frame { draws, layer: None, progress };
        };
        let layer = layer.clone();
        let draws = self.strip(draws);
        Frame { draws, layer: Some(layer), progress }
    }

    /// The draws with the traced triangles removed (edges and points stay).
    pub fn strip(&mut self, draws: Vec<NodeDraw>) -> Vec<NodeDraw> {
        let mut out = Vec::with_capacity(draws.len());
        let mut live = Vec::new();
        for d in draws {
            if !traced(&d) {
                out.push(d);
                continue;
            }
            if d.mesh.lines.is_empty() && d.mesh.points.is_empty() {
                continue;
            }
            let ptr = Arc::as_ptr(&d.mesh) as usize;
            live.push(ptr);
            let mesh = self
                .stripped
                .entry(ptr)
                .or_insert_with(|| {
                    let m = &d.mesh;
                    Arc::new(MeshData {
                        tris: Vec::new(),
                        lines: m.lines.clone(),
                        points: m.points.clone(),
                        scalar_range: m.scalar_range,
                        has_scalars: m.has_scalars,
                        bounds: m.bounds,
                        origin: m.origin,
                        volume: None,
                        lic: Vec::new(),
                    })
                })
                .clone();
            out.push(NodeDraw { mesh, ..d });
        }
        self.stripped.retain(|p, _| live.contains(p));
        out
    }

    /// A finished picture at another size (for screenshots): None while it is being traced.
    pub fn still(&mut self, draws: &[NodeDraw], camera: &Camera, settings: &RenderSettings, w: u32, h: u32) -> Option<Arc<ImageLayer>> {
        let traced_draws: Vec<&NodeDraw> = draws.iter().filter(|d| traced(d)).collect();
        if traced_draws.is_empty() {
            return None;
        }
        let skey = scene_key(&traced_draws);
        let vkey = view_key(skey, camera, settings, w, h, self.environment.as_ref().map(|e| e.name.as_str()));
        if let Some((k, rx, done)) = &mut self.still {
            if *k == vkey {
                if done.is_none() {
                    if let Ok(img) = rx.try_recv() {
                        *done = Some(Arc::new(img));
                    }
                }
                return done.clone();
            }
        }
        let (tx, rx) = channel();
        let scene = self.scene.as_ref().filter(|s| s.0 == skey).map(|s| s.1.clone());
        let owned: Vec<NodeDraw> = if scene.is_none() { traced_draws.iter().map(|d| (*d).clone()).collect() } else { Vec::new() };
        let view = self.view(camera, settings, w, h);
        let wanted = settings.lighting.ray_samples.max(1);
        let gpu = self.gpu.clone().filter(|_| settings.lighting.ray_device != aurafx_pipeline::RayDevice::Cpu);
        std::thread::Builder::new()
            .name("ray traced screenshot".into())
            .spawn(move || {
                let scene = scene.unwrap_or_else(|| Arc::new(Scene::build(&owned)));
                let mut tracer = AnyTracer::new(gpu.as_ref().map(|(d, q)| (d, q)), scene, view);
                while tracer.passes() < wanted {
                    tracer.pass();
                    if tracer.passes() % 4 == 0 {
                        tracer.finish();
                    }
                }
                match tracer.image() {
                    Ok(img) => {
                        let _ = tx.send(img);
                    }
                    Err(e) => tracing::warn!("ray traced screenshot: {e:#}"),
                }
            })
            .ok();
        self.still = Some((vkey, rx, None));
        None
    }

    fn view(&self, camera: &Camera, settings: &RenderSettings, w: u32, h: u32) -> View {
        let bg = settings.background;
        View {
            camera: camera.clone(),
            lighting: settings.lighting.clone(),
            width: w,
            height: h,
            background: if settings.transparent { [0.0; 3] } else { [srgb_to_linear(bg[0]), srgb_to_linear(bg[1]), srgb_to_linear(bg[2])] },
            bounces: settings.lighting.ray_bounces,
            environment: self.environment.clone(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn start(&mut self, ctx: &egui::Context, draws: &[&NodeDraw], skey: u64, vkey: u64, camera: &Camera, settings: &RenderSettings, w: u32, h: u32) {
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let scene = self.scene.as_ref().filter(|s| s.0 == skey).map(|s| s.1.clone());
        let owned: Vec<NodeDraw> = if scene.is_none() { draws.iter().map(|d| (*d).clone()).collect() } else { Vec::new() };
        let view = self.view(camera, settings, w, h);
        let wanted = settings.lighting.ray_samples.max(1);
        let flag = stop.clone();
        let ctx = ctx.clone();
        let gpu = self.gpu.clone().filter(|_| settings.lighting.ray_device != aurafx_pipeline::RayDevice::Cpu);
        let on_gpu = self.on_gpu.clone();
        std::thread::Builder::new()
            .name("ray tracing".into())
            .spawn(move || {
                let scene = match scene {
                    Some(s) => s,
                    None => {
                        let s = Arc::new(Scene::build(&owned));
                        let _ = tx.send(Msg::Scene(skey, s.clone()));
                        s
                    }
                };
                let mut tracer = AnyTracer::new(gpu.as_ref().map(|(d, q)| (d, q)), scene, view);
                on_gpu.store(tracer.on_gpu(), Ordering::Relaxed);
                let mut sent = Instant::now();
                while tracer.passes() < wanted && !flag.load(Ordering::Relaxed) {
                    tracer.pass();
                    let p = tracer.passes();
                    // Early passes right away, then a few pictures a second, and the last one.
                    if p.is_power_of_two() || p == wanted || sent.elapsed() > Duration::from_millis(250) {
                        sent = Instant::now();
                        let img = match tracer.image() {
                            Ok(img) => img,
                            Err(e) => {
                                tracing::warn!("ray tracing: {e:#}");
                                return;
                            }
                        };
                        if tx.send(Msg::Image(vkey, img, p)).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                    } else if p % 4 == 0 {
                        // Don't queue far ahead of the GPU (stopping stays quick).
                        tracer.finish();
                    }
                }
            })
            .ok();
        self.job = Some((vkey, stop, rx));
    }
}

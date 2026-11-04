//! The 3D view: renders through `aurafx-render`, shows the result as an image, handles
//! camera interaction and paints overlays (scalar bar, orientation axes).

use crate::app::State;
use aurafx_filters::Association;
use aurafx_pipeline::session::CameraRequest;
use aurafx_pipeline::{colormap, DisplayProps, NodeId};
use aurafx_render::{Camera, MeshData, NodeDraw, RenderSettings, Renderer};
use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

struct CachedMesh {
    data_ptr: usize,
    key: String,
    mesh: Arc<MeshData>,
}

pub struct Viewport {
    render_state: Option<egui_wgpu::RenderState>,
    renderer: Option<Renderer>,
    pub camera: Camera,
    pub settings: RenderSettings,
    texture: Option<egui::TextureId>,
    meshes: HashMap<NodeId, CachedMesh>,
    start: Instant,
    frame_times: Vec<f64>,
    last_frame: f64,
    screenshot: Option<(PathBuf, Option<(u32, u32)>)>,
    camera_initialized: bool,
}

fn display_key(d: &DisplayProps) -> String {
    format!("{:?}|{:?}|{:?}|{}", d.representation, d.color_by, d.range, d.lighting)
}

impl Viewport {
    pub fn new(cc: &eframe::CreationContext<'_>, background: [f32; 3]) -> Self {
        let render_state = cc.wgpu_render_state.clone();
        let renderer = render_state.as_ref().map(|rs| {
            let info = rs.adapter.get_info();
            Renderer::new(rs.device.clone(), rs.queue.clone(), format!("{} ({:?})", info.name, info.backend))
        });
        Self {
            render_state,
            renderer,
            camera: Camera::default(),
            settings: RenderSettings { background, background_top: None },
            texture: None,
            meshes: HashMap::new(),
            start: Instant::now(),
            frame_times: Vec::new(),
            last_frame: 0.0,
            screenshot: None,
            camera_initialized: false,
        }
    }

    pub fn time_seconds(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    pub fn adapter_name(&self) -> String {
        self.renderer.as_ref().map(|r| r.adapter_info.clone()).unwrap_or_else(|| "no GPU".into())
    }

    pub fn fps(&self) -> f64 {
        if self.frame_times.len() < 2 {
            return 0.0;
        }
        let span = self.frame_times.last().unwrap() - self.frame_times.first().unwrap();
        if span <= 0.0 { 0.0 } else { (self.frame_times.len() - 1) as f64 / span }
    }

    pub fn frame_begin(&mut self, _ctx: &egui::Context, _frame: &eframe::Frame) {
        let t = self.time_seconds();
        self.last_frame = t;
        self.frame_times.push(t);
        if self.frame_times.len() > 60 {
            self.frame_times.remove(0);
        }
    }

    pub fn frame_end(&mut self, _ctx: &egui::Context) {}

    pub fn request_screenshot(&mut self, path: PathBuf, size: Option<(u32, u32)>) {
        self.screenshot = Some((path, size));
    }

    fn sync_texture(&mut self, recreated: bool) {
        let (Some(rs), Some(r)) = (&self.render_state, &self.renderer) else { return };
        let mut er = rs.renderer.write();
        match self.texture {
            Some(id) if recreated => er.update_egui_texture_from_wgpu_texture(&rs.device, r.color_view(), wgpu::FilterMode::Linear, id),
            Some(_) => {}
            None => self.texture = Some(er.register_native_texture(&rs.device, r.color_view(), wgpu::FilterMode::Linear)),
        }
    }
}

/// Build the draw list from the session (tessellating what changed).
fn collect_draws(state: &mut State) -> (Vec<NodeDraw>, Option<(String, String, (f64, f64), bool)>) {
    let s = state.session.lock();
    let mut draws = Vec::new();
    let mut scalar_bar = None;
    let live: Vec<NodeId> = s.nodes.keys().copied().collect();
    state.viewport.meshes.retain(|id, _| live.contains(id));
    for n in s.nodes.values() {
        if !n.visible {
            continue;
        }
        let Some(out) = &n.output else { continue };
        let ptr = Arc::as_ptr(out) as *const () as usize;
        let key = display_key(&n.display);
        let cached = state.viewport.meshes.get(&n.id).filter(|c| c.data_ptr == ptr && c.key == key);
        let mesh = match cached {
            Some(c) => c.mesh.clone(),
            None => {
                let t0 = Instant::now();
                let m = Arc::new(aurafx_render::tessellate::tessellate(out, &n.display));
                tracing::debug!("tessellated {} in {:.1} ms ({} tris)", n.name, t0.elapsed().as_secs_f64() * 1e3, m.tris.len() / 3);
                state.viewport.meshes.insert(n.id, CachedMesh { data_ptr: ptr, key, mesh: m.clone() });
                m
            }
        };
        if n.display.show_scalar_bar && mesh.has_scalars && (scalar_bar.is_none() || s.active == Some(n.id)) {
            if let (Some(cb), Some(r)) = (&n.display.color_by, mesh.scalar_range) {
                let label = format!("{}{}", cb.name, match cb.component { Some(c) => format!(" [{c}]"), None => String::new() });
                scalar_bar = Some((label, n.display.colormap.clone(), r, n.display.log_scale));
            }
        }
        draws.push(NodeDraw { id: n.id, mesh, display: n.display.clone() });
    }
    (draws, scalar_bar)
}

pub fn viewport_ui(state: &mut State, ui: &mut egui::Ui) {
    let avail = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
    let ppp = ui.ctx().pixels_per_point();
    let w = (rect.width() * ppp).round().max(1.0) as u32;
    let h = (rect.height() * ppp).round().max(1.0) as u32;

    // Camera requests from the session (Python, menus) and first-data auto fit.
    let (req, bounds, empty) = {
        let mut s = state.session.lock();
        // Wait for running/dirty nodes so the camera fits the final data.
        let req = if s.is_busy() { None } else { s.camera_request.take() };
        (req, s.visible_bounds(), s.nodes.is_empty())
    };
    if !state.viewport.camera_initialized && !bounds.is_empty() && req.is_none() {
        // Flat data is viewed face-on; 3-D data isometrically.
        let s = bounds.size();
        let d = bounds.diagonal().max(1e-30);
        let (dir, up) = if s[2] < 1e-6 * d {
            ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0])
        } else if s[1] < 1e-6 * d {
            ([0.0, -1.0, 0.0], [0.0, 0.0, 1.0])
        } else if s[0] < 1e-6 * d {
            ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0])
        } else {
            ([1.0, 1.0, 1.0], [0.0, 0.0, 1.0])
        };
        state.viewport.camera.set_view(dir, up, &bounds);
        state.viewport.camera_initialized = true;
    }
    match req {
        Some(CameraRequest::ResetToBounds) => state.viewport.camera.reset_to_bounds(&bounds),
        Some(CameraRequest::View { direction, up }) => {
            state.viewport.camera.set_view(direction, up, &bounds);
            state.viewport.camera_initialized = true;
        }
        None => {}
    }

    // Interaction.
    let cam = &mut state.viewport.camera;
    let drag = response.drag_delta();
    let size = rect.size().max(Vec2::splat(1.0));
    let mods = ui.input(|i| i.modifiers);
    if response.dragged_by(egui::PointerButton::Primary) && !mods.shift && !mods.command {
        cam.rotate((drag.x / size.x) as f64 * 1.5, (drag.y / size.y) as f64 * 1.5);
    } else if response.dragged_by(egui::PointerButton::Middle) || (response.dragged_by(egui::PointerButton::Primary) && mods.shift) {
        cam.pan((drag.x / size.y) as f64, (drag.y / size.y) as f64);
    } else if response.dragged_by(egui::PointerButton::Secondary) || (response.dragged_by(egui::PointerButton::Primary) && mods.command) {
        cam.zoom((1.0 - (drag.y / size.y) as f64 * 2.0).max(0.05));
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.0 {
            cam.zoom((1.0 + scroll as f64 * 0.002).clamp(0.5, 2.0));
        }
    }
    if response.double_clicked() {
        cam.reset_to_bounds(&bounds);
    }
    if !bounds.is_empty() {
        cam.update_clip(&bounds);
    }

    // Render.
    let (draws, scalar_bar) = collect_draws(state);
    if let Some(r) = state.viewport.renderer.as_mut() {
        let recreated = r.resize(w, h);
        let settings = state.viewport.settings.clone();
        r.render(&state.viewport.camera, &draws, &settings);
        state.viewport.sync_texture(recreated);
        // Screenshot requests (UI or Python).
        let py_req = state.session.lock().screenshot_request.take();
        let req = state.viewport.screenshot.take().or_else(|| py_req.map(|(p, w, h)| (p, w.zip(h))));
        if let Some((path, _size)) = req {
            match state.viewport.renderer.as_ref().unwrap().save_png(&path) {
                Ok(()) => state.toast(format!("Saved {}", path.display()), false),
                Err(e) => state.toast(format!("Screenshot failed: {e:#}"), true),
            }
        }
    }
    if let Some(tex) = state.viewport.texture {
        ui.painter().image(tex, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }

    // Overlays.
    let painter = ui.painter_at(rect);
    if empty {
        let c = rect.center();
        painter.text(c + Vec2::new(0.0, -30.0), egui::Align2::CENTER_CENTER, "auraFX", egui::FontId::proportional(34.0), Color32::from_gray(140));
        painter.text(c + Vec2::new(0.0, 6.0), egui::Align2::CENTER_CENTER, "Drop a file here, press Ctrl+O to open one, or Ctrl+K to search for a source or filter.", egui::FontId::proportional(15.0), Color32::from_gray(120));
        painter.text(c + Vec2::new(0.0, 30.0), egui::Align2::CENTER_CENTER, "Supported: VTK (.vtk .vtu .vtp .vti .vtr .vts .vtm .pvd), LAMMPS dumps, STL, CSV", egui::FontId::proportional(12.0), Color32::from_gray(100));
    }
    if let Some((label, cmap, (lo, hi), log)) = scalar_bar {
        draw_scalar_bar(&painter, rect, &label, &cmap, lo, hi, log);
    }
    if state.settings.show_orientation_axes {
        draw_axes(&painter, rect, &state.viewport.camera);
    }
    // Keep animating while dragging or playing.
    if response.dragged() {
        ui.ctx().request_repaint();
    }
}

fn draw_scalar_bar(p: &egui::Painter, rect: Rect, label: &str, cmap: &str, lo: f64, hi: f64, log: bool) {
    let map = colormap::get(cmap);
    let bar_w = 18.0;
    let bar_h = (rect.height() * 0.45).clamp(80.0, 320.0);
    let x = rect.right() - 70.0;
    let y0 = rect.center().y - bar_h / 2.0;
    let n = 64;
    for i in 0..n {
        let t0 = i as f32 / n as f32;
        let t1 = (i + 1) as f32 / n as f32;
        let c = map.sample(1.0 - (t0 + t1) * 0.5);
        let col = Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8);
        p.rect_filled(Rect::from_min_max(Pos2::new(x, y0 + t0 * bar_h), Pos2::new(x + bar_w, y0 + t1 * bar_h + 0.5)), 0.0, col);
    }
    p.rect_stroke(Rect::from_min_size(Pos2::new(x, y0), Vec2::new(bar_w, bar_h)), 0.0, Stroke::new(1.0, Color32::from_gray(200)), egui::StrokeKind::Outside);
    let font = egui::FontId::proportional(12.0);
    let txt = Color32::from_gray(230);
    let ticks = 5;
    for k in 0..ticks {
        let f = k as f64 / (ticks - 1) as f64;
        let v = if log && lo > 0.0 { (lo.ln() + f * (hi.ln() - lo.ln())).exp() } else { lo + f * (hi - lo) };
        let y = y0 + bar_h * (1.0 - f as f32);
        p.line_segment([Pos2::new(x + bar_w, y), Pos2::new(x + bar_w + 4.0, y)], Stroke::new(1.0, txt));
        p.text(Pos2::new(x + bar_w + 6.0, y), egui::Align2::LEFT_CENTER, aurafx_data::info::g(v), font.clone(), txt);
    }
    p.text(Pos2::new(x + bar_w / 2.0, y0 - 8.0), egui::Align2::CENTER_BOTTOM, label, egui::FontId::proportional(13.0), txt);
}

fn draw_axes(p: &egui::Painter, rect: Rect, cam: &Camera) {
    let view = cam.view_matrix();
    let origin = Pos2::new(rect.left() + 48.0, rect.bottom() - 48.0);
    let len = 32.0;
    let mut axes: Vec<(f64, Pos2, &str, Color32)> = Vec::new();
    for (i, (name, col)) in [("X", Color32::from_rgb(240, 90, 90)), ("Y", Color32::from_rgb(120, 220, 120)), ("Z", Color32::from_rgb(110, 150, 255))].into_iter().enumerate() {
        let mut v = glam::DVec4::ZERO;
        v[i] = 1.0;
        let r = view * v; // rotation only (w = 0)
        let end = origin + Vec2::new(r.x as f32 * len, -r.y as f32 * len);
        axes.push((r.z, end, name, col));
    }
    axes.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for (_, end, name, col) in axes {
        p.line_segment([origin, end], Stroke::new(2.0, col));
        p.text(end + (end - origin).normalized() * 8.0, egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(12.0), col);
    }
}

/// Colors for the pipeline tree dots etc.
pub fn assoc_glyph(a: Association) -> &'static str {
    match a {
        Association::Points => crate::icons::CIRCLE,
        Association::Cells => crate::icons::SQUARE,
    }
}

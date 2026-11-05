//! The 3D view: renders through `aurafx-render`, shows the result as an image, handles
//! camera interaction and paints overlays (scalar bar, orientation axes).

use crate::app::State;
use aurafx_filters::Association;
use aurafx_pipeline::session::{CameraRequest, ScreenshotRequest};
use aurafx_pipeline::{colormap, DisplayProps, NodeId};
use aurafx_render::{Camera, MeshData, NodeDraw, RenderSettings, Renderer};
use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

struct CachedMesh {
    data_ptr: usize,
    key: String,
    mesh: Arc<MeshData>,
}

static APP_START: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);

pub struct Viewport {
    /// 0 for the main 3D view; extra views get increasing ids.
    pub id: u32,
    /// Share the camera with other linked views.
    pub linked: bool,
    render_state: Option<egui_wgpu::RenderState>,
    renderer: Option<Renderer>,
    pub camera: Camera,
    pub settings: RenderSettings,
    texture: Option<egui::TextureId>,
    meshes: HashMap<NodeId, CachedMesh>,
    /// Axes grid geometry per object: (bounds + far-face key, mesh).
    grid_meshes: HashMap<NodeId, ([u64; 7], Arc<MeshData>)>,
    /// Bounds of the objects with an axes grid this frame (for the labels).
    grid_boxes: Vec<aurafx_data::Bounds>,
    start: Instant,
    frame_times: Vec<f64>,
    last_frame: f64,
    screenshot: Option<ScreenshotRequest>,
    /// Frames a scripted screenshot still waits.
    screenshot_wait: u8,
    camera_initialized: bool,
    pub mode: InteractMode,
    drag_start: Option<Pos2>,
    pub pick: Option<PickResult>,
    selection_mesh: Option<(u64, Arc<MeshData>)>,
    widget_drag: Option<WidgetDrag>,
    /// Rectangle cell selection waiting for the id pass: (node, pixel rect, shift, ctrl).
    pending_id_pick: Option<(NodeId, [u32; 4], bool, bool)>,
    /// Select every cell inside the rectangle, including hidden ones.
    pub select_through: bool,
    /// Ruler end points (0, 1 or 2 picked so far).
    pub ruler: Vec<glam::DVec3>,
    /// Pictures of server-drawn objects, per host.
    remote_images: HashMap<String, RemoteImage>,
    /// Path tracing of the opaque surfaces (View › Lighting › Ray tracing).
    ray_tracing: crate::ray_tracing::RayTracing,
}

/// The latest picture a server drew for this view, and the request in flight.
#[derive(Default)]
struct RemoteImage {
    /// What the picture shows (camera, size, objects, time).
    key: u64,
    layer: Option<Arc<aurafx_render::ImageLayer>>,
    pending: Option<(u64, std::sync::mpsc::Receiver<anyhow::Result<aurafx_render::ImageLayer>>)>,
    error: Option<String>,
}

/// What a drag in the view moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WidgetDrag {
    /// Handle of the active filter's widget.
    Filter(usize),
    /// Handle of a custom light.
    Light(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractMode {
    Navigate,
    Pick,
    /// Click two surface points to measure the distance between them.
    Ruler,
    SelectCells,
    SelectPoints,
}

pub struct PickResult {
    pub node_name: String,
    pub world: glam::DVec3,
    pub values: Vec<(String, String)>,
}

pub const SELECTION_NODE_ID: NodeId = u64::MAX;
/// Axes grid of node `n` is drawn with id `GRID_NODE_ID_BASE | n`.
const GRID_NODE_ID_BASE: NodeId = 1 << 62;

fn display_key(d: &DisplayProps) -> String {
    format!("{:?}|{:?}|{:?}|{}|{}", d.representation, d.color_by, d.range, d.lighting, d.nonlinear_subdivision)
}

impl Viewport {
    pub fn new(cc: &eframe::CreationContext<'_>, background: [f32; 3]) -> Self {
        Self::with_render_state(cc.wgpu_render_state.clone(), background, 0)
    }

    pub fn with_render_state(render_state: Option<egui_wgpu::RenderState>, background: [f32; 3], id: u32) -> Self {
        let renderer = render_state.as_ref().map(|rs| {
            let info = rs.adapter.get_info();
            Renderer::new(rs.device.clone(), rs.queue.clone(), format!("{} ({:?})", info.name, info.backend))
        });
        Self {
            id,
            linked: false,
            render_state,
            renderer,
            camera: Camera::default(),
            settings: RenderSettings { background, ..Default::default() },
            texture: None,
            meshes: HashMap::new(),
            grid_meshes: HashMap::new(),
            grid_boxes: Vec::new(),
            start: Instant::now(),
            frame_times: Vec::new(),
            last_frame: 0.0,
            screenshot: None,
            screenshot_wait: 0,
            camera_initialized: false,
            mode: InteractMode::Navigate,
            drag_start: None,
            pick: None,
            selection_mesh: None,
            widget_drag: None,
            pending_id_pick: None,
            select_through: false,
            ruler: Vec::new(),
            remote_images: HashMap::new(),
            ray_tracing: Default::default(),
        }
    }

    pub fn time_seconds(&self) -> f64 {
        let _ = self.start;
        APP_START.elapsed().as_secs_f64()
    }

    pub fn render_state(&self) -> Option<egui_wgpu::RenderState> {
        self.render_state.clone()
    }

    pub fn adapter_name(&self) -> String {
        self.renderer.as_ref().map(|r| r.adapter_info.clone()).unwrap_or_else(|| "no GPU".into())
    }

    pub fn fps(&self) -> f64 {
        if self.frame_times.len() < 2 {
            return 0.0;
        }
        let span = self.frame_times.last().unwrap() - self.frame_times.first().unwrap();
        if span <= 0.0 {
            0.0
        } else {
            (self.frame_times.len() - 1) as f64 / span
        }
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

    /// Save what was rendered last frame.
    pub fn save_current_png(&self, path: &std::path::Path) -> anyhow::Result<()> {
        self.renderer.as_ref().ok_or_else(|| anyhow::anyhow!("no renderer"))?.save_png(path)
    }

    pub fn camera_initialized(&self) -> bool {
        self.camera_initialized
    }

    pub fn mark_camera_initialized(&mut self) {
        self.camera_initialized = true;
    }

    /// The view's size in pixels.
    pub fn size(&self) -> (u32, u32) {
        self.renderer.as_ref().map(|r| (r.width, r.height)).unwrap_or((1, 1))
    }

    pub fn request_screenshot(&mut self, req: ScreenshotRequest) {
        self.screenshot = Some(req);
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
fn collect_draws(state: &mut State) -> (Vec<NodeDraw>, Option<(String, colormap::ColorMap, (f64, f64), bool)>) {
    let s = state.session.lock();
    let mut draws = Vec::new();
    let mut scalar_bar = None;
    state.viewport.grid_boxes.clear();
    let live: Vec<NodeId> = s.nodes.keys().copied().collect();
    state.viewport.meshes.retain(|id, _| live.contains(id));
    for n in s.nodes.values() {
        if !n.visible {
            continue;
        }
        // Drawn by a server: no local geometry; the scalar bar comes from its summary.
        if n.remote.as_ref().is_some_and(|r| r.payload == aurafx_pipeline::remote::Payload::Drawn) {
            if let (true, Some(cb), Some(info)) = (n.display.show_scalar_bar, &n.display.color_by, &n.info) {
                let arrays = if cb.assoc == Association::Points { &info.point_arrays } else { &info.cell_arrays };
                if let Some(a) = arrays.iter().find(|a| a.name == cb.name) {
                    if scalar_bar.is_none() || s.active == Some(n.id) {
                        scalar_bar = Some((cb.name.clone(), n.display.effective_colormap(), n.display.range.unwrap_or(a.range), n.display.log_scale));
                    }
                }
            }
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
                let label = format!(
                    "{}{}",
                    cb.name,
                    match cb.component {
                        Some(c) => format!(" [{c}]"),
                        None => String::new(),
                    }
                );
                scalar_bar = Some((label, n.display.effective_colormap(), r, n.display.log_scale));
            }
        }
        if n.display.axes_grid && !mesh.bounds.is_empty() {
            let b = mesh.bounds;
            let back = crate::axes_grid::back_sides(&state.viewport.camera, &b);
            let mut key = [0u64; 7];
            for a in 0..3 {
                key[a] = b.min[a].to_bits();
                key[a + 3] = b.max[a].to_bits();
            }
            key[6] = back.iter().enumerate().map(|(i, &f)| (f as u64) << i).sum();
            let grid = match state.viewport.grid_meshes.get(&n.id).filter(|(k, _)| *k == key) {
                Some((_, m)) => m.clone(),
                None => {
                    let m = Arc::new(crate::axes_grid::build_mesh(&b, back));
                    state.viewport.grid_meshes.insert(n.id, (key, m.clone()));
                    m
                }
            };
            let bg = state.viewport.settings.background;
            let lum = 0.2126 * bg[0] + 0.7152 * bg[1] + 0.0722 * bg[2];
            let mut d = DisplayProps::default();
            d.solid_color = if lum > 0.5 { [0.55, 0.55, 0.58] } else { [0.42, 0.43, 0.47] };
            d.lighting = false;
            d.show_scalar_bar = false;
            draws.push(NodeDraw { id: GRID_NODE_ID_BASE | n.id, mesh: grid, display: d, overlay: false });
            state.viewport.grid_boxes.push(b);
        }
        draws.push(NodeDraw { id: n.id, mesh, display: n.display.clone(), overlay: false });
    }
    state.viewport.grid_meshes.retain(|id, _| live.contains(id));
    // Selection highlight.
    if let Some(sel) = s.selection.clone() {
        let cached = state.viewport.selection_mesh.as_ref().filter(|(r, _)| *r == sel.revision).map(|(_, m)| m.clone());
        let mesh = cached.or_else(|| {
            let src = s.nodes.get(&sel.node)?.output.clone()?;
            let ex = aurafx_filters::selection::extract_ids(&src, sel.assoc, &sel.ids).ok()?;
            let mut d = DisplayProps::default();
            d.representation =
                if sel.assoc == Association::Points { aurafx_pipeline::Representation::Points } else { aurafx_pipeline::Representation::SurfaceWithEdges };
            d.color_by = None;
            let m = Arc::new(aurafx_render::tessellate::tessellate(&ex, &d));
            state.viewport.selection_mesh = Some((sel.revision, m.clone()));
            Some(m)
        });
        if let Some(mesh) = mesh {
            let mut d = DisplayProps::default();
            d.solid_color = [1.0, 0.1, 0.85];
            d.opacity = 0.35;
            d.point_size = 9.0;
            d.lighting = true;
            d.representation =
                if sel.assoc == Association::Points { aurafx_pipeline::Representation::Points } else { aurafx_pipeline::Representation::SurfaceWithEdges };
            draws.push(NodeDraw { id: SELECTION_NODE_ID, mesh, display: d, overlay: true });
        }
    }
    (draws, scalar_bar)
}

/// Pictures of the objects servers draw for this view: ask for a new one whenever what it
/// should show changed (camera, size, objects, time) and none is on its way; meanwhile the
/// last one is shown.
fn remote_layers(state: &mut State, ctx: &egui::Context, w: u32, h: u32) -> Vec<Arc<aurafx_render::ImageLayer>> {
    use std::hash::{Hash, Hasher};
    let (groups, time) = {
        let s = state.session.lock();
        let mut groups: HashMap<
            String,
            (Arc<aurafx_pipeline::remote::RemoteClient>, Vec<(NodeId, DisplayProps, usize)>, Vec<aurafx_pipeline::remote::NodeSpec>),
        > = HashMap::new();
        for (id, host) in s.drawn_on_server() {
            let Some(client) = s.remote_clients.get(&aurafx_pipeline::remote::client_key(&host)).cloned() else { continue };
            let n = &s.nodes[&id];
            let ptr = n.output.as_ref().map(|o| Arc::as_ptr(o) as usize).unwrap_or(0);
            let g = groups.entry(host.name.clone()).or_insert_with(|| (client, Vec::new(), Vec::new()));
            g.1.push((id, n.display.clone(), ptr));
            for spec in s.remote_specs(id) {
                if !g.2.iter().any(|x| x.id == spec.id) {
                    g.2.push(spec);
                }
            }
        }
        (groups, if s.times.is_empty() { None } else { s.current_time() })
    };
    let cam = aurafx_render::server::render_camera(&state.viewport.camera);
    let lighting = state.viewport.settings.lighting.for_server();
    state.viewport.remote_images.retain(|h, _| groups.contains_key(h));
    let mut out = Vec::new();
    for (host, (client, targets, specs)) in groups {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for v in cam.eye.iter().chain(&cam.target).chain(&cam.up).chain([&cam.fov_y_deg, &cam.ortho_height, &cam.near, &cam.far]) {
            v.to_bits().hash(&mut hasher);
        }
        (cam.orthographic, w, h, time.map(f64::to_bits), format!("{lighting:?}")).hash(&mut hasher);
        for (id, d, ptr) in &targets {
            (id, ptr, format!("{d:?}")).hash(&mut hasher);
        }
        let key = hasher.finish();
        let img = state.viewport.remote_images.entry(host.clone()).or_default();
        if let Some((k, rx)) = &img.pending {
            match rx.try_recv() {
                Ok(Ok(layer)) => {
                    img.key = *k;
                    img.layer = Some(Arc::new(layer));
                    img.error = None;
                    img.pending = None;
                }
                Ok(Err(e)) => {
                    img.error = Some(format!("{e:#}"));
                    img.key = *k;
                    img.pending = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => img.pending = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if img.pending.is_none() && img.key != key {
            let (tx, rx) = std::sync::mpsc::channel();
            let c = ctx.clone();
            let job = aurafx_pipeline::remote::RenderJob {
                targets: targets.iter().map(|(id, d, _)| (*id, d.clone())).collect(),
                camera: cam,
                width: w,
                height: h,
                lighting: lighting.clone(),
            };
            std::thread::spawn(move || {
                let r = client.render(specs, time, job).map(|(width, height, color, depth, _ms)| aurafx_render::ImageLayer { width, height, color, depth });
                let _ = tx.send(r);
                c.request_repaint();
            });
            img.pending = Some((key, rx));
        }
        if img.pending.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        if let Some(e) = &img.error {
            state.status = format!("{host} could not draw: {e}");
        }
        if let Some(l) = &img.layer {
            out.push(l.clone());
        }
    }
    out
}

/// Set (or extend with Shift / reduce with Ctrl) the selection of `node`.
fn apply_selection(state: &mut State, node: NodeId, assoc: Association, mut ids: Vec<usize>, add: bool, remove: bool) {
    if add || remove {
        let prev = state.session.lock().selection.clone().filter(|p| p.node == node && p.assoc == assoc).map(|p| p.ids).unwrap_or_default();
        if add {
            ids.extend(prev);
        } else {
            let rm: std::collections::HashSet<usize> = ids.into_iter().collect();
            ids = prev.into_iter().filter(|i| !rm.contains(i)).collect();
        }
    }
    let n = ids.len();
    state.session.lock().set_selection(node, assoc, ids);
    state.status = format!("Selected {n} {}", if assoc == Association::Points { "points" } else { "cells" });
}

fn do_pick(state: &mut State, rect: Rect, pos: Pos2) {
    let ray = crate::pick::screen_ray(&state.viewport.camera, rect, pos);
    let visible: std::collections::HashSet<NodeId> = state.session.lock().nodes.values().filter(|n| n.visible).map(|n| n.id).collect();
    let meshes: Vec<(NodeId, Arc<MeshData>)> =
        state.viewport.meshes.iter().filter(|(id, _)| visible.contains(*id)).map(|(id, c)| (*id, c.mesh.clone())).collect();
    let radius = state.viewport.camera.world_per_pixel(rect.height() as f64) * 6.0;
    match crate::pick::raycast(&ray, &meshes, radius) {
        Some(hit) => {
            let (name, ds) = {
                let s = state.session.lock();
                let n = &s.nodes[&hit.node];
                (n.name.clone(), n.output.clone())
            };
            let values = ds.map(|d| crate::pick::values_at(&d, hit.world)).unwrap_or_default();
            state.session.lock().active = Some(hit.node);
            state.viewport.pick = Some(PickResult { node_name: name, world: hit.world, values });
        }
        None => state.viewport.pick = None,
    }
}

/// Draw the view currently in `state.viewport`. Returns true when the user started interacting
/// with it (used to make a view active).
pub fn viewport_ui(state: &mut State, ui: &mut egui::Ui, is_active: bool) -> bool {
    let avail = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
    let interacted = response.drag_started() || response.clicked() || response.secondary_clicked();
    let ppp = ui.ctx().pixels_per_point();
    let w = (rect.width() * ppp).round().max(1.0) as u32;
    let h = (rect.height() * ppp).round().max(1.0) as u32;

    // Camera requests from the session (Python, menus) and first-data auto fit.
    let (req, bounds, empty) = {
        let mut s = state.session.lock();
        // Wait for running/dirty nodes so the camera fits the final data.
        let req = if s.is_busy() || !is_active { None } else { s.camera_request.take() };
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

    // Mode keys (when the pointer is over the view and no text field has focus).
    if response.hovered() && !ui.ctx().text_edit_focused() {
        ui.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::S) {
                state.viewport.mode = InteractMode::SelectCells;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::D) {
                state.viewport.mode = InteractMode::SelectPoints;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::P) {
                state.viewport.mode = InteractMode::Pick;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::M) {
                state.viewport.mode = InteractMode::Ruler;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                state.viewport.mode = InteractMode::Navigate;
                state.viewport.pick = None;
                state.viewport.ruler.clear();
            }
        });
    }
    let selecting = matches!(state.viewport.mode, InteractMode::SelectCells | InteractMode::SelectPoints);

    // Interaction.
    let drag = response.drag_delta();
    let size = rect.size().max(Vec2::splat(1.0));
    let mods = ui.input(|i| i.modifiers);
    let mut selection_rect: Option<Rect> = None;
    if selecting && response.drag_started_by(egui::PointerButton::Primary) {
        state.viewport.drag_start = response.interact_pointer_pos();
    }
    if selecting && response.dragged_by(egui::PointerButton::Primary) {
        if let (Some(a), Some(b)) = (state.viewport.drag_start, response.interact_pointer_pos()) {
            selection_rect = Some(Rect::from_two_pos(a, b));
        }
    }
    if selecting && response.drag_stopped_by(egui::PointerButton::Primary) {
        if let (Some(a), Some(b)) = (state.viewport.drag_start.take(), response.interact_pointer_pos()) {
            let sel = Rect::from_two_pos(a, b);
            let assoc = if state.viewport.mode == InteractMode::SelectPoints { Association::Points } else { Association::Cells };
            let target = {
                let s = state.session.lock();
                s.active_node()
                    .filter(|n| n.visible)
                    .and_then(|n| n.output.clone().map(|o| (n.id, o)))
                    .or_else(|| s.nodes.values().rev().find(|n| n.visible && n.output.is_some()).map(|n| (n.id, n.output.clone().unwrap())))
            };
            match target {
                Some((id, _)) if assoc == Association::Cells && !state.viewport.select_through => {
                    // Visible cells only: resolved with the GPU id pass after this frame's render.
                    let ppp = ui.ctx().pixels_per_point();
                    let px = |p: Pos2| ((p.x - rect.left()).max(0.0) * ppp, (p.y - rect.top()).max(0.0) * ppp);
                    let (a, b) = (px(sel.min), px(sel.max));
                    state.viewport.pending_id_pick =
                        Some((id, [a.0 as u32, a.1 as u32, b.0.ceil() as u32 + 1, b.1.ceil() as u32 + 1], mods.shift, mods.command));
                }
                Some((id, ds)) => {
                    let ids = crate::pick::select_in_rect(&state.viewport.camera, rect, sel, &ds, assoc);
                    apply_selection(state, id, assoc, ids, mods.shift, mods.command);
                }
                None => state.toast("Select a visible dataset first", true),
            }
        }
    }
    // Widgets: the active filter's plane, sphere, box, line or point, and custom lights.
    let widget = if state.viewport.mode == InteractMode::Navigate { crate::widgets3d::active(state) } else { None };
    let light_widgets = state.viewport.mode == InteractMode::Navigate && state.settings.show_light_widgets && !state.settings.lighting.lights.is_empty();
    if response.drag_started_by(egui::PointerButton::Primary) {
        if let Some(pos) = response.interact_pointer_pos() {
            let cam = &state.viewport.camera;
            let light =
                light_widgets.then(|| crate::widgets3d::grab_light(&state.settings.lighting.lights, cam, rect, &bounds, pos)).flatten().map(WidgetDrag::Light);
            let filter = || widget.as_ref().and_then(|w| crate::widgets3d::grab(w, cam, rect, pos)).map(WidgetDrag::Filter);
            state.viewport.widget_drag = light.or_else(filter);
        }
    }
    if let (Some(h), Some(pos)) = (state.viewport.widget_drag, response.interact_pointer_pos()) {
        if response.dragged_by(egui::PointerButton::Primary) {
            match h {
                WidgetDrag::Filter(k) => {
                    if let Some(w) = &widget {
                        crate::widgets3d::drag(state, w, k, &state.viewport.camera, rect, pos, drag);
                    }
                }
                WidgetDrag::Light(k) => {
                    let cam = state.viewport.camera.clone();
                    crate::widgets3d::drag_light(&mut state.settings.lighting.lights, k, &cam, rect, &bounds, pos);
                }
            }
        }
    }
    if response.drag_stopped() {
        state.viewport.widget_drag = None;
    }
    let widget_dragging = state.viewport.widget_drag.is_some();
    let cam = &mut state.viewport.camera;
    if widget_dragging {
        // The widget owns this drag.
    } else if !selecting && response.dragged_by(egui::PointerButton::Primary) && !mods.shift && !mods.command {
        cam.rotate((drag.x / size.x) as f64 * 1.5, (drag.y / size.y) as f64 * 1.5);
    } else if response.dragged_by(egui::PointerButton::Middle) || (!selecting && response.dragged_by(egui::PointerButton::Primary) && mods.shift) {
        cam.pan((drag.x / size.y) as f64, (drag.y / size.y) as f64);
    } else if response.dragged_by(egui::PointerButton::Secondary) || (!selecting && response.dragged_by(egui::PointerButton::Primary) && mods.command) {
        cam.zoom((1.0 - (drag.y / size.y) as f64 * 2.0).max(0.05));
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.0 {
            cam.zoom((1.0 + scroll as f64 * 0.002).clamp(0.5, 2.0));
        }
        // Pinch to zoom on touch screens and trackpads.
        let pinch = ui.input(|i| i.zoom_delta());
        if (pinch - 1.0).abs() > 1e-4 {
            cam.zoom(pinch as f64);
        }
    }
    if response.double_clicked() && state.viewport.mode == InteractMode::Navigate {
        cam.reset_to_bounds(&bounds);
    }
    // Ruler: each click adds an end point on the surface under the cursor.
    if response.clicked() && state.viewport.mode == InteractMode::Ruler {
        if let Some(pos) = response.interact_pointer_pos() {
            let ray = crate::pick::screen_ray(&state.viewport.camera, rect, pos);
            let visible: std::collections::HashSet<NodeId> = state.session.lock().nodes.values().filter(|n| n.visible).map(|n| n.id).collect();
            let meshes: Vec<(NodeId, Arc<MeshData>)> =
                state.viewport.meshes.iter().filter(|(id, _)| visible.contains(*id)).map(|(id, c)| (*id, c.mesh.clone())).collect();
            let radius = state.viewport.camera.world_per_pixel(rect.height() as f64) * 6.0;
            if let Some(hit) = crate::pick::raycast(&ray, &meshes, radius) {
                if state.viewport.ruler.len() >= 2 {
                    state.viewport.ruler.clear();
                }
                state.viewport.ruler.push(hit.world);
                if state.viewport.ruler.len() == 2 {
                    let d = state.viewport.ruler[1] - state.viewport.ruler[0];
                    state.status = format!(
                        "Distance {} (dx {}, dy {}, dz {})",
                        aurafx_data::info::g(d.length()),
                        aurafx_data::info::g(d.x),
                        aurafx_data::info::g(d.y),
                        aurafx_data::info::g(d.z)
                    );
                }
            }
        }
    }
    // Click to pick (Pick mode, or Ctrl+click in any mode).
    if response.clicked() && (state.viewport.mode == InteractMode::Pick || (mods.command && !selecting)) {
        if let Some(pos) = response.interact_pointer_pos() {
            do_pick(state, rect, pos);
        }
    }
    let cam = &mut state.viewport.camera;
    if !bounds.is_empty() {
        cam.update_clip(&bounds);
    }

    // Render.
    let (draws, scalar_bar) = collect_draws(state);
    let remote = remote_layers(state, ui.ctx(), w, h);
    let mut layers = remote.clone();
    // Ray tracing: a traced picture replaces the opaque surfaces once it is ready.
    if let Some(r) = &state.viewport.renderer {
        state.viewport.ray_tracing.environment = r.environment();
        if state.viewport.ray_tracing.gpu.is_none() {
            state.viewport.ray_tracing.gpu = Some((r.device().clone(), r.queue().clone()));
        }
    }
    let (shown, trace_progress) = if state.viewport.settings.lighting.ray_tracing && state.viewport.renderer.is_some() {
        let f = state.viewport.ray_tracing.frame(ui.ctx(), draws.clone(), &state.viewport.camera, &state.viewport.settings, w, h);
        layers.extend(f.layer);
        (f.draws, f.progress)
    } else {
        state.viewport.ray_tracing.stop();
        (draws.clone(), None)
    };
    if state.viewport.renderer.is_some() {
        let vp = &mut state.viewport;
        let r = vp.renderer.as_mut().unwrap();
        let mut recreated = r.resize(w, h);
        r.set_layers(&layers);
        r.render(&vp.camera, &shown, &vp.settings);
        if let Some((node, px, add, remove)) = state.viewport.pending_id_pick.take() {
            match state.viewport.renderer.as_mut().unwrap().pick_ids(&state.viewport.camera, &draws, node, px) {
                Ok(ids) => {
                    let ids: Vec<usize> = ids.into_iter().map(|i| i as usize).collect();
                    apply_selection(state, node, Association::Cells, ids, add, remove);
                }
                Err(e) => state.toast(format!("Selection failed: {e:#}"), true),
            }
        }
        // Screenshot requests (UI or Python). Requests from scripts wait until what the script
        // did before is in the picture: data updated, camera moved, and app commands (lighting,
        // ...) applied over a few frames.
        let py_req = if is_active {
            let mut s = state.session.lock();
            if !s.is_busy() && s.camera_request.is_none() {
                s.screenshot_request.take()
            } else {
                None
            }
        } else {
            None
        };
        if let Some(r) = py_req {
            state.viewport.screenshot = Some(r);
            state.viewport.screenshot_wait = 2;
        }
        let req = if state.viewport.screenshot_wait > 0 {
            state.viewport.screenshot_wait -= 1;
            ui.ctx().request_repaint();
            None
        } else {
            state.viewport.screenshot.take()
        };
        let mut outcome = None;
        if let Some(req) = req {
            // A 0 keeps the view's shape.
            let (sw, sh) = match req.size {
                Some((0, 0)) | None => (w, h),
                Some((sw, 0)) => (sw, (sw as u64 * h as u64 / w.max(1) as u64).max(1) as u32),
                Some((0, sh)) => ((sh as u64 * w as u64 / h.max(1) as u64).max(1) as u32, sh),
                Some(s) => s,
            };
            let as_shown = (sw, sh) == (w, h) && !req.transparent;
            let vp = &mut state.viewport;
            let max = vp.renderer.as_ref().unwrap().max_size();
            let ext = req.path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            if matches!(ext.as_str(), "glb" | "obj") {
                // Scene export: the objects as drawn, named as in the pipeline.
                let s = state.session.lock();
                let objects: Vec<aurafx_render::scene_export::ExportObject> = draws
                    .iter()
                    .filter_map(|d| s.nodes.get(&d.id).map(|n| aurafx_render::scene_export::ExportObject { name: n.name.clone(), draw: d }))
                    .collect();
                let res = aurafx_render::scene_export::export(&req.path, &objects);
                drop(s);
                outcome = Some((req.clone(), res));
            } else if as_shown && trace_progress.is_some() {
                // A ray-traced screenshot waits for the tracer to finish.
                vp.screenshot = Some(req);
            } else if as_shown {
                outcome = Some((req.clone(), vp.renderer.as_ref().unwrap().save_png(&req.path)));
            } else if sw == 0 || sh == 0 || sw.max(sh) > max {
                outcome = Some((req.clone(), Err(anyhow::anyhow!("{sw}x{sh} pixels: this GPU draws at most {max}x{max}"))));
            } else {
                // Render once more at the asked size (traced at that size when ray tracing).
                let mut settings = vp.settings.clone();
                settings.transparent = req.transparent;
                let mut shot_layers = remote.clone();
                let mut shot_draws = Some(draws.clone());
                if settings.lighting.ray_tracing {
                    match vp.ray_tracing.still(&draws, &vp.camera, &settings, sw, sh) {
                        Some(img) => {
                            shot_layers.push(img);
                            shot_draws = Some(vp.ray_tracing.strip(draws.clone()));
                        }
                        None => shot_draws = None,
                    }
                }
                match shot_draws {
                    None => {
                        vp.screenshot = Some(req);
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                    }
                    Some(shot_draws) => {
                        let r = vp.renderer.as_mut().unwrap();
                        r.resize(sw, sh);
                        r.set_layers(&shot_layers);
                        r.render(&vp.camera, &shot_draws, &settings);
                        let res = r.save_png_with(&req.path, req.transparent);
                        r.resize(w, h);
                        r.set_layers(&layers);
                        r.render(&vp.camera, &shown, &vp.settings);
                        recreated = true;
                        outcome = Some((req.clone(), res));
                    }
                }
            }
            if state.viewport.screenshot.is_some() {
                let text = if trace_progress.is_some() || state.viewport.settings.lighting.ray_tracing {
                    "Ray tracing the screenshot…"
                } else {
                    "Saving the screenshot…"
                };
                state.status = text.into();
            }
        }
        state.viewport.sync_texture(recreated);
        if let Some((req, res)) = outcome {
            match &res {
                Ok(()) => state.toast(format!("Saved {}", req.path.display()), false),
                Err(e) => state.toast(format!("Screenshot failed: {e:#}"), true),
            }
            if req.id != 0 {
                state.session.lock().screenshot_results.push((req.id, res.map_err(|e| format!("{e:#}"))));
            }
            state.status.clear();
        }
    }
    if let Some(tex) = state.viewport.texture {
        ui.painter().image(tex, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }

    // Overlays.
    let painter = ui.painter_at(rect);
    let fg = crate::theme::overlay_text(state.viewport.settings.background);
    let fg_weak = fg.linear_multiply(0.6);
    if let Some((done, wanted)) = trace_progress {
        let on = if state.viewport.ray_tracing.on_gpu.load(std::sync::atomic::Ordering::Relaxed) { "GPU" } else { "CPU" };
        let text = if done == 0 { "Ray tracing…".to_string() } else { format!("Ray tracing {done}/{wanted} ({on})") };
        painter.text(rect.right_bottom() + Vec2::new(-10.0, -8.0), egui::Align2::RIGHT_BOTTOM, text, egui::FontId::proportional(12.0), fg_weak);
    }
    if empty {
        let c = rect.center();
        painter.text(c + Vec2::new(0.0, -30.0), egui::Align2::CENTER_CENTER, "auraFX", egui::FontId::proportional(34.0), fg_weak);
        painter.text(
            c + Vec2::new(0.0, 6.0),
            egui::Align2::CENTER_CENTER,
            "Drop a file here, press Ctrl+O to open one, or Ctrl+K to search for a source or filter.",
            egui::FontId::proportional(15.0),
            fg_weak,
        );
        painter.text(
            c + Vec2::new(0.0, 30.0),
            egui::Align2::CENTER_CENTER,
            "Supported: Exodus, VTK (.vtk .vtu .vtp .vti .vtr .vts .vtm .pvd), OpenFOAM, LAMMPS dump/data, XYZ, STL, PLY, OBJ, CSV",
            egui::FontId::proportional(12.0),
            fg_weak,
        );
    }
    if empty {
        let last = crate::app::State::last_session_path();
        if last.exists() {
            let r = Rect::from_center_size(rect.center() + Vec2::new(0.0, 75.0), Vec2::new(220.0, 30.0));
            if ui.put(r, egui::Button::new(format!("{} Restore last session", crate::icons::ARROW_CLOCKWISE))).clicked() {
                state.load_state(last);
            }
        }
        let r = Rect::from_center_size(rect.center() + Vec2::new(0.0, 112.0), Vec2::new(220.0, 30.0));
        if ui.put(r, egui::Button::new(format!("{} Try a sample dataset", crate::icons::CUBE))).clicked() {
            state.open_sample("disk_out_ref.ex2");
        }
    }
    for b in state.viewport.grid_boxes.clone() {
        crate::axes_grid::draw_labels(&painter, &state.viewport.camera, rect, &b, ["X", "Y", "Z"], fg);
    }
    if let Some((label, cmap, (lo, hi), log)) = scalar_bar {
        let bgc = state.viewport.settings.background;
        let plate = Color32::from_rgb((bgc[0] * 255.0) as u8, (bgc[1] * 255.0) as u8, (bgc[2] * 255.0) as u8);
        draw_scalar_bar(&painter, rect, &label, &cmap, lo, hi, log, fg, plate);
    }
    if let Some(r) = selection_rect {
        painter.rect(r, 0.0, Color32::from_rgba_unmultiplied(255, 40, 200, 30), Stroke::new(1.5, Color32::from_rgb(255, 40, 200)), egui::StrokeKind::Inside);
    }
    if let Some(pk) = &state.viewport.pick {
        if let Some(sp) = crate::pick::project(&state.viewport.camera, rect, pk.world) {
            painter.circle_stroke(sp, 7.0, Stroke::new(2.0, Color32::from_rgb(255, 40, 200)));
            painter.line_segment([sp - Vec2::new(11.0, 0.0), sp + Vec2::new(11.0, 0.0)], Stroke::new(1.0, fg));
            painter.line_segment([sp - Vec2::new(0.0, 11.0), sp + Vec2::new(0.0, 11.0)], Stroke::new(1.0, fg));
        }
    }
    if state.viewport.mode == InteractMode::Navigate {
        let active = |d: Option<WidgetDrag>, f: fn(usize) -> WidgetDrag| (0..32).find(|k| d == Some(f(*k)));
        if let Some(w) = crate::widgets3d::active(state) {
            crate::widgets3d::draw(&painter, &state.viewport.camera, rect, &w, active(state.viewport.widget_drag, WidgetDrag::Filter));
        }
        if state.settings.show_light_widgets && !state.settings.lighting.lights.is_empty() {
            crate::widgets3d::draw_lights(
                &painter,
                &state.viewport.camera,
                rect,
                &state.settings.lighting.lights,
                &bounds,
                active(state.viewport.widget_drag, WidgetDrag::Light),
            );
        }
    }
    // Ruler.
    if !state.viewport.ruler.is_empty() {
        let col = Color32::from_rgb(80, 220, 255);
        let pts: Vec<Pos2> = state.viewport.ruler.iter().filter_map(|p| crate::pick::project(&state.viewport.camera, rect, *p)).collect();
        for p in &pts {
            painter.circle(*p, 5.0, col, Stroke::new(1.5, Color32::WHITE));
        }
        if pts.len() == 2 && state.viewport.ruler.len() == 2 {
            painter.line_segment([pts[0], pts[1]], Stroke::new(2.5, col));
            let d = (state.viewport.ruler[1] - state.viewport.ruler[0]).length();
            let mid = pts[0] + (pts[1] - pts[0]) * 0.5;
            let galley = painter.layout_no_wrap(aurafx_data::info::g(d), egui::FontId::proportional(14.0), Color32::WHITE);
            let r = Rect::from_center_size(mid + Vec2::new(0.0, -14.0), galley.size() + Vec2::splat(8.0));
            painter.rect_filled(r, 4.0, Color32::from_black_alpha(170));
            painter.galley(r.min + Vec2::splat(4.0), galley, Color32::WHITE);
        }
    }
    // Annotations and the time label.
    let (notes, time_label) = {
        let s = state.session.lock();
        (s.annotations.clone(), if s.show_time_label { s.current_time().map(|t| format!("t = {}", aurafx_data::info::g(t))) } else { None })
    };
    let mut stacks: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
    let mut place = |text: &str, corner: &str, size: f32| {
        let (anchor, base) = match corner {
            "upper right" => (egui::Align2::RIGHT_TOP, rect.right_top() + Vec2::new(-12.0, 52.0)),
            "lower left" => (egui::Align2::LEFT_BOTTOM, rect.left_bottom() + Vec2::new(90.0, -12.0)),
            "lower right" => (egui::Align2::RIGHT_BOTTOM, rect.right_bottom() + Vec2::new(-12.0, -12.0)),
            "upper center" => (egui::Align2::CENTER_TOP, rect.center_top() + Vec2::new(0.0, 52.0)),
            "lower center" => (egui::Align2::CENTER_BOTTOM, rect.center_bottom() + Vec2::new(0.0, -12.0)),
            _ => (egui::Align2::LEFT_TOP, rect.left_top() + Vec2::new(12.0, 52.0)),
        };
        let off = stacks.entry(corner.to_string()).or_insert(0.0);
        let dy = if anchor.y() == egui::Align::Max { -*off } else { *off };
        painter.text(base + Vec2::new(0.0, dy), anchor, text, egui::FontId::proportional(size), fg);
        *off += size * 1.35;
    };
    if let Some(t) = &time_label {
        place(t, "upper right", 16.0);
    }
    for a in &notes {
        place(&a.text, &a.corner, a.size);
    }
    viewport_toolbar(state, ui, rect);
    // Mark the active view when several are open.
    if state.views.len() > 0 && is_active {
        ui.painter().rect_stroke(rect.shrink(1.0), 0.0, Stroke::new(2.0, crate::theme::ACCENT), egui::StrokeKind::Inside);
    }
    if state.settings.show_orientation_axes {
        draw_axes(&painter, rect, &state.viewport.camera);
    }
    // Keep animating while dragging or playing.
    if response.dragged() {
        ui.ctx().request_repaint();
    }
    interacted
}

fn draw_scalar_bar(p: &egui::Painter, rect: Rect, label: &str, map: &colormap::ColorMap, lo: f64, hi: f64, log: bool, txt: Color32, bg: Color32) {
    let bar_w = 18.0;
    let bar_h = (rect.height() * 0.45).clamp(80.0, 320.0);
    let x = rect.right() - 96.0;
    let y0 = rect.center().y - bar_h / 2.0;
    // A soft plate behind the bar keeps its labels readable over the data.
    let title_w = (label.len() as f32 * 7.5).max(bar_w + 70.0);
    let plate = Rect::from_min_max(Pos2::new((x + bar_w / 2.0 - title_w / 2.0).min(x - 6.0), y0 - 28.0), Pos2::new(rect.right() - 6.0, y0 + bar_h + 10.0));
    p.rect_filled(plate, 6.0, bg.gamma_multiply(0.72));
    let n = 64;
    for i in 0..n {
        let t0 = i as f32 / n as f32;
        let t1 = (i + 1) as f32 / n as f32;
        let c = map.sample(1.0 - (t0 + t1) * 0.5);
        let col = Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8);
        p.rect_filled(Rect::from_min_max(Pos2::new(x, y0 + t0 * bar_h), Pos2::new(x + bar_w, y0 + t1 * bar_h + 0.5)), 0.0, col);
    }
    p.rect_stroke(Rect::from_min_size(Pos2::new(x, y0), Vec2::new(bar_w, bar_h)), 0.0, Stroke::new(1.0, txt), egui::StrokeKind::Outside);
    let font = egui::FontId::proportional(12.0);
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
    for (i, (name, col)) in
        [("X", Color32::from_rgb(240, 90, 90)), ("Y", Color32::from_rgb(120, 220, 120)), ("Z", Color32::from_rgb(110, 150, 255))].into_iter().enumerate()
    {
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

fn viewport_toolbar(state: &mut State, ui: &mut egui::Ui, rect: Rect) {
    use crate::icons;
    let sel = state.session.lock().selection.clone();
    #[allow(deprecated)]
    let id = egui::Id::new(("viewport_toolbar", state.viewport.id));
    egui::Area::new(id).fixed_pos(rect.left_top() + Vec2::new(8.0, 8.0)).order(egui::Order::Middle).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let m = &mut state.viewport.mode;
                ui.selectable_value(m, InteractMode::Navigate, icons::ARROWS_OUT_CARDINAL).on_hover_text("Navigate: rotate, pan, zoom (Esc)");
                ui.selectable_value(m, InteractMode::Pick, icons::CROSSHAIR).on_hover_text("Pick: click a surface to read values (P, or Ctrl+click)");
                ui.selectable_value(m, InteractMode::Ruler, icons::RULER).on_hover_text("Ruler: click two points to measure the distance (M)");
                ui.selectable_value(m, InteractMode::SelectCells, icons::SELECTION)
                    .on_hover_text("Select visible cells in a rectangle (S). Shift adds, Ctrl removes.");
                ui.selectable_value(m, InteractMode::SelectPoints, icons::DOTS_NINE)
                    .on_hover_text("Select points in a rectangle (D). Shift adds, Ctrl removes.");
                if matches!(state.viewport.mode, InteractMode::SelectCells | InteractMode::SelectPoints) {
                    let t = state.viewport.select_through;
                    if ui.selectable_label(t, icons::STACK).on_hover_text("Select through: include cells hidden behind the visible surface").clicked() {
                        state.viewport.select_through = !t;
                    }
                }
                ui.separator();
                let vid = state.viewport.id;
                if ui.small_button(icons::COLUMNS).on_hover_text("Split: add a 3D view to the right").clicked() {
                    state.layout_actions.push(crate::app::LayoutAction::SplitView { from: vid, vertical: false, only: None });
                }
                if ui.small_button(icons::ROWS).on_hover_text("Split: add a 3D view below").clicked() {
                    state.layout_actions.push(crate::app::LayoutAction::SplitView { from: vid, vertical: true, only: None });
                }
                ui.menu_button(icons::CUBE, |ui| {
                    ui.label(egui::RichText::new("Look from").weak());
                    for (label, dir, up) in crate::views::VIEW_DIRECTIONS {
                        if ui.button(*label).clicked() {
                            ui.close();
                            let b = state.session.lock().visible_bounds();
                            state.viewport.camera.set_view(*dir, *up, &b);
                            state.viewport.mark_camera_initialized();
                        }
                    }
                    ui.separator();
                    if ui.button("Reset camera").clicked() {
                        ui.close();
                        let b = state.session.lock().visible_bounds();
                        state.viewport.camera.reset_to_bounds(&b);
                    }
                })
                .response
                .on_hover_text("Camera direction for this view");
                if !state.views.is_empty() {
                    let link_icon = if state.viewport.linked { icons::LINK } else { icons::LINK_BREAK };
                    if ui.selectable_label(state.viewport.linked, link_icon).on_hover_text("Link this view's camera with the other linked views").clicked() {
                        state.viewport.linked = !state.viewport.linked;
                    }
                    if ui.small_button(icons::COPY).on_hover_text("Match: copy this camera to every other view once").clicked() {
                        state.camera_match_from = Some(vid);
                    }
                }
                if let Some(sel) = &sel {
                    ui.separator();
                    ui.label(format!("{} {}", sel.ids.len(), if sel.assoc == Association::Points { "points" } else { "cells" }));
                    if ui.small_button("Extract").on_hover_text("Create an Extract Selection node").clicked() {
                        let r = state.session.lock().extract_selection();
                        if let Err(e) = r {
                            state.toast(format!("{e:#}"), true);
                        }
                    }
                    if ui.small_button(icons::X).on_hover_text("Clear selection").clicked() {
                        state.session.lock().clear_selection();
                    }
                }
            });
        });
    });
    // Pick result card.
    let mut close = false;
    if let Some(pk) = &state.viewport.pick {
        #[allow(deprecated)]
        let id = egui::Id::new(("pick_card", state.viewport.id));
        egui::Area::new(id).fixed_pos(rect.left_top() + Vec2::new(8.0, 52.0)).order(egui::Order::Middle).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(320.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{} {}", icons::CROSSHAIR, pk.node_name)).strong());
                    if ui.small_button(icons::X).clicked() {
                        close = true;
                    }
                });
                ui.label(
                    egui::RichText::new(format!(
                        "x {}  y {}  z {}",
                        aurafx_data::info::g(pk.world.x),
                        aurafx_data::info::g(pk.world.y),
                        aurafx_data::info::g(pk.world.z)
                    ))
                    .monospace()
                    .small(),
                );
                egui::Grid::new("pick_vals").striped(true).num_columns(2).show(ui, |ui| {
                    for (k, v) in pk.values.iter().take(24) {
                        ui.label(k);
                        ui.label(egui::RichText::new(v).monospace());
                        ui.end_row();
                    }
                });
            });
        });
    }
    if close {
        state.viewport.pick = None;
    }
}

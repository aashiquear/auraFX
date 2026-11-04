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
    pub mode: InteractMode,
    drag_start: Option<Pos2>,
    pub pick: Option<PickResult>,
    selection_mesh: Option<(u64, Arc<MeshData>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractMode {
    Navigate,
    Pick,
    SelectCells,
    SelectPoints,
}

pub struct PickResult {
    pub node_name: String,
    pub world: glam::DVec3,
    pub values: Vec<(String, String)>,
}

pub const SELECTION_NODE_ID: NodeId = u64::MAX;

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
            mode: InteractMode::Navigate,
            drag_start: None,
            pick: None,
            selection_mesh: None,
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

    pub fn mark_camera_initialized(&mut self) {
        self.camera_initialized = true;
    }

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
                let label = format!(
                    "{}{}",
                    cb.name,
                    match cb.component {
                        Some(c) => format!(" [{c}]"),
                        None => String::new(),
                    }
                );
                scalar_bar = Some((label, n.display.colormap.clone(), r, n.display.log_scale));
            }
        }
        draws.push(NodeDraw { id: n.id, mesh, display: n.display.clone(), overlay: false });
    }
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
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                state.viewport.mode = InteractMode::Navigate;
                state.viewport.pick = None;
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
                Some((id, ds)) => {
                    let mut ids = crate::pick::select_in_rect(&state.viewport.camera, rect, sel, &ds, assoc);
                    if mods.shift || mods.command {
                        // Shift adds to, Ctrl removes from, the existing selection.
                        let prev = state.session.lock().selection.clone().filter(|p| p.node == id && p.assoc == assoc).map(|p| p.ids).unwrap_or_default();
                        if mods.shift {
                            ids.extend(prev);
                        } else {
                            let rm: std::collections::HashSet<usize> = ids.into_iter().collect();
                            ids = prev.into_iter().filter(|i| !rm.contains(i)).collect();
                        }
                    }
                    let n = ids.len();
                    state.session.lock().set_selection(id, assoc, ids);
                    state.status = format!("Selected {n} {}", if assoc == Association::Points { "points" } else { "cells" });
                }
                None => state.toast("Select a visible dataset first", true),
            }
        }
    }
    let cam = &mut state.viewport.camera;
    if !selecting && response.dragged_by(egui::PointerButton::Primary) && !mods.shift && !mods.command {
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
    }
    if response.double_clicked() && state.viewport.mode == InteractMode::Navigate {
        cam.reset_to_bounds(&bounds);
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
    let fg = crate::theme::overlay_text(state.viewport.settings.background);
    let fg_weak = fg.linear_multiply(0.6);
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
            "Supported: VTK (.vtk .vtu .vtp .vti .vtr .vts .vtm .pvd), LAMMPS dumps, STL, CSV",
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
    }
    if let Some((label, cmap, (lo, hi), log)) = scalar_bar {
        draw_scalar_bar(&painter, rect, &label, &cmap, lo, hi, log, fg);
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
    viewport_toolbar(state, ui, rect);
    if state.settings.show_orientation_axes {
        draw_axes(&painter, rect, &state.viewport.camera);
    }
    // Keep animating while dragging or playing.
    if response.dragged() {
        ui.ctx().request_repaint();
    }
}

fn draw_scalar_bar(p: &egui::Painter, rect: Rect, label: &str, cmap: &str, lo: f64, hi: f64, log: bool, txt: Color32) {
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
    let id = egui::Id::new("viewport_toolbar");
    egui::Area::new(id).fixed_pos(rect.left_top() + Vec2::new(8.0, 8.0)).order(egui::Order::Middle).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let m = &mut state.viewport.mode;
                ui.selectable_value(m, InteractMode::Navigate, icons::ARROWS_OUT_CARDINAL).on_hover_text("Navigate: rotate, pan, zoom (Esc)");
                ui.selectable_value(m, InteractMode::Pick, icons::CROSSHAIR).on_hover_text("Pick: click a surface to read values (P, or Ctrl+click)");
                ui.selectable_value(m, InteractMode::SelectCells, icons::SELECTION)
                    .on_hover_text("Select cells in a rectangle (S). Shift adds, Ctrl removes. Selects through the model.");
                ui.selectable_value(m, InteractMode::SelectPoints, icons::DOTS_NINE)
                    .on_hover_text("Select points in a rectangle (D). Shift adds, Ctrl removes.");
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
        let id = egui::Id::new("pick_card");
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

//! What is drawn over the 3D picture and belongs in screenshots: the scalar bar, axes grid
//! labels, the time label, text annotations and the orientation axes. The view paints them with
//! egui; screenshots and animation frames paint the same shapes at the picture's scale with an
//! offscreen egui pass, so text stays sharp at any size.

use aurafx_pipeline::colormap;
use aurafx_render::{Camera, Renderer};
use egui::{Color32, Pos2, Rect, Stroke, Vec2};

#[derive(Clone)]
pub struct Overlays {
    pub scalar_bar: Option<(String, colormap::ColorMap, (f64, f64), bool)>,
    pub grid_boxes: Vec<aurafx_data::Bounds>,
    pub notes: Vec<aurafx_pipeline::session::Annotation>,
    pub time_label: Option<String>,
    pub orientation_axes: bool,
    pub background: [f32; 3],
    pub camera: Camera,
}

/// Paint the overlays into `rect` (points).
pub fn paint(p: &egui::Painter, rect: Rect, o: &Overlays) {
    let fg = crate::theme::overlay_text(o.background);
    for b in &o.grid_boxes {
        crate::axes_grid::draw_labels(p, &o.camera, rect, b, ["X", "Y", "Z"], fg);
    }
    if let Some((label, cmap, (lo, hi), log)) = &o.scalar_bar {
        let bgc = o.background;
        let plate = Color32::from_rgb((bgc[0] * 255.0) as u8, (bgc[1] * 255.0) as u8, (bgc[2] * 255.0) as u8);
        draw_scalar_bar(p, rect, label, cmap, *lo, *hi, *log, fg, plate);
    }
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
        p.text(base + Vec2::new(0.0, dy), anchor, text, egui::FontId::proportional(size), fg);
        *off += size * 1.35;
    };
    if let Some(t) = &o.time_label {
        place(t, "upper right", 16.0);
    }
    for a in &o.notes {
        place(&a.text, &a.corner, a.size);
    }
    if o.orientation_axes {
        draw_axes(p, rect, &o.camera);
    }
}

#[allow(clippy::too_many_arguments)]
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

/// Paints overlays into pictures with its own egui context and renderer.
#[derive(Default)]
pub struct Offscreen {
    ctx: Option<egui::Context>,
    renderer: Option<egui_wgpu::Renderer>,
}

impl Offscreen {
    /// Lay the overlays out for a `w`×`h` pixel picture of a view `width_points` wide.
    pub fn run(&mut self, o: &Overlays, width_points: f32, w: u32, h: u32) -> egui::FullOutput {
        let ppp = w as f32 / width_points.max(1.0);
        let size = Vec2::new(w as f32 / ppp, h as f32 / ppp);
        let ctx = self.ctx.get_or_insert_with(|| {
            let c = egui::Context::default();
            crate::theme::install_fonts(&c);
            c
        });
        let mut raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)), ..Default::default() };
        raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
        ctx.run_pass(raw, |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            paint(&painter, Rect::from_min_size(Pos2::ZERO, size), o);
        })
    }

    /// The renderer's last picture with the overlays laid out as on a view `width_points` wide.
    pub fn read(&mut self, r: &Renderer, o: &Overlays, width_points: f32) -> anyhow::Result<(u32, u32, Vec<u8>)> {
        r.read_pixels_with(|device, queue, view, w, h| {
            let out = self.run(o, width_points, w, h);
            let ctx = self.ctx.as_ref().unwrap();
            let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
            let renderer = self.renderer.get_or_insert_with(|| {
                egui_wgpu::Renderer::new(device, wgpu::TextureFormat::Rgba8Unorm, egui_wgpu::RendererOptions { dithering: false, ..Default::default() })
            });
            for (id, deltas) in &out.textures_delta.set {
                for delta in deltas {
                    renderer.update_texture(device, queue, *id, delta);
                }
            }
            let sd = egui_wgpu::ScreenDescriptor { size_in_pixels: [w, h], pixels_per_point: out.pixels_per_point };
            let mut enc = device.create_command_encoder(&Default::default());
            let extra = renderer.update_buffers(device, queue, &mut enc, &prims, &sd);
            {
                let pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("overlays"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    ..Default::default()
                });
                renderer.render(&mut pass.forget_lifetime(), &prims, &sd);
            }
            queue.submit(extra.into_iter().chain(Some(enc.finish())));
            for id in &out.textures_delta.free {
                renderer.free_texture(id);
            }
            Ok(())
        })
    }
}

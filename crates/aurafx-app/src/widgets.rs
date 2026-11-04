//! Small reusable UI widgets.

use egui::{Color32, Mesh, Pos2, Rect, Sense, Vec2};

/// Paint a horizontal gradient of color map `name` into `rect`.
pub fn paint_colormap(ui: &egui::Ui, rect: Rect, name: &str) {
    let map = aurafx_pipeline::colormap::get(name);
    let mut mesh = Mesh::default();
    let n = 32;
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let c = map.sample(t);
        let col = Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8);
        let x = rect.left() + t * rect.width();
        mesh.colored_vertex(Pos2::new(x, rect.top()), col);
        mesh.colored_vertex(Pos2::new(x, rect.bottom()), col);
        if i > 0 {
            let k = (2 * i) as u32;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k + 1, k);
        }
    }
    ui.painter().add(mesh);
}

/// Combo box listing every color map with a gradient preview. Returns true when changed.
pub fn colormap_combo(ui: &mut egui::Ui, id: &str, value: &mut String, width: f32) -> bool {
    let before = value.clone();
    let resp = egui::ComboBox::from_id_salt(id).selected_text(value.as_str()).width(width).show_ui(ui, |ui| {
        ui.set_min_width(230.0);
        for n in aurafx_pipeline::colormap::names() {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::new(90.0, 14.0), Sense::hover());
                paint_colormap(ui, r, n);
                if ui.selectable_label(value == n, n).clicked() {
                    *value = n.to_string();
                }
            });
        }
    });
    // Swatch of the current map under the combo box.
    let r = resp.response.rect;
    let strip = Rect::from_min_size(Pos2::new(r.left() + 4.0, r.bottom() - 3.0), Vec2::new(r.width() - 8.0, 2.5));
    paint_colormap(ui, strip, value);
    *value != before
}

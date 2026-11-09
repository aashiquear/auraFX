//! Reference planes of the CAD environment: the form that makes one (from a principal plane, a
//! reference plane or a flat face of the part, moved and turned; or through three points), the
//! list of planes with Sketch and Remove, and their outlines over the 3D view.

use super::op;
use crate::app::State;
use aurafx_cad::PlaneFrame;
use egui::RichText;
use serde_json::json;

/// What the New plane form collects.
pub struct PlaneForm {
    /// 0: from a plane, 1: from a face, 2: through three points.
    pub method: usize,
    pub name: String,
    /// The plane it starts from (XY, XZ, YZ or a reference plane's name).
    pub from: String,
    /// The face it starts from (an index into the part's faces).
    pub face: usize,
    pub offset: String,
    pub angle: String,
    pub about_x: bool,
    pub points: [[String; 3]; 3],
}

impl Default for PlaneForm {
    fn default() -> Self {
        let z = || [String::from("0"), String::from("0"), String::from("0")];
        let mut points = [z(), z(), z()];
        points[1][0] = "10".into();
        points[2][1] = "10".into();
        Self { method: 0, name: String::new(), from: "XY".into(), face: 0, offset: "10".into(), angle: "0".into(), about_x: true, points }
    }
}

fn num(t: &str) -> f64 {
    t.trim().parse().unwrap_or(0.0)
}

/// The form in the PropertyManager.
pub fn form_ui(state: &mut State, ui: &mut egui::Ui) {
    let (planes, faces) = {
        let s = state.session.lock();
        let e = s.cad_edit.as_ref();
        (
            e.map(|e| e.document.planes.iter().map(|p| p.name.clone()).collect::<Vec<_>>()).unwrap_or_default(),
            e.and_then(|e| e.summary.as_ref()).map(|s| s.faces.iter().filter(|f| f.kind == "plane").cloned().collect::<Vec<_>>()).unwrap_or_default(),
        )
    };
    let Some(f) = state.cad.plane_form.as_mut() else { return };
    ui.label(RichText::new("New reference plane").strong());
    ui.label(RichText::new("A plane to sketch on, kept with the part. Planes show as dashed squares in the 3D view.").weak().small());
    ui.horizontal(|ui| {
        for (i, l) in ["From a plane", "From a face", "Through 3 points"].iter().enumerate() {
            ui.selectable_value(&mut f.method, i, *l);
        }
    });
    egui::Grid::new("plane form").num_columns(2).show(ui, |ui| {
        ui.label("Name");
        ui.add(egui::TextEdit::singleline(&mut f.name).hint_text("Plane 1").desired_width(140.0));
        ui.end_row();
        match f.method {
            0 => {
                ui.label("Start from");
                egui::ComboBox::from_id_salt("plane from").selected_text(f.from.clone()).show_ui(ui, |ui| {
                    for (v, l) in [("XY", "Top (XY)"), ("XZ", "Front (XZ)"), ("YZ", "Right (YZ)")] {
                        ui.selectable_value(&mut f.from, v.to_string(), l);
                    }
                    for p in &planes {
                        ui.selectable_value(&mut f.from, p.clone(), p);
                    }
                });
                ui.end_row();
            }
            1 => {
                ui.label("Face");
                if faces.is_empty() {
                    ui.label(RichText::new("no flat face (build the part first)").weak());
                } else {
                    f.face = f.face.min(faces.len() - 1);
                    egui::ComboBox::from_id_salt("plane face").selected_text(faces[f.face].name.clone()).show_ui(ui, |ui| {
                        for (i, fc) in faces.iter().enumerate() {
                            ui.selectable_value(&mut f.face, i, &fc.name);
                        }
                    });
                }
                ui.end_row();
            }
            _ => {
                for (k, l) in ["Point 1", "Point 2", "Point 3"].iter().enumerate() {
                    ui.label(*l);
                    ui.horizontal(|ui| {
                        for c in 0..3 {
                            ui.add(egui::TextEdit::singleline(&mut f.points[k][c]).desired_width(46.0));
                        }
                    });
                    ui.end_row();
                }
            }
        }
        if f.method < 2 {
            ui.label("Offset");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut f.offset).desired_width(60.0));
                ui.label(RichText::new("mm along the normal").weak().small());
            });
            ui.end_row();
            ui.label("Angle");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut f.angle).desired_width(60.0));
                ui.label(RichText::new("degrees about its").weak().small());
                ui.selectable_value(&mut f.about_x, true, "x");
                ui.selectable_value(&mut f.about_x, false, "y");
            });
            ui.end_row();
        }
    });
    if f.method == 2 {
        ui.label(RichText::new("The normal follows the right-hand rule from point 1 to 2 to 3; x runs from point 1 to 2.").weak().small());
    }
    let (mut make, mut cancel) = (false, false);
    ui.horizontal(|ui| {
        make = ui.add_enabled(f.method != 1 || !faces.is_empty(), egui::Button::new(RichText::new("Make plane").strong())).clicked();
        cancel = ui.button("Cancel").clicked();
    });
    if cancel {
        state.cad.plane_form = None;
        return;
    }
    if !make {
        return;
    }
    let mut a = match f.method {
        0 => json!({"from": f.from, "offset": num(&f.offset), "angle": num(&f.angle), "about": if f.about_x { "x" } else { "y" }}),
        1 => {
            let fc = &faces[f.face];
            let Some(base) = PlaneFrame::new(fc.centroid, fc.direction, None) else { return };
            let moved = base.offset(num(&f.offset));
            let r = base.basis();
            let axis = if f.about_x { r.x_axis } else { r.y_axis };
            let q = glam::DQuat::from_axis_angle(axis, num(&f.angle).to_radians());
            let mut how = format!("on {}", fc.name);
            if num(&f.offset) != 0.0 {
                how += &format!(" offset {}", super::num_text(num(&f.offset)));
            }
            if num(&f.angle) != 0.0 {
                how += &format!(" turned {}°", super::num_text(num(&f.angle)));
            }
            json!({"origin": moved.origin, "normal": (q * r.z_axis).to_array(), "x_axis": (q * r.x_axis).to_array(), "how": how})
        }
        _ => json!({"points": f.points.iter().map(|p| p.iter().map(|c| num(c)).collect::<Vec<_>>()).collect::<Vec<_>>()}),
    };
    if !f.name.trim().is_empty() {
        a["name"] = json!(f.name.trim());
    }
    state.cad.plane_form = None;
    op(state, "plane", a);
}

/// The reference planes in the PropertyManager's part page: sketch on one, or remove it.
pub fn list_ui(state: &mut State, ui: &mut egui::Ui) {
    let planes = state.session.lock().cad_edit.as_ref().map(|e| e.document.planes.clone()).unwrap_or_default();
    ui.add_space(6.0);
    egui::CollapsingHeader::new(RichText::new(format!("Reference planes ({})", planes.len()))).id_salt("pm planes").default_open(true).show(ui, |ui| {
        let mut sketch_on = None;
        let mut remove = None;
        for p in &planes {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&p.name).strong());
                ui.label(RichText::new(&p.how).weak().small());
                if ui.small_button("Sketch").on_hover_text("Start a sketch on this plane").clicked() {
                    sketch_on = Some(p.clone());
                }
                if ui.small_button("×").on_hover_text("Remove the plane (sketches already on it keep their place)").clicked() {
                    remove = Some(p.name.clone());
                }
            });
        }
        if ui.button("New plane…").clicked() {
            state.cad.plane_form = Some(PlaneForm::default());
        }
        if let Some(p) = sketch_on {
            super::sketch::begin_on_frame(state, p.frame, Some(p.name), false);
        }
        if let Some(n) = remove {
            op(state, "remove_plane", json!({"name": n}));
        }
    });
}

/// The reference planes (and the plane of the open sketch) as dashed squares over the 3D view,
/// sized to the part, with their names.
pub fn draw_overlay(painter: &egui::Painter, cam: &aurafx_render::Camera, rect: egui::Rect, state: &State) {
    let (planes, half) = {
        let s = state.session.lock();
        let Some(e) = s.cad_edit.as_ref() else { return };
        let half = e
            .summary
            .as_ref()
            .map(|s| (0..3).map(|k| s.bounds.1[k] - s.bounds.0[k]).fold(0.0f64, f64::max) * 0.6)
            .filter(|h| h.is_finite() && *h > 0.0)
            .unwrap_or(25.0);
        (e.document.planes.iter().map(|p| (p.name.clone(), p.frame)).collect::<Vec<_>>(), half)
    };
    let col = egui::Color32::from_rgb(70, 150, 230);
    for (name, f) in &planes {
        let r = f.basis();
        let o = glam::DVec3::from_array(f.origin);
        let corners: Vec<glam::DVec3> =
            [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].iter().map(|(u, v)| o + r.x_axis * (u * half) + r.y_axis * (v * half)).collect();
        let pts: Vec<Option<egui::Pos2>> = corners.iter().map(|c| crate::pick::project(cam, rect, *c)).collect();
        for k in 0..4 {
            if let (Some(a), Some(b)) = (pts[k], pts[(k + 1) % 4]) {
                painter.extend(egui::Shape::dashed_line(&[a, b], egui::Stroke::new(1.2, col), 6.0, 4.0));
            }
        }
        if let Some(p) = pts[3] {
            painter.text(p + egui::vec2(4.0, -4.0), egui::Align2::LEFT_BOTTOM, name, egui::FontId::proportional(12.0), col);
        }
    }
}

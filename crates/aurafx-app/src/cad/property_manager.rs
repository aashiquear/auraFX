//! The feature panel: the selected feature's every field, in place of the Properties panel
//! while the environment is open. Every number accepts an expression of the dimensions; typing
//! one makes the field follow the dimension.

use super::{editing, expr_field, num_text, op};
use crate::app::State;
use aurafx_cad::{Document, Feature};
use egui::RichText;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Keys of a shape that are not plain numeric fields.
const SKIP: &[&str] = &["kind", "type", "sketch", "profile", "path", "sections", "part", "heightmap"];

fn options_for(key: &str) -> Option<&'static [&'static str]> {
    match key {
        "end" => Some(&["Blind", "MidPlane", "ThroughAll"]),
        "plane" => Some(&["YZ", "XZ", "XY"]),
        "axis" => Some(&["X", "Y", "Z"]),
        _ => None,
    }
}

/// The variants of the tagged objects (`"type"`), with a default for each.
fn variants_for(key: &str) -> Option<Vec<(&'static str, Value)>> {
    match key {
        "top_edge" | "bottom_edge" => Some(vec![
            ("None", json!({"type": "None"})),
            ("Fillet", json!({"type": "Fillet", "radius": 1.0})),
            ("Chamfer", json!({"type": "Chamfer", "distance": 1.0})),
        ]),
        "hole" => Some(vec![
            ("Simple", json!({"type": "Simple"})),
            ("Counterbore", json!({"type": "Counterbore", "diameter": 11.0, "depth": 4.0})),
            ("Countersink", json!({"type": "Countersink", "diameter": 12.0, "angle_deg": 90.0})),
            ("Tapped", json!({"type": "Tapped", "pitch": 1.25, "cosmetic": false})),
            ("Drilled", json!({"type": "Drilled"})),
        ]),
        _ => None,
    }
}

struct Ctx<'a> {
    feature: &'a str,
    bindings: HashMap<String, String>,
    acts: Vec<(&'static str, Value)>,
}

/// What a field means, for its tooltip (by the last part of its name).
fn help_for(path: &str) -> &'static str {
    let key = path.rsplit('/').find(|p| p.parse::<usize>().is_err()).unwrap_or("");
    match key {
        "size" => "The size along x, y and z (mm). A number, or an expression of the dimensions such as width / 2.",
        "radius" | "r_bottom" | "r_top" => "A radius (mm).",
        "diameter" => "A diameter (mm). A hole's diameter is its drilled size.",
        "height" | "depth" => "A length (mm). A hole's depth is measured down from the point it is placed at; an extrusion's from its sketch plane.",
        "corner_radius" => "Rounds the box's edges by this radius.",
        "major" | "minor" => "The torus's ring radius and tube radius (mm).",
        "draft_deg" => "Taper of an extrusion's walls in degrees: positive widens it away from the sketch plane.",
        "angle_deg" => "An angle in degrees: a revolve's sweep (360: all the way round) or a draft's taper.",
        "origin" | "neutral_origin" => "A point on the plane (mm): the mirror's plane, or the draft's neutral plane.",
        "normal" | "neutral_normal" => "The direction the plane faces (x, y, z).",
        "by" => "Moves the copies made so far by this much in the feature's own frame (mm).",
        "reverse" => "Extrude against the sketch plane's normal.",
        "end" => "Blind: a given depth. MidPlane: half each way. ThroughAll: through the whole part (for cuts).",
        "blend" => "Radius of a rounded junction where this feature meets the part (0: a sharp edge).",
        "thickness" => "A wall thickness (mm).",
        "count" => "The number of copies, including the original.",
        "step" => "The offset between copies (mm).",
        "total_deg" => "The angle the copies cover: 360 is a full circle.",
        "translate" => "Where the feature is placed (mm).",
        "rotate_deg" => "How the feature is turned about x, y and z (degrees).",
        "pitch" => "The distance between thread crests (mm).",
        "factor" => "Scale factor about the origin.",
        "offset" => "A plane's offset from the origin (mm).",
        _ => "",
    }
}

impl Ctx<'_> {
    fn number(&mut self, ui: &mut egui::Ui, label: &str, path: &str, value: f64) {
        let l = ui.label(label);
        let h = help_for(path);
        if !h.is_empty() {
            l.on_hover_text(h);
        }
        let shown = self.bindings.get(path).cloned().unwrap_or_else(|| num_text(value));
        let bound = self.bindings.contains_key(path);
        ui.horizontal(|ui| {
            if let Some(t) = expr_field(ui, ("pm", self.feature, path), &shown, 96.0) {
                match t.parse::<f64>() {
                    Ok(x) => self.acts.push(("set", json!({"feature": self.feature, "path": path, "value": x}))),
                    Err(_) => self.acts.push(("bind", json!({"feature": self.feature, "path": path, "expr": t}))),
                }
            }
            if bound {
                ui.label(RichText::new(format!("= {}", num_text(value))).weak().small()).on_hover_text("This field follows an expression of the dimensions");
            }
        });
        ui.end_row();
    }

    fn walk(&mut self, ui: &mut egui::Ui, v: &Value, prefix: &str) {
        let Some(obj) = v.as_object() else { return };
        for (k, val) in obj {
            if SKIP.contains(&k.as_str()) && !(variants_for(k).is_some()) {
                continue;
            }
            let path = format!("{prefix}/{k}");
            // Fields of a nested object are named with it ("hole depth"), as two can share a name.
            let parent = prefix.rsplit('/').next().filter(|p| !matches!(*p, "shape" | "") && p.parse::<usize>().is_err() && !prefix.starts_with("/modifiers"));
            let label = match parent {
                Some(p) => format!("{} {}", p.replace('_', " "), k.replace('_', " ")),
                None => k.replace('_', " "),
            };
            if let Some(variants) = variants_for(k) {
                let cur = val.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string();
                ui.label(&label);
                let mut sel = cur.clone();
                egui::ComboBox::from_id_salt(("pm variant", self.feature, &path)).selected_text(&cur).show_ui(ui, |ui| {
                    for (name, _) in &variants {
                        ui.selectable_value(&mut sel, name.to_string(), *name);
                    }
                });
                if sel != cur {
                    if let Some((_, d)) = variants.iter().find(|(n, _)| *n == sel) {
                        self.acts.push(("set_json", json!({"feature": self.feature, "path": path, "json": d})));
                    }
                }
                ui.end_row();
                self.walk(ui, val, &path);
                continue;
            }
            match val {
                Value::Number(n) => self.number(ui, &label, &path, n.as_f64().unwrap_or(0.0)),
                Value::Array(a) if a.iter().all(|x| x.is_number()) => {
                    let axes = ["x", "y", "z"];
                    for (i, x) in a.iter().enumerate() {
                        let l = if a.len() <= 3 { format!("{label} {}", axes[i]) } else { format!("{label} {i}") };
                        self.number(ui, &l, &format!("{path}/{i}"), x.as_f64().unwrap_or(0.0));
                    }
                }
                Value::Bool(b) => {
                    ui.label(&label);
                    let mut on = *b;
                    if ui.checkbox(&mut on, "").changed() {
                        self.acts.push(("set_json", json!({"feature": self.feature, "path": path, "json": on})));
                    }
                    ui.end_row();
                }
                Value::String(s) => {
                    if let Some(opts) = options_for(k) {
                        ui.label(&label);
                        let mut sel = s.clone();
                        egui::ComboBox::from_id_salt(("pm opt", self.feature, &path)).selected_text(s).show_ui(ui, |ui| {
                            for o in opts {
                                ui.selectable_value(&mut sel, o.to_string(), *o);
                            }
                        });
                        if &sel != s {
                            self.acts.push(("set_json", json!({"feature": self.feature, "path": path, "json": sel})));
                        }
                        ui.end_row();
                    }
                }
                _ => {}
            }
        }
    }
}

pub fn property_manager_ui(state: &mut State, ui: &mut egui::Ui) {
    if !editing(state) {
        return;
    }
    let (doc, selected, error) = {
        let s = state.session.lock();
        let Some(e) = s.cad_edit.as_ref() else { return };
        (e.document.clone(), e.selected, e.error.clone())
    };
    // An empty part has nothing to build yet: that is not an error to show.
    let error = error.filter(|_| !doc.features.is_empty() || !doc.components.is_empty());
    if let Some(e) = &error {
        // Stays until the part builds again (the message in the corner goes away after a moment).
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(RichText::new(format!("{} The part does not build", crate::icons::WARNING)).color(crate::theme::ERROR).strong());
            ui.add(egui::Label::new(RichText::new(e).color(crate::theme::ERROR).small()).wrap());
            ui.horizontal(|ui| {
                if ui.button("Undo the last change").on_hover_text("Ctrl+Z").clicked() {
                    state.undo();
                }
                ui.label(RichText::new("or change the feature marked in the tree").weak().small());
            });
        });
    }
    if state.cad.pick.is_some() {
        super::pick::ui(state, ui);
        return;
    }
    if state.cad.plane_form.is_some() {
        super::planes::form_ui(state, ui);
        return;
    }
    if super::sketching(state) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| super::sketch::props_ui(state, ui));
        return;
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match selected.and_then(|id| doc.feature(id)) {
        Some(f) => feature_properties(state, ui, &doc, f),
        None => part_properties(state, ui, &doc),
    });
}

fn part_properties(state: &mut State, ui: &mut egui::Ui, doc: &Document) {
    ui.label(RichText::new("Part").strong());
    ui.label(
        RichText::new("Select a feature in the model tree to edit it. Dimensions are listed there; a number anywhere may be an expression of them.")
            .weak()
            .small(),
    );
    ui.horizontal(|ui| {
        ui.label("Name");
        if let Some(t) = expr_field(ui, "part name", &doc.name, 160.0) {
            op(state, "rename_document", json!({"name": t}));
        }
    });
    ui.add_space(6.0);
    ui.label(
        RichText::new(format!("{} feature(s), {} dimension(s), {} configuration(s)", doc.features.len(), doc.parameters.len(), doc.configurations.len()))
            .weak()
            .small(),
    );
    let mut sketch_on: Option<aurafx_pipeline::CadFace> = None;
    let summary = state.session.lock().cad_edit.as_ref().and_then(|e| e.summary.clone());
    if let Some(s) = &summary {
        ui.add_space(6.0);
        egui::CollapsingHeader::new(RichText::new(format!("Faces ({})", s.faces.len())).strong()).id_salt("pm faces").show(ui, |ui| {
            for (i, f) in s.faces.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{i}  {}  ({}, {} mm²)", f.name, f.kind, num_text(f.area))).small());
                    if f.kind == "plane" && ui.small_button("Sketch here").on_hover_text("Start a sketch on this flat face").clicked() {
                        sketch_on = Some(f.clone());
                    }
                });
            }
        });
    }
    start_face_sketch(state, sketch_on);
    super::planes::list_ui(state, ui);
}

fn start_face_sketch(state: &mut State, face: Option<aurafx_pipeline::CadFace>) {
    if let Some(f) = face {
        super::sketch::begin_on_face(state, &f);
    }
}

fn feature_properties(state: &mut State, ui: &mut egui::Ui, doc: &Document, f: &Feature) {
    let fv = serde_json::to_value(f).unwrap_or(Value::Null);
    let mut cx = Ctx { feature: &f.name, bindings: f.bindings.iter().map(|b| (b.path.clone(), b.expr.clone())).collect(), acts: Vec::new() };
    let mut edit_sketch = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(&f.name).strong());
        if matches!(f.shape, aurafx_cad::Shape::SketchExtrude { .. } | aurafx_cad::Shape::SketchRevolve { .. })
            && ui.small_button("Edit sketch…").on_hover_text("Open the sketch canvas on this feature's sketch").clicked()
        {
            edit_sketch = true;
        }
        if ui.small_button("Delete").clicked() {
            cx.acts.push(("remove", json!({"feature": f.name})));
        }
    });
    egui::Grid::new(("pm head", f.id)).num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
        ui.label("Name");
        if let Some(t) = expr_field(ui, ("pm name", f.id), &f.name, 140.0) {
            cx.acts.push(("rename", json!({"feature": f.name, "to": t})));
        }
        ui.end_row();
        ui.label("Operation");
        let cur = format!("{:?}", f.op);
        let mut sel = cur.clone();
        egui::ComboBox::from_id_salt(("pm op", f.id)).selected_text(&cur).show_ui(ui, |ui| {
            for o in ["Add", "Cut", "Intersect"] {
                ui.selectable_value(&mut sel, o.to_string(), o);
            }
        });
        if sel != cur {
            cx.acts.push(("set_op", json!({"feature": f.name, "to": sel.to_lowercase()})));
        }
        ui.end_row();
        ui.label("Suppressed");
        let mut sup = f.suppressed;
        if ui.checkbox(&mut sup, "").changed() {
            cx.acts.push(("suppress", json!({"feature": f.name, "suppressed": sup})));
        }
        ui.end_row();
    });
    egui::CollapsingHeader::new(RichText::new("Shape").strong()).id_salt(("pm shape", f.id)).default_open(true).show(ui, |ui| {
        egui::Grid::new(("pm shape grid", f.id)).num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
            if let Some(shape) = fv.get("shape") {
                cx.walk(ui, shape, "/shape");
            }
        });
    });
    // Sketch dimensions.
    let mut sk = f.clone();
    if let Some(sketch) = sk.shape.sketch_mut() {
        let dims: Vec<(usize, String, f64, Option<String>)> = sketch
            .constraints
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let d = c.dim()?;
                let name = serde_json::to_value(c).ok().and_then(|v| v.as_object().and_then(|o| o.keys().next().cloned())).unwrap_or_default();
                Some((i, name, d.value, d.expr.clone()))
            })
            .collect();
        egui::CollapsingHeader::new(RichText::new(format!("Sketch ({} dimension(s), {} entities)", dims.len(), sketch.entities.len())).strong())
            .id_salt(("pm sketch", f.id))
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new(("pm sketch grid", f.id)).num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                    for (i, name, value, expr) in &dims {
                        ui.label(format!("{name} {i}"));
                        let shown = expr.clone().unwrap_or_else(|| num_text(*value));
                        if let Some(t) = expr_field(ui, ("pm dim", f.id, *i), &shown, 96.0) {
                            let v = match t.parse::<f64>() {
                                Ok(x) => json!(x),
                                Err(_) => json!(t),
                            };
                            cx.acts.push(("set_dim", json!({"feature": f.name, "constraint": i, "value": v})));
                        }
                        ui.end_row();
                    }
                });
            });
    }
    // Placement and blend.
    egui::CollapsingHeader::new(RichText::new("Placement").strong()).id_salt(("pm place", f.id)).default_open(false).show(ui, |ui| {
        egui::Grid::new(("pm place grid", f.id)).num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
            for (key, label) in [("translate", "position"), ("rotate_deg", "rotation (°)")] {
                if let Some(a) = fv.pointer(&format!("/placement/{key}")).and_then(|a| a.as_array()) {
                    for (i, x) in a.iter().enumerate() {
                        cx.number(ui, &format!("{label} {}", ["x", "y", "z"][i]), &format!("/placement/{key}/{i}"), x.as_f64().unwrap_or(0.0));
                    }
                }
            }
            cx.number(ui, "blend radius", "/blend", f.blend);
            ui.label("blend style");
            let cur = format!("{:?}", f.blend_kind);
            let mut sel = cur.clone();
            egui::ComboBox::from_id_salt(("pm blend", f.id)).selected_text(&cur).show_ui(ui, |ui| {
                for o in ["Round", "Chamfer"] {
                    ui.selectable_value(&mut sel, o.to_string(), o);
                }
            });
            if sel != cur {
                cx.acts.push(("set_json", json!({"feature": f.name, "path": "/blend_kind", "json": sel})));
            }
            ui.end_row();
        });
    });
    // Modifiers.
    egui::CollapsingHeader::new(RichText::new(format!("Modifiers ({})", f.modifiers.len())).strong())
        .id_salt(("pm mods", f.id))
        .default_open(!f.modifiers.is_empty())
        .show(ui, |ui| {
            for (i, m) in f.modifiers.iter().enumerate() {
                let mv = serde_json::to_value(m).unwrap_or(Value::Null);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(mv.get("kind").and_then(|k| k.as_str()).unwrap_or("?")).strong());
                    if ui.small_button("Remove").clicked() {
                        cx.acts.push(("remove_modifier", json!({"feature": f.name, "index": i})));
                    }
                });
                egui::Grid::new(("pm mod grid", f.id, i)).num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                    cx.walk(ui, &mv, &format!("/modifiers/{i}"));
                });
            }
        });
    // Bindings.
    if !f.bindings.is_empty() {
        egui::CollapsingHeader::new(RichText::new(format!("Driven by dimensions ({})", f.bindings.len())).strong()).id_salt(("pm bind", f.id)).show(ui, |ui| {
            for b in &f.bindings {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} = {}", b.path, b.expr)).small().monospace());
                    if ui.small_button("×").on_hover_text("Stop following the expression; keep the current value").clicked() {
                        let val = fv.pointer(&b.path).and_then(|v| v.as_f64()).unwrap_or(0.0);
                        cx.acts.push(("set", json!({"feature": f.name, "path": b.path, "value": val})));
                    }
                });
            }
        });
    }
    let _ = doc;
    for (name, args) in cx.acts {
        op(state, name, args);
    }
    if edit_sketch {
        super::sketch_edit(state, f.id);
    }
}

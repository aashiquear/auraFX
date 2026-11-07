//! The CAD modeling environment's part of the app (roadmap section 13). This file holds what
//! a CAD Model node shows in the Properties panel: its dimensions (the document's parameters,
//! editable without entering the environment), its features and the faces of the last result.

use crate::app::State;
use aurafx_pipeline::NodeId;
use egui::RichText;

/// The Properties of a CAD Model node.
pub fn properties_ui(state: &mut State, ui: &mut egui::Ui, id: NodeId) {
    let (doc, effective, overrides) = {
        let s = state.session.lock();
        (s.cad_document(id), s.cad_effective_document(id), s.cad_overrides(id))
    };
    let doc = match doc {
        Ok(d) => d,
        Err(e) => {
            ui.label(RichText::new(format!("{e:#}")).color(crate::theme::ERROR));
            return;
        }
    };
    let values = effective.as_ref().ok().and_then(|d| d.parameter_values().ok());
    egui::CollapsingHeader::new(RichText::new("Dimensions").strong()).id_salt(("cad dims", id)).default_open(true).show(ui, |ui| {
        if doc.parameters.is_empty() {
            ui.label(RichText::new("The part has no named dimensions. Add them in Python: node.cad.parameter(\"width\", 40).").weak().small());
        }
        egui::Grid::new(("cad dims grid", id)).num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
            for p in &doc.parameters {
                let over = overrides.iter().find(|(n, _)| n == &p.name).map(|(_, e)| e.clone());
                let text_id = ui.make_persistent_id(("cad dim", id, &p.name));
                let mut text: String = ui.data_mut(|d| d.get_temp(text_id)).unwrap_or_else(|| over.clone().unwrap_or_else(|| p.expr.clone()));
                ui.label(&p.name).on_hover_text(if p.comment.is_empty() { "A named dimension of the part" } else { &p.comment });
                ui.horizontal(|ui| {
                    let r = ui.add_sized([110.0, 20.0], egui::TextEdit::singleline(&mut text));
                    if r.changed() {
                        ui.data_mut(|d| d.insert_temp(text_id, text.clone()));
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) || !i.pointer.any_down()) {
                        ui.data_mut(|d| d.remove_temp::<String>(text_id));
                        if text.trim() != over.clone().unwrap_or_else(|| p.expr.clone()).trim() {
                            let value = if text.trim() == p.expr.trim() { serde_json::Value::Null } else { serde_json::Value::String(text.trim().to_string()) };
                            let result = state.session.lock().cad_set_parameter(id, &p.name, &value);
                            if let Err(e) = result {
                                state.toast(format!("{e:#}"), true);
                            }
                        }
                    }
                    match values.as_ref().and_then(|v| v.get(&p.name)) {
                        Some(v) => ui.label(RichText::new(format!("= {} mm", trim_num(*v))).weak()),
                        None => ui.label(RichText::new("?").color(crate::theme::ERROR)),
                    };
                    if over.is_some() && ui.small_button("reset").on_hover_text("Use the document's own value again").clicked() {
                        ui.data_mut(|d| d.remove_temp::<String>(text_id));
                        let _ = state.session.lock().cad_set_parameter(id, &p.name, &serde_json::Value::Null);
                    }
                });
                ui.end_row();
            }
        });
        if let Err(e) = &effective {
            ui.label(RichText::new(format!("{e}")).color(crate::theme::ERROR).small());
        }
    });
    if !doc.configurations.is_empty() {
        let current = state.session.lock().nodes.get(&id).map(|n| n.params.str("configuration")).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label("Configuration");
            let mut sel = current.clone();
            egui::ComboBox::from_id_salt(("cad cfg", id)).selected_text(if sel.is_empty() { "(document)".to_string() } else { sel.clone() }).show_ui(ui, |ui| {
                ui.selectable_value(&mut sel, String::new(), "(document)");
                for c in &doc.configurations {
                    ui.selectable_value(&mut sel, c.name.clone(), &c.name);
                }
            });
            if sel != current {
                state.session.lock().set_param(id, "configuration", aurafx_filters::ParamValue::Str(sel));
            }
        });
    }
    egui::CollapsingHeader::new(RichText::new(format!("Features ({})", doc.features.len())).strong()).id_salt(("cad feats", id)).default_open(true).show(ui, |ui| {
        let mut toggle: Option<(String, bool)> = None;
        for f in &doc.features {
            ui.horizontal(|ui| {
                let mut on = !f.suppressed;
                if ui.checkbox(&mut on, "").on_hover_text("Include this feature (off: suppressed)").changed() {
                    toggle = Some((f.name.clone(), !on));
                }
                let kind = serde_json::to_value(&f.shape).ok().and_then(|v| v.get("kind").and_then(|k| k.as_str().map(str::to_string))).unwrap_or_default();
                let kind = match kind.as_str() {
                    "SketchExtrude" | "Extrude" => "extruded sketch".to_string(),
                    "SketchRevolve" | "Revolve" => "revolved sketch".to_string(),
                    k => k.to_lowercase(),
                };
                let text = format!("{}  ·  {} {}", f.name, match f.op {
                    aurafx_cad::BooleanOp::Add => "adds",
                    aurafx_cad::BooleanOp::Cut => "cuts",
                    aurafx_cad::BooleanOp::Intersect => "intersects",
                }, kind);
                ui.label(if f.suppressed { RichText::new(text).weak().strikethrough() } else { RichText::new(text) });
            });
        }
        if let Some((name, sup)) = toggle {
            let _ = state.session.lock().cad_edit(id, "suppress", &serde_json::json!({"feature": name, "suppressed": sup}));
        }
    });
    let faces = {
        let s = state.session.lock();
        s.nodes.get(&id).and_then(|n| n.output.clone()).and_then(|o| match &*o {
            aurafx_data::DataSet::PolyData(p) => {
                let names = p.field_data.get("FaceNames")?;
                let vol = p.field_data.get("Volume").map(|v| v.get(0, 0));
                let area = p.field_data.get("SurfaceArea").map(|v| v.get(0, 0));
                let list = match &names.data {
                    aurafx_data::ArrayData::String(v) => v.clone(),
                    _ => Vec::new(),
                };
                Some((list, vol, area))
            }
            _ => None,
        })
    };
    if let Some((names, vol, area)) = faces {
        egui::CollapsingHeader::new(RichText::new(format!("Faces ({})", names.len())).strong()).id_salt(("cad faces", id)).show(ui, |ui| {
            if let (Some(v), Some(a)) = (vol, area) {
                ui.label(RichText::new(format!("volume {} mm³, surface {} mm²", trim_num(v), trim_num(a))).weak().small());
            }
            for (i, n) in names.iter().enumerate() {
                ui.label(RichText::new(format!("{i}  {n}")).small()).on_hover_text("Downstream filters refer to a face by this name or number (FaceId)");
            }
        });
    }
}

fn trim_num(v: f64) -> String {
    if v.abs() >= 1000.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.3}").trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

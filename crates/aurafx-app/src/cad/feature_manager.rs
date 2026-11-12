//! The model tree: the part's tree (dimensions, features in order, finishing modifiers,
//! configurations) in place of the Pipeline panel while the environment is open.

use super::{editing, expr_field, number_or_expr, op};
use crate::app::State;
use aurafx_cad::{BooleanOp, Id};
use egui::RichText;
use serde_json::{json, Value};

enum Act {
    Op(&'static str, Value),
    Select(Option<Id>),
    Rollback(Option<usize>),
    Rename(Id, String),
    EditSketch(Id),
}

fn kind_label(f: &aurafx_cad::Feature) -> String {
    let kind = serde_json::to_value(&f.shape).ok().and_then(|v| v.get("kind").and_then(|k| k.as_str().map(str::to_string))).unwrap_or_default();
    match kind.as_str() {
        "SketchExtrude" | "Extrude" => "extruded sketch".into(),
        "SketchRevolve" | "Revolve" => "revolved sketch".into(),
        k => k.to_lowercase(),
    }
}

pub fn feature_manager_ui(state: &mut State, ui: &mut egui::Ui) {
    if !editing(state) {
        return;
    }
    let (doc, selected, rollback, summary) = {
        let s = state.session.lock();
        let Some(e) = s.cad_edit.as_ref() else { return };
        (e.document.clone(), e.selected, e.rollback, e.summary.clone())
    };
    let values = doc.parameter_values().ok();
    let mut acts: Vec<Act> = Vec::new();
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} {}", crate::icons::CUBE, if doc.name.is_empty() { "Part" } else { &doc.name })).strong());
        if let Some(s) = &summary {
            ui.label(RichText::new(format!("{} mm³", super::num_text(s.volume))).weak().small());
        }
    });
    super::guide::guide_ui(state, ui);
    let (build_error, approximate) = {
        let s = state.session.lock();
        let e = s.cad_edit.as_ref();
        (e.and_then(|e| e.error.clone()), e.and_then(|e| e.summary.as_ref()).map(|m| m.approximate.clone()).unwrap_or_default())
    };
    ui.separator();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // ---------------------------------------------------------------- dimensions
        egui::CollapsingHeader::new(RichText::new(format!("Dimensions ({})", doc.parameters.len())).strong())
            .id_salt("fm dims")
            .default_open(doc.features.len() < 2)
            .show(ui, |ui| {
                egui::Grid::new("fm dims grid").num_columns(3).spacing([8.0, 3.0]).show(ui, |ui| {
                    for p in &doc.parameters {
                        ui.label(&p.name).on_hover_text(if p.comment.is_empty() { "A named dimension of the part" } else { &p.comment });
                        if let Some(t) = expr_field(ui, ("fm dim", &p.name), &p.expr, 100.0) {
                            acts.push(Act::Op("parameter", json!({"name": p.name, "expr": t})));
                        }
                        ui.horizontal(|ui| {
                            match values.as_ref().and_then(|v| v.get(&p.name)) {
                                Some(v) => ui.label(RichText::new(format!("= {}", super::num_text(*v))).weak().small()),
                                None => ui.label(RichText::new("?").color(crate::theme::ERROR)),
                            };
                            if ui.small_button("×").on_hover_text("Remove this dimension").clicked() {
                                acts.push(Act::Op("remove_parameter", json!({"name": p.name})));
                            }
                        });
                        ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    let (n, e) = &mut state.cad.new_param;
                    ui.add_sized([70.0, 20.0], egui::TextEdit::singleline(n).hint_text("name"));
                    ui.add_sized([80.0, 20.0], egui::TextEdit::singleline(e).hint_text("40 mm"));
                    if ui.small_button("Add").clicked() && !n.trim().is_empty() && !e.trim().is_empty() {
                        acts.push(Act::Op("parameter", json!({"name": n.trim(), "expr": e.trim()})));
                        n.clear();
                        e.clear();
                    }
                });
            });
        // ---------------------------------------------------------------- features
        egui::CollapsingHeader::new(RichText::new(format!("Features ({})", doc.features.len())).strong()).id_salt("fm features").default_open(true).show(
            ui,
            |ui| {
                if doc.features.is_empty() {
                    ui.label(RichText::new("No feature yet. Use the buttons above, the ribbon's Sketch and Features tabs, or press S to search.").weak().small());
                }
                let upto = rollback.unwrap_or(doc.features.len());
                for (i, f) in doc.features.iter().enumerate() {
                    if i == upto && rollback.is_some() {
                        ui.label(RichText::new("▮ rollback marker").color(crate::theme::ACCENT).small());
                    }
                    ui.horizontal(|ui| {
                        let mut on = !f.suppressed;
                        if ui.checkbox(&mut on, "").on_hover_text("Include this feature (off: suppressed)").changed() {
                            acts.push(Act::Op("suppress", json!({"feature": f.name, "suppressed": !on})));
                        }
                        if state.cad.rename.as_ref().is_some_and(|(id, _)| *id == f.id) {
                            let (_, text) = state.cad.rename.as_mut().unwrap();
                            let r = ui.add_sized([120.0, 20.0], egui::TextEdit::singleline(text));
                            r.request_focus();
                            if r.lost_focus() {
                                let (id, t) = state.cad.rename.take().unwrap();
                                if ui.input(|i| i.key_pressed(egui::Key::Enter)) || !t.trim().is_empty() && t != f.name {
                                    acts.push(Act::Rename(id, t));
                                }
                            }
                            return;
                        }
                        let verb = match f.op {
                            BooleanOp::Add => "adds",
                            BooleanOp::Cut => "cuts",
                            BooleanOp::Intersect => "intersects",
                        };
                        let mut text = RichText::new(&f.name);
                        // The feature the build error names, and those built only roughly.
                        let failed = build_error.as_ref().filter(|e| !f.suppressed && e.contains(&format!("'{}'", f.name)));
                        let rough = approximate.iter().find(|a| a.contains(&f.name));
                        if f.suppressed {
                            text = text.weak().strikethrough();
                        } else if failed.is_some() {
                            text = text.color(crate::theme::ERROR);
                        } else if i >= upto {
                            text = text.weak();
                        }
                        let mut hover = format!("{} {}\nRight-click: rename, suppress, roll back, move, delete. Double-click: rename.", verb, kind_label(f));
                        if let Some(e) = failed {
                            hover = format!("This feature stops the part from building:\n{e}\n\nChange its numbers on the right, suppress it (untick the box), or undo.");
                        } else if let Some(a) = rough {
                            hover = format!("{hover}\n\nShown roughly: {a}");
                        }
                        let r = ui.selectable_label(selected == Some(f.id), text).on_hover_text(hover);
                        if failed.is_some() {
                            ui.label(RichText::new(crate::icons::WARNING).color(crate::theme::ERROR));
                        } else if rough.is_some() {
                            ui.label(RichText::new("≈").weak()).on_hover_text("Built roughly by this kernel; the exact kernel builds it at Apply");
                        }
                        ui.label(RichText::new(format!("{verb} {}", kind_label(f))).weak().small());
                        if r.clicked() {
                            acts.push(Act::Select(Some(f.id)));
                        }
                        if r.double_clicked() {
                            state.cad.rename = Some((f.id, f.name.clone()));
                        }
                        r.context_menu(|ui| {
                            if ui.button("Rename").clicked() {
                                ui.close();
                                state.cad.rename = Some((f.id, f.name.clone()));
                            }
                            if matches!(f.shape, aurafx_cad::Shape::SketchExtrude { .. } | aurafx_cad::Shape::SketchRevolve { .. })
                                && ui.button("Edit sketch…").clicked()
                            {
                                ui.close();
                                acts.push(Act::EditSketch(f.id));
                            }
                            if ui.button(if f.suppressed { "Unsuppress" } else { "Suppress" }).clicked() {
                                ui.close();
                                acts.push(Act::Op("suppress", json!({"feature": f.name, "suppressed": !f.suppressed})));
                            }
                            if ui.button("Roll back to here").clicked() {
                                ui.close();
                                acts.push(Act::Rollback(Some(i + 1)));
                            }
                            if ui.add_enabled(i > 0, egui::Button::new("Move up")).clicked() {
                                ui.close();
                                acts.push(Act::Op("move", json!({"feature": f.name, "to": i - 1})));
                            }
                            if ui.add_enabled(i + 1 < doc.features.len(), egui::Button::new("Move down")).clicked() {
                                ui.close();
                                acts.push(Act::Op("move", json!({"feature": f.name, "to": i + 1})));
                            }
                            ui.separator();
                            if ui.button("Delete").clicked() {
                                ui.close();
                                acts.push(Act::Op("remove", json!({"feature": f.name})));
                            }
                        });
                    });
                }
                if rollback.is_some() && ui.small_button("Roll forward to the end").clicked() {
                    acts.push(Act::Rollback(None));
                }
            },
        );
        // ---------------------------------------------------------------- components
        if !doc.components.is_empty() {
            egui::CollapsingHeader::new(RichText::new(format!("Components ({})", doc.components.len())).strong())
                .id_salt("fm components")
                .default_open(true)
                .show(ui, |ui| {
                    for c in &doc.components {
                        let on_it = state.cad.component.as_deref() == Some(c.name.as_str());
                        ui.horizontal(|ui| {
                            let mut vis = c.visible;
                            if ui.checkbox(&mut vis, "").on_hover_text("Show this part (off: left out of the assembly)").changed() {
                                acts.push(Act::Op("component_visible", json!({"component": c.name, "visible": vis})));
                            }
                            if state.cad.component_rename.as_ref().is_some_and(|(n, _)| *n == c.name) {
                                let (_, text) = state.cad.component_rename.as_mut().unwrap();
                                let r = ui.add_sized([120.0, 20.0], egui::TextEdit::singleline(text));
                                r.request_focus();
                                if r.lost_focus() {
                                    let (old, new) = state.cad.component_rename.take().unwrap();
                                    if !new.trim().is_empty() && new.trim() != old {
                                        acts.push(Act::Op("component_rename", json!({"component": old, "to": new.trim()})));
                                    }
                                }
                                return;
                            }
                            let mut text = RichText::new(&c.name);
                            if !c.visible {
                                text = text.weak().strikethrough();
                            }
                            let r = ui
                                .selectable_label(on_it, text)
                                .on_hover_text(format!("{} ({} features)\nClick to move or turn it; double-click to rename", c.document.name, c.document.features.len()));
                            if r.double_clicked() {
                                state.cad.component_rename = Some((c.name.clone(), c.name.clone()));
                            } else if r.clicked() {
                                state.cad.component = if on_it { None } else { Some(c.name.clone()) };
                            }
                            if ui.small_button("×").on_hover_text("Remove this part from the assembly").clicked() {
                                acts.push(Act::Op("component_remove", json!({"component": c.name})));
                            }
                        });
                        if on_it {
                            egui::Grid::new(("fm component", c.id.0)).num_columns(6).spacing([6.0, 2.0]).show(ui, |ui| {
                                let mut at = c.placement.translate;
                                let mut rot = c.placement.rotate_deg;
                                let mut moved = false;
                                for (label, v) in ["x", "y", "z"].into_iter().zip(at.iter_mut()) {
                                    ui.label(RichText::new(label).weak().small());
                                    moved |= ui.add(egui::DragValue::new(v).speed(0.5).suffix(" mm")).changed();
                                }
                                ui.end_row();
                                for (label, v) in ["rx", "ry", "rz"].into_iter().zip(rot.iter_mut()) {
                                    ui.label(RichText::new(label).weak().small());
                                    moved |= ui.add(egui::DragValue::new(v).speed(1.0).suffix("°")).changed();
                                }
                                if moved {
                                    acts.push(Act::Op("component_move", json!({"component": c.name, "at": at, "rotate": rot})));
                                }
                            });
                        }
                    }
                });
        }
        // ---------------------------------------------------------------- finish
        if !doc.finish.is_empty() {
            egui::CollapsingHeader::new(RichText::new(format!("Finish ({})", doc.finish.len())).strong()).id_salt("fm finish").default_open(true).show(
                ui,
                |ui| {
                    ui.label(RichText::new("Modifiers on the whole part").weak().small());
                    for (i, m) in doc.finish.iter().enumerate() {
                        let v = serde_json::to_value(m).unwrap_or(Value::Null);
                        let kind = v.get("kind").and_then(|k| k.as_str()).unwrap_or("?").to_string();
                        ui.horizontal(|ui| {
                            ui.label(&kind);
                            if let Some(o) = v.as_object() {
                                for (k, val) in o.iter().filter(|(k, _)| k.as_str() != "kind") {
                                    if let Some(x) = val.as_f64() {
                                        ui.label(RichText::new(k).weak().small());
                                        if let Some(t) = expr_field(ui, ("fm finish", i, k), &super::num_text(x), 56.0) {
                                            if let Ok(n) = t.parse::<f64>() {
                                                acts.push(Act::Op("set_finish", json!({"index": i, "field": k, "value": n})));
                                            }
                                        }
                                    }
                                }
                            }
                            if ui.small_button("×").clicked() {
                                acts.push(Act::Op("remove_finish", json!({"index": i})));
                            }
                        });
                    }
                },
            );
        }
        // ---------------------------------------------------------------- configurations
        if !doc.configurations.is_empty() || !doc.parameters.is_empty() {
            egui::CollapsingHeader::new(RichText::new(format!("Configurations ({})", doc.configurations.len())).strong())
                .id_salt("fm cfg")
                .show(ui, |ui| {
                    ui.label(RichText::new("Named sets of dimension values: variants of one part.").weak().small());
                    let active = doc.active_configuration.clone();
                    if ui.selectable_label(active.is_none(), "(the dimensions as listed above)").clicked() {
                        acts.push(Act::Op("activate", json!({"name": Value::Null})));
                    }
                    for c in &doc.configurations {
                        ui.horizontal(|ui| {
                            let r = ui.selectable_label(active.as_deref() == Some(c.name.as_str()), &c.name);
                            r.clone().on_hover_text(c.overrides.iter().map(|p| format!("{} = {}", p.name, p.expr)).collect::<Vec<_>>().join("\n"));
                            if r.clicked() {
                                acts.push(Act::Op("activate", json!({"name": c.name})));
                            }
                            if ui.small_button("×").on_hover_text("Remove this configuration").clicked() {
                                acts.push(Act::Op("remove_configuration", json!({"name": c.name})));
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        let name = &mut state.cad.new_configuration;
                        ui.add_sized([110.0, 20.0], egui::TextEdit::singleline(name).hint_text("name, e.g. large"));
                        let ok = !name.trim().is_empty() && !doc.parameters.is_empty();
                        if ui
                            .add_enabled(ok, egui::Button::new("Save values"))
                            .on_hover_text("Keep the dimensions' present values under this name (an existing name is replaced). To change a configuration: set the dimensions, then save under its name again.")
                            .on_disabled_hover_text("Type a name; the part needs at least one dimension")
                            .clicked()
                        {
                            let overrides: serde_json::Map<String, Value> = doc.parameters.iter().map(|p| (p.name.clone(), json!(p.expr))).collect();
                            acts.push(Act::Op("configuration", json!({"name": name.trim(), "overrides": overrides})));
                            name.clear();
                        }
                    });
                });
        }
    });
    for a in acts {
        match a {
            Act::Op(name, args) => {
                op(state, name, args);
            }
            Act::Select(id) => state.session.lock().cad_env_select(id),
            Act::EditSketch(id) => super::sketch_edit(state, id),
            Act::Rollback(n) => state.session.lock().cad_env_rollback(n),
            Act::Rename(id, text) => {
                if let Some(f) = doc.feature(id) {
                    op(state, "rename", json!({"feature": f.name, "to": text}));
                }
            }
        }
    }
    let _ = number_or_expr;
}
